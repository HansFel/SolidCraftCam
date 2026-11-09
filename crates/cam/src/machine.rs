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

/// What a machine does. A setup is run on a machine of its own kind only, and a machine's
/// programs are written by a post for its kind only ([`Post::machine`](crate::Post)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineKind {
    /// A CNC mill or router: rotating tools, depth passes.
    #[default]
    Mill,
    /// A CNC lathe.
    Lathe,
    /// A plasma table: a torch instead of tools, each cut pierced through the sheet.
    Plasma,
}

impl MachineKind {
    pub const ALL: &'static [MachineKind] = &[MachineKind::Mill, MachineKind::Lathe, MachineKind::Plasma];

    /// The key commands and files use.
    pub fn key(self) -> &'static str {
        match self {
            MachineKind::Mill => "mill",
            MachineKind::Lathe => "lathe",
            MachineKind::Plasma => "plasma",
        }
    }

    pub fn is_mill(&self) -> bool {
        *self == MachineKind::Mill
    }
}

/// A plasma table's cutting data, suggested to each new plasma cut on it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Torch {
    /// Width of the cut the arc burns (mm).
    pub kerf: f64,
    /// Cutting feed (mm/min).
    pub feed: f64,
    /// Height over the sheet the arc is struck at (mm).
    pub pierce_height: f64,
    /// Height over the sheet while cutting (mm).
    pub cut_height: f64,
    /// Wait after striking the arc until the sheet is pierced (s).
    pub pierce_delay: f64,
}

impl Default for Torch {
    fn default() -> Torch {
        Torch { kerf: 1.5, feed: 2000.0, pierce_height: 3.8, cut_height: 1.5, pierce_delay: 0.5 }
    }
}

impl Torch {
    pub fn validate(&self) -> Result<()> {
        if !(self.kerf > 0.0 && self.kerf <= 50.0) {
            return invalid("the kerf must be above 0 and at most 50 mm");
        }
        if !(self.feed > 0.0 && self.feed <= MAX_TRAVEL) {
            return invalid("the cutting feed must be above 0 and at most 1000000 mm/min");
        }
        if !(self.cut_height > 0.0 && self.cut_height <= 100.0) {
            return invalid("the cut height must be above 0 and at most 100 mm");
        }
        if !(self.pierce_height >= self.cut_height && self.pierce_height <= 100.0) {
            return invalid("the pierce height must be at least the cut height and at most 100 mm");
        }
        if !(0.0..=60.0).contains(&self.pierce_delay) {
            return invalid("the pierce delay must be 0 to 60 s");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    /// What the machine does: which setups it serves and which posts write its programs.
    #[serde(default, skip_serializing_if = "MachineKind::is_mill")]
    pub kind: MachineKind,
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
    /// A plasma table's cutting data, suggested to new plasma cuts. Plasma tables only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torch: Option<Torch>,
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
            kind: MachineKind::Mill,
            x: [0.0, 300.0],
            y: [0.0, 200.0],
            z: [-100.0, 0.0],
            a: None,
            spindle_min: 1000.0,
            spindle_max: 24000.0,
            max_feed: 5000.0,
            post: None,
            torch: None,
        }
    }

    /// A plasma table of this name with a 1500 × 3000 mm bed and the default cutting data. Its
    /// spindle range only lets the torch's on and off through.
    pub fn plasma(name: impl Into<String>) -> Machine {
        Machine {
            kind: MachineKind::Plasma,
            x: [0.0, 3000.0],
            y: [0.0, 1500.0],
            z: [-50.0, 0.0],
            spindle_min: 0.0,
            spindle_max: 1.0,
            max_feed: 20000.0,
            post: Some("plasma".into()),
            torch: Some(Torch::default()),
            ..Machine::new(name)
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
        match (&self.torch, self.kind) {
            (Some(torch), MachineKind::Plasma) => torch.validate()?,
            (Some(_), _) => return invalid("cutting data for a torch are for plasma tables only"),
            (None, _) => {}
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
