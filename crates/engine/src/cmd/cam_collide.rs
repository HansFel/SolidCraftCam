//! Whether a step's tool, its shank and its holder stay clear of the part and of the setup's
//! fixtures, decided on the bodies' exact faces.
//!
//! The path is cut into runs that are straight to within [`STRAIGHT`] and nearly level; each run
//! sweeps every part of the tool (a cylinder of the part's radius from its lowest to its highest
//! point above the tip) into a solid: a stadium prism, or a cylinder for a run along the tool
//! axis. A swept solid whose exact box meets a body's exact box is intersected with that body
//! (SMLib's boolean); any shared volume is a collision. No mesh decides anything.
//!
//! Against the part, the flutes cut on purpose: only the shank above them and the holder are
//! swept at feed and along the tool axis (where the cutter goes in and out of what it cut), and
//! the whole tool at a rapid across the axis. Each is swept [`SKIM`] thinner and that much
//! higher, so a shank running along a wall its own flutes cut, or a tip resting on a floor, is
//! not a collision. Against a fixture every part is swept at every move, [`FIXTURE_GAP`] wider.

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_cam::{Tool, Toolpath};
use solidcraft_doc::ModelBody;
use solidcraft_doc::cam::CamSetup;
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, BoolOp};

use super::cam::{WorkFrame, work_frame};
use crate::Session;

/// How far a run's points may stand off the straight line from its first to its last (mm). The
/// swept solid is widened by the most any of them does, so it holds the whole run.
const STRAIGHT: f64 = 0.02;
/// How far a run that moves across the tool axis may climb or fall (mm); the swept solid spans
/// the run's whole height.
const LEVEL: f64 = 0.25;
/// How far a shank or holder may skim the part without it counting (mm).
const SKIM: f64 = 0.02;
/// The room every part of the tool keeps from a fixture (mm).
pub(super) const FIXTURE_GAP: f64 = 0.5;
/// Most swept solids intersected with bodies for one step.
const MAX_TESTS: usize = 20_000;
/// Most points of one step's path.
const MAX_POINTS: usize = 2_000_000;

/// Which part of the tool would hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CamToolPart {
    /// The cutting part, at a rapid move (or anywhere near a fixture).
    Cutter,
    Shank,
    Holder,
}

/// A part of the tool meeting a body.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CamCollision {
    pub part: CamToolPart,
    /// The body it meets.
    pub body: String,
    /// The body is one of the setup's fixtures (else part of the model).
    pub fixture: bool,
    /// Whether the tool was moving at rapid.
    pub rapid: bool,
    /// The index of the path point the run that hits ends at.
    pub point: usize,
    /// Where, in model coordinates: the middle of the shared volume.
    pub at: Vec3,
}

/// A body the tool must keep clear of, with its box in work coordinates.
struct Obstacle<'a> {
    name: &'a str,
    body: &'a Body,
    fixture: bool,
    lo: Vec3,
    hi: Vec3,
}

/// A cylinder above the tool tip: the cutter, the shank or the holder.
#[derive(Clone, Copy)]
struct Piece {
    part: CamToolPart,
    radius: f64,
    from: f64,
    to: f64,
}

/// The tool's parts, lowest first.
fn pieces(tool: &Tool) -> Vec<Piece> {
    // A stick-out shorter than the flutes holds part of them in the holder.
    let reach = tool.reach();
    let mut out = vec![Piece { part: CamToolPart::Cutter, radius: tool.radius(), from: 0.0, to: tool.flute_length.min(reach) }];
    if reach > tool.flute_length + 1e-6 {
        let radius = tool.shank_diameter.map_or(tool.radius(), |d| d * 0.5);
        out.push(Piece { part: CamToolPart::Shank, radius, from: tool.flute_length, to: reach });
    }
    if let Some(h) = &tool.holder
        && h.diameter > 0.0
        && h.length > 0.0
    {
        out.push(Piece { part: CamToolPart::Holder, radius: h.diameter * 0.5, from: reach, to: reach + h.length });
    }
    out
}

/// A run of the path: from point `first` to point `last` (work coordinates).
struct Run {
    a: Vec3,
    b: Vec3,
    /// The most a point of the run stands off the line `a`–`b` across the tool axis.
    off: f64,
    zlo: f64,
    zhi: f64,
    rapid: bool,
    last: usize,
}

/// The path's runs: points reached the same way (rapid or feed), straight to within
/// [`STRAIGHT`], and level to within [`LEVEL`] unless along the tool axis.
fn runs(points: &[(Vec3, bool)]) -> Vec<Run> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < points.len() {
        let Some(&(a, _)) = points.get(i) else { break };
        let Some(&(_, cutting)) = points.get(i + 1) else { break };
        let mut j = i + 1;
        let mut best = (j, 0.0);
        while let Some(&(b, c)) = points.get(j) {
            if c != cutting {
                break;
            }
            let (zlo, zhi) = points[i..=j].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (p, _)| (lo.min(p.z), hi.max(p.z)));
            let (a2, b2) = (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y));
            let off = points[i..=j].iter().map(|(p, _)| off_segment(Vec2::new(p.x, p.y), a2, b2)).fold(0.0, f64::max);
            let across = points[i..=j].iter().map(|(p, _)| Vec2::new(p.x, p.y).dist(a2)).fold(0.0, f64::max);
            if off > STRAIGHT || (across > STRAIGHT && zhi - zlo > LEVEL) {
                break;
            }
            best = (j, off);
            j += 1;
        }
        let (end, off) = best;
        let (zlo, zhi) = points[i..=end].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (p, _)| (lo.min(p.z), hi.max(p.z)));
        if let Some(&(b, _)) = points.get(end) {
            out.push(Run { a, b, off, zlo, zhi, rapid: !cutting, last: end });
        }
        i = end;
    }
    out
}

/// The distance from `p` to the segment `a`–`b`.
fn off_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let len2 = d.dot(d);
    if len2 <= 1e-24 {
        return p.dist(a);
    }
    let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
    p.dist(a + d * t)
}

/// The solid one piece sweeps along a run, in model coordinates: `radius` round the line `a`–`b`
/// (work XY), from height `zlo` to `zhi` along the tool axis.
fn swept(frame: &WorkFrame, a: Vec2, b: Vec2, radius: f64, zlo: f64, zhi: f64) -> solidcraft_kernel::Result<Body> {
    let plane = Plane { origin: frame.o, x: frame.x, y: frame.y };
    let d = b - a;
    let len = d.len();
    // A seam at an odd angle, as the kernel's own cylinders have it: booleans do not survive a
    // seam lying exactly on a face placed on a grid.
    let outer = if len <= 1e-6 {
        Loop2::circle_from(a, radius, SEAM)
    } else {
        let u = d * (1.0 / len);
        let n = Vec2::new(-u.y, u.x);
        let right = Vec2::new(u.y, -u.x).angle();
        Loop2 {
            segs: vec![
                Seg2::Line { a: a - n * radius, b: b - n * radius },
                Seg2::Arc { center: b, radius, start: right, sweep: std::f64::consts::PI },
                Seg2::Line { a: b + n * radius, b: a + n * radius },
                Seg2::Arc { center: a, radius, start: right + std::f64::consts::PI, sweep: std::f64::consts::PI },
            ],
        }
    };
    let mut bodies = solidcraft_kernel::extrude(&plane, &[Region2 { outer, holes: Vec::new() }], zlo, zhi)?;
    bodies.pop().ok_or_else(|| solidcraft_kernel::KernelError::Failed("the swept tool".into()))
}

/// An odd seam angle for swept circles (radians), as `kernel::cylinder` uses.
const SEAM: f64 = 0.6131;

/// The obstacles of `setup`: its fixtures, and its model bodies (none named: every body but
/// the fixtures), each with its exact box in work coordinates.
fn obstacles<'a>(bodies: &'a [ModelBody], setup: &CamSetup, frame: &WorkFrame) -> std::result::Result<Vec<Obstacle<'a>>, String> {
    let mut out = Vec::new();
    for fixture in &setup.fixtures {
        if !bodies.iter().any(|b| &b.name == fixture) {
            return Err(format!("fixture `{fixture}` is not a body of the design"));
        }
    }
    for body in bodies {
        let fixture = setup.fixtures.contains(&body.name);
        if !fixture && !setup.model_bodies.is_empty() && !setup.model_bodies.contains(&body.name) {
            continue;
        }
        let b = body.body.exact_bounds().map_err(|e| format!("body `{}`: {e}", body.name))?;
        if !(b.min.x <= b.max.x && b.min.y <= b.max.y && b.min.z <= b.max.z) {
            continue;
        }
        let (mut lo, mut hi) =
            (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            );
            let w = frame.local(corner);
            (lo, hi) = (Vec3::new(lo.x.min(w.x), lo.y.min(w.y), lo.z.min(w.z)), Vec3::new(hi.x.max(w.x), hi.y.max(w.y), hi.z.max(w.z)));
        }
        let pad = 2.0 * body.body.record().tolerance;
        out.push(Obstacle { name: &body.name, body: &body.body, fixture, lo: lo - Vec3::new(pad, pad, pad), hi: hi + Vec3::new(pad, pad, pad) });
    }
    Ok(out)
}

/// The first place where `tool`, following `path` in `setup`, meets the part with its shank or
/// holder (with any part at rapid) or comes within [`FIXTURE_GAP`] of a fixture; `None` when it
/// stays clear. An error when a check could not be made: nothing is said to be clear unchecked.
pub(super) fn first_collision(s: &Session, setup: &CamSetup, tool: &Tool, path: &Toolpath) -> std::result::Result<Option<CamCollision>, String> {
    let frame = work_frame(setup)?;
    let state = s.model.state();
    let obstacles = obstacles(&state.bodies, setup, &frame)?;
    if obstacles.is_empty() {
        return Ok(None);
    }
    let points = path.checked_points(STRAIGHT, MAX_POINTS).map_err(|e| e.to_string())?;
    let pieces = pieces(tool);
    let mut tests = 0usize;
    for run in runs(&points) {
        // Straight in or out along the tool axis the cutter moves through what it cut itself.
        let along_axis = Vec2::new(run.a.x, run.a.y).dist(Vec2::new(run.b.x, run.b.y)) <= STRAIGHT && run.off <= STRAIGHT;
        for piece in &pieces {
            for o in &obstacles {
                // Against the part the flutes cut at feed; the rest skims by SKIM.
                let (grow, lift) = if o.fixture {
                    (FIXTURE_GAP, -FIXTURE_GAP)
                } else if piece.part != CamToolPart::Cutter || (run.rapid && !along_axis) {
                    (-SKIM, SKIM)
                } else {
                    continue;
                };
                let radius = piece.radius + run.off + grow;
                let (zlo, zhi) = (run.zlo + piece.from + lift, run.zhi + piece.to + if o.fixture { FIXTURE_GAP } else { 0.0 });
                if !(radius > 1e-3) || !(zhi > zlo) {
                    continue;
                }
                let (lo, hi) = (
                    Vec3::new(run.a.x.min(run.b.x) - radius, run.a.y.min(run.b.y) - radius, zlo),
                    Vec3::new(run.a.x.max(run.b.x) + radius, run.a.y.max(run.b.y) + radius, zhi),
                );
                if lo.x >= o.hi.x || hi.x <= o.lo.x || lo.y >= o.hi.y || hi.y <= o.lo.y || lo.z >= o.hi.z || hi.z <= o.lo.z {
                    continue;
                }
                tests += 1;
                if tests > MAX_TESTS {
                    return Err(format!("more than {MAX_TESTS} moves pass near the part or a fixture to check them for collisions; split the step"));
                }
                let (a, b) = (Vec2::new(run.a.x, run.a.y), Vec2::new(run.b.x, run.b.y));
                let sweep = swept(&frame, a, b, radius, zlo, zhi).map_err(|e| format!("the tool's sweep could not be built to check it: {e}"))?;
                let shared = solidcraft_kernel::boolean(o.body, &sweep, BoolOp::Intersect)
                    .map_err(|e| format!("the tool's sweep could not be checked against `{}`: {e}", o.name))?;
                if let Some(shared) = shared {
                    let at = shared.exact_bounds().map(|bb| (bb.min + bb.max) * 0.5).unwrap_or_else(|_| frame.model(run.b));
                    return Ok(Some(CamCollision {
                        part: piece.part,
                        body: o.name.to_string(),
                        fixture: o.fixture,
                        rapid: run.rapid,
                        point: run.last,
                        at,
                    }));
                }
            }
        }
    }
    Ok(None)
}

/// What a collision says, for an error or a warning.
pub(super) fn describe(c: &CamCollision) -> String {
    let what = match c.part {
        CamToolPart::Cutter if c.rapid => "the tool, moving at rapid,",
        CamToolPart::Cutter => "the tool",
        CamToolPart::Shank => "the tool's shank",
        CamToolPart::Holder => "the tool holder",
    };
    let (verb, kind) = if c.fixture { ("comes within 0.5 mm of", "fixture") } else { ("runs into", "body") };
    format!("{what} {verb} {kind} `{}` (at {:.1}, {:.1}, {:.1})", c.body, c.at.x, c.at.y, c.at.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_join_straight_points_and_split_at_bends_and_rapids() {
        let p = |x: f64, y: f64, z: f64, cut: bool| (Vec3::new(x, y, z), cut);
        let points = [p(0.0, 0.0, 5.0, false), p(0.0, 0.0, -2.0, false), p(10.0, 0.0, -2.0, true), p(20.0, 0.0, -2.0, true), p(20.0, 10.0, -2.0, true)];
        let r = runs(&points);
        assert_eq!(r.len(), 3, "a plunge, the straight cut along X and the turn into Y");
        assert!(r[0].rapid && !r[1].rapid);
        assert_eq!((r[1].a, r[1].b), (Vec3::new(0.0, 0.0, -2.0), Vec3::new(20.0, 0.0, -2.0)));
        assert_eq!(r[2].last, 4);
    }
}
