//! Cutting data by material: per tool and material group, the cutting speed, the feed per tooth
//! and the depth and width of cut, and the spindle speed and feeds they give for that tool.
//!
//! n = vc · 1000 / (π · D) rev/min; vf = n · z · fz mm/min (a drill: n · fz, fz per revolution).

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::tool::{Tool, ToolKind};
use crate::{Result, invalid};

/// The material groups every tool starts with. A library can add its own names.
pub const MATERIALS: &[&str] = &["aluminium", "steel", "stainless", "brass", "plastic", "wood"];
/// Longest material group name.
pub const MAX_MATERIAL_NAME: usize = 64;
/// Most material groups one tool keeps cutting data for.
pub const MAX_MATERIALS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CuttingData {
    /// Cutting speed at the tool's edge (m/min).
    pub vc: f64,
    /// Feed per tooth (mm); for a drill, the feed per revolution.
    pub fz: f64,
    /// Depth per pass as a share of the diameter; for a drill, the peck depth.
    pub ap: f64,
    /// Width of cut (pocket stepover) as a share of the diameter.
    pub ae: f64,
}

/// What [`CuttingData`] gives for one tool.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Speeds {
    /// rev/min.
    pub spindle_rpm: f64,
    /// mm/min.
    pub feed: f64,
    /// mm/min.
    pub plunge_feed: f64,
    /// mm.
    pub stepdown: f64,
    /// mm.
    pub stepover: f64,
}

/// A material group name: 1 to 64 characters, no control characters, no surrounding spaces.
pub fn valid_material(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_MATERIAL_NAME && name.trim() == name && !name.chars().any(char::is_control)
}

impl CuttingData {
    pub fn validate(&self) -> Result<()> {
        let ok = |v: f64, max: f64| v > 0.0 && v <= max;
        if !ok(self.vc, 10_000.0) || !ok(self.fz, 10.0) || !ok(self.ap, 10.0) || !ok(self.ae, 1.0) {
            return invalid("cutting data needs vc 0…10000 m/min, fz 0…10 mm, ap 0…10 × D and ae 0…1 × D, all above 0");
        }
        Ok(())
    }

    /// Rule-of-thumb starting values for a solid carbide tool in one of [`MATERIALS`]; check
    /// them against the tool maker's data and the machine. `None` for another material.
    pub fn starting(kind: ToolKind, material: &str, diameter: f64) -> Option<CuttingData> {
        // vc m/min, fz as a share of D, ap and ae as shares of D.
        let (vc, fz, ap, ae) = match (kind, material) {
            (ToolKind::Drill, "aluminium") => (80.0, 0.020, 1.0, 1.0),
            (ToolKind::Drill, "steel") => (25.0, 0.015, 1.0, 1.0),
            (ToolKind::Drill, "stainless") => (12.0, 0.010, 0.5, 1.0),
            (ToolKind::Drill, "brass") => (50.0, 0.020, 1.0, 1.0),
            (ToolKind::Drill, "plastic") => (40.0, 0.020, 1.0, 1.0),
            (ToolKind::Drill, "wood") => (60.0, 0.030, 2.0, 1.0),
            (_, "aluminium") => (250.0, 0.010, 0.5, 0.4),
            (_, "steel") => (100.0, 0.005, 0.25, 0.3),
            (_, "stainless") => (60.0, 0.004, 0.2, 0.25),
            (_, "brass") => (150.0, 0.008, 0.4, 0.4),
            (_, "plastic") => (200.0, 0.012, 0.5, 0.5),
            (_, "wood") => (300.0, 0.015, 0.75, 0.5),
            _ => return None,
        };
        let diameter = if diameter.is_finite() && diameter > 0.0 { diameter } else { 6.0 };
        Some(CuttingData { vc, fz: (fz * diameter).clamp(0.002, 1.0), ap, ae })
    }

    /// The spindle speed, feeds, stepdown and stepover these data give `tool`, the spindle
    /// held to `max_rpm` when given (the feeds follow the speed actually reached).
    pub fn speeds(&self, tool: &Tool, max_rpm: Option<f64>) -> Result<Speeds> {
        self.validate()?;
        tool.validate()?;
        let mut rpm = self.vc * 1000.0 / (PI * tool.diameter);
        if let Some(max) = max_rpm.filter(|m| m.is_finite() && *m > 0.0) {
            rpm = rpm.min(max);
        }
        let (feed, plunge_feed) = match tool.kind {
            ToolKind::Drill => (rpm * self.fz, rpm * self.fz),
            _ => {
                let feed = rpm * f64::from(tool.flutes) * self.fz;
                (feed, feed * 0.5)
            }
        };
        let speeds = Speeds {
            spindle_rpm: rpm.round().max(1.0),
            feed: feed.round().max(1.0),
            plunge_feed: plunge_feed.round().max(1.0),
            stepdown: self.ap * tool.diameter,
            stepover: self.ae * tool.diameter,
        };
        if [speeds.spindle_rpm, speeds.feed, speeds.plunge_feed, speeds.stepdown, speeds.stepover].iter().any(|v| !v.is_finite() || *v > 1.0e6) {
            return invalid("the cutting data give speeds out of range for this tool");
        }
        Ok(speeds)
    }
}

impl Tool {
    /// The tool's cutting data for `material`: its own, else the starting values.
    pub fn cutting_for(&self, material: &str) -> Option<CuttingData> {
        self.cutting.get(material).copied().or_else(|| CuttingData::starting(self.kind, material, self.diameter))
    }
}
