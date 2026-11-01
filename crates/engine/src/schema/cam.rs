//! Contracts for CAM drilling and the guided LinuxCNC job form.
use super::NumberExpr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ToolParams {
    pub number: u32,
    pub diameter: f64,
    pub flute_length: f64,
    pub kind: Option<String>,
    pub description: Option<String>,
    pub flutes: Option<u32>,
    pub spindle_rpm: Option<f64>,
    pub feed: Option<f64>,
    pub plunge_feed: Option<f64>,
    pub stepdown: Option<f64>,
    pub stepover: Option<f64>,
    /// Cutting data by material group: `{"aluminium": {vc, fz, ap, ae}, …}`.
    pub cutting: Option<std::collections::BTreeMap<String, CuttingParams>>,
}

/// vc m/min; fz mm per tooth (a drill: per revolution); ap and ae as shares of the diameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CuttingParams {
    pub vc: f64,
    pub fz: f64,
    pub ap: f64,
    pub ae: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ToolRemoveParams {
    pub number: u32,
}

/// A CNC machine's limits. Travel as [lowest, highest] position per axis (mm).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MachineParams {
    pub name: String,
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub z: [f64; 2],
    /// The rotary A axis: `{kind: "limited", min, max}` in degrees, or `{kind: "endless"}`.
    pub a: Option<serde_json::Value>,
    /// Slowest and fastest spindle speed (rev/min).
    pub spindle_min: f64,
    pub spindle_max: f64,
    /// Fastest feed (mm/min).
    pub max_feed: f64,
    /// The post its programs are written with: a built-in post or one saved on the design.
    pub post: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MachineRemoveParams {
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MachineResult {
    pub machine: serde_json::Value,
}

/// A post saved on the design: a built-in or saved post with some settings changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PostDefineParams {
    pub name: String,
    /// The built-in or saved post it starts from. Default: the saved post of this name, else `linuxcnc`.
    pub base: Option<String>,
    /// File extension without the dot (1 to 8 letters or digits).
    pub extension: Option<String>,
    /// `mm` or `inch`.
    pub units: Option<String>,
    /// Decimals of coordinates (0 to 6).
    pub decimals: Option<u32>,
    /// Decimals of feed rates (0 to 4).
    pub feed_decimals: Option<u32>,
    /// Whether every line gets an `N` number.
    pub line_numbers: Option<bool>,
    pub line_start: Option<u32>,
    pub line_step: Option<u32>,
    /// `parentheses`, `semicolon` or `none`.
    pub comments: Option<String>,
    /// Lines written once at the start (the safe start), with `{placeholders}`.
    pub header: Option<Vec<String>>,
    /// Lines written before the first operation of each tool.
    pub tool_change: Option<Vec<String>>,
    pub spindle_on: Option<Vec<String>>,
    pub spindle_off: Option<Vec<String>>,
    pub coolant_on: Option<Vec<String>>,
    pub coolant_off: Option<Vec<String>>,
    /// Lines written once at the end.
    pub footer: Option<Vec<String>>,
    /// Whether the tool change is also written for the program's first tool.
    pub tool_change_first: Option<bool>,
    /// Whether arcs are written as `G2`/`G3`; otherwise as short lines.
    pub arcs: Option<bool>,
    /// Chord error of arcs written as lines (mm).
    pub arc_tolerance: Option<f64>,
    /// Whether an arc that also moves in Z is written as an arc.
    pub helical_arcs: Option<bool>,
    /// Whether whole numbers carry a decimal point (`X10.`).
    pub trailing_decimal: Option<bool>,
    /// Whether motion codes have two digits (`G01`).
    pub two_digit_codes: Option<bool>,
    /// How a dwell is written: `{seconds}` or `{milliseconds}` stands for its length.
    pub dwell: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PostRemoveParams {
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PostDefineResult {
    pub post: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SetupParams {
    pub name: Option<String>,
    /// A new name for the existing setup `name`; its steps and settings stay.
    pub rename: Option<String>,
    /// Material group name, or null to clear it.
    pub material: Option<serde_json::Value>,
    /// Highest spindle speed (rev/min), or null to clear it.
    pub max_rpm: Option<serde_json::Value>,
    pub work_offset: Option<u8>,
    pub tool_axis: Option<[f64; 3]>,
    pub x_axis: Option<[f64; 3]>,
    pub work_origin: Option<[f64; 3]>,
    pub stock: Option<serde_json::Value>,
    pub delete: Option<bool>,
    pub stock_spec: Option<serde_json::Value>,
    pub wcs: Option<serde_json::Value>,
    pub wcs_orientation: Option<serde_json::Value>,
    pub wcs_origin_mode: Option<serde_json::Value>,
    pub box_point: Option<serde_json::Value>,
    pub flip_z: Option<bool>,
    pub flip_x: Option<bool>,
    pub model_bodies: Option<Vec<String>>,
    pub operation_type: Option<serde_json::Value>,
    pub post_opts: Option<serde_json::Value>,
    /// A machine's name, a machine as `cam.machine.define` takes it (copied into the design), or null.
    pub machine: Option<serde_json::Value>,
    /// The post a machine given in full names, in full as `cam.post.define` returns it (copied into the design), or null.
    pub machine_post: Option<serde_json::Value>,
    pub post: Option<serde_json::Value>,
    /// Programmed +Z out of the flat face picked here.
    pub z_face: Option<PickParams>,
    /// Programmed +X along the straight edge picked here (square to the tool axis).
    pub x_edge: Option<PickParams>,
    /// Part zero on the edge picked here: a round edge's centre, else the edge's middle.
    pub zero_edge: Option<PickParams>,
    /// Part zero axis by axis: each pick sets zero along that work axis only.
    pub zero_axes: Option<ZeroAxesParams>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ZeroAxesParams {
    pub x: Option<AxisPickParams>,
    pub y: Option<AxisPickParams>,
    pub z: Option<AxisPickParams>,
}

/// A point picked for one axis's zero; `edge`: on an edge (a round edge gives its centre).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AxisPickParams {
    pub body: String,
    pub at: [f64; 3],
    pub edge: Option<bool>,
}

/// A point picked on a body, in model coordinates.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PickParams {
    pub body: String,
    pub at: [f64; 3],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MillingParams {
    pub setup: String,
    pub name: String,
    pub tool: u32,
    pub sketch: Option<super::IdOrName>,
    pub profiles: Option<super::ProfilesParam>,
    pub faces: Option<serde_json::Value>,
    pub edges: Option<serde_json::Value>,
    /// The picked `edges` themselves, joined end to end into closed contours.
    pub chain: Option<bool>,
    /// A step's stored geometry, unchanged (as `cam.list` shows it).
    pub geometry: Option<serde_json::Value>,
    pub start: Option<[f64; 3]>,
    /// How the tool gets onto (off) each contour: `{kind: direct|arc|tangent|perpendicular,
    /// radius?, sweep?, length?}`.
    pub lead_in: Option<serde_json::Value>,
    pub lead_out: Option<serde_json::Value>,
    /// Closed contours: how far the cut runs on past its start before the lead-out.
    pub overlap: Option<NumberExpr>,
    /// Open contours (an edge chain that does not close): left, right or on, along the cut.
    pub open_side: Option<String>,
    /// Open contours: cut from the chain's end to its start.
    pub reverse: Option<bool>,
    /// Open contours: run on along the tangent before the start and after the end.
    pub extend_start: Option<NumberExpr>,
    pub extend_end: Option<NumberExpr>,
    /// Closed contours: holding tabs `{count?, spacing?, width, height}`, or null for none.
    pub tabs: Option<serde_json::Value>,
    pub outer: Option<Vec<[f64; 2]>>,
    pub holes: Option<Vec<Vec<[f64; 2]>>>,
    pub regions: Option<serde_json::Value>,
    pub side: Option<String>,
    pub climb: Option<bool>,
    /// Material left on the walls.
    pub stock_to_leave: Option<NumberExpr>,
    /// Material left on the floor.
    pub floor_stock: Option<NumberExpr>,
    /// Facing: how far the tool's edge runs past the stock.
    pub overrun: Option<NumberExpr>,
    /// Slot: its width (default the tool's diameter).
    pub width: Option<NumberExpr>,
    /// Facing: the material above the finished height (`bottom`); facing starts that high.
    pub allowance: Option<NumberExpr>,
    pub tolerance: Option<NumberExpr>,
    pub ramp_angle: Option<NumberExpr>,
    pub clearance: Option<NumberExpr>,
    pub retract: Option<NumberExpr>,
    pub top: Option<NumberExpr>,
    pub bottom: Option<NumberExpr>,
    pub stepdown: Option<NumberExpr>,
    pub stepover: Option<NumberExpr>,
    pub feed: Option<NumberExpr>,
    pub plunge_feed: Option<NumberExpr>,
    pub cut: Option<serde_json::Value>,
    pub spindle_rpm: Option<f64>,
    pub coolant: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RemoveParams {
    pub setup: String,
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PostParams {
    pub setup: String,
    pub post: Option<String>,
    pub program: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ListParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ChainParams {
    pub setup: String,
    /// Picked edges of one body, `{body, at}`.
    pub edges: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ChainResult {
    /// The chain in order, `{body, at, index}`.
    pub edges: Vec<serde_json::Value>,
    pub closed: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolResult {
    pub tool: serde_json::Value,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteResult {
    pub deleted: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SetupResult {
    Setup { setup: serde_json::Value },
    Delete { deleted: String },
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct PostResult {
    pub gcode: String,
    pub extension: String,
    pub post: String,
    pub operations: usize,
    pub work_offset: u8,
    pub readback: serde_json::Value,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResult {
    pub tools: Vec<serde_json::Value>,
    pub machines: Vec<serde_json::Value>,
    pub setups: Vec<serde_json::Value>,
    pub posts: Vec<String>,
    pub builtin_posts: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrillParams {
    pub setup: String,
    pub name: String,
    pub tool: u32,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    /// Picked bores: `{body, at, edge?}`; an edge gives its centre, a flat face its round holes.
    pub holes: Option<Vec<AxisPickParams>>,
    pub bottom: NumberExpr,
    pub clearance: Option<NumberExpr>,
    pub retract: Option<NumberExpr>,
    pub top: Option<NumberExpr>,
    pub stepdown: Option<NumberExpr>,
    pub feed: Option<NumberExpr>,
    pub plunge_feed: Option<NumberExpr>,
    pub spindle_rpm: Option<f64>,
    #[serde(default)]
    pub dwell: f64,
    #[serde(default)]
    pub coolant: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OperationResult {
    pub setup: String,
    pub operation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MoveParams {
    pub setup: String,
    pub name: String,
    /// The operation's new place in the machining order, from 0.
    pub to: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SkipParams {
    pub setup: String,
    pub name: String,
    /// True leaves the operation out when posting; false puts it back.
    pub skip: bool,
}

/// The setup's operations in machining order after the change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OrderResult {
    pub setup: String,
    pub order: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SpeedsParams {
    pub tool: u32,
    /// Material group; omitted, the setup's.
    pub material: Option<String>,
    /// Takes the material and the highest spindle speed from this setup.
    pub setup: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SpeedsResult {
    pub tool: u32,
    pub material: String,
    /// The cutting data used: the tool's own, else starting values.
    pub cutting: serde_json::Value,
    /// Whether these are the tool's own data (false: starting values to check).
    pub own: bool,
    pub spindle_rpm: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    pub stepdown: f64,
    pub stepover: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Contour,
    Pocket,
    Drill,
    /// Facing the setup's stock.
    Face,
    /// A slot along picked edges or contours.
    Slot,
}

/// Millimetres, mm/min and rpm. The rectangle is in setup work XY unless `sketch` or `faces`
/// pick the geometry; drill uses `points`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobParams {
    pub setup: String,
    pub name: String,
    pub kind: JobKind,
    pub tool: u32,
    pub diameter: f64,
    pub flute_length: f64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    /// Picked geometry instead of the rectangle: a sketch with `profiles`, planar `faces`
    /// (`{body, name, at}`) or the contours through picked `edges` (`{body, at}`), as
    /// `cam.op.contour` takes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<serde_json::Value>,
    /// The picked `edges` themselves, joined end to end into closed contours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<bool>,
    /// Contours: leads, overlap and the open-contour settings, as `cam.op.contour` takes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_in: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_out: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extend_start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extend_end: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<serde_json::Value>,
    /// A step's stored geometry, unchanged: what an edit keeps when nothing new is picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<serde_json::Value>,
    /// Sent by the dialogs with sketch profiles; a picked profile is always its own region here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_regions: Option<bool>,
    /// The setup's material group; omitted, the setup keeps its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Where a contour starts: each loop at its point nearest this model point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<[f64; 3]>,
    /// The contour's side: auto, outside, inside or on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// Climb milling (default); false: conventional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub climb: Option<bool>,
    /// Material left on the walls for a finishing pass (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_to_leave: Option<f64>,
    /// Material left on the floor for a finishing pass (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_stock: Option<f64>,
    /// Facing: how far the tool's edge runs past the stock (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrun: Option<f64>,
    /// Slot: its width (mm; default the tool's diameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_width: Option<f64>,
    pub top: f64,
    pub bottom: f64,
    pub clearance: f64,
    pub retract: f64,
    pub stepdown: f64,
    pub stepover: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    pub spindle_rpm: f64,
    #[serde(default)]
    pub coolant: bool,
    #[serde(default)]
    pub dwell: f64,
    pub work_offset: u8,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PreviewPoint {
    /// Setup work coordinates.
    pub position: [f64; 3],
    /// The same point in model coordinates, for drawing on the model.
    pub model: [f64; 3],
    pub cutting: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct JobResult {
    pub setup: String,
    pub operation: String,
    pub gcode: String,
    pub extension: String,
    pub points: Vec<PreviewPoint>,
    pub cut_minutes: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub setup: String,
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    pub path: String,
    pub bytes: usize,
}
