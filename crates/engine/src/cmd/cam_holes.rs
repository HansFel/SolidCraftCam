//! Holes picked by the hole feature that made them (`{feature}` in a step's `holes`): every bore
//! of the feature, its pattern copies too, with its centre, diameter, entry and lower end from the
//! body's exact faces and edges, and its thread from the feature's designation. A step that is
//! given no depth (or no thread size) takes the feature's.

use solidcore_record::{Vec2, Vec3};
use solidcraft_doc::thread_forms::{ThreadForm, ThreadHand};
use solidcraft_doc::{FeatureKind, HoleKind, expr::Kind};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{FaceSurface, PointClass};

use super::cam::WorkFrame;
use crate::Session;

/// Bores of a feature with more than this many faces are not looked through (bounded work).
const MAX_FACES: usize = 20_000;
/// Edges of a body looked through for the bores' rims.
const MAX_EDGES: usize = 40_000;
/// How far past the exit a drill's full diameter goes through, in mm.
pub(super) const BREAKTHROUGH: f64 = 1.0;

/// One bore of a hole feature, in work coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Bore {
    pub centre: Vec2,
    pub radius: f64,
    /// Where the bore's wall starts (its highest rim).
    pub entry: f64,
    /// Where the wall ends: the shoulder of a blind hole, the exit of a through hole.
    pub floor: f64,
    pub through: bool,
}

/// A hole feature's thread, as its designation and depth give it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BoreThread {
    pub major: f64,
    pub pitch: f64,
    pub left_hand: bool,
    /// The threaded length from the entry; `None`: the bore's full length.
    pub length: Option<f64>,
}

/// The bores of one hole feature and what it says about them.
#[derive(Clone, Debug)]
pub(super) struct FeatureBores {
    pub name: String,
    pub bores: Vec<Bore>,
    /// The drill point's depth below the shoulder of a blind bore (0: flat).
    pub tip: f64,
    pub thread: Option<BoreThread>,
}

/// What a step does in the bores, which sets the depth it goes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BoreUse {
    /// Drilling with a drill whose point is this long.
    Drill {
        point: f64,
    },
    /// Tapping with a tap of this pitch.
    Tap {
        pitch: f64,
    },
    HelicalBore,
    ThreadMill,
}

/// The bores the hole feature `reference` (its name or id) made, in `frame`'s coordinates.
pub(super) fn feature_bores(s: &Session, reference: &str, frame: &WorkFrame) -> Result<FeatureBores, String> {
    let feature = s.doc.find_feature(reference).ok_or_else(|| format!("there is no feature `{reference}`"))?;
    let FeatureKind::Hole { hole, thread, thread_depth, hand, .. } = &feature.kind else {
        return Err(format!("`{}` is not a hole feature", feature.name));
    };
    if feature.suppressed {
        return Err(format!("the hole feature `{}` is suppressed", feature.name));
    }
    // Half the drill point's angle (radians), where the hole has one.
    let tip_half = match hole {
        HoleKind::Drilled { tip_angle } => Some(tip_angle),
        HoleKind::Counterbore { tip_angle, .. } | HoleKind::Countersink { tip_angle, .. } => tip_angle.as_ref(),
        HoleKind::Simple => None,
    }
    .map(|e| s.doc.eval(e.as_str(), Kind::Angle))
    .transpose()
    .map_err(|e| format!("the tip angle of `{}`: {e}", feature.name))?
    .map(|a| a * 0.5)
    .filter(|h| *h > 1e-3 && *h < std::f64::consts::FRAC_PI_2);
    let prefix = format!("F{}:bore:", feature.id);
    let state = s.model.state();
    let mut bores: Vec<Bore> = Vec::new();
    for body in &state.bodies {
        let names = solidcraft_doc::naming::face_names(body);
        let size = body.body.size().max(1.0);
        let tol = size * 1e-6;
        // The bore walls: cylinders named after the feature's bore, the copies of a pattern too.
        let mut walls: Vec<(Vec2, f64)> = Vec::new();
        for (index, name) in names.iter().enumerate().take(MAX_FACES) {
            let base = name.split('@').next().unwrap_or_default();
            if !base.starts_with(&prefix) {
                continue;
            }
            let Some(FaceSurface::Cylinder { origin, axis, radius }) = solidcraft_kernel::face_surface(&body.body, index) else { continue };
            let along = Vec3::new(axis.x, axis.y, axis.z).dot(frame.z);
            if !(along.abs() >= 1.0 - 1e-6) {
                return Err(format!("a hole of `{}` does not point along the setup's tool axis: machine it in a setup facing it", feature.name));
            }
            let w = frame.local(origin);
            let centre = Vec2::new(w.x, w.y);
            // The kernel splits one cylinder into halves: one bore each.
            if !walls.iter().any(|(c, r)| c.dist(centre) <= tol && (r - radius).abs() <= tol) {
                walls.push((centre, radius));
            }
        }
        if walls.is_empty() {
            continue;
        }
        // Each wall's rims: the exact circles of its radius round its axis.
        let mut rims: Vec<Vec<f64>> = vec![Vec::new(); walls.len()];
        for edge in 0..body.body.edge_count().min(MAX_EDGES) {
            let Some(circle) = solidcraft_kernel::edge_circle(&body.body, edge) else { continue };
            let w = frame.local(circle.center);
            let at = Vec2::new(w.x, w.y);
            for (k, (c, r)) in walls.iter().enumerate() {
                if c.dist(at) <= tol
                    && (circle.radius - r).abs() <= tol
                    && let Some(list) = rims.get_mut(k)
                {
                    list.push(w.z);
                }
            }
        }
        for ((centre, radius), zs) in walls.into_iter().zip(rims) {
            let (Some(entry), Some(floor)) = (zs.iter().copied().reduce(f64::max), zs.iter().copied().reduce(f64::min)) else {
                return Err(format!("a hole of `{}` has no round rim to measure its depth by", feature.name));
            };
            if !(entry - floor > tol) {
                return Err(format!("a hole of `{}` has no depth along the tool axis", feature.name));
            }
            // Through when there is no material just past the bore's lower end (past its drill
            // point, where it has one): the body's exact classification.
            let point = tip_half.map_or(0.0, |h| radius / h.tan());
            let probe = frame.model(Vec3::new(centre.x, centre.y, floor - point - size * 1e-4));
            let through = matches!(body.body.classify_point(probe).map_err(|e| e.to_string())?, PointClass::Outside);
            bores.push(Bore { centre, radius, entry, floor, through });
        }
    }
    if bores.is_empty() {
        return Err(format!("the hole feature `{}` left no hole in the model", feature.name));
    }
    // The point below a blind bore's shoulder (every bore of one feature has its diameter).
    let tip = match (tip_half, bores.first()) {
        (Some(half), Some(b)) => b.radius / half.tan(),
        _ => 0.0,
    };
    let thread = match thread.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => None,
        Some(designation) => {
            let form = ThreadForm::of_designation(designation).ok_or_else(|| format!("`{designation}` names no thread form SolidCraft knows"))?;
            let size = form.parse(designation, 1)?;
            let length = thread_depth
                .as_ref()
                .map(|e| s.doc.eval(e.as_str(), Kind::Length))
                .transpose()
                .map_err(|e| format!("the thread depth of `{}`: {e}", feature.name))?;
            Some(BoreThread { major: size.major, pitch: size.pitch, left_hand: *hand == ThreadHand::Left, length })
        }
    };
    Ok(FeatureBores { name: feature.name.clone(), bores, tip, thread })
}

/// The depth (work Z) a step goes to in every bore of `holes`: the same for all of them, or an
/// error asking for the `bottom`.
pub(super) fn bottom_for(holes: &[FeatureBores], use_: BoreUse) -> Result<f64, String> {
    let mut bottom: Option<f64> = None;
    for feature in holes {
        for bore in &feature.bores {
            let length = feature.thread.as_ref().and_then(|t| t.length).map(|l| (bore.entry - l).max(bore.floor));
            let z = match use_ {
                // Blind: to the drill point's tip; through: the drill's whole point out of the exit.
                BoreUse::Drill { point } if bore.through => bore.floor - point - BREAKTHROUGH,
                BoreUse::Drill { .. } => bore.floor - feature.tip,
                // The tap's lead has a few incomplete threads: through a hole, two pitches past it.
                BoreUse::Tap { pitch } => length.unwrap_or(if bore.through { bore.floor - 2.0 * pitch } else { bore.floor }),
                BoreUse::ThreadMill => length.unwrap_or(bore.floor),
                BoreUse::HelicalBore if bore.through => bore.floor - BREAKTHROUGH,
                BoreUse::HelicalBore => bore.floor,
            };
            match bottom {
                None => bottom = Some(z),
                Some(b) if (b - z).abs() <= 1e-6 => {}
                Some(_) => return Err("the picked holes end at different depths: give the `bottom`, or make one step per depth".into()),
            }
        }
    }
    // To a micrometre, as typed depths are.
    bottom.map(|b| (b * 1000.0).round() / 1000.0).ok_or_else(|| "give the `bottom`".to_string())
}

/// The one thread the picked hole features name, for a thread mill or a tap given no size.
pub(super) fn thread_of(holes: &[FeatureBores]) -> Result<Option<BoreThread>, String> {
    let mut found: Option<&BoreThread> = None;
    for feature in holes {
        let Some(t) = &feature.thread else { return Err(format!("the hole feature `{}` has no thread", feature.name)) };
        match found {
            None => found = Some(t),
            Some(f) if (f.major - t.major).abs() <= 1e-9 && (f.pitch - t.pitch).abs() <= 1e-9 && f.left_hand == t.left_hand => {}
            Some(_) => return Err("the picked hole features name different threads: one step per thread".into()),
        }
    }
    Ok(found.cloned())
}

/// The length of a drill's point: its radius over the tangent of half its point angle.
pub(super) fn drill_point(tool: &solidcraft_cam::Tool) -> f64 {
    match tool.half_angle() {
        Some(h) if h > 1e-3 && h < std::f64::consts::FRAC_PI_2 && tool.kind == solidcraft_cam::ToolKind::Drill => tool.radius() / h.tan(),
        _ => 0.0,
    }
}
