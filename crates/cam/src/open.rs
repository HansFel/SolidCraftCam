//! Open contours: chains of segments that do not close. The tool runs along one side of the
//! chain, the chain extended past its ends; leads then bring it on and take it off.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Seg2, Vec2};

use crate::offset::MAX_PIECES;
use crate::ops::MAX_COORD;
use crate::piece::{Piece, turn};
use crate::{CamError, Result, invalid};

/// How far apart consecutive segments may start and end (mm).
const JOIN_TOL: f64 = 1e-6;

/// Which side of an open path the tool runs on, seen along the direction it cuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSide {
    Left,
    Right,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenContourParams {
    pub side: OpenSide,
    /// Cut the path from its end to its start.
    pub reverse: bool,
    /// Wall stock (mm, >= 0), as [`crate::ContourParams`].
    pub stock_to_leave: f64,
    /// Chord error for spline/conic segments (mm), as [`crate::ContourParams`].
    pub tolerance: f64,
    /// The path extended along its tangent before its start and after its end (mm, >= 0), both
    /// measured in the cutting direction (after `reverse`).
    pub extend_start: f64,
    pub extend_end: f64,
}

impl OpenContourParams {
    pub fn validate(&self) -> Result<()> {
        for (v, what) in
            [(self.stock_to_leave, "the stock to leave"), (self.extend_start, "the start extension"), (self.extend_end, "the end extension")]
        {
            if !v.is_finite() || !(0.0..=MAX_COORD).contains(&v) {
                return invalid(format!("{what} must be 0 or more"));
            }
        }
        if !(self.tolerance > 0.0) || !self.tolerance.is_finite() {
            return invalid("the tolerance must be above 0");
        }
        Ok(())
    }
}

/// An open path in work XY: segments joined end to end, in their given order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenPath {
    pub segs: Vec<Seg2>,
}

impl OpenPath {
    /// The path as lines and arcs (its other curves as lines within `tol`), checked to run from
    /// segment to segment without closing.
    fn pieces(&self, tol: f64) -> Result<Vec<Piece>> {
        let mut out = Vec::new();
        for seg in &self.segs {
            match *seg {
                Seg2::Line { a, b } => {
                    if a.dist(b) > 1e-9 {
                        out.push(Piece::Line { a, b });
                    }
                }
                Seg2::Arc { center, radius, start, sweep } => {
                    if radius > 1e-9 && sweep.abs() > 1e-12 {
                        out.push(Piece::Arc { c: center, r: radius, a0: start, sweep });
                    }
                }
                _ => {
                    for w in seg.polyline(tol).windows(2) {
                        if let [a, b] = *w
                            && a.dist(b) > 1e-9
                        {
                            out.push(Piece::Line { a, b });
                        }
                    }
                }
            }
            if out.len() > MAX_PIECES {
                return Err(CamError::TooLarge(format!("the open path has more than {MAX_PIECES} segments")));
            }
        }
        if out.is_empty() {
            return invalid("the open path has no segments");
        }
        if !out.iter().all(Piece::is_finite) {
            return invalid("the open path has a coordinate that is not a finite number");
        }
        for piece in &out {
            let (lo, hi) = piece.bounds();
            if [lo.x, lo.y, hi.x, hi.y].iter().any(|c| c.abs() > MAX_COORD) {
                return invalid("the open path must stay within 1e6 mm");
            }
        }
        if out.iter().zip(out.iter().skip(1)).any(|(a, b)| a.end().dist(b.start()) > JOIN_TOL) {
            return invalid("the open path's segments do not join end to end");
        }
        let (first, last) = (out.first().map(Piece::start), out.last().map(Piece::end));
        if let (Some(a), Some(b)) = (first, last)
            && a.dist(b) <= JOIN_TOL
        {
            return invalid("the open path closes: machine it as a contour of regions");
        }
        Ok(out)
    }
}

/// The unit normal of the side the tool runs on, square to the travel `t`.
pub(crate) fn side_normal(side: OpenSide, t: Vec2) -> Vec2 {
    match side {
        OpenSide::Right => -t.perp(),
        OpenSide::Left | OpenSide::On => t.perp(),
    }
}

/// The tool-centre path along `path`, offset by the tool's radius (+ stock) to the side the
/// tool runs on, extended at both ends. Arcs stay arcs; corners opening away from the tool side
/// get an arc round them, corners closing towards it are trimmed to their crossing.
pub(crate) fn offset_path(path: &OpenPath, params: &OpenContourParams, radius: f64) -> Result<Vec<Piece>> {
    params.validate()?;
    let mut pieces = path.pieces(params.tolerance)?;
    if params.reverse {
        pieces = pieces.iter().rev().map(Piece::reversed).collect();
    }
    let away = match params.side {
        OpenSide::On => 0.0,
        OpenSide::Left => radius + params.stock_to_leave,
        OpenSide::Right => -(radius + params.stock_to_leave),
    };
    let mut out = if away.abs() > 1e-9 { offset_pieces(&pieces, away)? } else { pieces };
    extend(&mut out, params.extend_start, params.extend_end)?;
    check_self(&out)?;
    Ok(out)
}

fn offset_pieces(pieces: &[Piece], d: f64) -> Result<Vec<Piece>> {
    let mut off = Vec::with_capacity(pieces.len());
    for piece in pieces {
        off.push(match *piece {
            Piece::Line { a, b } => {
                let n = piece.tangent_at(0.0).perp() * d;
                Piece::Line { a: a + n, b: b + n }
            }
            Piece::Arc { c, r, a0, sweep } => {
                // Round a counter-clockwise arc the centre is on the left of the travel.
                let r = r - d * sweep.signum();
                if !(r > 1e-9) {
                    return invalid("the tool does not fit the open contour");
                }
                Piece::Arc { c, r, a0, sweep }
            }
        });
    }
    // Corners, read off the pieces as they came: an arc round the vertex where the two sides
    // open away from each other, the crossing where they close.
    let mut arcs: Vec<Option<Piece>> = vec![None; pieces.len()];
    let mut starts: Vec<Option<f64>> = vec![None; pieces.len()];
    let mut ends: Vec<Option<f64>> = vec![None; pieces.len()];
    for (i, (piece, next)) in pieces.iter().zip(pieces.iter().skip(1)).enumerate() {
        let bend = turn(piece.tangent_at(1.0), next.tangent_at(0.0));
        if bend.abs() <= 1e-12 {
            continue;
        }
        if bend * d < 0.0 {
            let (end, v) = (off.get(i).map(Piece::end), piece.end());
            if let Some(end) = end {
                arcs[i] = Some(Piece::Arc { c: v, r: d.abs(), a0: (end - v).angle(), sweep: bend });
            }
        } else {
            let (Some(a), Some(b)) = (off.get(i), off.get(i + 1)) else { continue };
            match a.intersect_extended(b) {
                Some((t, u)) if t.is_finite() && u.is_finite() => {
                    ends[i] = Some(t);
                    starts[i + 1] = Some(u);
                }
                _ => return invalid("the tool does not fit the open contour"),
            }
        }
    }
    let mut out = Vec::with_capacity(off.len() * 2);
    for (i, piece) in off.iter().enumerate() {
        let t0 = starts.get(i).copied().flatten().unwrap_or(0.0);
        let t1 = ends.get(i).copied().flatten().unwrap_or(1.0);
        if !(t1 > t0) {
            return invalid("the tool does not fit the open contour");
        }
        out.push(piece.sub(t0, t1));
        if let Some(Some(arc)) = arcs.get(i) {
            out.push(*arc);
        }
    }
    if out.iter().any(|p| p.length() <= 1e-9) {
        return invalid("the tool does not fit the open contour");
    }
    Ok(out)
}

/// The first piece's start back by `start` and the last piece's end on by `end`, along their
/// tangents (an arc's sweep grows).
fn extend(pieces: &mut [Piece], start: f64, end: f64) -> Result<()> {
    let count = pieces.len();
    if start > 0.0
        && let Some(first) = pieces.first_mut()
    {
        *first = extended(*first, start, true)?;
    }
    if end > 0.0
        && let Some(last) = pieces.get_mut(count.saturating_sub(1))
    {
        *last = extended(*last, end, false)?;
    }
    Ok(())
}

fn extended(piece: Piece, by: f64, at_start: bool) -> Result<Piece> {
    match piece {
        Piece::Line { a, b } => {
            if at_start {
                let t = piece.tangent_at(0.0);
                Ok(Piece::Line { a: a - t * by, b })
            } else {
                let t = piece.tangent_at(1.0);
                Ok(Piece::Line { a, b: b + t * by })
            }
        }
        Piece::Arc { c, r, a0, sweep } => {
            let extra = by / r;
            let sign = sweep.signum();
            if at_start {
                Ok(Piece::Arc { c, r, a0: a0 - sign * extra, sweep: sweep + sign * extra })
            } else {
                Ok(Piece::Arc { c, r, a0, sweep: sweep + sign * extra })
            }
        }
    }
}

/// Refuses a path that runs into itself.
fn check_self(pieces: &[Piece]) -> Result<()> {
    for (i, a) in pieces.iter().enumerate() {
        let (alo, ahi) = a.bounds();
        for b in pieces.iter().skip(i + 2) {
            let (blo, bhi) = b.bounds();
            if alo.x > bhi.x + 1e-9 || blo.x > ahi.x + 1e-9 || alo.y > bhi.y + 1e-9 || blo.y > ahi.y + 1e-9 {
                continue;
            }
            let mut hits = Vec::new();
            a.intersections(b, &mut hits);
            if !hits.is_empty() {
                return invalid("the tool does not fit the open contour");
            }
        }
    }
    Ok(())
}
