//! Trochoidal milling strategies and corner peeling for low tool engagement.
//!
//! Conventional slotting cuts and sharp inside corners cause tool engagement (the radial
//! contact angle) to spike up to 180°, causing excessive cutting forces, tool deflection,
//! and chatter. Trochoidal milling maintains a low, bounded radial depth of cut by advancing
//! along the path in circular forward arcs (trochoids) with stepover $p \le s$, paired with
//! return moves through previously cleared space.
//!
//! Corner peeling applies this principle to inside corners and cusp spurs: rather than plunging
//! or slotting straight into a sharp corner apex, the tool advances in progressive slices,
//! sweeping trochoidal arcs wherever transverse width permits.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::offset::Shape;
use crate::open::OpenPath;
use crate::ops::{Builder, Cut, mills};
use crate::path::Toolpath;
use crate::piece::Piece;
use crate::tool::Tool;
use crate::{Result, invalid};

/// Largest allowed channel width or coordinate.
const MAX_CHANNEL_WIDTH: f64 = 1.0e4;

/// Parameters for trochoidal channel milling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrochoidParams {
    /// Desired width of the channel (mm). Must be greater than or equal to the tool diameter.
    pub width: f64,
    /// Forward advance per trochoidal loop (mm), > 0 and at most the tool diameter.
    pub stepover: f64,
    /// Climb milling if true, conventional if false.
    pub climb: bool,
    /// Material left on both walls for a subsequent finish pass (mm, >= 0).
    pub stock_to_leave: f64,
    /// Chord error for spline/conic segments (mm).
    pub tolerance: f64,
}

impl TrochoidParams {
    /// Validates the parameters against the cutting tool.
    pub fn validate(&self, tool: &Tool) -> Result<()> {
        if !self.width.is_finite() || self.width < tool.diameter - 1e-9 || self.width > MAX_CHANNEL_WIDTH {
            return invalid("the channel width must be at least the tool diameter and within 10000 mm");
        }
        if !self.stepover.is_finite() || !(self.stepover > 0.0) || self.stepover > tool.diameter + 1e-9 {
            return invalid("the stepover must be above 0 and at most the tool diameter");
        }
        if !self.stock_to_leave.is_finite() || !(0.0..=MAX_CHANNEL_WIDTH).contains(&self.stock_to_leave) {
            return invalid("the stock to leave must be 0 or more");
        }
        if !self.tolerance.is_finite() || !(self.tolerance > 0.0) {
            return invalid("the tolerance must be above 0");
        }
        let wall = (self.width * 0.5) - tool.radius() - self.stock_to_leave;
        if wall < -1e-9 {
            return invalid("stock to leave is too large for the requested channel width");
        }
        Ok(())
    }
}

/// A single trochoidal forward cutting arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrochoidArc {
    /// Start of the cutting arc.
    pub start: Vec2,
    /// Center of curvature.
    pub center: Vec2,
    /// Radius of curvature.
    pub radius: f64,
    /// End of the cutting arc.
    pub end: Vec2,
    /// Counter-clockwise if true, clockwise if false.
    pub ccw: bool,
}

/// Computes the public trochoidal forward arc description.
pub fn trochoid_arc_calc(pos: Vec2, dir: Vec2, normal: Vec2, pitch: f64, r_lat: f64, climb: bool) -> Option<TrochoidArc> {
    if pitch <= 1e-9 || r_lat <= 1e-9 {
        return None;
    }
    let xc = pitch * 0.25 - (r_lat * r_lat) / pitch;
    let rc = pitch * 0.25 + (r_lat * r_lat) / pitch;
    let center = pos + dir * xc;

    let (start, end, ccw) =
        if climb { (pos + normal * r_lat, pos - normal * r_lat, false) } else { (pos - normal * r_lat, pos + normal * r_lat, true) };

    Some(TrochoidArc { start, center, radius: rc, end, ccw })
}

/// Computes the circular forward cutting arc piece for a trochoidal loop.
pub(crate) fn trochoid_arc_piece(pos: Vec2, dir: Vec2, normal: Vec2, pitch: f64, r_lat: f64, climb: bool) -> Option<Piece> {
    let arc = trochoid_arc_calc(pos, dir, normal, pitch, r_lat, climb)?;
    let v_start = arc.start - arc.center;
    let a0 = v_start.angle();
    let xc = pitch * 0.25 - (r_lat * r_lat) / pitch;
    let sweep_mag = 2.0 * r_lat.atan2(-xc);
    let sweep = if arc.ccw { sweep_mag } else { -sweep_mag };

    Some(Piece::Arc { c: arc.center, r: arc.radius, a0, sweep })
}

/// Peels a sharp corner or cusp spur from `corner` to `target` using progressive, low-engagement passes.
///
/// Rather than taking a heavy, full-width slotting cut straight into the corner apex,
/// this advances in steps of at most `stepover` (or the tool radius). At each step, if lateral
/// space allows without gouging the pocket walls, a circular trochoidal arc is cut across the
/// corner front. In tight corners, progressive slices advance and step back along already cleared
/// ground.
pub(crate) fn peel_corner(
    b: &mut Builder,
    shape: &Shape,
    corner: Vec2,
    target: Vec2,
    stepover: f64,
    tool_r: f64,
    wall: f64,
    z: f64,
    climb: bool,
) -> Result<()> {
    let span = corner.dist(target);
    if span <= 1e-6 {
        return Ok(());
    }
    let u = match (target - corner).normalized() {
        Some(dir) => dir,
        None => return Ok(()),
    };
    let n = Vec2::new(-u.y, u.x);
    let max_pitch = (stepover.min(tool_r)).max(1e-3);
    let steps = ((span / max_pitch).ceil()).clamp(1.0, 500.0) as usize;
    let step_len = span / steps as f64;

    for i in 1..=steps {
        let d = step_len * i as f64;
        let p_front = corner + u * d;

        // Check lateral room at this depth without touching pocket walls.
        let max_lat = (stepover * 0.5).min(tool_r * 0.5);
        let mut lat = 0.0;
        for frac in [1.0, 0.75, 0.5, 0.25] {
            let test_lat = max_lat * frac;
            if shape.clear(p_front + n * test_lat, wall) && shape.clear(p_front - n * test_lat, wall) {
                lat = test_lat;
                break;
            }
        }

        if lat > 0.05 {
            // Cut a trochoidal arc across the front.
            let (start_pt, end_pt, sweep_sign) =
                if climb { (p_front + n * lat, p_front - n * lat, -1.0) } else { (p_front - n * lat, p_front + n * lat, 1.0) };

            // Feed to arc start safely: through p_front or direct if clear.
            if shape.clear(start_pt, wall) {
                b.cut_to(p_front, z)?;
                b.cut_to(start_pt, z)?;
            } else {
                b.cut_to(p_front, z)?;
            }

            let p_eff = step_len.min(lat * 2.0).max(1e-4);
            let xc = p_eff * 0.25 - (lat * lat) / p_eff;
            let rc = p_eff * 0.25 + (lat * lat) / p_eff;
            let c = p_front + u * xc;
            let v_s = start_pt - c;
            let a0 = v_s.angle();
            let sweep_mag = 2.0 * lat.atan2(-xc);
            let arc = Piece::Arc { c, r: rc, a0, sweep: sweep_sign * sweep_mag };
            b.cut_piece(&arc, z, z)?;

            // Return to corner along cleared path.
            b.cut_to(end_pt, z)?;
            b.cut_to(p_front, z)?;
            b.cut_to(corner, z)?;
        } else {
            // Narrow corner: incremental plunge along bisector & return.
            b.cut_to(p_front, z)?;
            b.cut_to(corner, z)?;
        }
    }

    Ok(())
}

/// Mills a slot or channel along `paths` using low tool engagement trochoidal loops.
///
/// For each depth pass, the tool moves forward in overlapping circular arcs that sweep across
/// the channel width, cutting a chip of thickness no greater than `params.stepover`.
/// Return links stay in previously cleared material.
pub fn trochoid_channel(paths: &[OpenPath], tool: &Tool, params: &TrochoidParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    params.validate(tool)?;

    if paths.is_empty() {
        return invalid("trochoidal channel needs at least one path");
    }

    let r = tool.radius();
    let r_lat = (params.width * 0.5) - r - params.stock_to_leave;
    let levels = cut.levels()?;

    let mut b = Builder::new(cut);

    for z in levels {
        for open_path in paths {
            let pieces = open_path.pieces(params.tolerance, true)?;
            for piece in pieces {
                let total_len = piece.length();
                if total_len <= 1e-9 {
                    continue;
                }

                let start = piece.start();
                b.travel_to(start)?;
                b.plunge_to(z)?;

                if r_lat <= 1e-4 {
                    // Channel is the same width as the tool: direct single cut.
                    b.cut_piece(&piece, z, z)?;
                } else {
                    let pitch = params.stepover.min(total_len).max(1e-3);
                    let count = ((total_len / pitch).ceil()).clamp(1.0, 5000.0) as usize;
                    let dt = 1.0 / count as f64;

                    for k in 0..count {
                        let t0 = k as f64 * dt;
                        let p0 = piece.point_at(t0);
                        let dir = piece.tangent_at(t0);
                        let norm = Vec2::new(-dir.y, dir.x);

                        if let Some(arc) = trochoid_arc_piece(p0, dir, norm, pitch, r_lat, params.climb) {
                            b.cut_to(arc.start(), z)?;
                            b.cut_piece(&arc, z, z)?;
                            // Return move across cleared space.
                            let p_ret = p0 - dir * (pitch * 0.25);
                            b.cut_to(p_ret, z)?;
                        } else {
                            let p1 = piece.point_at((k + 1) as f64 * dt);
                            b.cut_to(p1, z)?;
                        }
                    }

                    let p_end = piece.end();
                    b.cut_to(p_end, z)?;
                }

                b.retract()?;
            }
        }
    }

    let path = Toolpath { moves: b.into_moves(), cycles: Vec::new() };
    verify(&path, &cut.heights)?;
    Ok(path)
}
