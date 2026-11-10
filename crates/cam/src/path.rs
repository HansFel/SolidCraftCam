//! Toolpaths: the moves of the tool tip's centre in work coordinates.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::piece::wrap;

/// Most moves in one toolpath.
pub const MAX_MOVES: usize = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Move {
    /// Positioning at the machine's rapid rate; never through material.
    Rapid { to: Vec3 },
    /// Straight cut at `feed` mm/min.
    Feed { to: Vec3, feed: f64 },
    /// Circular cut about `center` in XY (a helix when Z changes), always less than a full
    /// turn: an arc that ends where it starts is no move at all.
    Arc { to: Vec3, center: Vec2, ccw: bool, feed: f64 },
    /// Pause with the spindle running.
    Dwell { seconds: f64 },
}

impl Move {
    /// Where the tool is after the move (`None`: it stays).
    pub fn target(&self) -> Option<Vec3> {
        match *self {
            Move::Rapid { to } | Move::Feed { to, .. } | Move::Arc { to, .. } => Some(to),
            Move::Dwell { .. } => None,
        }
    }

    /// The feed of a cutting move (mm/min); `None` for a rapid or a dwell.
    pub fn feed(&self) -> Option<f64> {
        match *self {
            Move::Feed { feed, .. } | Move::Arc { feed, .. } => Some(feed),
            Move::Rapid { .. } | Move::Dwell { .. } => None,
        }
    }
}

/// Signed angle (positive = counter-clockwise) of an arc move from `from` to `to`; zero when
/// the two coincide in XY.
pub fn arc_sweep(from: Vec3, to: Vec3, center: Vec2, ccw: bool) -> f64 {
    let (a, b) = (Vec2::new(from.x, from.y) - center, Vec2::new(to.x, to.y) - center);
    if a.dist(b) <= 1e-9 {
        return 0.0;
    }
    let delta = wrap(b.angle() - a.angle());
    if ccw { delta } else { delta - TAU }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    pub moves: Vec<Move>,
    /// Runs of `moves` that drill one hole, so a post may write each as a canned cycle. The
    /// moves stay the truth: a post without canned cycles writes them as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycles: Vec<DrillCycle>,
    /// Runs of `moves` the control compensates for the tool's radius: the program carries
    /// [`CompRun::programmed`] instead, and the moves are what the control makes of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comp: Vec<CompRun>,
}

impl Toolpath {
    /// A toolpath of plain moves.
    pub fn new(moves: Vec<Move>) -> Toolpath {
        Toolpath { moves, cycles: Vec::new(), comp: Vec::new() }
    }

    /// `other`'s moves after this path's, its cycles and compensated runs kept with them.
    pub fn append(&mut self, other: Toolpath) {
        let shift = self.moves.len();
        self.moves.extend(other.moves);
        self.cycles.extend(other.cycles.into_iter().map(|c| DrillCycle { start: c.start + shift, end: c.end + shift, ..c }));
        self.comp.extend(other.comp.into_iter().map(|r| CompRun { start: r.start + shift, end: r.end + shift, ..r }));
    }

    /// The moves as the program says them: each compensated run's programmed moves in place of
    /// the tool centre's. The same as `moves` without compensation.
    pub fn programmed(&self) -> Vec<Move> {
        if self.comp.is_empty() {
            return self.moves.clone();
        }
        let mut out = Vec::with_capacity(self.moves.len());
        let mut index = 0;
        for run in &self.comp {
            if run.start < index || run.end > self.moves.len() || run.start > run.end {
                continue;
            }
            out.extend_from_slice(&self.moves[index..run.start]);
            out.extend_from_slice(&run.programmed);
            index = run.end;
        }
        out.extend_from_slice(&self.moves[index.min(self.moves.len())..]);
        out
    }
}

/// Which side of the programmed path the control puts the tool, seen along the travel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompSide {
    /// `G41`: the tool left of the path.
    Left,
    /// `G42`: the tool right of the path.
    Right,
}

/// One pass the control compensates: `moves[start..end]` are the tool centre's moves along the
/// entry line, the contour and the exit line; `programmed` is what the program says instead
/// (the contour itself, or the nominal centre path for wear compensation). The control switches
/// compensation on with the first programmed move (the entry line) and off with the last (the
/// exit line).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompRun {
    pub start: usize,
    pub end: usize,
    pub side: CompSide,
    /// The control's offset register the radius is read from (`D`), by default the tool number.
    pub register: u32,
    pub programmed: Vec<Move>,
}

/// One hole drilled by `moves[start..end]`: from the initial height above `at` down to the
/// retract height at rapid, to `bottom` at `feed` (in pecks of `peck`), and back up at rapid to
/// the initial height, as a canned cycle with `G98` does.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrillCycle {
    pub start: usize,
    pub end: usize,
    pub at: Vec2,
    pub retract: f64,
    pub bottom: f64,
    /// Depth of each peck (mm); `None`: one feed to the bottom.
    pub peck: Option<f64>,
    /// Pause at the bottom (seconds).
    pub dwell: f64,
    pub feed: f64,
    /// A tapped hole: the thread's pitch (mm). The tap feeds in and out at `feed`, one pitch a
    /// turn, the spindle reversing at the bottom, so only a rigid tapping cycle can write it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap: Option<f64>,
}

/// Lengths and time of a toolpath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathStats {
    /// Distance travelled at feed (mm).
    pub cut_length: f64,
    /// Distance travelled at rapid (mm).
    pub rapid_length: f64,
    /// Time at feed, including dwells (minutes).
    pub cut_minutes: f64,
    /// Lowest Z reached (mm); `None` for a path without moves.
    pub lowest_z: Option<f64>,
}

impl Toolpath {
    /// Bound allocation before flattening arcs from untrusted job parameters.
    pub fn checked_points(&self, tol: f64, max: usize) -> crate::Result<Vec<(Vec3, bool)>> {
        let mut count = 0usize;
        let mut pos: Option<Vec3> = None;
        for m in &self.moves {
            let extra = if let (Some(p), Move::Arc { to, center, ccw, .. }) = (pos, *m) {
                let start = Vec2::new(p.x, p.y) - center;
                let r = start.len();
                let tol = tol.max(1e-6);
                let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { TAU / 8.0 };
                ((arc_sweep(p, to, center, ccw).abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096)
            } else {
                usize::from(m.target().is_some())
            };
            count = count.saturating_add(extra);
            if count > max {
                return Err(crate::CamError::TooLarge(format!("toolpath preview exceeds {max} points")));
            }
            if let Some(p) = m.target() {
                pos = Some(p);
            }
        }
        Ok(self.points(tol))
    }

    pub fn stats(&self) -> PathStats {
        let mut s = PathStats::default();
        let mut pos: Option<Vec3> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid { to } => s.rapid_length += pos.map_or(0.0, |p| p.dist(to)),
                Move::Feed { to, feed } => {
                    let len = pos.map_or(0.0, |p| p.dist(to));
                    s.cut_length += len;
                    s.cut_minutes += if feed > 0.0 { len / feed } else { 0.0 };
                }
                Move::Arc { to, center, ccw, feed } => {
                    let len = pos.map_or(0.0, |p| {
                        let r = Vec2::new(p.x, p.y).dist(center);
                        (r * arc_sweep(p, to, center, ccw)).hypot(to.z - p.z)
                    });
                    s.cut_length += len;
                    s.cut_minutes += if feed > 0.0 { len / feed } else { 0.0 };
                }
                Move::Dwell { seconds } => s.cut_minutes += seconds.max(0.0) / 60.0,
            }
            if let Some(to) = m.target() {
                s.lowest_z = Some(s.lowest_z.map_or(to.z, |z: f64| z.min(to.z)));
                pos = Some(to);
            }
        }
        s
    }

    /// The path as points for display and checking: `(point, cutting)` where `cutting` says
    /// whether the tool reached the point at feed. Arcs are followed within `tol`.
    pub fn points(&self, tol: f64) -> Vec<(Vec3, bool)> {
        let mut out = Vec::new();
        let mut pos: Option<Vec3> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid { to } => out.push((to, false)),
                Move::Feed { to, .. } => out.push((to, true)),
                Move::Arc { to, center, ccw, .. } => {
                    if let Some(p) = pos {
                        let start = Vec2::new(p.x, p.y) - center;
                        let (r, sweep) = (start.len(), arc_sweep(p, to, center, ccw));
                        let tol = tol.max(1e-6);
                        let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { TAU / 8.0 };
                        let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
                        for i in 1..n {
                            let t = i as f64 / n as f64;
                            let q = center + Vec2::from_angle(start.angle() + sweep * t) * r;
                            out.push((q.extend(p.z + (to.z - p.z) * t), true));
                        }
                    }
                    out.push((to, true));
                }
                Move::Dwell { .. } => {}
            }
            if let Some(to) = m.target() {
                pos = Some(to);
            }
        }
        out
    }
}
