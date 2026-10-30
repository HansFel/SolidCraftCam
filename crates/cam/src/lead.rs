//! Leads: how the tool gets onto a contour and off it again. A lead is one straight move or one
//! arc, cut at the pass's depth, that starts (ends) on the free side of the contour where the
//! tool is and meets the contour tangent to the direction it cuts.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec2;

use crate::piece::Piece;
use crate::{Result, invalid};

/// Longest lead radius or length (mm), and the overlap at most this.
const MAX_LEAD: f64 = 1.0e4;

/// How the tool gets onto, and off, a contour. Lengths and radii in mm, angles in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Lead {
    /// Straight down on the contour; a closed loop ramps along itself at the ramp angle.
    /// Lead-out: straight up where the cut ends.
    #[default]
    Direct,
    /// An arc tangent to the contour at its start (end), coming from (going to) the free side.
    /// `sweep` is positive, usually a quarter turn.
    Arc { radius: f64, sweep: f64 },
    /// A straight line in line with the contour's tangent at its start (end).
    Tangent { length: f64 },
    /// A straight line square to the contour at its start (end), from (to) the free side.
    Perpendicular { length: f64 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Leads {
    pub lead_in: Lead,
    pub lead_out: Lead,
    /// Closed loops: how far the cut runs on past its start before the lead-out (mm, >= 0).
    pub overlap: f64,
}

impl Lead {
    pub fn validate(&self) -> Result<()> {
        let length = |v: f64, what: &str| {
            if v.is_finite() && v > 0.0 && v <= MAX_LEAD {
                Ok(())
            } else {
                invalid(format!("the lead {what} must be above 0 and at most {MAX_LEAD} mm"))
            }
        };
        match *self {
            Lead::Direct => Ok(()),
            Lead::Arc { radius, sweep } => {
                length(radius, "radius")?;
                if sweep.is_finite() && sweep > 0.0 && sweep <= PI {
                    Ok(())
                } else {
                    invalid("the lead arc's sweep must be above 0 and at most a half turn")
                }
            }
            Lead::Tangent { length: l } => length(l, "length"),
            Lead::Perpendicular { length: l } => length(l, "length"),
        }
    }
}

impl Leads {
    pub fn validate(&self) -> Result<()> {
        self.lead_in.validate()?;
        self.lead_out.validate()?;
        if !self.overlap.is_finite() || !(0.0..=MAX_LEAD).contains(&self.overlap) {
            return invalid(format!("the overlap must be between 0 and {MAX_LEAD} mm"));
        }
        Ok(())
    }

    /// Whether these leads cut nothing before or after the contour: today's plunge at its start
    /// and retract at its end (a closed loop may ramp down along itself).
    pub fn is_direct(&self) -> bool {
        self.lead_in == Lead::Direct && self.lead_out == Lead::Direct && self.overlap <= 0.0
    }
}

/// The piece cut before `s` to reach it, where the contour starts there with tangent `t` and
/// the tool is on the free side `f` (a unit vector square to `t`). `None`: plunge at `s`.
pub(crate) fn lead_in(lead: &Lead, s: Vec2, t: Vec2, f: Vec2) -> Option<Piece> {
    match *lead {
        Lead::Direct => None,
        Lead::Tangent { length } => Some(Piece::Line { a: s - t * length, b: s }),
        Lead::Perpendicular { length } => Some(Piece::Line { a: s + f * length, b: s }),
        Lead::Arc { radius, sweep } => {
            // A quarter turn round a centre a radius out on the free side rolls onto the
            // contour; the sweep curls the way the free side lies.
            let sweep = sweep * side_sign(t, f);
            let c = s + f * radius;
            let a0 = (s - c).angle() - sweep;
            Some(Piece::Arc { c, r: radius, a0, sweep })
        }
    }
}

/// The piece cut after `e`, where the contour ends there with tangent `t` and the tool is on the
/// free side `f`. `None`: retract at `e`.
pub(crate) fn lead_out(lead: &Lead, e: Vec2, t: Vec2, f: Vec2) -> Option<Piece> {
    match *lead {
        Lead::Direct => None,
        Lead::Tangent { length } => Some(Piece::Line { a: e, b: e + t * length }),
        Lead::Perpendicular { length } => Some(Piece::Line { a: e, b: e + f * length }),
        Lead::Arc { radius, sweep } => {
            let sweep = sweep * side_sign(t, f);
            let c = e + f * radius;
            Some(Piece::Arc { c, r: radius, a0: (e - c).angle(), sweep })
        }
    }
}

/// +1 when `f` is the left of the travel `t` (turning counter-clockwise rolls towards it).
fn side_sign(t: Vec2, f: Vec2) -> f64 {
    if t.cross(f) >= 0.0 { 1.0 } else { -1.0 }
}
