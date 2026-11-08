//! Cutting tools, and a tool library as a file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cutting::{CuttingData, MAX_MATERIALS, valid_material};
use crate::{CamError, Result, invalid};

/// Largest tool number a post processor is asked to write.
pub const MAX_TOOL_NUMBER: u32 = 9999;
/// Largest tool diameter (mm).
pub const MAX_TOOL_DIAMETER: f64 = 1000.0;
/// Largest tool or holder length (mm).
pub const MAX_TOOL_LENGTH: f64 = 10_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    FlatEndmill,
    BallEndmill,
    Drill,
    /// An end mill with rounded corners (`corner_radius`).
    BullNose,
    /// A chamfer cutter: a cone of `tip_angle` (included) from `tip_diameter` up to `diameter`.
    Chamfer,
    /// An engraving cutter: a cone of `tip_angle` (included) to a point or a small `tip_diameter`.
    VBit,
    /// A T-slot cutter: a disc `diameter` across and `flute_length` high on a thinner neck
    /// (`shank_diameter`), cutting sideways under an overhang.
    TSlot,
    /// A centre (spot) drill: a short stiff point of `tip_angle` that marks where a drill starts.
    CentreDrill,
    /// A reamer: sizes a drilled hole, cutting with its side over `flute_length`.
    Reamer,
    /// A tap: cuts a thread of `pitch` into a drilled hole, the feed tied to the spindle.
    Tap,
    /// A thread mill: cuts a thread of `pitch` by running a helix round the hole.
    ThreadMill,
    /// A plasma torch: cuts through a sheet from above it, `diameter` the width of its cut
    /// (kerf) and `flute_length` the thickest sheet it cuts, piercing at `pierce_height` for
    /// `pierce_delay` and cutting at `cut_height` above the sheet.
    Plasma,
}

impl ToolKind {
    /// Every kind, in the order the tool form offers them.
    pub const ALL: &[ToolKind] = &[
        ToolKind::FlatEndmill,
        ToolKind::BallEndmill,
        ToolKind::BullNose,
        ToolKind::Chamfer,
        ToolKind::VBit,
        ToolKind::TSlot,
        ToolKind::ThreadMill,
        ToolKind::Drill,
        ToolKind::CentreDrill,
        ToolKind::Reamer,
        ToolKind::Tap,
        ToolKind::Plasma,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "flat end mill",
            ToolKind::BallEndmill => "ball end mill",
            ToolKind::Drill => "drill",
            ToolKind::BullNose => "corner-radius end mill",
            ToolKind::Chamfer => "chamfer cutter",
            ToolKind::VBit => "engraving cutter",
            ToolKind::TSlot => "T-slot cutter",
            ToolKind::CentreDrill => "centre drill",
            ToolKind::Reamer => "reamer",
            ToolKind::Tap => "tap",
            ToolKind::ThreadMill => "thread mill",
            ToolKind::Plasma => "plasma torch",
        }
    }

    /// The name a command or a file gives the kind (`flat_endmill`, `v_bit`, ...).
    pub fn key(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "flat_endmill",
            ToolKind::BallEndmill => "ball_endmill",
            ToolKind::Drill => "drill",
            ToolKind::BullNose => "bull_nose",
            ToolKind::Chamfer => "chamfer",
            ToolKind::VBit => "v_bit",
            ToolKind::TSlot => "t_slot",
            ToolKind::CentreDrill => "centre_drill",
            ToolKind::Reamer => "reamer",
            ToolKind::Tap => "tap",
            ToolKind::ThreadMill => "thread_mill",
            ToolKind::Plasma => "plasma",
        }
    }

    /// The kind a command or a file names, if it is one.
    pub fn from_key(key: &str) -> Option<ToolKind> {
        ToolKind::ALL.iter().copied().find(|k| k.key() == key)
    }

    /// Tools that go straight down the hole axis and cut only there (drilling cycles).
    pub fn cuts_down(self) -> bool {
        matches!(self, ToolKind::Drill | ToolKind::CentreDrill | ToolKind::Reamer | ToolKind::Tap)
    }

    /// Tools with a cone at the tip (`tip_angle`).
    pub fn has_cone(self) -> bool {
        matches!(self, ToolKind::Chamfer | ToolKind::VBit | ToolKind::CentreDrill)
    }

    /// Tools that cut a thread (`pitch`).
    pub fn threads(self) -> bool {
        matches!(self, ToolKind::Tap | ToolKind::ThreadMill)
    }
}

/// What holds the tool in the spindle, as a cylinder above the stick-out: what a collision
/// check keeps clear of the stock, the model and the fixtures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolHolder {
    #[serde(default)]
    pub description: String,
    /// Largest diameter (mm).
    pub diameter: f64,
    /// Length from the holder's nose up to the spindle face (mm).
    pub length: f64,
}

impl ToolHolder {
    pub fn validate(&self) -> Result<()> {
        if !(self.diameter > 0.0 && self.diameter <= MAX_TOOL_DIAMETER) || !(self.length > 0.0 && self.length <= MAX_TOOL_LENGTH) {
            return invalid("a tool holder needs a diameter up to 1000 mm and a length up to 10000 mm, both above 0");
        }
        if self.description.chars().count() > 256 {
            return invalid("a tool holder's description is at most 256 characters");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// The pocket the machine holds it in (`T` word).
    pub number: u32,
    pub kind: ToolKind,
    /// Cutting diameter (mm).
    pub diameter: f64,
    /// Length of the cutting edges (mm).
    pub flute_length: f64,
    #[serde(default)]
    pub description: String,
    /// Number of cutting edges.
    #[serde(default = "two")]
    pub flutes: u32,
    /// The cutting data operations start from: spindle speed (rev/min) and feeds (mm/min).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle_rpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plunge_feed: Option<f64>,
    /// The depth per pass and the distance between passes operations start from (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepover: Option<f64>,
    /// Cutting data by material group (`aluminium`, `steel`, … or a library's own names).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cutting: BTreeMap<String, CuttingData>,
    /// Bull nose: the corner radius (mm), at most the radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f64>,
    /// Chamfer, engraving and centre drill (and a drill's point): the included angle of the
    /// cone (degrees; 90 for a 45 degree chamfer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_angle: Option<f64>,
    /// Chamfer and engraving: the diameter of the flat at the cone's tip (mm; absent: a point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_diameter: Option<f64>,
    /// Tap and thread mill: the thread's pitch (mm per turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    /// Diameter of the shank, or a T-slot cutter's neck (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shank_diameter: Option<f64>,
    /// The whole tool's length (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall_length: Option<f64>,
    /// How far the tool stands out of its holder (mm): the deepest it can reach next to a wall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stick_out: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<ToolHolder>,
    /// Plasma torch: how high above the sheet it pierces (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pierce_height: Option<f64>,
    /// Plasma torch: how high above the sheet it cuts (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_height: Option<f64>,
    /// Plasma torch: how long it waits at the pierce height with the arc on (seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pierce_delay: Option<f64>,
}

fn two() -> u32 {
    2
}

impl Tool {
    pub fn new(number: u32, kind: ToolKind, diameter: f64, flute_length: f64) -> Tool {
        Tool {
            number,
            kind,
            diameter,
            flute_length,
            description: String::new(),
            flutes: 2,
            spindle_rpm: None,
            feed: None,
            plunge_feed: None,
            stepdown: None,
            stepover: None,
            cutting: BTreeMap::new(),
            corner_radius: None,
            tip_angle: None,
            tip_diameter: None,
            pitch: None,
            shank_diameter: None,
            overall_length: None,
            stick_out: None,
            holder: None,
            pierce_height: None,
            cut_height: None,
            pierce_delay: None,
        }
    }

    /// Half the cone's included angle (radians), for the tools with a cone; a drill's point
    /// is 118 degrees unless the tool says otherwise.
    pub fn half_angle(&self) -> Option<f64> {
        let default = match self.kind {
            ToolKind::Drill => Some(118.0),
            ToolKind::CentreDrill | ToolKind::Chamfer => Some(90.0),
            ToolKind::VBit => Some(60.0),
            _ => None,
        };
        self.tip_angle.or(default).map(|a| (a * 0.5).to_radians())
    }

    /// The radius of the flat at the tip of a cone (0: a point).
    pub fn tip_radius(&self) -> f64 {
        self.tip_diameter.map_or(0.0, |d| d * 0.5)
    }

    /// How deep the tool reaches below its holder: the stick-out, else the overall length,
    /// else the flute length.
    pub fn reach(&self) -> f64 {
        self.stick_out.or(self.overall_length).unwrap_or(self.flute_length)
    }
    pub fn radius(&self) -> f64 {
        self.diameter * 0.5
    }
    pub fn validate(&self) -> Result<()> {
        if self.number > MAX_TOOL_NUMBER {
            return invalid(format!("tool number {} is above {MAX_TOOL_NUMBER}", self.number));
        }
        if !(self.diameter > 0.0) || !(self.diameter <= MAX_TOOL_DIAMETER) {
            return invalid("the tool diameter must be above 0 and at most 1000 mm");
        }
        if !(self.flute_length > 0.0) || !self.flute_length.is_finite() {
            return invalid("the tool's flute length must be above 0");
        }
        if !(1..=100).contains(&self.flutes) {
            return invalid("a tool has 1 to 100 flutes");
        }
        if [self.spindle_rpm, self.feed, self.plunge_feed, self.stepdown, self.stepover].iter().flatten().any(|v| !(*v > 0.0) || !(*v <= 1.0e6)) {
            return invalid("a tool's spindle speed, feeds, stepdown and stepover must be above 0");
        }
        if self.cutting.len() > MAX_MATERIALS || self.cutting.keys().any(|m| !valid_material(m)) {
            return invalid("a tool keeps cutting data for at most 64 materials, each named by 1 to 64 characters");
        }
        self.validate_shape()?;
        if let Some(holder) = &self.holder {
            holder.validate()?;
        }
        self.cutting.values().try_for_each(CuttingData::validate)
    }

    /// The kind's own sizes: present where the kind uses them, and inside the tool.
    fn validate_shape(&self) -> Result<()> {
        let length = |v: Option<f64>| v.is_none_or(|v| v > 0.0 && v <= MAX_TOOL_LENGTH);
        if !length(self.shank_diameter) || !length(self.overall_length) || !length(self.stick_out) {
            return invalid("a tool's shank diameter, overall length and stick-out are above 0 and at most 10000 mm");
        }
        if let (Some(all), Some(out)) = (self.overall_length, self.stick_out)
            && out > all
        {
            return invalid("a tool cannot stick out further than it is long");
        }
        if let Some(r) = self.corner_radius
            && (self.kind != ToolKind::BullNose || !(r > 0.0 && r <= self.radius()))
        {
            return invalid("a corner radius belongs to a corner-radius end mill, above 0 and at most the tool's radius");
        }
        if self.kind == ToolKind::BullNose && self.corner_radius.is_none() {
            return invalid("a corner-radius end mill needs its corner radius");
        }
        if let Some(a) = self.tip_angle
            && (!(self.kind.has_cone() || self.kind == ToolKind::Drill) || !(1.0..=179.0).contains(&a))
        {
            return invalid("a tip angle belongs to a chamfer cutter, an engraving cutter or a drill, from 1 to 179 degrees");
        }
        if let Some(d) = self.tip_diameter
            && (!matches!(self.kind, ToolKind::Chamfer | ToolKind::VBit) || !(d >= 0.0 && d < self.diameter))
        {
            return invalid("a tip diameter belongs to a chamfer or engraving cutter, from 0 and below the tool's diameter");
        }
        match (self.kind.threads(), self.pitch) {
            (true, None) => return invalid("a tap or thread mill needs the thread's pitch"),
            (true, Some(p)) if !(0.05..=20.0).contains(&p) => return invalid("a thread pitch is from 0.05 to 20 mm"),
            (false, Some(_)) => return invalid("a pitch belongs to a tap or a thread mill"),
            _ => {}
        }
        if self.kind == ToolKind::TSlot && self.shank_diameter.is_some_and(|neck| neck >= self.diameter) {
            return invalid("a T-slot cutter's neck is thinner than its disc");
        }
        let torch = [self.pierce_height, self.cut_height, self.pierce_delay];
        if self.kind != ToolKind::Plasma {
            if torch.iter().any(Option::is_some) {
                return invalid("a pierce height, cut height and pierce delay belong to a plasma torch");
            }
            return Ok(());
        }
        let (Some(pierce), Some(cut), Some(delay)) = (self.pierce_height, self.cut_height, self.pierce_delay) else {
            return invalid("a plasma torch needs its pierce height, cut height and pierce delay");
        };
        if !(cut > 0.0 && cut <= 100.0) || !(pierce >= cut && pierce <= 100.0) {
            return invalid("a plasma torch cuts above 0 and up to 100 mm over the sheet, and pierces no lower than it cuts");
        }
        if !(0.0..=60.0).contains(&delay) {
            return invalid("a plasma torch's pierce delay is from 0 to 60 seconds");
        }
        Ok(())
    }
}

/// What marks a tool library file, so any other JSON is refused rather than half-read.
pub const LIBRARY_FORMAT: &str = "solidcraft-tools";
/// The newest library version this build reads.
pub const LIBRARY_VERSION: u32 = 1;
/// Most tools in one file.
pub const MAX_LIBRARY_TOOLS: usize = 10_000;
/// Most text [`ToolLibraryFile::from_json`] reads (bytes).
pub const MAX_LIBRARY_TEXT: usize = 16 * 1024 * 1024;

/// A tool library as a file: `{"format":"solidcraft-tools","version":1,"tools":[...]}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolLibraryFile {
    pub tools: Vec<Tool>,
}

/// What is written: the format marker, the version, then the tools in number order.
#[derive(Serialize)]
struct LibraryOut<'a> {
    format: &'static str,
    version: u32,
    tools: &'a [Tool],
}

impl ToolLibraryFile {
    /// Pretty JSON, tools sorted by number.
    pub fn to_json(&self) -> Result<String> {
        let mut tools = self.tools.clone();
        tools.sort_by_key(|tool| tool.number);
        let out = LibraryOut { format: LIBRARY_FORMAT, version: LIBRARY_VERSION, tools: &tools };
        serde_json::to_string_pretty(&out).map_err(|e| cam_error(format!("the tool library cannot be written as JSON: {e}")))
    }

    /// Reads a tool library, refusing another `format`, a newer `version`, more tools than are
    /// kept, a file too large to read, duplicate numbers and tools that are not valid.
    pub fn from_json(text: &str) -> Result<ToolLibraryFile> {
        if text.len() > MAX_LIBRARY_TEXT {
            return Err(CamError::TooLarge(format!("the tool library is larger than {MAX_LIBRARY_TEXT} bytes")));
        }
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| cam_error(format!("the tool library is not valid JSON: {e}")))?;
        let Some(object) = value.as_object() else {
            return invalid("a tool library is a JSON object");
        };
        match object.get("format").and_then(|format| format.as_str()) {
            Some(LIBRARY_FORMAT) => {}
            _ => return invalid(format!("not a SolidCraft tool library: `format` must be \"{LIBRARY_FORMAT}\"")),
        }
        let version = object.get("version").and_then(|version| version.as_u64()).unwrap_or(0);
        if version > u64::from(LIBRARY_VERSION) {
            return invalid(format!("the tool library is version {version}; this build reads up to {LIBRARY_VERSION}"));
        }
        let Some(list) = object.get("tools").and_then(|tools| tools.as_array()) else {
            return invalid("the tool library has no `tools` list");
        };
        if list.len() > MAX_LIBRARY_TOOLS {
            return Err(CamError::TooLarge(format!("the tool library has more than {MAX_LIBRARY_TOOLS} tools")));
        }
        let mut tools: Vec<Tool> = Vec::with_capacity(list.len());
        let mut seen = std::collections::BTreeSet::new();
        for value in list {
            let tool: Tool = serde_json::from_value(value.clone()).map_err(|e| cam_error(format!("a tool in the library is not valid: {e}")))?;
            if !seen.insert(tool.number) {
                return invalid(format!("tool {} is in the file twice", tool.number));
            }
            if let Err(error) = tool.validate() {
                return invalid(format!("tool {}: {error}", tool.number));
            }
            tools.push(tool);
        }
        Ok(ToolLibraryFile { tools })
    }
}

/// A JSON problem as this crate's error.
fn cam_error(why: String) -> CamError {
    CamError::Invalid(why)
}
