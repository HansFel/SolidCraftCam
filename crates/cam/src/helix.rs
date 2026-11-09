//! Holes milled round their axis: helical boring (a hole wider than the tool, cut by a helix
//! down and one full turn at the bottom) and thread milling (a helix of the thread's pitch).

use serde::{Deserialize, Serialize};
use solidcore_record::Vec2;

use crate::check::verify;
use crate::ops::{Builder, Cut, MAX_COORD, mills};
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most holes in one operation.
const MAX_HOLES: usize = 10_000;
/// Most half turns one hole takes.
const MAX_HALF_TURNS: f64 = 200_000.0;

/// A helically milled hole, or a round boss milled from outside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HelixParams {
    /// The finished diameter (mm). A hole is wider than the tool and at most twice it (a wider
    /// hole would leave a core standing in the middle); a boss is any size above 0.
    pub diameter: f64,
    /// Climb milling (counter-clockwise inside a hole, clockwise round a boss, with the spindle
    /// turning clockwise).
    pub climb: bool,
    /// Rise per turn (mm); none: the cut's stepdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    /// Round a boss (outside) instead of in a hole.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside: bool,
}

/// A milled thread.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadParams {
    /// The thread's major (nominal) diameter (mm).
    pub diameter: f64,
    /// Rise per turn (mm).
    pub pitch: f64,
    /// A thread in a hole (nut); otherwise on a stud.
    pub internal: bool,
    /// A left-hand thread.
    pub left_hand: bool,
}

/// The centres of the holes, checked.
fn holes(points: &[Vec2]) -> Result<()> {
    if points.is_empty() || points.len() > MAX_HOLES {
        return invalid(format!("a hole operation needs 1 to {MAX_HOLES} hole centres"));
    }
    if points.iter().any(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("hole centres must be finite and within 1e6 mm");
    }
    Ok(())
}

/// Half turns about `c` at radius `r` from `from_z` to `to_z` (either way), each rising or
/// falling the same, starting and ending at `c + (r, 0)`; `ccw` turns counter-clockwise.
fn half_turns(b: &mut Builder, c: Vec2, r: f64, from_z: f64, to_z: f64, half_rise: f64, ccw: bool, feed: f64) -> Result<()> {
    let span = (to_z - from_z).abs();
    let n = (span / half_rise).ceil().max(0.0);
    if n > MAX_HALF_TURNS {
        return Err(CamError::TooLarge("the helix has too many turns: raise the depth per turn".into()));
    }
    // Whole turns keep the start and the end on the same side of the hole.
    let n = (n as usize).next_multiple_of(2);
    let step = if n == 0 { 0.0 } else { (to_z - from_z) / n as f64 };
    let side = [c + Vec2::new(r, 0.0), c - Vec2::new(r, 0.0)];
    for i in 1..=n {
        let z = from_z + step * i as f64;
        let to = side.get(i % 2).copied().unwrap_or(c).extend(z);
        b.push(Move::Arc { to, center: c, ccw, feed })?;
    }
    Ok(())
}

/// Helical milling: for each centre the tool goes down a helix of `pitch` (else `cut.stepdown`)
/// per turn from the top to the bottom and makes one full turn there to clean the ramp the helix
/// leaves. In a hole it then moves to the centre and leaves straight up; round a boss it leaves
/// straight up in its own groove, clear of the boss and of the stock round it.
pub fn helical_bore(points: &[Vec2], tool: &Tool, params: &HelixParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    cut.validate()?;
    mills(tool)?;
    holes(points)?;
    let d = params.diameter;
    if params.outside {
        if !(d.is_finite() && d > 0.0 && d <= 2.0 * MAX_COORD) {
            return invalid("a boss's diameter is above 0 and within the machine's range");
        }
    } else {
        if !d.is_finite() || d <= tool.diameter + 1e-6 {
            return invalid("a helically bored hole is wider than the tool: drill or plunge a hole the tool's size");
        }
        if d > 2.0 * tool.diameter {
            return invalid("a hole more than twice as wide as the tool would leave a core standing: pocket it instead");
        }
    }
    let rise = params.pitch.unwrap_or(cut.stepdown);
    if !(rise.is_finite() && rise > 0.0 && rise <= 1000.0) {
        return invalid("the helix's rise per turn is above 0 and at most 1000 mm");
    }
    let h = cut.heights;
    if h.top - h.bottom > tool.flute_length {
        return invalid("cutting depth exceeds the tool's flute length");
    }
    let r = if params.outside { (d + tool.diameter) * 0.5 } else { (d - tool.diameter) * 0.5 };
    if points.len() as f64 * ((h.top - h.bottom) / (rise * 0.5) + 8.0) > MAX_MOVES as f64 {
        return Err(CamError::TooLarge("too many helix moves".into()));
    }
    // Climb milling turns counter-clockwise inside a hole and clockwise round a boss.
    let ccw = params.climb != params.outside;
    let mut b = Builder::new(cut);
    for c in points {
        let start = *c + Vec2::new(r, 0.0);
        b.travel_to(start)?;
        b.plunge_to(h.top)?;
        half_turns(&mut b, *c, r, h.top, h.bottom, rise * 0.5, ccw, cut.feeds.cut)?;
        // One more turn at the bottom: the helix leaves a ramp there.
        b.push(Move::Arc { to: (*c - Vec2::new(r, 0.0)).extend(h.bottom), center: *c, ccw, feed: cut.feeds.cut })?;
        b.push(Move::Arc { to: start.extend(h.bottom), center: *c, ccw, feed: cut.feeds.cut })?;
        if params.outside {
            b.push(Move::Rapid { to: start.extend(h.clearance) })?;
        } else {
            b.push(Move::Feed { to: c.extend(h.bottom), feed: cut.feeds.cut })?;
            b.push(Move::Rapid { to: c.extend(h.clearance) })?;
        }
    }
    let path = Toolpath::new(b.into_moves());
    verify(&path, &h)?;
    Ok(path)
}

/// Thread milling: for each hole the tool goes down the middle to the bottom (an internal
/// thread, in the drilled core hole) or beside the stud (an external one), moves out to the
/// thread, rises one pitch a turn to above the top, and goes back to the middle or out. A
/// right-hand thread turns counter-clockwise as it rises (climb milling inside a hole).
pub fn thread_mill(points: &[Vec2], tool: &Tool, params: &ThreadParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    cut.validate()?;
    holes(points)?;
    if tool.kind != ToolKind::ThreadMill {
        return invalid("thread milling needs a thread mill");
    }
    let (d, p) = (params.diameter, params.pitch);
    if !(p.is_finite() && (0.05..=20.0).contains(&p)) {
        return invalid("a thread pitch is from 0.05 to 20 mm");
    }
    if !(d.is_finite() && d > 0.0 && d <= 2000.0) {
        return invalid("a thread diameter is above 0 and at most 2000 mm");
    }
    // ISO metric minor diameter: the core hole the tool has to fit in.
    let minor = d - 1.0825 * p;
    if params.internal && tool.diameter >= minor - 1e-6 {
        return invalid("the thread mill does not fit the thread's core hole");
    }
    let h = cut.heights;
    if h.top - h.bottom > tool.flute_length.max(tool.reach()) {
        return invalid("the thread is deeper than the thread mill reaches");
    }
    let r = if params.internal { (d - tool.diameter) * 0.5 } else { (d + tool.diameter) * 0.5 };
    if !(r > 1e-6) {
        return invalid("the thread mill is too large for the thread");
    }
    if points.len() as f64 * (2.0 * (h.top - h.bottom) / p + 8.0) > MAX_MOVES as f64 {
        return Err(CamError::TooLarge("too many thread milling moves".into()));
    }
    let ccw = !params.left_hand;
    let mut b = Builder::new(cut);
    for c in points {
        let on = *c + Vec2::new(r, 0.0);
        // Inside: down the middle of the core hole. Outside: down beside the stud, clear of it.
        let approach = if params.internal { *c } else { *c + Vec2::new(r + tool.diameter, 0.0) };
        b.travel_to(approach)?;
        b.plunge_to(h.bottom)?;
        b.push(Move::Feed { to: on.extend(h.bottom), feed: cut.feeds.cut })?;
        // One pitch a turn exactly, ending at or just above the top.
        let half = p * 0.5;
        let n = ((h.top - h.bottom) / half).ceil().max(2.0);
        if n > MAX_HALF_TURNS {
            return Err(CamError::TooLarge("the thread has too many turns".into()));
        }
        let n = (n as usize).next_multiple_of(2);
        let side = [on, *c - Vec2::new(r, 0.0)];
        for i in 1..=n {
            let to = side.get(i % 2).copied().unwrap_or(on).extend(h.bottom + half * i as f64);
            b.push(Move::Arc { to, center: *c, ccw, feed: cut.feeds.cut })?;
        }
        let top = h.bottom + half * n as f64;
        b.push(Move::Feed { to: approach.extend(top), feed: cut.feeds.cut })?;
        b.push(Move::Rapid { to: approach.extend(h.clearance) })?;
    }
    let path = Toolpath::new(b.into_moves());
    verify(&path, &h)?;
    Ok(path)
}
