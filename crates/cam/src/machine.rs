//! A CNC machine's limits: how far each axis travels, the spindle speed range and the fastest
//! feed. Cutting data are held to them, and a program that would leave them is not posted.
//!
//! Where part zero sits on the table is set at the machine, so a program is checked by how far
//! it reaches along each axis, not by its positions.

use serde::{Deserialize, Serialize};

use crate::cutting::Speeds;
use crate::path::Toolpath;
use crate::{Result, invalid};

/// Longest machine name.
pub const MAX_MACHINE_NAME: usize = 128;
/// Largest travel or position (mm, degrees).
const MAX_TRAVEL: f64 = 1.0e6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    /// Travel of each linear axis as its lowest and highest position (mm): X−, X+ and so on.
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub z: [f64; 2],
    /// The rotary A axis, when the machine has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<RotaryAxis>,
    /// Slowest and fastest spindle speed (rev/min).
    pub spindle_min: f64,
    pub spindle_max: f64,
    /// Fastest feed the axes move at (mm/min).
    pub max_feed: f64,
    /// The post its programs are written with: a built-in post or one saved on the design. None:
    /// whatever the caller writes by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
}

/// A rotary axis: turning between two angles (degrees), or round without end.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RotaryAxis {
    Limited { min: f64, max: f64 },
    Endless,
}

impl Machine {
    /// A machine of this name travelling 300 × 200 × 100 mm, 1000 to 24000 rev/min, 5000 mm/min.
    pub fn new(name: impl Into<String>) -> Machine {
        Machine {
            name: name.into(),
            x: [0.0, 300.0],
            y: [0.0, 200.0],
            z: [-100.0, 0.0],
            a: None,
            spindle_min: 1000.0,
            spindle_max: 24000.0,
            max_feed: 5000.0,
            post: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() || self.name.len() > MAX_MACHINE_NAME || name != self.name || self.name.chars().any(char::is_control) {
            return invalid("a machine is named by 1 to 128 characters without surrounding spaces");
        }
        for (axis, [lo, hi]) in [("X", self.x), ("Y", self.y), ("Z", self.z)] {
            if !lo.is_finite() || !hi.is_finite() || lo.abs() > MAX_TRAVEL || hi.abs() > MAX_TRAVEL || !(hi > lo) {
                return invalid(format!("the {axis} travel needs a lowest position below the highest, within 1e6 mm"));
            }
        }
        if let Some(RotaryAxis::Limited { min, max }) = self.a
            && (!min.is_finite() || !max.is_finite() || min.abs() > MAX_TRAVEL || max.abs() > MAX_TRAVEL || !(max > min))
        {
            return invalid("the A travel needs a lowest angle below the highest");
        }
        if !(self.spindle_min >= 0.0) || !(self.spindle_max > 0.0) || self.spindle_max > MAX_TRAVEL || self.spindle_min > self.spindle_max {
            return invalid("the spindle speeds need 0 ≤ slowest ≤ fastest ≤ 1000000 rev/min");
        }
        if !(self.max_feed > 0.0) || self.max_feed > MAX_TRAVEL {
            return invalid("the fastest feed must be above 0 and at most 1000000 mm/min");
        }
        if let Some(post) = &self.post
            && (post.trim().is_empty() || post.len() > MAX_MACHINE_NAME || post.trim() != post || post.chars().any(char::is_control))
        {
            return invalid("a machine's post is named by 1 to 128 characters without surrounding spaces");
        }
        Ok(())
    }

    /// `speeds` held to the machine: the spindle within its range (the feeds follow the speed
    /// actually reached), then every feed at most the fastest.
    pub fn hold(&self, speeds: Speeds) -> Speeds {
        let rpm = speeds.spindle_rpm.clamp(self.spindle_min.max(1.0), self.spindle_max.max(1.0));
        let ratio = if speeds.spindle_rpm > 0.0 { rpm / speeds.spindle_rpm } else { 1.0 };
        let feed = |f: f64| (f * ratio).round().clamp(1.0, self.max_feed.max(1.0));
        Speeds { spindle_rpm: rpm.round(), feed: feed(speeds.feed), plunge_feed: feed(speeds.plunge_feed), ..speeds }
    }

    /// Whether an operation's spindle speed and feeds are within the machine's.
    pub fn check_operation(&self, spindle_rpm: f64, path: &Toolpath) -> Result<()> {
        if spindle_rpm < self.spindle_min || spindle_rpm > self.spindle_max {
            return invalid(format!(
                "spindle speed {spindle_rpm} rev/min is outside the machine's {} to {} rev/min",
                self.spindle_min, self.spindle_max
            ));
        }
        let fastest = path.moves.iter().filter_map(|m| m.feed()).fold(0.0, f64::max);
        if fastest > self.max_feed {
            return invalid(format!("feed {fastest} mm/min is faster than the machine's {} mm/min", self.max_feed));
        }
        Ok(())
    }

    /// Whether a program reaching from `low` to `high` (work X, Y, Z in mm, tool axis along Z)
    /// fits the machine's travel.
    pub fn check_reach(&self, low: [f64; 3], high: [f64; 3]) -> Result<()> {
        for ((axis, [lo, hi]), (a, b)) in [("X", self.x), ("Y", self.y), ("Z", self.z)].into_iter().zip(low.into_iter().zip(high)) {
            let reach = b - a;
            if reach > hi - lo + 1e-6 {
                return invalid(format!("the program reaches {reach:.3} mm along {axis}; the machine travels {:.3} mm", hi - lo));
            }
        }
        Ok(())
    }
}

/// The lowest and highest point a path reaches (arcs followed within 0.05 mm); `None` when it
/// does not move.
pub fn reach(path: &Toolpath) -> Option<([f64; 3], [f64; 3])> {
    let mut out: Option<([f64; 3], [f64; 3])> = None;
    for (p, _) in path.points(0.05) {
        let q = [p.x, p.y, p.z];
        out = Some(match out {
            None => (q, q),
            Some((lo, hi)) => (std::array::from_fn(|i| lo[i].min(q[i])), std::array::from_fn(|i| hi[i].max(q[i]))),
        });
    }
    out
}
