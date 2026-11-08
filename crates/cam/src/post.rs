//! Post processors: how a program is written for one kind of controller. A post is data (it
//! can be read from JSON), not code: line templates around the motion blocks, and a few
//! switches for how the motion itself is written.
//!
//! Templates may use `{program}` (program name), `{units}` (`G21` or `G20`), `{work_offset}`
//! (the setup's work offset, `G54` to `G59`), `{tool}` (tool number), `{diameter}` (tool
//! diameter in output units) and `{rpm}` (spindle speed); the dwell template uses `{seconds}` or
//! `{milliseconds}`.

use serde::{Deserialize, Serialize};

use crate::{CamError, Result, invalid};

/// Most lines in one template block.
const MAX_TEMPLATE_LINES: usize = 64;
/// Longest template line.
const MAX_TEMPLATE_LEN: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Mm,
    Inch,
}

impl Units {
    /// Millimetres per output unit.
    pub fn mm(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Inch => 25.4,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Units::Mm => "G21",
            Units::Inch => "G20",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentStyle {
    /// `(comment)`
    Parentheses,
    /// `; comment`
    Semicolon,
    /// No comments are written.
    None,
}

/// The language a post writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// G-code (RS-274 and its relatives): the templates and switches below apply.
    #[default]
    GCode,
    /// Heidenhain's conversational language: numbered blocks, `L`, `CC`/`C`, `TOOL CALL`.
    /// Only the units, decimals, comments, arcs and arc tolerance of the post apply.
    Klartext,
}

/// How a post writes a tapped hole: the spindle reverses at the bottom in step with the feed,
/// so it is always one block of the controller's rigid tapping, never plain moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tapping {
    /// The controller cannot tap: a program with a tapped hole is refused.
    #[default]
    None,
    /// `G98 G84 X Y Z R F` and `G80` (Fanuc, Mach3, Centroid), feed one pitch a turn.
    G84,
    /// `M29 S` then `G98 G84 X Y Z R F` and `G80`: Fanuc rigid tapping.
    G84Rigid,
    /// `G33.1 Z K` from the retract height, which it returns to (LinuxCNC).
    G33_1,
    /// `SPOS=0`, `G331 Z K S` in and `G332 Z K` out (Sinumerik tapping without a compensating chuck).
    G331,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineNumbers {
    pub start: u32,
    pub step: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub name: String,
    /// File extension without the dot.
    pub extension: String,
    pub units: Units,
    /// Decimals of coordinates (at most 6).
    pub decimals: u8,
    /// Decimals of feed rates (at most 4).
    pub feed_decimals: u8,
    #[serde(default)]
    pub line_numbers: Option<LineNumbers>,
    pub comments: CommentStyle,
    /// Written once at the start.
    pub header: Vec<String>,
    /// Written before the first operation of each tool.
    pub tool_change: Vec<String>,
    /// Whether `tool_change` is also written for the program's first tool.
    pub tool_change_first: bool,
    pub spindle_on: Vec<String>,
    pub spindle_off: Vec<String>,
    pub coolant_on: Vec<String>,
    pub coolant_off: Vec<String>,
    /// Written once at the end.
    pub footer: Vec<String>,
    /// Whether arcs are written as `G2`/`G3` with `I` `J`; otherwise as short lines.
    pub arcs: bool,
    /// Chord error of arcs written as lines (mm).
    pub arc_tolerance: f64,
    /// Whether an arc that also moves in Z (a helix, as ramps make) is written as an arc; otherwise as short lines.
    #[serde(default = "yes")]
    pub helical_arcs: bool,
    /// Whether whole numbers are written with a decimal point (`X10.`), for controllers that
    /// read `X10` as ten of their smallest unit.
    #[serde(default)]
    pub trailing_decimal: bool,
    /// Whether motion codes are written with two digits (`G01` rather than `G1`).
    #[serde(default)]
    pub two_digit_codes: bool,
    /// How a dwell is written: `{seconds}` or `{milliseconds}` (a whole number) stands for its length.
    #[serde(default = "default_dwell")]
    pub dwell: String,
    /// Whether each drilled hole is written as one canned cycle (`G81`, `G82` with a dwell,
    /// `G83` with pecks, in `G98` and cancelled by `G80`); otherwise move by move. A pecked hole
    /// with a dwell is always written move by move, as `G83` has no dwell.
    #[serde(default)]
    pub canned_cycles: bool,
    /// How tapped holes are written; `none` refuses them.
    #[serde(default)]
    pub tapping: Tapping,
    #[serde(default)]
    pub language: Language,
}

fn yes() -> bool {
    true
}

fn default_dwell() -> String {
    "G4 P{seconds}".into()
}

fn lines(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

impl Post {
    /// Controllers that follow the NIST RS-274/NGC language closely (LinuxCNC and relatives):
    /// automatic tool change with tool length offsets.
    pub fn rs274() -> Post {
        Post {
            name: "rs274".into(),
            extension: "ngc".into(),
            units: Units::Mm,
            decimals: 4,
            feed_decimals: 1,
            line_numbers: None,
            comments: CommentStyle::Parentheses,
            header: lines(&["G17 {units} G40 G49 {work_offset} G80 G90 G94"]),
            tool_change: lines(&["M5", "T{tool} M6", "G43 H{tool}"]),
            tool_change_first: true,
            spindle_on: lines(&["S{rpm} M3"]),
            spindle_off: lines(&["M5"]),
            coolant_on: lines(&["M8"]),
            coolant_off: lines(&["M9"]),
            footer: lines(&["M30"]),
            arcs: true,
            arc_tolerance: 0.005,
            helical_arcs: true,
            trailing_decimal: false,
            two_digit_codes: false,
            dwell: default_dwell(),
            canned_cycles: false,
            tapping: Tapping::G33_1,
            language: Language::GCode,
        }
    }

    /// The built-in post called `name`.
    /// LinuxCNC: incremental arc centres and exact paths are explicitly established,
    /// independent of the previously loaded program's modal state.
    pub fn linuxcnc() -> Post {
        let mut post = Self::rs274();
        post.name = "linuxcnc".into();
        post.header.push("G91.1 G61 G97".into());
        post.header.push("M5 M9".into());
        post
    }

    /// GRBL (public GRBL 1.1 G-code notes): no tool change at all — the operator fits the tool,
    /// so the program only stops the spindle and says which one — and no tool length offset
    /// (`G43`); moves are written the same way, peck by peck, so no canned cycle is used.
    /// Dwells are `G4 P` in seconds, arcs take relative `I` `J` like every GRBL program, and
    /// work offsets are `G54` to `G59`.
    pub fn grbl() -> Post {
        Post {
            name: "grbl".into(),
            extension: "nc".into(),
            units: Units::Mm,
            decimals: 3,
            feed_decimals: 1,
            line_numbers: None,
            comments: CommentStyle::Parentheses,
            header: lines(&["G17 G21 G90 G94 {work_offset}"]),
            // No `M6`: GRBL has no tool changer. The comment tells the operator which tool.
            tool_change: lines(&["M5", "(change tool T{tool})"]),
            tool_change_first: true,
            spindle_on: lines(&["S{rpm} M3"]),
            spindle_off: lines(&["M5"]),
            coolant_on: lines(&["M8"]),
            coolant_off: lines(&["M9"]),
            footer: lines(&["M30"]),
            arcs: true,
            arc_tolerance: 0.005,
            helical_arcs: true,
            trailing_decimal: false,
            two_digit_codes: false,
            dwell: default_dwell(),
            canned_cycles: false,
            tapping: Tapping::None,
            language: Language::GCode,
        }
    }

    /// Mach3 (public Mach3 G-code reference): tool change `M6 T`, tool length offset `G43 H`,
    /// cutter and length compensation off, canned cycles cancelled, dwells `G4 P` in seconds.
    /// Every number carries a decimal point, so `X10` can never read as ten machine steps.
    pub fn mach3() -> Post {
        let mut post = Self::rs274();
        post.name = "mach3".into();
        post.extension = "tap".into();
        post.header = lines(&["G17 G21 G40 G49 G80 G90 G94 {work_offset}"]);
        post.tool_change = lines(&["M5", "M6 T{tool}", "G43 H{tool}"]);
        post.trailing_decimal = true;
        post.tapping = Tapping::G84;
        post
    }

    /// Centroid (public Centroid G-code programming manual): `T M6` tool change with `G43 H`,
    /// compensation off, dwells `G4 P` in seconds, decimals on every number.
    pub fn centroid() -> Post {
        let mut post = Self::rs274();
        post.name = "centroid".into();
        post.extension = "nc".into();
        post.header = lines(&["G17 G21 G40 G49 G80 G90 G94 {work_offset}"]);
        post.tool_change = lines(&["M5", "T{tool} M6", "G43 H{tool}"]);
        post.trailing_decimal = true;
        post.tapping = Tapping::G84;
        post
    }

    /// Fanuc 0i/30i style (public Fanuc programming manuals): program between `%` lines with an
    /// `O` number, two-digit codes, a decimal point on every number (Fanuc reads `X10` as 10
    /// micrometres), `T M06` with `G43 H`, canned drilling cycles, dwells `G04 X` in seconds and
    /// rigid tapping with `M29`.
    pub fn fanuc() -> Post {
        Post {
            name: "fanuc".into(),
            extension: "nc".into(),
            units: Units::Mm,
            decimals: 3,
            feed_decimals: 1,
            line_numbers: None,
            comments: CommentStyle::Parentheses,
            header: lines(&["%", "O0001 ({program})", "G17 {units} G40 G49 G80 G90 G94 {work_offset}"]),
            tool_change: lines(&["M05", "T{tool} M06", "G43 H{tool}"]),
            tool_change_first: true,
            spindle_on: lines(&["S{rpm} M03"]),
            spindle_off: lines(&["M05"]),
            coolant_on: lines(&["M08"]),
            coolant_off: lines(&["M09"]),
            footer: lines(&["M30", "%"]),
            arcs: true,
            arc_tolerance: 0.005,
            helical_arcs: true,
            trailing_decimal: true,
            two_digit_codes: true,
            dwell: "G04 X{seconds}".into(),
            canned_cycles: true,
            tapping: Tapping::G84Rigid,
            language: Language::GCode,
        }
    }

    /// Siemens Sinumerik 808D/828D/840D in its own language (public Sinumerik programming
    /// manuals): `;` comments, `G71` millimetres, `T D1` and `M6`, dwells `G4 F` in seconds,
    /// arcs with incremental `I` `J`, holes move by move (no `CYCLE8x`), tapping with `G331`/`G332`.
    pub fn sinumerik() -> Post {
        Post {
            name: "sinumerik".into(),
            extension: "mpf".into(),
            units: Units::Mm,
            decimals: 3,
            feed_decimals: 1,
            line_numbers: None,
            comments: CommentStyle::Semicolon,
            header: lines(&["G17 G71 G40 G90 G94 {work_offset}"]),
            tool_change: lines(&["M5", "T{tool} D1", "M6"]),
            tool_change_first: true,
            spindle_on: lines(&["S{rpm} M3"]),
            spindle_off: lines(&["M5"]),
            coolant_on: lines(&["M8"]),
            coolant_off: lines(&["M9"]),
            footer: lines(&["M30"]),
            arcs: true,
            arc_tolerance: 0.005,
            helical_arcs: true,
            trailing_decimal: false,
            two_digit_codes: false,
            dwell: "G4 F{seconds}".into(),
            canned_cycles: false,
            tapping: Tapping::G331,
            language: Language::GCode,
        }
    }

    /// Heidenhain TNC in conversational Klartext: numbered blocks, `TOOL CALL`, `L` lines with
    /// `FMAX` rapids, `CC`/`C` arcs, cycle 9 dwells; helices as short lines, holes move by move.
    /// The control's active preset is part zero. Not checked on a control yet: try the first
    /// programs in its test run.
    pub fn heidenhain() -> Post {
        Post {
            name: "heidenhain".into(),
            extension: "h".into(),
            units: Units::Mm,
            decimals: 3,
            feed_decimals: 0,
            line_numbers: None,
            comments: CommentStyle::Semicolon,
            header: Vec::new(),
            tool_change: Vec::new(),
            tool_change_first: true,
            spindle_on: Vec::new(),
            spindle_off: Vec::new(),
            coolant_on: Vec::new(),
            coolant_off: Vec::new(),
            footer: Vec::new(),
            arcs: true,
            arc_tolerance: 0.005,
            helical_arcs: false,
            trailing_decimal: false,
            two_digit_codes: false,
            dwell: default_dwell(),
            canned_cycles: false,
            tapping: Tapping::None,
            language: Language::Klartext,
        }
    }

    pub fn builtin(name: &str) -> Option<Post> {
        match name.trim().to_ascii_lowercase().as_str() {
            "centroid" => Some(Post::centroid()),
            "fanuc" => Some(Post::fanuc()),
            "grbl" => Some(Post::grbl()),
            "heidenhain" => Some(Post::heidenhain()),
            "linuxcnc" => Some(Post::linuxcnc()),
            "mach3" => Some(Post::mach3()),
            "rs274" => Some(Post::rs274()),
            "sinumerik" => Some(Post::sinumerik()),
            _ => None,
        }
    }

    pub const BUILTIN: &'static [&'static str] = &["centroid", "fanuc", "grbl", "heidenhain", "linuxcnc", "mach3", "rs274", "sinumerik"];

    /// Reads `text`, written through this post, back into moves with their line numbers (from 1).
    pub fn read_back(&self, text: &str) -> Result<Vec<(usize, crate::Move)>> {
        match self.language {
            Language::GCode => crate::gcode::parse_lines(text),
            Language::Klartext => crate::klartext::parse_lines(text),
        }
    }

    /// Whether the post writes the work offset (a `{work_offset}` in its header or tool change).
    pub fn writes_work_offset(&self) -> bool {
        self.header.iter().chain(&self.tool_change).any(|l| l.contains("{work_offset}"))
    }

    pub fn validate(&self) -> Result<()> {
        if self.decimals > 6 || self.feed_decimals > 4 {
            return invalid("a post writes at most 6 decimals for coordinates and 4 for feeds");
        }
        if !(self.arc_tolerance > 0.0) || !self.arc_tolerance.is_finite() {
            return invalid("the post's arc tolerance must be above 0");
        }
        if self.line_numbers.is_some_and(|n| n.step == 0) {
            return invalid("the post's line number step must be above 0");
        }
        if self.extension.is_empty() || self.extension.len() > 8 || !self.extension.chars().all(|c| c.is_ascii_alphanumeric()) {
            return invalid("the post's file extension must be 1 to 8 letters or digits");
        }
        if self.dwell.len() > MAX_TEMPLATE_LEN || self.dwell.chars().any(char::is_control) {
            return invalid("the post's dwell template is too long or has control characters");
        }
        fill(&self.dwell, &Values::default())?;
        for block in [&self.header, &self.tool_change, &self.spindle_on, &self.spindle_off, &self.coolant_on, &self.coolant_off, &self.footer] {
            if block.len() > MAX_TEMPLATE_LINES {
                return Err(CamError::TooLarge(format!("a post template has more than {MAX_TEMPLATE_LINES} lines")));
            }
            for line in block {
                if line.len() > MAX_TEMPLATE_LEN || line.chars().any(char::is_control) {
                    return invalid("a post template line is too long or has control characters");
                }
                // Catches unknown placeholders before any program is written.
                fill(line, &Values::default())?;
            }
        }
        Ok(())
    }
}

/// What the placeholders stand for.
#[derive(Clone, Debug, Default)]
pub(crate) struct Values {
    pub program: String,
    pub units: String,
    pub work_offset: String,
    pub tool: String,
    pub diameter: String,
    pub rpm: String,
    pub seconds: String,
    pub milliseconds: String,
}

/// `line` with its placeholders replaced.
pub(crate) fn fill(line: &str, v: &Values) -> Result<String> {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = after.find('}') else { return invalid(format!("post template `{line}`: `{{` without `}}`")) };
        let (name, tail) = after.split_at(close);
        out.push_str(match name.strip_prefix('{').unwrap_or(name) {
            "program" => &v.program,
            "units" => &v.units,
            "work_offset" => &v.work_offset,
            "tool" => &v.tool,
            "diameter" => &v.diameter,
            "rpm" => &v.rpm,
            "seconds" => &v.seconds,
            "milliseconds" => &v.milliseconds,
            other => return invalid(format!("post template `{line}`: unknown placeholder `{{{other}}}`")),
        });
        rest = tail.strip_prefix('}').unwrap_or(tail);
    }
    out.push_str(rest);
    Ok(out)
}
