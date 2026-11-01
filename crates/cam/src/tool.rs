//! Cutting tools, and a tool library as a file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cutting::{CuttingData, MAX_MATERIALS, valid_material};
use crate::{CamError, Result, invalid};

/// Largest tool number a post processor is asked to write.
pub const MAX_TOOL_NUMBER: u32 = 9999;
/// Largest tool diameter (mm).
pub const MAX_TOOL_DIAMETER: f64 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    FlatEndmill,
    BallEndmill,
    Drill,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "flat end mill",
            ToolKind::BallEndmill => "ball end mill",
            ToolKind::Drill => "drill",
        }
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
        }
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
        self.cutting.values().try_for_each(CuttingData::validate)
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
