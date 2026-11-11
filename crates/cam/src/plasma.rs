//! Plasma cutting: a contour cut once along its loops, turned into torch cuts.
//!
//! A torch cuts through the sheet from above it: it never goes into the material. Each cut is
//! one stretch of the contour's feed moves (lead-in, loop, overlap, lead-out). The torch comes
//! down at rapid to its pierce height, pierces there for the pierce delay, feeds down to its cut
//! height and cuts the whole stretch at that height. Between cuts it goes up to the travel
//! height. A post writes the torch on before each pierce and off before each rapid
//! ([`crate::Tool`] of kind [`crate::ToolKind::Plasma`]), so the moves stay plain moves.
//!
//! Cuts inside other cuts come first (the holes before the outline round them), so a part is
//! never cut free before its holes are. A closed cut no wider than `slow_holes` is cut at a share
//! of the feed, as small holes need.

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::path::{Move, Toolpath};
use crate::tool::{Tool, ToolKind};
use crate::{Result, invalid};

/// How a torch cuts, in the work frame (mm, seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlasmaParams {
    /// The sheet's top face.
    pub top: f64,
    /// Where the torch goes between cuts, at least its pierce height above the top.
    pub travel: f64,
    /// The first and last height of the program's moves.
    pub clearance: f64,
    /// The feed from the pierce height down to the cut height.
    pub plunge_feed: f64,
    /// Closed cuts at most this wide (mm) are cut at `slow_feed` times the feed; `None`: all at the feed.
    pub slow_holes: Option<(f64, f64)>,
}

/// One stretch of feed moves: where the torch pierces and what it cuts from there.
struct Stretch {
    start: Vec2,
    moves: Vec<Move>,
    lo: Vec2,
    hi: Vec2,
    closed: bool,
}

/// The torch cuts of `path` (a contour cut in one pass, its moves in work coordinates).
pub fn plasma(path: &Toolpath, tool: &Tool, p: &PlasmaParams) -> Result<Toolpath> {
    if tool.kind != ToolKind::Plasma {
        return invalid("a plasma cut needs a plasma torch");
    }
    tool.validate()?;
    let (Some(pierce), Some(cut), Some(delay)) = (tool.pierce_height, tool.cut_height, tool.pierce_delay) else {
        return invalid("a plasma torch needs its pierce height, cut height and pierce delay");
    };
    let all = [p.top, p.travel, p.clearance, p.plunge_feed];
    if !all.iter().all(|v| v.is_finite() && v.abs() <= 1.0e6) || !(p.plunge_feed > 0.0) {
        return invalid("the plasma cut's heights and plunge feed must be finite, the feed above 0");
    }
    let (pierce_z, cut_z) = (p.top + pierce, p.top + cut);
    let travel = p.travel.max(pierce_z);
    let clearance = p.clearance.max(travel);
    if let Some((width, share)) = p.slow_holes
        && (!(width > 0.0 && width <= 1.0e4) || !(share > 0.0 && share <= 1.0))
    {
        return invalid("small holes are wider than 0 mm and cut at a share of the feed above 0 and at most 1");
    }
    let mut stretches = stretches(path);
    if stretches.is_empty() {
        return invalid("there is nothing for the torch to cut");
    }
    // Inside-out: a cut whose box lies inside more other cuts' boxes comes first.
    let depth = |i: usize| {
        let Some(a) = stretches.get(i) else { return 0 };
        stretches
            .iter()
            .enumerate()
            .filter(|(j, b)| {
                *j != i
                    && b.closed
                    && b.lo.x <= a.lo.x
                    && b.lo.y <= a.lo.y
                    && b.hi.x >= a.hi.x
                    && b.hi.y >= a.hi.y
                    && (b.hi - b.lo).len() > (a.hi - a.lo).len()
            })
            .count()
    };
    let depths: Vec<usize> = (0..stretches.len()).map(depth).collect();
    let mut order: Vec<usize> = (0..stretches.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(depths.get(i).copied().unwrap_or(0)));
    let mut sorted = Vec::with_capacity(stretches.len());
    for i in order {
        if let Some(s) = stretches.get_mut(i) {
            sorted.push(std::mem::replace(s, Stretch { start: Vec2::ZERO, moves: Vec::new(), lo: Vec2::ZERO, hi: Vec2::ZERO, closed: false }));
        }
    }
    let mut out = Vec::new();
    let mut first = true;
    for s in sorted {
        let at = |z: f64| Vec3::new(s.start.x, s.start.y, z);
        out.push(Move::Rapid { to: at(if first { clearance } else { travel }) });
        first = false;
        out.push(Move::Rapid { to: at(pierce_z) });
        out.push(Move::Dwell { seconds: delay });
        if (pierce_z - cut_z).abs() > 1e-9 {
            out.push(Move::Feed { to: at(cut_z), feed: p.plunge_feed });
        }
        let slow = match p.slow_holes {
            Some((width, share)) if s.closed && (s.hi.x - s.lo.x).max(s.hi.y - s.lo.y) <= width => share,
            _ => 1.0,
        };
        let mut pos = at(cut_z);
        for m in s.moves {
            let moved = match m {
                Move::Feed { to, feed } => Move::Feed { to: Vec3::new(to.x, to.y, cut_z), feed: feed * slow },
                Move::Arc { to, center, ccw, feed } => Move::Arc { to: Vec3::new(to.x, to.y, cut_z), center, ccw, feed: feed * slow },
                other => other,
            };
            // A plunge or a ramp of the contour is no move at the cut height.
            if let Some(to) = moved.target() {
                if matches!(moved, Move::Feed { .. }) && to.dist(pos) <= 1e-9 {
                    continue;
                }
                pos = to;
            }
            out.push(moved);
        }
        out.push(Move::Rapid { to: Vec3::new(pos.x, pos.y, travel) });
    }
    if let Some(Move::Rapid { to }) = out.last().copied() {
        out.push(Move::Rapid { to: Vec3::new(to.x, to.y, clearance) });
    }
    Ok(Toolpath::new(out))
}

/// The stretches of feed moves of `path`, each with where it starts in XY and its box.
fn stretches(path: &Toolpath) -> Vec<Stretch> {
    let mut out: Vec<Stretch> = Vec::new();
    let mut pos: Option<Vec3> = None;
    let mut current: Option<Stretch> = None;
    let close = |s: Stretch, out: &mut Vec<Stretch>| {
        if s.moves.iter().any(|m| matches!(m, Move::Feed { .. } | Move::Arc { .. })) {
            out.push(s);
        }
    };
    for m in &path.moves {
        match m {
            Move::Rapid { to } => {
                if let Some(s) = current.take() {
                    close(s, &mut out);
                }
                pos = Some(*to);
            }
            Move::Feed { .. } | Move::Arc { .. } | Move::Dwell { .. } => {
                let s = current.get_or_insert_with(|| {
                    let p = pos.map_or(Vec2::ZERO, |p| Vec2::new(p.x, p.y));
                    Stretch { start: p, moves: Vec::new(), lo: p, hi: p, closed: false }
                });
                if let (Some(from), Move::Arc { to, center, .. }) = (pos, m) {
                    // An arc's box holds its whole circle: enough to tell inside from outside.
                    let r = Vec2::new(from.x, from.y).dist(*center);
                    s.lo = Vec2::new(s.lo.x.min(center.x - r), s.lo.y.min(center.y - r));
                    s.hi = Vec2::new(s.hi.x.max(center.x + r), s.hi.y.max(center.y + r));
                    let _ = to;
                }
                if let Some(to) = m.target() {
                    s.lo = Vec2::new(s.lo.x.min(to.x), s.lo.y.min(to.y));
                    s.hi = Vec2::new(s.hi.x.max(to.x), s.hi.y.max(to.y));
                    pos = Some(to);
                }
                s.moves.push(*m);
            }
        }
    }
    if let Some(s) = current.take() {
        close(s, &mut out);
    }
    // Closed: the cut ends near where it started (its lead-out may leave it a little).
    for s in &mut out {
        let end = s.moves.iter().rev().find_map(Move::target).map(|p| Vec2::new(p.x, p.y)).unwrap_or(s.start);
        let size = (s.hi - s.lo).len();
        s.closed = size > 0.0 && end.dist(s.start) <= 0.25 * size;
    }
    out
}
