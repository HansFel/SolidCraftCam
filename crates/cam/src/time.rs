//! Machining time: how long a program runs on a machine that has to accelerate.
//!
//! Moves that join almost straight (within [`BLEND_ANGLE`]) at the same feed form one run, and
//! the tool accelerates once at its start and brakes once at its end. A sharper corner, a feed
//! change, a switch between rapid and cut, and a dwell stop the tool. Each run gets a
//! trapezoidal speed profile (a triangle when it is too short to reach full speed), at the
//! lowest speed and acceleration any of its moves allows, so the estimate errs long.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Vec2, Vec3};

use crate::gcode::Program;
use crate::path::{MAX_MOVES, Move, arc_sweep};
use crate::{CamError, Result, invalid};

/// Moves meeting at a smaller angle than this (radians, 10°) run on without stopping.
pub const BLEND_ANGLE: f64 = 10.0 * std::f64::consts::PI / 180.0;

const MAX_FEED: f64 = 1e6;
const MAX_ACCEL: f64 = 1e6;
const MAX_SECONDS: f64 = 1e6;

/// What the machine can do, for the time estimate. Feeds in mm/min, accelerations in mm/s²,
/// per axis X, Y, Z.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeModel {
    /// Speed of a rapid along its path (mm/min), still held to `max_feed` per axis.
    pub rapid_feed: f64,
    /// Fastest each axis moves (mm/min); a programmed feed above it is clamped.
    pub max_feed: [f64; 3],
    /// Acceleration of each axis (mm/s²).
    pub accel: [f64; 3],
    /// Seconds per tool change.
    pub tool_change: f64,
}

impl Default for TimeModel {
    fn default() -> TimeModel {
        TimeModel { rapid_feed: 5000.0, max_feed: [5000.0; 3], accel: [500.0; 3], tool_change: 10.0 }
    }
}

impl TimeModel {
    pub fn validate(&self) -> Result<()> {
        let within = |x: f64, max: f64| x.is_finite() && x > 0.0 && x <= max;
        if !within(self.rapid_feed, MAX_FEED) {
            return invalid(format!("the rapid feed must be above 0 and at most {MAX_FEED} mm/min"));
        }
        if !self.max_feed.iter().all(|&f| within(f, MAX_FEED)) {
            return invalid(format!("every axis feed limit must be above 0 and at most {MAX_FEED} mm/min"));
        }
        if !self.accel.iter().all(|&a| within(a, MAX_ACCEL)) {
            return invalid(format!("every axis acceleration must be above 0 and at most {MAX_ACCEL} mm/s²"));
        }
        if !(self.tool_change.is_finite() && (0.0..=MAX_SECONDS).contains(&self.tool_change)) {
            return invalid(format!("the tool change time must be 0 to {MAX_SECONDS} s"));
        }
        Ok(())
    }
}

/// How long a program runs, in seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TimeEstimate {
    /// Feed and arc moves.
    pub cutting: f64,
    pub rapid: f64,
    pub dwell: f64,
    /// All tool changes together.
    pub tool_changes: f64,
    pub total: f64,
    /// Per operation, each with the tool change before it; they add up to `total`.
    pub per_operation: Vec<f64>,
}

/// One move's share of the time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MoveTime {
    pub seconds: f64,
    /// Path length (mm).
    pub length: f64,
    /// Highest speed reached during the move (mm/min).
    pub peak: f64,
}

/// The time of `program` on `model`. Each operation starts at its first move's target, so that
/// move costs nothing; the way between operations is taken to be part of the tool change. A
/// tool change counts whenever the tool number differs from the previous operation's.
pub fn estimate(program: &Program, model: &TimeModel) -> Result<TimeEstimate> {
    model.validate()?;
    let mut out = TimeEstimate::default();
    let mut tool = None;
    for op in &program.operations {
        let mut seconds = 0.0;
        if tool.is_some_and(|t| t != op.tool.number) {
            out.tool_changes += model.tool_change;
            seconds += model.tool_change;
        }
        tool = Some(op.tool.number);
        let start = op.path.moves.iter().find_map(Move::target).unwrap_or_default();
        let times = move_times(&op.path.moves, start, model)?;
        for (m, t) in op.path.moves.iter().zip(&times) {
            match m {
                Move::Rapid { .. } => out.rapid += t.seconds,
                Move::Feed { .. } | Move::Arc { .. } => out.cutting += t.seconds,
                Move::Dwell { .. } => out.dwell += t.seconds,
            }
            seconds += t.seconds;
        }
        out.per_operation.push(seconds);
    }
    out.total = out.cutting + out.rapid + out.dwell + out.tool_changes;
    Ok(out)
}

/// The time of each of `moves`, starting with the tool tip at `start`.
pub fn move_times(moves: &[Move], start: Vec3, model: &TimeModel) -> Result<Vec<MoveTime>> {
    model.validate()?;
    if moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("a toolpath of {} moves (at most {MAX_MOVES})", moves.len())));
    }
    if !start.is_finite() {
        return invalid("the start of the toolpath is not a finite point");
    }
    let mut out = vec![MoveTime::default(); moves.len()];
    let mut run: Vec<Seg> = Vec::new();
    let mut at = start;
    for (index, m) in moves.iter().enumerate() {
        if let Move::Dwell { seconds } = *m {
            if !(seconds.is_finite() && (0.0..=MAX_SECONDS).contains(&seconds)) {
                return invalid(format!("move {index}: a dwell must be 0 to {MAX_SECONDS} s"));
            }
            flush(&mut run, &mut out);
            if let Some(slot) = out.get_mut(index) {
                slot.seconds = seconds;
            }
            continue;
        }
        let seg = segment(index, at, m, model)?;
        if let Some(to) = m.target() {
            at = to;
        }
        let Some(seg) = seg else { continue };
        if run.last().is_some_and(|prev| !prev.joins(&seg)) {
            flush(&mut run, &mut out);
        }
        run.push(seg);
    }
    flush(&mut run, &mut out);
    Ok(out)
}

/// A move with length, as the planner sees it. Speeds in mm/s.
struct Seg {
    index: usize,
    length: f64,
    speed: f64,
    accel: f64,
    /// Unit tangents where the move starts and ends.
    start: Vec3,
    end: Vec3,
    /// The programmed feed; `None` for a rapid.
    feed: Option<f64>,
}

impl Seg {
    fn joins(&self, next: &Seg) -> bool {
        self.feed == next.feed && self.end.dot(next.start) >= BLEND_ANGLE.cos()
    }
}

/// The planner's view of move `m` from `from`; `None` when it does not move the tool.
fn segment(index: usize, from: Vec3, m: &Move, model: &TimeModel) -> Result<Option<Seg>> {
    let Some(to) = m.target() else { return Ok(None) };
    if !to.is_finite() {
        return invalid(format!("move {index} goes to a point that is not finite"));
    }
    let feed = m.feed();
    if let Some(f) = feed
        && !(f.is_finite() && f > 0.0 && f <= MAX_FEED)
    {
        return invalid(format!("move {index}: the feed must be above 0 and at most {MAX_FEED} mm/min"));
    }
    let target = feed.unwrap_or(model.rapid_feed) / 60.0;
    let axis_speed = model.max_feed.map(|f| f / 60.0);
    let dz = to.z - from.z;
    if let Move::Arc { center, ccw, .. } = *m {
        let radius = Vec2::new(from.x - center.x, from.y - center.y).len();
        let sweep = arc_sweep(from, to, center, ccw);
        let around = radius * sweep.abs();
        if radius.is_finite() && around > 1e-9 {
            let length = around.hypot(dz);
            let (flat, rise) = (around / length, dz / length);
            // The direction turns through XY, so both X and Y may carry all of the XY part.
            let share = [flat, flat, rise.abs()];
            let accel = limit(model.accel, share);
            let [ax, ay, _] = model.accel;
            let xy_accel = ax.min(ay);
            // Centripetal acceleration v_xy² / r stays within what X and Y can do.
            let bend = (xy_accel * radius).sqrt() / flat;
            let speed = target.min(limit(axis_speed, share)).min(bend);
            let tangent = |p: Vec3| {
                let (rx, ry) = ((p.x - center.x) / radius, (p.y - center.y) / radius);
                let (tx, ty) = if ccw { (-ry, rx) } else { (ry, -rx) };
                Vec3::new(tx * flat, ty * flat, rise)
            };
            return Ok(Some(Seg { index, length, speed, accel, start: tangent(from), end: tangent(to), feed }));
        }
    }
    let d = to - from;
    let length = d.len();
    let Some(u) = d.normalized().filter(|_| length > 1e-9) else { return Ok(None) };
    let share = [u.x.abs(), u.y.abs(), u.z.abs()];
    let speed = target.min(limit(axis_speed, share));
    Ok(Some(Seg { index, length, speed, accel: limit(model.accel, share), start: u, end: u, feed }))
}

/// The most the path may do when each axis carries `share` of it and allows `axis`.
fn limit(axis: [f64; 3], share: [f64; 3]) -> f64 {
    axis.iter().zip(share).filter(|(_, s)| *s > 1e-12).map(|(a, s)| a / s).fold(f64::INFINITY, f64::min)
}

/// Gives every move of `run` its share of one trapezoidal profile, then empties it.
fn flush(run: &mut Vec<Seg>, out: &mut [MoveTime]) {
    let length: f64 = run.iter().map(|s| s.length).sum();
    let speed = run.iter().map(|s| s.speed).fold(f64::INFINITY, f64::min);
    let accel = run.iter().map(|s| s.accel).fold(f64::INFINITY, f64::min);
    if length > 0.0 && speed.is_finite() && accel.is_finite() {
        let profile = Profile::new(length, speed, accel);
        let mut s0 = 0.0;
        for seg in run.iter() {
            let s1 = (s0 + seg.length).min(length);
            if let Some(slot) = out.get_mut(seg.index) {
                *slot = MoveTime { seconds: profile.time_at(s1) - profile.time_at(s0), length: seg.length, peak: profile.peak(s0, s1) * 60.0 };
            }
            s0 = s1;
        }
    }
    run.clear();
}

/// Speed over a run of `length` from rest to rest: accelerate over `ramp`, cruise at `top`,
/// brake over `ramp`.
struct Profile {
    length: f64,
    accel: f64,
    ramp: f64,
    top: f64,
}

impl Profile {
    fn new(length: f64, speed: f64, accel: f64) -> Profile {
        let ramp = (speed * speed / (2.0 * accel)).min(length / 2.0);
        Profile { length, accel, ramp, top: (2.0 * accel * ramp).sqrt() }
    }

    /// Seconds from the start of the run until the tool has gone `s` mm.
    fn time_at(&self, s: f64) -> f64 {
        let ramp_time = self.top / self.accel;
        if s <= self.ramp {
            (2.0 * s / self.accel).sqrt()
        } else if s <= self.length - self.ramp {
            ramp_time + (s - self.ramp) / self.top
        } else {
            let cruise = (self.length - 2.0 * self.ramp) / self.top;
            let left = (self.length - s).max(0.0);
            2.0 * ramp_time + cruise - (2.0 * left / self.accel).sqrt()
        }
    }

    fn speed_at(&self, s: f64) -> f64 {
        (2.0 * self.accel * s.min(self.length - s).max(0.0)).sqrt().min(self.top)
    }

    /// Highest speed between `s0` and `s1`.
    fn peak(&self, s0: f64, s1: f64) -> f64 {
        if s0 <= self.length - self.ramp && s1 >= self.ramp { self.top } else { self.speed_at(s0).max(self.speed_at(s1)) }
    }
}
