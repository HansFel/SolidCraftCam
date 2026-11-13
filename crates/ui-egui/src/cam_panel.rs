//! The CAM window: the job list (each setup's material, highest spindle speed and machining
//! steps in order, with skip, move and remove), the personal tool library with cutting data
//! by material, the personal machine list (travel, spindle speeds, fastest feed) and the
//! personal posts (a built-in post with its settings changed). The lists live in the preferences
//! and are shared by every design; a tool, machine or post used in a design is copied into it
//! (`cam.tool.define`, `cam.machine.define`, `cam.post.define`), so a shared file stays
//! complete. The ribbon's Job list (`cam.list`), Tool library (`cam.tool.define`) and Machine
//! list (`cam.machine.define`) open it.

use egui::RichText;
use serde::{Deserialize, Serialize, de};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::cam::{
    CommentStyle, CuttingData, LineNumbers, MATERIALS, Machine, MachineKind, Post, RotaryAxis, Tapping, Tool, ToolKind, Torch, Units,
};
use solidcraft_engine::doc::cam::{CamOperation, CamSettings, CamSetup, SetupOperationType};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::ui_kit;
use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr;

/// Most tools the library keeps.
const MAX_TOOLS: usize = 500;
/// Most machines the machine list keeps.
const MAX_MACHINES: usize = 100;
/// Most posts the post list keeps.
const MAX_POSTS: usize = 100;

/// The personal tool library: tools by number, each with its cutting data. A malformed row is
/// dropped without discarding the others.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Library(pub Vec<Tool>);

impl<'de> Deserialize<'de> for Library {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rows = Vec::<Value>::deserialize(deserializer)?;
        let mut library = Library::default();
        for row in rows.into_iter().take(MAX_TOOLS) {
            if let Ok(tool) = serde_json::from_value::<Tool>(row)
                && tool.validate().is_ok()
                && library.find(tool.number).is_none()
            {
                library.0.push(tool);
            }
        }
        library.0.sort_by_key(|t| t.number);
        Ok(library)
    }
}

impl Library {
    pub fn find(&self, number: u32) -> Option<&Tool> {
        self.0.iter().find(|t| t.number == number)
    }

    /// Adds `tool`, or replaces the library's tool with its number.
    pub fn put(&mut self, tool: Tool) -> Result<(), String> {
        tool.validate().map_err(|e| e.to_string())?;
        let full = self.0.len() >= MAX_TOOLS;
        match self.0.iter_mut().find(|t| t.number == tool.number) {
            Some(slot) => *slot = tool,
            None if full => return Err(format!("the library holds at most {MAX_TOOLS} tools")),
            None => self.0.push(tool),
        }
        self.0.sort_by_key(|t| t.number);
        Ok(())
    }
}

/// The personal machine list, by name. A malformed row is dropped without discarding the others.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Machines(pub Vec<Machine>);

impl<'de> Deserialize<'de> for Machines {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rows = Vec::<Value>::deserialize(deserializer)?;
        let mut list = Machines::default();
        for row in rows.into_iter().take(MAX_MACHINES) {
            if let Ok(machine) = serde_json::from_value::<Machine>(row)
                && machine.validate().is_ok()
                && list.find(&machine.name).is_none()
            {
                list.0.push(machine);
            }
        }
        list.0.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }
}

impl Machines {
    pub fn find(&self, name: &str) -> Option<&Machine> {
        self.0.iter().find(|m| m.name == name)
    }

    /// Whether `name` may be saved by the form that edits `original` (None for a new machine):
    /// each machine is made once, so a name another entry has is refused.
    pub fn name_free(&self, name: &str, original: Option<&str>) -> Result<(), String> {
        match self.find(name) {
            Some(_) if original != Some(name) => Err(tr("ui.cam.machine_exists").into()),
            _ => Ok(()),
        }
    }

    /// Adds `machine`, or replaces the list's machine of its name.
    pub fn put(&mut self, machine: Machine) -> Result<(), String> {
        machine.validate().map_err(|e| e.to_string())?;
        let full = self.0.len() >= MAX_MACHINES;
        match self.0.iter_mut().find(|m| m.name == machine.name) {
            Some(slot) => *slot = machine,
            None if full => return Err(format!("the list holds at most {MAX_MACHINES} machines")),
            None => self.0.push(machine),
        }
        self.0.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }
}

/// The personal posts, by name: built-in posts with their settings changed. A malformed row, or
/// one named like a built-in post, is dropped without discarding the others.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Posts(pub Vec<Post>);

impl<'de> Deserialize<'de> for Posts {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rows = Vec::<Value>::deserialize(deserializer)?;
        let mut list = Posts::default();
        for row in rows.into_iter().take(MAX_POSTS) {
            if let Ok(post) = serde_json::from_value::<Post>(row) {
                let _ = list.put(post);
            }
        }
        Ok(list)
    }
}

impl Posts {
    pub fn find(&self, name: &str) -> Option<&Post> {
        self.0.iter().find(|p| p.name == name)
    }

    /// Whether `name` may be saved by the form that edits `original` (None for a new post):
    /// not a built-in post's name, and not another listed post's.
    pub fn name_free(&self, name: &str, original: Option<&str>) -> Result<(), String> {
        if Post::builtin(name).is_some() {
            return Err(tr("ui.cam.post_builtin_name").into());
        }
        match self.find(name) {
            Some(_) if original != Some(name) => Err(tr("ui.cam.post_exists").into()),
            _ => Ok(()),
        }
    }

    /// Adds `post`, or replaces the list's post of its name.
    pub fn put(&mut self, post: Post) -> Result<(), String> {
        post.validate().map_err(|e| e.to_string())?;
        if post.name.trim().is_empty() || Post::builtin(&post.name).is_some() {
            return Err(tr("ui.cam.post_builtin_name").into());
        }
        let full = self.0.len() >= MAX_POSTS;
        match self.0.iter_mut().find(|p| p.name == post.name) {
            Some(slot) => *slot = post,
            None if full => return Err(format!("the list holds at most {MAX_POSTS} posts")),
            None => self.0.push(post),
        }
        self.0.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }
}

/// The parameters of `cam.post.define` for `post`: every setting, so nothing comes from a base.
pub fn post_params(post: &Post) -> Value {
    let mut p = serde_json::to_value(post).unwrap_or(Value::Null);
    if let Some(map) = p.as_object_mut() {
        map.remove("line_numbers");
        map.insert("line_numbers".into(), json!(post.line_numbers.is_some()));
        if let Some(n) = post.line_numbers {
            map.insert("line_start".into(), json!(n.start));
            map.insert("line_step".into(), json!(n.step));
        }
    }
    p
}

/// The personal post machine `machine` writes with, if it names one of `posts`.
pub fn machine_post<'a>(posts: &'a Posts, machine: &Machine) -> Option<&'a Post> {
    machine.post.as_deref().filter(|p| Post::builtin(p).is_none()).and_then(|p| posts.find(p))
}

/// Where each machine's programs go, by machine name: a local folder or the machine's share
/// (`\\linuxcnc\nc_files`). An entry that is not a usable folder name is dropped on reading.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProgramFolders(pub std::collections::BTreeMap<String, String>);

impl<'de> Deserialize<'de> for ProgramFolders {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let rows = std::collections::BTreeMap::<String, Value>::deserialize(deserializer)?;
        let mut folders = ProgramFolders::default();
        for (machine, folder) in rows.into_iter().take(MAX_MACHINES) {
            if let Some(folder) = folder.as_str() {
                let _ = folders.set(&machine, folder);
            }
        }
        Ok(folders)
    }
}

impl ProgramFolders {
    /// The folder `machine`'s programs go to, if one is set.
    pub fn get(&self, machine: &str) -> Option<&str> {
        self.0.get(machine).map(String::as_str)
    }

    /// Sets `machine`'s folder; an empty one removes it. At most 1024 characters, no control characters.
    pub fn set(&mut self, machine: &str, folder: &str) -> Result<(), String> {
        let folder = folder.trim();
        if folder.is_empty() {
            self.0.remove(machine);
            return Ok(());
        }
        if machine.is_empty() || folder.chars().count() > 1024 || folder.chars().any(char::is_control) {
            return Err(tr("ui.cam.folder_invalid").into());
        }
        self.0.insert(machine.to_string(), folder.to_string());
        Ok(())
    }
}

/// The parts the design's setups make, in the order their first setup comes; None is the
/// design's one part, for setups that name none.
pub(crate) fn parts(cam: &CamSettings) -> Vec<Option<String>> {
    let mut parts: Vec<Option<String>> = Vec::new();
    for setup in &cam.setups {
        if !parts.contains(&setup.part) {
            parts.push(setup.part.clone());
        }
    }
    parts
}

/// The posts that write for machines of `kind`: the built-in ones, the design's, then the
/// personal ones the design lacks (with the post in full, to copy into the design).
pub(crate) fn posts_for(app: &SolveApp, kind: MachineKind) -> Vec<(String, Option<Post>)> {
    let cam = &app.session.doc.cam;
    let mut out: Vec<(String, Option<Post>)> =
        Post::BUILTIN.iter().filter_map(|n| Post::builtin(n)).filter(|p| p.machine == kind).map(|p| (p.name, None)).collect();
    out.extend(cam.posts.iter().filter(|p| p.machine == kind).map(|p| (p.name.clone(), None)));
    out.extend(
        app.preferences.cam_posts.0.iter().filter(|p| p.machine == kind && cam.post(&p.name).is_none()).map(|p| (p.name.clone(), Some(p.clone()))),
    );
    out
}

/// A kind of machine as the user reads it.
pub(crate) fn machine_kind_label(kind: MachineKind) -> &'static str {
    match kind {
        MachineKind::Mill => tr("ui.cam.machine_mill"),
        MachineKind::Lathe => tr("ui.cam.machine_lathe"),
        MachineKind::Plasma => tr("ui.cam.machine_plasma"),
    }
}

/// What a setup does as the user reads it.
pub(crate) fn setup_type_label(kind: SetupOperationType) -> String {
    match crate::dialogs_cam_setup::TYPES.iter().find(|(t, _)| *t == kind) {
        Some((_, key)) => tr(key).to_string(),
        None => kind.key().replace('_', " "),
    }
}

/// The post setup `setup`'s programs are written with: its own, its machine's, else its kind's default.
pub(crate) fn setup_post(cam: &CamSettings, setup: &CamSetup) -> String {
    setup.post.clone().or_else(|| cam.setup_machine(setup).and_then(|m| m.post.clone())).unwrap_or_else(|| match setup.operation_type {
        SetupOperationType::Plasma => "plasma".into(),
        _ => "rs274".into(),
    })
}

/// The program file's name for setup `setup`: characters a file name cannot hold become `_`.
fn program_file(setup: &str, extension: &str) -> String {
    let stem: String = setup.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let stem = stem.trim().trim_end_matches('.');
    format!("{}.{extension}", if stem.is_empty() { "program" } else { stem })
}

/// The extension the post of setup `setup` writes: the setup's post, else its machine's, else LinuxCNC.
fn program_extension(app: &SolveApp, setup: &str) -> String {
    let cam = &app.session.doc.cam;
    let post = cam.setup(setup).map(|s| setup_post(cam, s)).unwrap_or_else(|| "linuxcnc".into());
    cam.post(&post).map(|p| p.extension.clone()).or_else(|| Post::builtin(&post).map(|p| p.extension)).unwrap_or_else(|| "ngc".into())
}

/// The folder setup `setup`'s machine writes its programs to, if one is set.
fn program_folder(app: &SolveApp, setup: &str) -> Option<String> {
    let cam = &app.session.doc.cam;
    let machine = cam.setup(setup).and_then(|s| s.machine.as_deref())?;
    app.preferences.cam_folders.get(machine).map(str::to_string)
}

/// A post's name as the user reads it: the controller's own spelling for the built-in ones.
pub(crate) fn post_label(name: &str) -> String {
    match name {
        "centroid" => "Centroid".into(),
        "fanuc" => "Fanuc".into(),
        "grbl" => "GRBL".into(),
        "heidenhain" => "Heidenhain".into(),
        "linuxcnc" => "LinuxCNC".into(),
        "mach3" => "Mach3".into(),
        "plasma" => "Plasma (RS-274)".into(),
        "rs274" => "RS-274".into(),
        "sinumerik" => "Sinumerik".into(),
        other => other.to_string(),
    }
}

/// One line naming a machine and its limits.
pub fn machine_label(m: &Machine) -> String {
    let span = |[lo, hi]: [f64; 2]| number(hi - lo);
    // A plasma table: its bed, fastest feed and torch; it has no spindle.
    if m.kind == MachineKind::Plasma {
        let mut line = format!("{} – {}, {} × {} mm, vf {}", m.name, machine_kind_label(m.kind), span(m.x), span(m.y), number(m.max_feed));
        if let Some(t) = &m.torch {
            line.push_str(&format!(", {} {}", tr("ui.cam.kerf"), number(t.kerf)));
        }
        if let Some(post) = &m.post {
            line.push_str(&format!(", {}", post_label(post)));
        }
        return line;
    }
    let mut line = format!(
        "{} – {} × {} × {} mm, n {}–{}, vf {}",
        m.name,
        span(m.x),
        span(m.y),
        span(m.z),
        number(m.spindle_min),
        number(m.spindle_max),
        number(m.max_feed)
    );
    match m.a {
        Some(RotaryAxis::Endless) => line.push_str(&format!(", A {}", tr("ui.cam.a_endless"))),
        Some(RotaryAxis::Limited { min, max }) => line.push_str(&format!(", A {}…{}°", number(min), number(max))),
        None => {}
    }
    if let Some(post) = &m.post {
        line.push_str(&format!(", {}", post_label(post)));
    }
    line
}

/// The parameters of `cam.tool.define` for `tool`.
pub fn define_params(tool: &Tool) -> Value {
    serde_json::to_value(tool).unwrap_or(Value::Null)
}

/// A material group's name in the interface; a library's own names stay as they are.
pub fn material_label(material: &str) -> String {
    if MATERIALS.contains(&material) { tr(&format!("ui.cam.mat.{material}")).to_string() } else { material.to_string() }
}

/// One line naming a tool: number, type, diameter, flutes, description.
pub fn tool_label(tool: &Tool) -> String {
    let mut line = format!("T{} {} Ø{} z{}", tool.number, tr(kind_key(tool.kind)), number(tool.diameter), tool.flutes);
    if !tool.description.is_empty() {
        line.push_str(" – ");
        line.push_str(&tool.description);
    }
    line
}

fn kind_key(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::FlatEndmill => "ui.cam.kind.flat",
        ToolKind::BallEndmill => "ui.cam.kind.ball",
        ToolKind::Drill => "ui.cam.kind.drill",
        ToolKind::BullNose => "ui.cam.kind.bull_nose",
        ToolKind::Chamfer => "ui.cam.kind.chamfer",
        ToolKind::VBit => "ui.cam.kind.v_bit",
        ToolKind::TSlot => "ui.cam.kind.t_slot",
        ToolKind::CentreDrill => "ui.cam.kind.centre_drill",
        ToolKind::Reamer => "ui.cam.kind.reamer",
        ToolKind::Tap => "ui.cam.kind.tap",
        ToolKind::ThreadMill => "ui.cam.kind.thread_mill",
        ToolKind::Plasma => "ui.cam.kind.plasma",
    }
}

const KINDS: &[ToolKind] = ToolKind::ALL;

/// The optional sizes the tool form shows: field, label key, and the kinds that use it.
const SIZES: &[(usize, &str, fn(ToolKind) -> bool)] = &[
    (0, "ui.cam.corner_radius", |k| k == ToolKind::BullNose),
    (1, "ui.cam.tip_angle", |k| k.has_cone() || k == ToolKind::Drill),
    (2, "ui.cam.tip_diameter", |k| matches!(k, ToolKind::Chamfer | ToolKind::VBit)),
    (3, "ui.cam.pitch", ToolKind::threads),
    (4, "ui.cam.shank_diameter", |_| true),
    (5, "ui.cam.overall_length", |_| true),
    (6, "ui.cam.stick_out", |_| true),
];

/// A size as typed: empty is none, else a number (a comma is a decimal point).
fn optional(text: &str) -> Result<Option<f64>, String> {
    let t = text.trim().replace(',', ".");
    if t.is_empty() { Ok(None) } else { t.parse::<f64>().map(Some).map_err(|_| tr("ui.cam.invalid_number").to_string()) }
}

fn number(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A tool as edited: text fields, and per material the four cutting values (empty: the starting value).
#[derive(Clone, Debug, Default)]
struct ToolEdit {
    number: String,
    kind: usize,
    diameter: String,
    flutes: String,
    flute_length: String,
    description: String,
    cutting: Vec<[String; 4]>,
    /// Corner radius, tip angle, tip diameter, pitch, shank diameter, overall length, stick-out.
    sizes: [String; 7],
    /// The holder's description, diameter and length; no diameter: no holder.
    holder: [String; 3],
}

impl ToolEdit {
    fn from_tool(tool: &Tool) -> ToolEdit {
        let cutting = MATERIALS
            .iter()
            .map(|m| match tool.cutting.get(*m) {
                Some(c) => [number(c.vc), number(c.fz), number(c.ap), number(c.ae)],
                None => Default::default(),
            })
            .collect();
        ToolEdit {
            number: tool.number.to_string(),
            kind: KINDS.iter().position(|k| *k == tool.kind).unwrap_or(0),
            diameter: number(tool.diameter),
            flutes: tool.flutes.to_string(),
            flute_length: number(tool.flute_length),
            description: tool.description.clone(),
            cutting,
            sizes: [tool.corner_radius, tool.tip_angle, tool.tip_diameter, tool.pitch, tool.shank_diameter, tool.overall_length, tool.stick_out]
                .map(|v| v.map(number).unwrap_or_default()),
            holder: match &tool.holder {
                Some(h) => [h.description.clone(), number(h.diameter), number(h.length)],
                None => Default::default(),
            },
        }
    }

    /// The tool these fields describe; `base` keeps what the form does not show (other materials, feeds).
    fn tool(&self, base: Option<&Tool>) -> Result<Tool, String> {
        let bad = || tr("ui.cam.invalid_number").to_string();
        let parse = |s: &str| s.trim().replace(',', ".").parse::<f64>().map_err(|_| bad());
        let number: u32 = self.number.trim().parse().map_err(|_| bad())?;
        let kind = KINDS.get(self.kind).copied().unwrap_or(ToolKind::FlatEndmill);
        let mut tool = base.cloned().unwrap_or_else(|| Tool::new(number, kind, 1.0, 1.0));
        (tool.number, tool.kind) = (number, kind);
        tool.diameter = parse(&self.diameter)?;
        tool.flute_length = parse(&self.flute_length)?;
        tool.flutes = self.flutes.trim().parse().map_err(|_| bad())?;
        tool.description = self.description.trim().chars().take(256).collect();
        // Sizes the kind does not use are dropped, so changing the kind never leaves one behind.
        let mut sizes = [None; 7];
        for &(i, _, used) in SIZES {
            if used(kind)
                && let Some(text) = self.sizes.get(i)
            {
                sizes[i] = optional(text)?;
            }
        }
        [tool.corner_radius, tool.tip_angle, tool.tip_diameter, tool.pitch, tool.shank_diameter, tool.overall_length, tool.stick_out] = sizes;
        tool.holder = match (optional(&self.holder[1])?, optional(&self.holder[2])?) {
            (None, None) => None,
            (Some(diameter), Some(length)) => {
                Some(solidcraft_engine::cam::ToolHolder { description: self.holder[0].trim().chars().take(256).collect(), diameter, length })
            }
            _ => return Err(tr("ui.cam.holder_needs_both").to_string()),
        };
        for (material, row) in MATERIALS.iter().zip(&self.cutting) {
            if row.iter().all(|v| v.trim().is_empty()) {
                tool.cutting.remove(*material);
                continue;
            }
            let start = CuttingData::starting(kind, material, tool.diameter);
            let value = |i: usize, fallback: Option<f64>| match row.get(i).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                Some(s) => parse(s),
                None => fallback.ok_or_else(bad),
            };
            let data = CuttingData {
                vc: value(0, start.map(|c| c.vc))?,
                fz: value(1, start.map(|c| c.fz))?,
                ap: value(2, start.map(|c| c.ap))?,
                ae: value(3, start.map(|c| c.ae))?,
            };
            tool.cutting.insert((*material).to_string(), data);
        }
        tool.validate().map_err(|e| e.to_string())?;
        Ok(tool)
    }
}

/// A machine as edited: name, travel X−, X+, Y−, Y+, Z−, Z+, the A axis (none, limited, endless)
/// with A− and A+, slowest and fastest spindle speed, fastest feed.
#[derive(Clone, Debug, Default)]
struct MachineEdit {
    name: String,
    /// What the machine does, as [`MachineKind::ALL`] lists it.
    kind: usize,
    /// A plasma table's kerf, feed, pierce height, cut height and pierce delay.
    torch: [String; 5],
    travel: [String; 6],
    a_kind: usize,
    a: [String; 2],
    spindle: [String; 2],
    max_feed: String,
    /// The spindle motor's rated power (kW); empty: unknown.
    power: String,
    /// The post its programs are written with; none: the default.
    post: Option<String>,
    /// Where its programs are written on this computer; empty: asked on each export.
    folder: String,
}

impl MachineEdit {
    fn from_machine(m: &Machine) -> MachineEdit {
        let (a_kind, a) = match m.a {
            None => (0, ["-360".into(), "360".into()]),
            Some(RotaryAxis::Limited { min, max }) => (1, [number(min), number(max)]),
            Some(RotaryAxis::Endless) => (2, ["-360".into(), "360".into()]),
        };
        let torch = m.torch.unwrap_or_default();
        MachineEdit {
            name: m.name.clone(),
            kind: MachineKind::ALL.iter().position(|k| *k == m.kind).unwrap_or(0),
            torch: [torch.kerf, torch.feed, torch.pierce_height, torch.cut_height, torch.pierce_delay].map(number),
            travel: [number(m.x[0]), number(m.x[1]), number(m.y[0]), number(m.y[1]), number(m.z[0]), number(m.z[1])],
            a_kind,
            a,
            spindle: [number(m.spindle_min), number(m.spindle_max)],
            max_feed: number(m.max_feed),
            power: m.spindle_power.map(number).unwrap_or_default(),
            post: m.post.clone(),
            folder: String::new(),
        }
    }

    fn machine(&self) -> Result<Machine, String> {
        let parse = |s: &str| s.trim().replace(',', ".").parse::<f64>().map_err(|_| tr("ui.cam.invalid_number").to_string());
        let t = |i: usize| self.travel.get(i).map_or(Ok(0.0), |s| parse(s));
        let a = match self.a_kind {
            1 => Some(RotaryAxis::Limited { min: parse(&self.a[0])?, max: parse(&self.a[1])? }),
            2 => Some(RotaryAxis::Endless),
            _ => None,
        };
        let kind = MachineKind::ALL.get(self.kind).copied().unwrap_or_default();
        let plasma = kind == MachineKind::Plasma;
        let torch = if plasma {
            let v = |i: usize| self.torch.get(i).map_or(Ok(0.0), |s| parse(s));
            Some(Torch { kerf: v(0)?, feed: v(1)?, pierce_height: v(2)?, cut_height: v(3)?, pierce_delay: v(4)? })
        } else {
            None
        };
        // A plasma table's spindle range only lets the torch's on and off through.
        let (spindle_min, spindle_max) = if plasma { (0.0, 1.0) } else { (parse(&self.spindle[0])?, parse(&self.spindle[1])?) };
        let machine = Machine {
            name: self.name.trim().to_string(),
            kind,
            torch,
            x: [t(0)?, t(1)?],
            y: [t(2)?, t(3)?],
            z: [t(4)?, t(5)?],
            a: if plasma { None } else { a },
            spindle_min,
            spindle_max,
            max_feed: parse(&self.max_feed)?,
            spindle_power: if plasma || self.power.trim().is_empty() { None } else { Some(parse(&self.power)?) },
            post: self.post.clone(),
        };
        machine.validate().map_err(|e| e.to_string())?;
        Ok(machine)
    }
}

/// A post's settings as the form types them. Code blocks are one line per row.
struct PostEdit {
    name: String,
    extension: String,
    /// 0: mm, 1: inch.
    units: usize,
    decimals: String,
    feed_decimals: String,
    line_numbers: bool,
    line_start: String,
    line_step: String,
    /// 0: parentheses, 1: semicolon, 2: none.
    comments: usize,
    arcs: bool,
    arc_tolerance: String,
    helical_arcs: bool,
    trailing_decimal: bool,
    two_digit_codes: bool,
    canned_cycles: bool,
    /// Place in [`TAPPING`].
    tapping: usize,
    /// G-code or Klartext, as the post it started from writes.
    language: solidcraft_engine::cam::Language,
    /// The kind of machine it writes for, as the post it started from does.
    machine: MachineKind,
    tool_change_first: bool,
    dwell: String,
    /// Cutter compensation words: tool left, tool right, off.
    comp: [String; 3],
    /// Program start, tool change, spindle on, spindle off, coolant on, coolant off, program end.
    blocks: [String; 7],
}

/// The code blocks' labels, in [`PostEdit::blocks`] order.
const POST_BLOCKS: [&str; 7] = [
    "ui.cam.post_header",
    "ui.cam.post_tool_change",
    "ui.cam.post_spindle_on",
    "ui.cam.post_spindle_off",
    "ui.cam.post_coolant_on",
    "ui.cam.post_coolant_off",
    "ui.cam.post_footer",
];

impl PostEdit {
    fn from_post(p: &Post) -> PostEdit {
        let numbers = p.line_numbers.unwrap_or(LineNumbers { start: 10, step: 10 });
        let block = |lines: &[String]| lines.join("\n");
        PostEdit {
            machine: p.machine,
            name: p.name.clone(),
            extension: p.extension.clone(),
            units: usize::from(p.units == Units::Inch),
            decimals: p.decimals.to_string(),
            feed_decimals: p.feed_decimals.to_string(),
            line_numbers: p.line_numbers.is_some(),
            line_start: numbers.start.to_string(),
            line_step: numbers.step.to_string(),
            comments: match p.comments {
                CommentStyle::Parentheses => 0,
                CommentStyle::Semicolon => 1,
                CommentStyle::None => 2,
            },
            arcs: p.arcs,
            arc_tolerance: number(p.arc_tolerance),
            helical_arcs: p.helical_arcs,
            trailing_decimal: p.trailing_decimal,
            two_digit_codes: p.two_digit_codes,
            canned_cycles: p.canned_cycles,
            tapping: TAPPING.iter().position(|(t, _)| *t == p.tapping).unwrap_or(0),
            language: p.language,
            tool_change_first: p.tool_change_first,
            dwell: p.dwell.clone(),
            comp: [p.comp_left.clone(), p.comp_right.clone(), p.comp_off.clone()],
            blocks: [
                block(&p.header),
                block(&p.tool_change),
                block(&p.spindle_on),
                block(&p.spindle_off),
                block(&p.coolant_on),
                block(&p.coolant_off),
                block(&p.footer),
            ],
        }
    }

    fn post(&self) -> Result<Post, String> {
        let whole = |s: &str| s.trim().parse::<u32>().map_err(|_| tr("ui.cam.invalid_number").to_string());
        let small = |s: &str| whole(s).and_then(|v| u8::try_from(v).map_err(|_| tr("ui.cam.invalid_number").to_string()));
        let lines = |s: &str| -> Vec<String> { s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect() };
        let [header, tool_change, spindle_on, spindle_off, coolant_on, coolant_off, footer] = &self.blocks;
        let post = Post {
            name: self.name.trim().to_string(),
            extension: self.extension.trim().trim_start_matches('.').to_string(),
            units: if self.units == 1 { Units::Inch } else { Units::Mm },
            decimals: small(&self.decimals)?,
            feed_decimals: small(&self.feed_decimals)?,
            line_numbers: if self.line_numbers { Some(LineNumbers { start: whole(&self.line_start)?, step: whole(&self.line_step)? }) } else { None },
            comments: match self.comments {
                1 => CommentStyle::Semicolon,
                2 => CommentStyle::None,
                _ => CommentStyle::Parentheses,
            },
            header: lines(header),
            tool_change: lines(tool_change),
            tool_change_first: self.tool_change_first,
            spindle_on: lines(spindle_on),
            spindle_off: lines(spindle_off),
            coolant_on: lines(coolant_on),
            coolant_off: lines(coolant_off),
            footer: lines(footer),
            arcs: self.arcs,
            arc_tolerance: self.arc_tolerance.trim().replace(',', ".").parse::<f64>().map_err(|_| tr("ui.cam.invalid_number").to_string())?,
            helical_arcs: self.helical_arcs,
            trailing_decimal: self.trailing_decimal,
            two_digit_codes: self.two_digit_codes,
            canned_cycles: self.canned_cycles,
            tapping: TAPPING.get(self.tapping).map_or(Tapping::None, |(t, _)| *t),
            language: self.language,
            machine: self.machine,
            dwell: self.dwell.trim().to_string(),
            comp_left: self.comp[0].trim().to_string(),
            comp_right: self.comp[1].trim().to_string(),
            comp_off: self.comp[2].trim().to_string(),
        };
        post.validate().map_err(|e| e.to_string())?;
        Ok(post)
    }
}

/// The window's state.
#[derive(Default)]
pub struct CamPanel {
    /// 0: job list, 1: tool library, 2: machines, 3: posts.
    pub tab: usize,
    /// The highest spindle speed as typed, by setup.
    rpm: Vec<(String, String)>,
    /// The library tool being edited (its number before the edit), or a new one.
    editing: Option<(Option<u32>, ToolEdit)>,
    /// The listed machine being edited (its name before the edit), or a new one.
    machine: Option<(Option<String>, MachineEdit)>,
    /// The listed post being edited (its name before the edit), or a new one.
    post: Option<(Option<String>, PostEdit)>,
    /// The built-in post a new one starts from, by its place in `Post::BUILTIN`.
    post_base: usize,
    pub(crate) error: Option<String>,
    /// Every step's toolpath for the view, and the design they were made for.
    paths: Option<(usize, Vec<solidcraft_engine::cmd::CamToolpath>)>,
    /// The setup whose G-code is shown.
    pub(crate) gcode: Option<GcodeView>,
    /// A tool library file read and waiting for the user to say which of its tools to take.
    pub(crate) import: Option<Vec<Tool>>,
    /// The setups whose toolpaths are not drawn, by name: with several setups the paths lie
    /// over each other and hide the model. A view choice of this window, not of the design.
    pub(crate) hidden: Vec<String>,
    /// The window is folded to its title and the setups' eyes, so the model behind it is free;
    /// the toolpaths stay drawn.
    pub(crate) folded: bool,
    /// The step being renamed in the job list (setup, step) and the name as typed.
    renaming: Option<(String, String, String)>,
}

/// How a post can write a tapped hole, as the post form offers it.
const TAPPING: &[(Tapping, &str)] = &[
    (Tapping::None, "ui.cam.tapping_none"),
    (Tapping::G84, "ui.cam.tapping_g84"),
    (Tapping::G84Rigid, "ui.cam.tapping_g84_rigid"),
    (Tapping::G33_1, "ui.cam.tapping_g33_1"),
    (Tapping::G331, "ui.cam.tapping_g331"),
];

/// The tool library file's name offered when it is exported.
const LIBRARY_FILE: &str = "tool-library.json";

/// Ask where to write the personal tool library as a file, and write it there.
pub(crate) fn export_library(app: &mut SolveApp) {
    let file = solidcraft_engine::cam::ToolLibraryFile { tools: app.preferences.cam_tools.0.clone() };
    let text = match file.to_json() {
        Ok(text) => text,
        Err(e) => return set_error(app, e.to_string()),
    };
    app.pick_save(LIBRARY_FILE, &["json"], move |app, path| {
        let Some(path) = path else { return };
        match solidcraft_engine::io::write_atomic(std::path::Path::new(&path), text.as_bytes(), true) {
            Ok(()) => app.set_status(solidcraft_ui_text::format("ui.cam.library_exported", &[("path", &path)]), false),
            Err(e) => set_error(app, format!("{path}: {e}")),
        }
    });
}

/// Ask for a tool library file and read it; the window then asks which of its tools to take.
pub(crate) fn import_library(app: &mut SolveApp) {
    app.pick_open_for(&["json"], |app, path| {
        let Some(path) = path else { return };
        match read_library(&path) {
            Ok(tools) => {
                if let Some(window) = app.cam_window.as_mut() {
                    window.import = Some(tools);
                    window.error = None;
                }
            }
            Err(e) => set_error(app, e),
        }
    });
}

fn read_library(path: &str) -> Result<Vec<Tool>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut text = String::new();
    file.take(solidcraft_engine::cam::MAX_LIBRARY_TEXT as u64 + 1).read_to_string(&mut text).map_err(|e| format!("{path}: {e}"))?;
    solidcraft_engine::cam::ToolLibraryFile::from_json(&text).map(|file| file.tools).map_err(|e| e.to_string())
}

/// Puts `tools` in the library: all of them, or with `replace` off only the numbers it lacks.
/// Returns how many were added and how many replaced.
pub(crate) fn take_tools(library: &mut Library, tools: &[Tool], replace: bool) -> Result<(usize, usize), String> {
    let (mut added, mut replaced) = (0, 0);
    let mut next = library.clone();
    for tool in tools {
        match next.find(tool.number) {
            Some(old) if old == tool => {}
            Some(_) if !replace => {}
            Some(_) => {
                next.put(tool.clone())?;
                replaced += 1;
            }
            None => {
                next.put(tool.clone())?;
                added += 1;
            }
        }
    }
    *library = next;
    Ok((added, replaced))
}

/// Shows `e` in the window, or in the status line when it is closed.
fn set_error(app: &mut SolveApp, e: String) {
    match app.cam_window.as_mut() {
        Some(window) => window.error = Some(e),
        None => app.set_status(e, true),
    }
}

/// A setup's program in the window: its lines, each motion line's move, the selected line.
pub(crate) struct GcodeView {
    setup: String,
    /// The design it was posted from.
    key: usize,
    program: Result<GcodeProgram, String>,
    /// The selected line (from 1).
    pub(crate) selected: Option<usize>,
    /// Scroll the list to the selected line on the next frame.
    scroll: bool,
    /// Playback: how far the tool has come along the program (mm of travel), shown in the
    /// view once set; whether it moves on by itself; and how fast, by its place in [`SPEEDS`].
    pub(crate) travelled: Option<f64>,
    pub(crate) playing: bool,
    pub(crate) speed: usize,
    /// The setup's stock after its steps, drawn in the view while set ([`draw_paths`]).
    pub(crate) sim: Option<Result<solidcraft_engine::cmd::CamSimulation, String>>,
    /// The text searched for in the lines.
    search: String,
    /// The program as edited by hand, while it is; exported as it is, never read back.
    pub(crate) edited: Option<String>,
}

/// Playback speeds (mm of tool travel per second).
pub(crate) const SPEEDS: [f64; 5] = [5.0, 20.0, 100.0, 500.0, 2500.0];

pub(crate) struct GcodeProgram {
    text: String,
    lines: Vec<String>,
    post: String,
    moves: Vec<solidcraft_engine::cmd::CamProgramMove>,
    /// The travel at the end of each move (mm), rising.
    ends: Vec<f64>,
    axis: Vec3,
    time: Option<solidcraft_engine::cam::TimeEstimate>,
}

impl GcodeProgram {
    fn total(&self) -> f64 {
        self.ends.last().copied().unwrap_or(0.0)
    }

    /// The move the tool is on after `travelled` mm, and where on it the tool is.
    fn at(&self, travelled: f64) -> Option<(usize, Vec3)> {
        let i = self.ends.partition_point(|e| *e < travelled).min(self.moves.len().checked_sub(1)?);
        let m = self.moves.get(i)?;
        let mut left = travelled - i.checked_sub(1).and_then(|k| self.ends.get(k)).copied().unwrap_or(0.0);
        for pair in m.points.windows(2) {
            let [a, b] = pair else { continue };
            let len = a.dist(*b);
            if left <= len && len > 0.0 {
                return Some((i, *a + (*b - *a) * (left / len).max(0.0)));
            }
            left -= len;
        }
        m.points.last().map(|p| (i, *p))
    }
}

impl GcodeView {
    fn new(app: &SolveApp, setup: &str) -> GcodeView {
        let program = app.session.cam_program(setup).map(|p| {
            let mut sum = 0.0;
            let ends = p
                .moves
                .iter()
                .map(|m| {
                    sum += m.points.windows(2).map(|w| if let [a, b] = w { a.dist(*b) } else { 0.0 }).sum::<f64>();
                    sum
                })
                .collect();
            GcodeProgram {
                lines: p.gcode.lines().map(str::to_string).collect(),
                text: p.gcode,
                post: chosen_post_label(app, setup),
                moves: p.moves,
                ends,
                axis: p.axis,
                time: p.time,
            }
        });
        GcodeView {
            setup: setup.to_string(),
            key: design_key(app),
            program,
            selected: None,
            scroll: false,
            travelled: None,
            playing: false,
            speed: 2,
            sim: None,
            search: String::new(),
            edited: None,
        }
    }

    /// The first line after the selected one (round to the start) that holds `search`, any case.
    fn find_next(&self) -> Option<usize> {
        let program = self.program.as_ref().ok()?;
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        let from = self.selected.unwrap_or(0);
        let n = program.lines.len();
        (1..=n).map(|k| (from + k - 1) % n + 1).find(|line| program.lines.get(line - 1).is_some_and(|l| l.to_lowercase().contains(&needle)))
    }

    /// Simulates the material removal of the setup's steps, or hides a shown simulation.
    pub(crate) fn toggle_sim(&mut self, app: &SolveApp) {
        self.sim = match self.sim {
            Some(_) => None,
            None => Some(app.session.cam_simulate(&self.setup, None)),
        };
    }

    /// Where the tool is in the playback: its move and the point, in model coordinates.
    pub(crate) fn tool_at(&self) -> Option<(usize, Vec3)> {
        let (Ok(program), Some(travelled)) = (&self.program, self.travelled) else { return None };
        program.at(travelled)
    }

    /// Starts or pauses the playback; at the end it starts again from the beginning.
    pub(crate) fn play(&mut self) {
        let Ok(program) = &self.program else { return };
        if self.playing {
            self.playing = false;
            return;
        }
        if self.travelled.is_none_or(|t| t >= program.total()) {
            self.travelled = Some(0.0);
        }
        self.playing = program.total() > 0.0;
    }

    /// Back to the program's start, paused.
    pub(crate) fn rewind(&mut self) {
        (self.travelled, self.playing, self.selected) = (Some(0.0), false, None);
    }

    /// Moves the tool on by `seconds` of playback, selecting the line it is on.
    pub(crate) fn advance(&mut self, seconds: f64) {
        let Ok(program) = &self.program else { return };
        if !self.playing {
            return;
        }
        let speed = SPEEDS.get(self.speed).copied().unwrap_or(SPEEDS[0]);
        let total = program.total();
        let next = (self.travelled.unwrap_or(0.0) + speed * seconds.clamp(0.0, 0.25)).min(total);
        self.travelled = Some(next);
        if next >= total {
            self.playing = false;
        }
        if let Some(line) = program.at(next).and_then(|(i, _)| program.moves.get(i)).map(|m| m.line)
            && self.selected != Some(line)
        {
            self.selected = Some(line);
            self.scroll = true;
        }
    }

    /// Puts the tool at the end of line `line`'s move, paused.
    fn go_to_line(&mut self, line: usize) {
        self.selected = Some(line);
        self.playing = false;
        let Ok(program) = &self.program else { return };
        if let Some(i) = program.moves.iter().rposition(|m| m.line == line) {
            self.travelled = program.ends.get(i).copied();
        }
    }

    /// The selected line's move.
    pub(crate) fn selected_move(&self) -> Option<&solidcraft_engine::cmd::CamProgramMove> {
        let (Ok(program), Some(line)) = (&self.program, self.selected) else { return None };
        program.moves.binary_search_by_key(&line, |m| m.line).ok().and_then(|i| program.moves.get(i))
    }

    /// Selects the move before (`-1`) or after (`1`) the selected line.
    pub(crate) fn step(&mut self, by: i32) {
        let Ok(program) = &self.program else { return };
        let line = self.selected.unwrap_or(0);
        let next = if by < 0 { program.moves.iter().rev().find(|m| m.line < line) } else { program.moves.iter().find(|m| m.line > line) };
        if let Some(line) = next.map(|m| m.line) {
            self.go_to_line(line);
            self.scroll = true;
        }
    }
}

/// Identifies the design: a new one after every change.
fn design_key(app: &SolveApp) -> usize {
    std::sync::Arc::as_ptr(&app.session.doc) as usize
}

/// The post setup `setup` is exported with, as the machine editor names it.
fn chosen_post_label(app: &SolveApp, setup: &str) -> String {
    let cam = &app.session.doc.cam;
    let name =
        cam.setup(setup).and_then(|s| s.post.clone().or_else(|| s.machine.as_deref().and_then(|m| cam.machine(m)).and_then(|m| m.post.clone())));
    post_label(name.as_deref().unwrap_or("linuxcnc"))
}

/// Open the window on `tab`.
pub fn open(app: &mut SolveApp, tab: usize) {
    let panel = app.cam_window.get_or_insert_with(CamPanel::default);
    panel.tab = tab;
}

/// What a click asks for, done after the window is drawn.
enum Action {
    Run(&'static str, Value),
    Export(String),
    Start(&'static str),
    /// Opens the setup form for this setup.
    EditSetup(String),
    /// Opens the milling form on this step (setup, step).
    EditStep(String, String),
    Library(Box<Tool>),
    Unlibrary(u32),
    ExportLibrary,
    ImportLibrary,
    /// Takes the read file's tools into the library (`true`: also those replacing a tool).
    TakeImport(bool),
    /// Saves the hand-edited program of a setup (setup, text) as it is.
    ExportText(String, String),
    /// Puts a machine on the personal list.
    Machine(Box<Machine>),
    /// Takes a machine off the personal list.
    Unmachine(String),
    /// Sets where a listed machine's programs go (machine, folder; empty: asked on each export).
    Folder(String, String),
    /// Puts a post on the personal list.
    Post(Box<Post>),
    /// Takes a post off the personal list.
    Unpost(String),
    /// Saves the tool list and setup sheet: `cam.sheet`'s choice of setups (one setup, a part's,
    /// or every one) and the file's name without extension.
    Sheet(Value, String),
}

/// Export setup `setup`'s program: straight into its machine's folder when one is set (an
/// existing program of that name is replaced), else where the save dialog says. A folder that
/// cannot be written (the machine is off) says so and offers the dialog, so nothing is lost.
pub(crate) fn export(app: &mut SolveApp, setup: &str) -> Result<(), String> {
    let extension = program_extension(app, setup);
    let file = program_file(setup, &extension);
    if let Some(folder) = program_folder(app, setup).filter(|_| !cfg!(target_arch = "wasm32")) {
        let path = std::path::Path::new(&folder).join(&file).to_string_lossy().into_owned();
        match app.run("cam.export", json!({"setup": setup, "path": path, "overwrite": true})) {
            Ok(_) => {
                app.set_status(solidcraft_ui_text::format("ui.cam.exported_to", &[("path", &path)]), false);
                return Ok(());
            }
            Err(e) => {
                app.pick_save_command(&file, &[&extension], "cam.export", json!({"setup": setup}));
                return Err(solidcraft_ui_text::format("ui.cam.folder_unreachable", &[("folder", &folder), ("error", &e)]));
            }
        }
    }
    app.pick_save_command(&file, &[&extension], "cam.export", json!({"setup": setup}));
    Ok(())
}

fn op_kind(op: &CamOperation) -> &'static str {
    match op {
        CamOperation::Contour { .. } => "cmd.cam.op.contour",
        CamOperation::Pocket { .. } => "cmd.cam.op.pocket",
        CamOperation::Drill { .. } => "cmd.cam.op.drill",
        CamOperation::Face { .. } => "cmd.cam.op.face",
        CamOperation::Slot { .. } => "cmd.cam.op.slot",
        CamOperation::Chamfer { .. } => "cmd.cam.op.chamfer",
        CamOperation::Engrave { .. } => "cmd.cam.op.engrave",
        CamOperation::HelicalBore { .. } => "cmd.cam.op.helix",
        CamOperation::ThreadMill { .. } => "cmd.cam.op.thread",
        CamOperation::Plasma { .. } => "cmd.cam.op.plasma",
    }
}

/// Shows the setup's toolpaths if they are hidden, hides them if they are shown.
fn toggle_paths(d: &mut CamPanel, setup: &str) {
    if d.hidden.iter().any(|n| n == setup) {
        d.hidden.retain(|n| n != setup);
    } else {
        d.hidden.push(setup.to_string());
    }
}

fn setup_rows(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, setup: &CamSetup, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        // The eye shows or hides the setup's toolpaths in the view, as the browser's eyes do bodies.
        let shown = !d.hidden.contains(&setup.name);
        let side = ui_kit::row_height();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let (icon, label) = if shown { ("eye", tr("ui.cam.hide_paths")) } else { ("suppress", tr("ui.cam.show_paths")) };
        // Every setup's eye has its own id: the icon and label alone are the same for all.
        if ui.push_id(("cam-eye", &setup.name), |ui| ui_kit::quiet_icon_button_at(ui, rect, icon, label)).inner.clicked() {
            toggle_paths(d, &setup.name);
        }
        ui.label(RichText::new(&setup.name).strong().color(if shown { t.text } else { t.text_dim }));
        if ui_kit::button(ui, tr("ui.cam.edit_setup"), ui_kit::Button::Small).clicked() {
            actions.push(Action::EditSetup(setup.name.clone()));
        }
        if ui_kit::button(ui, tr("ui.cam.delete_setup"), ui_kit::Button::Small).clicked() {
            actions.push(Action::Run("cam.setup", json!({"name": setup.name, "delete": true})));
        }
    });
    // What the setup does, the machine it runs on and the post that writes its program.
    let cam = &app.session.doc.cam;
    let machine =
        setup.machine.as_ref().map_or_else(|| tr("ui.cam.machine_none").to_string(), |n| cam.machine(n).map_or_else(|| n.clone(), machine_label));
    let line = format!(
        "{} · {}: {machine} · {}: {}",
        setup_type_label(setup.operation_type),
        tr("ui.cam.machine"),
        tr("ui.cam.post"),
        post_label(&setup_post(cam, setup))
    );
    ui.add(egui::Label::new(RichText::new(line).color(t.text_dim)).wrap());
    if setup.operation_type == SetupOperationType::Plasma {
        return step_rows(app, ui, d, setup, actions);
    }
    // Material: none, the six groups, and the setup's own name if it is another.
    let mut names: Vec<String> = MATERIALS.iter().map(|m| (*m).to_string()).collect();
    if let Some(m) = &setup.material
        && !names.contains(m)
    {
        names.push(m.clone());
    }
    let labels: Vec<String> = std::iter::once(tr("ui.cam.mat_none").to_string()).chain(names.iter().map(|m| material_label(m))).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let current = setup.material.as_ref().and_then(|m| names.iter().position(|n| n == m)).map_or(0, |i| i + 1);
    let mut chosen = current;
    ui_kit::choice_row(ui, tr("ui.cam.material"), &format!("cam-material-{}", setup.name), &mut chosen, &refs);
    if chosen != current {
        let material = chosen.checked_sub(1).and_then(|i| names.get(i)).map_or(Value::Null, |m| json!(m));
        actions.push(Action::Run("cam.setup", json!({"name": setup.name, "material": material})));
    }
    let slot = match d.rpm.iter().position(|(n, _)| *n == setup.name) {
        Some(i) => i,
        None => {
            d.rpm.push((setup.name.clone(), setup.max_rpm.map(number).unwrap_or_default()));
            d.rpm.len() - 1
        }
    };
    if let Some((_, text)) = d.rpm.get_mut(slot) {
        let (_, enter) = ui_kit::text_row(ui, tr("ui.cam.max_rpm"), text, tr("ui.cam.max_rpm_hint"));
        let typed = text.trim().replace(',', ".");
        let value = if typed.is_empty() { Some(Value::Null) } else { typed.parse::<f64>().ok().map(|v| json!(v)) };
        let stored = setup.max_rpm.map_or(Value::Null, |v| json!(v));
        if let Some(value) = value
            && value != stored
            && (enter || ui_kit::button(ui, tr("ui.cam.apply"), ui_kit::Button::Small).clicked())
        {
            actions.push(Action::Run("cam.setup", json!({"name": setup.name, "max_rpm": value})));
        }
    }
    step_rows(app, ui, d, setup, actions);
}

/// A setup's steps in order, each with its tool or torch and its buttons, then its program's.
fn step_rows(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, setup: &CamSetup, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    let count = setup.operations.len();
    if count == 0 {
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.no_steps")).color(t.text_dim)).wrap());
    }
    egui::Grid::new(format!("cam-steps-{}", setup.name)).num_columns(6).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        // The window truncates text; the steps' names, tools and buttons are read in full.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        for (i, op) in setup.operations.iter().enumerate() {
            let name = op.name().to_string();
            let mut on = !setup.skipped.contains(&name);
            if ui_kit::checkbox(ui, &format!("{}.", i + 1), &mut on).changed() {
                actions.push(Action::Run("cam.op.skip", json!({"setup": setup.name, "name": name, "skip": !on})));
            }
            let editing = d.renaming.as_mut().filter(|(s, n, _)| *s == setup.name && *n == name);
            if let Some((_, _, text)) = editing {
                // Enter renames; Escape or an unchanged name leaves it.
                let (enter, _) = ui_kit::text_field(ui, text, tr("ui.cam.rename"));
                if enter {
                    let typed = text.trim().to_string();
                    if !typed.is_empty() && typed != name {
                        actions.push(Action::Run("cam.op.rename", json!({"setup": setup.name, "name": name, "new_name": typed})));
                    }
                    d.renaming = None;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    d.renaming = None;
                }
            } else {
                let label = ui.add(egui::Label::new(RichText::new(&name).color(if on { t.text } else { t.text_dim })).sense(egui::Sense::click()));
                if label.on_hover_text(tr("ui.cam.rename_hint")).double_clicked() {
                    d.renaming = Some((setup.name.clone(), name.clone(), name.clone()));
                }
            }
            ui.label(RichText::new(tr(op_kind(op))).color(t.text_dim));
            let tool = app.session.doc.cam.tool(op.tool());
            let rpm = op.spindle_rpm().or(tool.and_then(|t| t.spindle_rpm));
            let mut info = match op {
                CamOperation::Plasma { torch, .. } => format!("{} {}", tr("ui.cam.kerf"), torch.kerf.as_str()),
                _ => tool.map_or(format!("T{}", op.tool()), |t| format!("T{} Ø{}", t.number, number(t.diameter))),
            };
            if let Some(rpm) = rpm {
                info.push_str(&format!("  n {}", number(rpm.round())));
            }
            info.push_str(&format!("  vf {}", op.cut().feed.as_str()));
            ui.label(info);
            ui.horizontal(|ui| {
                if ui_kit::button(ui, tr("ui.cam.edit_setup"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::EditStep(setup.name.clone(), name.clone()));
                }
                if ui_kit::button(ui, tr("ui.cam.up"), ui_kit::Button::Enabled(i > 0)).clicked() {
                    actions.push(Action::Run("cam.op.move", json!({"setup": setup.name, "name": name, "to": i.saturating_sub(1)})));
                }
                if ui_kit::button(ui, tr("ui.cam.down"), ui_kit::Button::Enabled(i + 1 < count)).clicked() {
                    actions.push(Action::Run("cam.op.move", json!({"setup": setup.name, "name": name, "to": i + 1})));
                }
                // A copy right after the step, or at the end of another setup (S3 and S4 alike).
                ui_kit::compact_dropdown_menu(ui, ("cam-dup", &setup.name, &name), tr("ui.cam.duplicate"), |ui| {
                    if ui_kit::list_picker_start(ui, &format!("cam-dup-{}-{name}", setup.name), tr("ui.cam.duplicate"), false).clicked() {
                        actions.push(Action::Run("cam.op.duplicate", json!({"setup": setup.name, "name": name})));
                        ui.close();
                    }
                    // Only into setups that take the step: plasma cuts into plasma setups and so on.
                    for other in app.session.doc.cam.setups.iter().filter(|o| o.name != setup.name && o.operation_type.takes(op)) {
                        let text = solidcraft_ui_text::format("ui.cam.copy_to", &[("setup", &other.name)]);
                        if ui_kit::list_picker_start(ui, &format!("cam-dup-{}-{name}-{}", setup.name, other.name), &text, false).clicked() {
                            actions.push(Action::Run("cam.op.duplicate", json!({"setup": setup.name, "name": name, "to_setup": other.name})));
                            ui.close();
                        }
                    }
                });
                if ui_kit::button(ui, tr("ui.cam.rename"), ui_kit::Button::Small).clicked() {
                    d.renaming = Some((setup.name.clone(), name.clone(), name.clone()));
                }
                if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::Run("cam.op.remove", json!({"setup": setup.name, "name": name})));
                }
            });
            ui.end_row();
        }
    });
    if count > 0 {
        ui.horizontal(|ui| {
            if ui_kit::button(ui, tr("ui.cam.export_setup"), ui_kit::Button::Normal).clicked() {
                actions.push(Action::Export(setup.name.clone()));
            }
            if ui_kit::button(ui, tr("ui.cam.show_gcode"), ui_kit::Button::Normal).clicked() {
                d.gcode = Some(GcodeView::new(app, &setup.name));
            }
            // The sheet for this setup's machine: its frame, stock, machine, post and steps.
            if ui_kit::button(ui, tr("ui.cam.sheet.setup"), ui_kit::Button::Normal).on_hover_text(tr("ui.cam.sheet.setup_hint")).clicked() {
                actions.push(Action::Sheet(json!({"setup": setup.name}), setup.name.clone()));
            }
        });
    }
}

/// The G-code panel's body: the move buttons, then one row per line with its number, the
/// selected line marked and its move drawn on the model ([`draw_paths`]), filling the panel.
/// Posted again when the design changes.
fn gcode_rows(app: &SolveApp, ui: &mut egui::Ui, view: &mut GcodeView, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    // Editing by hand: the whole program as text, exported as it is.
    if let Some(text) = &mut view.edited {
        ui_kit::text(ui, tr("ui.cam.edit_unchecked"), ui_kit::Text::Error("cam_gcode_edit"));
        let mut done = false;
        ui.horizontal_wrapped(|ui| {
            if ui_kit::button(ui, tr("ui.cam.export_edited"), ui_kit::Button::Primary).clicked() {
                actions.push(Action::ExportText(view.setup.clone(), text.clone()));
            }
            done = ui_kit::button(ui, tr("ui.cam.edit_discard"), ui_kit::Button::Small).clicked();
        });
        // A long program scrolls inside the panel, which keeps its size.
        egui::ScrollArea::both().id_salt(("cam-gcode-edit-scroll", &view.setup)).max_height(ui.available_height()).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui_kit::text_area(ui, tr("ui.cam.edit_gcode"), egui::Id::unique(("cam-gcode-edit", &view.setup)), text, 16, 64 * 1024 * 1024);
            },
        );
        if done {
            view.edited = None;
        }
        return;
    }
    if view.key != design_key(app) {
        let (selected, travelled, speed, sim) = (view.selected, view.travelled, view.speed, view.sim.is_some());
        *view = GcodeView::new(app, &view.setup);
        (view.selected, view.speed) = (selected, speed);
        if sim {
            view.toggle_sim(app);
        }
        view.travelled = travelled.zip(view.program.as_ref().ok()).map(|(t, p)| t.min(p.total()));
    }
    view.advance(f64::from(ui.ctx().input(|i| i.stable_dt)));
    if view.playing {
        ui.ctx().request_repaint();
    }
    let program = match &view.program {
        Ok(program) => program,
        Err(e) => {
            ui_kit::text(ui, e, ui_kit::Text::Error("cam_gcode"));
            return;
        }
    };
    let mut step = 0;
    ui.horizontal_wrapped(|ui| {
        if ui_kit::button(ui, tr("ui.cam.prev_move"), ui_kit::Button::Normal).clicked() {
            step = -1;
        }
        if ui_kit::button(ui, tr("ui.cam.next_move"), ui_kit::Button::Normal).clicked() {
            step = 1;
        }
        if ui_kit::button(ui, tr("ui.cam.copy_gcode"), ui_kit::Button::Small).clicked() {
            ui.ctx().copy_text(program.text.clone());
        }
    });
    let Ok(program) = &view.program else { return };
    if let Some(time) = &program.time {
        let text = solidcraft_ui_text::format(
            "ui.cam.run_time",
            &[
                ("total", &clock(time.total)),
                ("cutting", &clock(time.cutting)),
                ("rapid", &clock(time.rapid)),
                ("changes", &clock(time.tool_changes)),
            ],
        );
        ui.add(egui::Label::new(text).wrap()).on_hover_text(tr("ui.cam.run_time_hint"));
    }
    let (mut simulate, mut edit) = (false, false);
    ui.horizontal_wrapped(|ui| {
        simulate = ui_kit::button(ui, tr(if view.sim.is_some() { "ui.cam.sim_hide" } else { "ui.cam.simulate" }), ui_kit::Button::Normal).clicked();
        edit = !cfg!(target_arch = "wasm32") && ui_kit::button(ui, tr("ui.cam.edit_gcode"), ui_kit::Button::Small).clicked();
    });
    if let Some(sim) = &view.sim {
        sim_rows(ui, sim);
    }
    let (mut play, mut rewind, mut speed) = (false, false, view.speed);
    let mut travelled = view.travelled.unwrap_or(0.0);
    let total = program.total();
    ui.horizontal_wrapped(|ui| {
        if ui_kit::button(ui, tr(if view.playing { "ui.cam.pause" } else { "ui.cam.play" }), ui_kit::Button::Normal).clicked() {
            play = true;
        }
        if ui_kit::button(ui, tr("ui.cam.rewind"), ui_kit::Button::Small).clicked() {
            rewind = true;
        }
    });
    let labels: Vec<String> = SPEEDS.iter().map(|s| format!("{} mm/s", number(*s))).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    ui_kit::choice_row(ui, tr("ui.cam.play_speed"), "cam-play-speed", &mut speed, &refs);
    let travel = format!("{travelled:.0} / {total:.0} mm");
    let slid = total > 0.0
        && ui_kit::row(ui, &travel, |ui| ui_kit::slider(ui, &mut travelled, 0.0..=total, ui_kit::Slider { show_value: false, ..Default::default() }))
            .changed();
    let count = program.lines.len().to_string();
    ui.add(
        egui::Label::new(
            RichText::new(solidcraft_ui_text::format("ui.cam.gcode_lines", &[("lines", &count), ("post", &program.post)])).color(t.text_dim),
        )
        .wrap(),
    );
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.gcode_hint")).color(t.text_dim)).wrap());
    // Searching the lines, right above them.
    // A form row keeps its width: a free text field would widen the panel frame by frame.
    let (_, enter) = ui_kit::text_row(ui, tr("ui.cam.search_gcode"), &mut view.search, "");
    let find = enter || ui_kit::row(ui, "", |ui| ui_kit::button(ui, tr("ui.cam.find_next"), ui_kit::Button::Small)).clicked();
    let row = ui_kit::row_height() + ui.spacing().item_spacing.y;
    let digits = program.lines.len().max(1).to_string().len();
    let mut area =
        egui::ScrollArea::both().id_salt(format!("cam-gcode-{}", view.setup)).max_height(ui.available_height()).auto_shrink([false, false]);
    if view.scroll
        && let Some(line) = view.selected
    {
        area = area.vertical_scroll_offset(((line.saturating_sub(4)) as f32 * row).max(0.0));
    }
    let mut clicked = None;
    area.show_rows(ui, row, program.lines.len(), |ui, range| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
        for i in range {
            let number = i + 1;
            let Some(text) = program.lines.get(i) else { continue };
            let label = format!("{number:>digits$}  {text}");
            // Read down a column, as an editor shows a program: every line from the left.
            if ui_kit::list_picker_start(ui, &format!("cam-gcode-{number}"), &label, view.selected == Some(number)).clicked() {
                clicked = Some(number);
            }
        }
    });
    view.scroll = false;
    view.speed = speed.min(SPEEDS.len() - 1);
    if slid {
        view.travelled = Some(travelled);
        view.playing = false;
        if let Some(line) = program.at(travelled).and_then(|(i, _)| program.moves.get(i)).map(|m| m.line) {
            view.selected = Some(line);
            view.scroll = true;
        }
    }
    if let Some(number) = clicked {
        view.go_to_line(number);
    }
    if find && let Some(line) = view.find_next() {
        view.go_to_line(line);
        view.scroll = true;
    }
    if step != 0 {
        view.step(step);
    }
    if rewind {
        view.rewind();
    }
    if play {
        view.play();
    }
    if simulate {
        view.toggle_sim(app);
    }
    if edit && let Ok(program) = &view.program {
        view.edited = Some(program.text.clone());
    }
}

/// What the simulation found: the material removed, then a line per step with a rapid through
/// stock or material above its flutes, and what the height field cannot see.
/// Seconds as `m:ss`, or `h:mm:ss` from an hour on.
pub(crate) fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() { seconds.max(0.0).round() as u64 } else { 0 };
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn sim_rows(ui: &mut egui::Ui, sim: &Result<solidcraft_engine::cmd::CamSimulation, String>) {
    let t = Tokens::get();
    let sim = match sim {
        Ok(sim) => sim,
        Err(e) => {
            ui_kit::text(ui, e, ui_kit::Text::Error("cam_sim"));
            return;
        }
    };
    let volume = format!("{:.1}", sim.removed / 1000.0);
    let cell = format!("{:.2}", sim.cell);
    ui.add(egui::Label::new(solidcraft_ui_text::format("ui.cam.sim_removed", &[("volume", &volume), ("cell", &cell)])).wrap());
    let mut warned = false;
    for step in &sim.operations {
        for (key, count) in [("ui.cam.sim_rapid_hits", step.report.rapid_hits.len()), ("ui.cam.sim_too_deep", step.report.too_deep.len())] {
            if count > 0 {
                warned = true;
                let text = solidcraft_ui_text::format(key, &[("step", &step.name), ("count", &count.to_string())]);
                ui.add(egui::Label::new(RichText::new(text).color(t.warning)).wrap());
            }
        }
        if step.removes_nothing() {
            warned = true;
            let text = solidcraft_ui_text::format("ui.cam.sim_removes_nothing", &[("step", &step.name)]);
            ui.add(egui::Label::new(RichText::new(text).color(t.warning)).wrap());
        }
    }
    if sim.stock_misses_part.is_some() {
        warned = true;
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.sim_stock_misses_part")).color(t.warning)).wrap());
    }
    if !warned {
        ui.add(egui::Label::new(tr("ui.cam.sim_ok")).wrap());
    }
    if sim.fits_model {
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.sim_fits_model")).color(t.warning)).wrap());
    }
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.sim_hint")).color(t.text_dim)).wrap());
}

fn jobs_tab(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    ui.horizontal_wrapped(|ui| {
        for id in [
            "cam.setup",
            "cam.op.face",
            "cam.op.contour",
            "cam.op.pocket",
            "cam.op.slot",
            "cam.op.chamfer",
            "cam.op.engrave",
            "cam.op.drill",
            "cam.op.helix",
            "cam.op.thread",
            "cam.op.tap",
            "cam.op.plasma",
        ] {
            if ui_kit::button(ui, tr(&format!("cmd.{id}")), ui_kit::Button::Normal).clicked() {
                actions.push(Action::Start(id));
            }
        }
    });
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.order_hint")).color(t.text_dim)).wrap());
    let setups = app.session.doc.cam.setups.clone();
    if setups.is_empty() {
        ui.add(egui::Label::new(tr("ui.cam.no_setups")).wrap());
    }
    // Each part, then the setups that make it, each on its own machine.
    for part in parts(&app.session.doc.cam) {
        ui.separator();
        let title = part.clone().unwrap_or_else(|| tr("ui.cam.part_default").to_string());
        let mine: Vec<&CamSetup> = setups.iter().filter(|s| s.part == part).collect();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{}: {title}", tr("ui.cam.part"))).strong());
            if mine.iter().any(|s| !s.operations.is_empty())
                && ui_kit::button(ui, tr("ui.cam.sheet.part"), ui_kit::Button::Small).on_hover_text(tr("ui.cam.sheet.hint")).clicked()
            {
                actions.push(Action::Sheet(json!({"part": part.clone().unwrap_or_default()}), title.clone()));
            }
        });
        for setup in mine {
            ui.indent(("cam-part-setup", &setup.name), |ui| setup_rows(app, ui, d, setup, actions));
        }
    }
    if setups.iter().any(|s| !s.operations.is_empty()) {
        ui.separator();
        if ui_kit::button(ui, tr("ui.cam.sheet.export"), ui_kit::Button::Normal).on_hover_text(tr("ui.cam.sheet.hint")).clicked() {
            actions.push(Action::Sheet(json!({}), "setup-sheet".into()));
        }
    }
}

/// The setup sheet's headings by `cam.sheet` key, and their messages.
const SHEET_HEADINGS: &[(&str, &str)] = &[
    ("axes", "ui.cam.sheet.axes"),
    ("bodies", "ui.cam.sheet.bodies"),
    ("comp_control", "ui.cam.sheet.comp_control"),
    ("comp_wear", "ui.cam.sheet.comp_wear"),
    ("description", "ui.cam.sheet.description"),
    ("diameter", "ui.cam.sheet.diameter"),
    ("fixtures", "ui.cam.sheet.fixtures"),
    ("flute_length", "ui.cam.sheet.flute_length"),
    ("flutes", "ui.cam.sheet.flutes"),
    ("heights", "ui.cam.sheet.heights"),
    ("holder", "ui.cam.sheet.holder"),
    ("kerf", "ui.cam.kerf"),
    ("kind", "ui.cam.sheet.kind"),
    ("machine", "ui.cam.sheet.machine"),
    ("material", "ui.cam.sheet.material"),
    ("none", "ui.cam.sheet.none"),
    ("number", "ui.cam.sheet.number"),
    ("operation_type", "ui.cam.setup_type"),
    ("origin", "ui.cam.sheet.origin"),
    ("part", "ui.cam.part"),
    ("part_zero", "ui.cam.sheet.part_zero"),
    ("post", "ui.cam.sheet.post"),
    ("problem", "ui.cam.sheet.problem"),
    ("run_time", "ui.cam.sheet.run_time"),
    ("setups", "ui.cam.sheet.setups"),
    ("sizes", "ui.cam.sheet.sizes"),
    ("skipped", "ui.cam.sheet.skipped"),
    ("step", "ui.cam.sheet.step"),
    ("steps", "ui.cam.sheet.steps"),
    ("stick_out", "ui.cam.sheet.stick_out"),
    ("stock", "ui.cam.sheet.stock"),
    ("tool", "ui.cam.sheet.tool"),
    ("tools", "ui.cam.sheet.tools"),
    ("total", "ui.cam.sheet.total"),
    ("used_in", "ui.cam.sheet.used_in"),
    ("work_offset", "ui.cam.sheet.work_offset"),
];

/// The step types of the setup sheet, and the names their commands have.
const SHEET_STEPS: &[(&str, &str)] = &[
    ("chamfer", "cmd.cam.op.chamfer"),
    ("contour", "cmd.cam.op.contour"),
    ("drill", "cmd.cam.op.drill"),
    ("engrave", "cmd.cam.op.engrave"),
    ("face", "cmd.cam.op.face"),
    ("helical_bore", "cmd.cam.op.helix"),
    ("plasma", "cmd.cam.op.plasma"),
    ("pocket", "cmd.cam.op.pocket"),
    ("slot", "cmd.cam.op.slot"),
    ("thread_mill", "cmd.cam.op.thread"),
];

/// The stock kinds of the setup sheet, and their names in the setup form.
const SHEET_STOCKS: &[(&str, &str)] = &[("box", "ui.cam.stock_box"), ("cylinder", "ui.cam.stock_round"), ("relative_box", "ui.cam.stock_around")];

/// The setup sheet's words in the language shown: headings, tool kinds, step types, stocks.
fn sheet_labels() -> Value {
    let mut labels = serde_json::Map::new();
    for (key, id) in SHEET_HEADINGS {
        labels.insert((*key).to_string(), json!(tr(id)));
    }
    for &kind in KINDS {
        labels.insert(format!("kind.{}", kind.key()), json!(tr(kind_key(kind))));
    }
    for (key, id) in SHEET_STEPS {
        labels.insert(format!("op.{key}"), json!(tr(id)));
    }
    for (key, id) in SHEET_STOCKS {
        labels.insert(format!("stock.{key}"), json!(tr(id)));
    }
    for &kind in MachineKind::ALL {
        labels.insert(format!("machine.{}", kind.key()), json!(machine_kind_label(kind)));
    }
    for (kind, id) in crate::dialogs_cam_setup::TYPES {
        labels.insert(format!("type.{}", kind.key()), json!(tr(id)));
    }
    Value::Object(labels)
}

fn cutting_rows(ui: &mut egui::Ui, edit: &mut ToolEdit) {
    let t = Tokens::get();
    let tool = edit.tool(None).ok();
    let kind = KINDS.get(edit.kind).copied().unwrap_or(ToolKind::FlatEndmill);
    let diameter = edit.diameter.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
    egui::Grid::new("cam-cutting").num_columns(7).striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        for h in ["ui.cam.material", "ui.cam.vc", "ui.cam.fz", "ui.cam.ap", "ui.cam.ae", "ui.cam.n", "ui.cam.vf"] {
            ui.label(RichText::new(tr(h)).strong().color(t.text_dim));
        }
        ui.end_row();
        for (material, row) in MATERIALS.iter().zip(edit.cutting.iter_mut()) {
            ui.label(material_label(material));
            let start = CuttingData::starting(kind, material, diameter);
            let hints = start.map(|c| [number(c.vc), number(c.fz), number(c.ap), number(c.ae)]).unwrap_or_default();
            for (value, hint) in row.iter_mut().zip(&hints) {
                ui.allocate_ui(egui::vec2(64.0, ui_kit::row_height()), |ui| {
                    ui_kit::text_field(ui, value, hint);
                });
            }
            let speeds = tool.as_ref().and_then(|tool| tool.cutting_for(material)).zip(tool.as_ref()).and_then(|(c, tool)| c.speeds(tool, None).ok());
            ui.label(speeds.map_or_else(String::new, |s| number(s.spindle_rpm)));
            ui.label(speeds.map_or_else(String::new, |s| number(s.feed)));
            ui.end_row();
        }
    });
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.starting_hint")).color(t.text_dim)).wrap());
}

fn tools_tab(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    let library = &app.preferences.cam_tools;
    ui.label(RichText::new(tr("ui.cam.library_tools")).strong());
    if library.0.is_empty() {
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.library_empty")).color(t.text_dim)).wrap());
    }
    egui::ScrollArea::vertical().id_salt("cam-library").max_height(180.0).show(ui, |ui| {
        for tool in &library.0 {
            // Right to left: the buttons keep their width and the picker takes the rest.
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::Unlibrary(tool.number));
                }
                let in_design = app.session.doc.cam.tool(tool.number);
                if in_design != Some(tool) && ui_kit::button(ui, tr("ui.cam.to_design"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::Run("cam.tool.define", define_params(tool)));
                }
                let picked = d.editing.as_ref().is_some_and(|(n, _)| *n == Some(tool.number));
                if ui_kit::list_picker_start(ui, &format!("cam-tool-{}", tool.number), &tool_label(tool), picked).clicked() {
                    d.editing = Some((Some(tool.number), ToolEdit::from_tool(tool)));
                }
            });
        }
    });
    if ui_kit::button(ui, tr("ui.cam.new_tool"), ui_kit::Button::Normal).clicked() {
        let number = (1..=9999).find(|n| library.find(*n).is_none()).unwrap_or(1);
        let mut edit = ToolEdit::from_tool(&Tool::new(number, ToolKind::FlatEndmill, 6.0, 20.0));
        edit.cutting = vec![Default::default(); MATERIALS.len()];
        d.editing = Some((None, edit));
    }
    let design: Vec<&Tool> = app.session.doc.cam.tools.iter().filter(|tool| library.find(tool.number) != Some(*tool)).collect();
    if !design.is_empty() {
        ui.separator();
        ui.label(RichText::new(tr("ui.cam.design_tools")).strong());
        for tool in design {
            ui.horizontal(|ui| {
                ui.label(tool_label(tool));
                if ui_kit::button(ui, tr("ui.cam.to_library"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::Library(Box::new(tool.clone())));
                }
            });
        }
    }
    // Files on this computer: the browser build has none to write or read.
    if !cfg!(target_arch = "wasm32") {
        ui.horizontal_wrapped(|ui| {
            if ui_kit::button(ui, tr("ui.cam.export_library"), ui_kit::Button::Enabled(!library.0.is_empty())).clicked() {
                actions.push(Action::ExportLibrary);
            }
            if ui_kit::button(ui, tr("ui.cam.import_library"), ui_kit::Button::Normal).clicked() {
                actions.push(Action::ImportLibrary);
            }
        });
    }
    if let Some(tools) = &d.import {
        let fresh = tools.iter().filter(|tool| library.find(tool.number).is_none()).count();
        let differ: Vec<String> =
            tools.iter().filter(|tool| library.find(tool.number).is_some_and(|old| old != *tool)).map(|tool| format!("T{}", tool.number)).collect();
        let (count, new, changed) = (tools.len().to_string(), fresh.to_string(), differ.join(", "));
        let line = if differ.is_empty() {
            solidcraft_ui_text::format("ui.cam.import_summary", &[("count", &count), ("new", &new)])
        } else {
            solidcraft_ui_text::format("ui.cam.import_summary_replace", &[("count", &count), ("new", &new), ("tools", &changed)])
        };
        ui.add(egui::Label::new(line).wrap());
        ui.horizontal_wrapped(|ui| {
            if !differ.is_empty() && ui_kit::button(ui, tr("ui.cam.import_all"), ui_kit::Button::Normal).clicked() {
                actions.push(Action::TakeImport(true));
            }
            if ui_kit::button(ui, tr("ui.cam.import_new"), ui_kit::Button::Enabled(fresh > 0)).clicked() {
                actions.push(Action::TakeImport(false));
            }
            if ui_kit::button(ui, tr("ui.cam.import_cancel"), ui_kit::Button::Small).clicked() {
                d.import = None;
            }
        });
    }
    let Some((original, edit)) = &mut d.editing else { return };
    ui.separator();
    let kinds: Vec<&str> = KINDS.iter().map(|k| tr(kind_key(*k))).collect();
    ui_kit::text_row(ui, tr("ui.cam.tool"), &mut edit.number, "");
    ui_kit::choice_row(ui, tr("ui.cam.kind"), "cam-tool-kind", &mut edit.kind, &kinds);
    let kind = KINDS.get(edit.kind).copied().unwrap_or(ToolKind::FlatEndmill);
    ui_kit::text_row(ui, tr("ui.cam.diameter"), &mut edit.diameter, "");
    ui_kit::text_row(ui, tr("ui.cam.flutes"), &mut edit.flutes, "");
    ui_kit::text_row(ui, tr("ui.cam.flute"), &mut edit.flute_length, "");
    ui_kit::text_row(ui, tr("ui.cam.description"), &mut edit.description, "");
    // The kind's own sizes, then lengths and the holder every tool can have.
    for &(i, key, used) in SIZES {
        if used(kind)
            && let Some(text) = edit.sizes.get_mut(i)
        {
            ui_kit::text_row(ui, tr(key), text, tr("ui.cam.optional"));
        }
    }
    ui_kit::text_row(ui, tr("ui.cam.holder"), &mut edit.holder[0], tr("ui.cam.optional"));
    ui_kit::text_row(ui, tr("ui.cam.holder_diameter"), &mut edit.holder[1], tr("ui.cam.optional"));
    ui_kit::text_row(ui, tr("ui.cam.holder_length"), &mut edit.holder[2], tr("ui.cam.optional"));
    cutting_rows(ui, edit);
    if ui_kit::button(ui, tr("ui.cam.save_tool"), ui_kit::Button::Primary).clicked() {
        let base = original.and_then(|n| library.find(n));
        match edit.tool(base) {
            Ok(tool) => {
                if let Some(old) = *original
                    && old != tool.number
                {
                    actions.push(Action::Unlibrary(old));
                }
                *original = Some(tool.number);
                actions.push(Action::Library(Box::new(tool)));
            }
            Err(e) => d.error = Some(e),
        }
    }
}

fn machines_tab(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    let list = &app.preferences.cam_machines;
    ui.label(RichText::new(tr("ui.cam.machines")).strong());
    if list.0.is_empty() {
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.machines_empty")).color(t.text_dim)).wrap());
    }
    egui::ScrollArea::vertical().id_salt("cam-machines").max_height(180.0).show(ui, |ui| {
        for machine in &list.0 {
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::Unmachine(machine.name.clone()));
                }
                let in_design = app.session.doc.cam.machine(&machine.name);
                if in_design != Some(machine) && ui_kit::button(ui, tr("ui.cam.to_design"), ui_kit::Button::Small).clicked() {
                    if let Some(post) = machine_post(&app.preferences.cam_posts, machine) {
                        actions.push(Action::Run("cam.post.define", post_params(post)));
                    }
                    actions.push(Action::Run("cam.machine.define", serde_json::to_value(machine).unwrap_or(Value::Null)));
                }
                let picked = d.machine.as_ref().is_some_and(|(n, _)| n.as_deref() == Some(machine.name.as_str()));
                if ui_kit::list_picker(ui, &format!("cam-machine-{}", machine.name), &machine_label(machine), picked).clicked() {
                    let mut edit = MachineEdit::from_machine(machine);
                    edit.folder = app.preferences.cam_folders.get(&machine.name).unwrap_or_default().to_string();
                    d.machine = Some((Some(machine.name.clone()), edit));
                }
            });
        }
    });
    ui.horizontal_wrapped(|ui| {
        let free = |base: &str| (1..=MAX_MACHINES).map(|i| format!("{base} {i}")).find(|n| list.find(n).is_none()).unwrap_or_default();
        if ui_kit::button(ui, tr("ui.cam.new_machine"), ui_kit::Button::Normal).clicked() {
            d.machine = Some((None, MachineEdit::from_machine(&Machine::new(free(tr("ui.cam.machine"))))));
        }
        if ui_kit::button(ui, tr("ui.cam.new_plasma_table"), ui_kit::Button::Normal).clicked() {
            d.machine = Some((None, MachineEdit::from_machine(&Machine::plasma(free(tr("ui.cam.machine_plasma"))))));
        }
    });
    let design: Vec<&Machine> = app.session.doc.cam.machines.iter().filter(|m| list.find(&m.name) != Some(*m)).collect();
    if !design.is_empty() {
        ui.separator();
        ui.label(RichText::new(tr("ui.cam.design_machines")).strong());
        for machine in design {
            ui.horizontal(|ui| {
                ui.label(machine_label(machine));
                if ui_kit::button(ui, tr("ui.cam.to_library"), ui_kit::Button::Small).clicked() {
                    // The list's machine of that name stays; it is changed in its own form.
                    match list.name_free(&machine.name, None) {
                        Err(e) => d.error = Some(e),
                        Ok(()) => actions.push(Action::Machine(Box::new(machine.clone()))),
                    }
                }
            });
        }
    }
    let Some((original, edit)) = &mut d.machine else { return };
    ui.separator();
    ui_kit::text_row(ui, tr("ui.cam.machine_name"), &mut edit.name, "");
    // What the machine does decides which setups it runs and which posts write for it.
    let kinds: Vec<&str> = MachineKind::ALL.iter().map(|k| machine_kind_label(*k)).collect();
    ui_kit::choice_row(ui, tr("ui.cam.machine_kind"), "cam-machine-kind", &mut edit.kind, &kinds);
    let kind = MachineKind::ALL.get(edit.kind).copied().unwrap_or_default();
    let plasma = kind == MachineKind::Plasma;
    for (label, value) in ["X−", "X+", "Y−", "Y+", "Z−", "Z+"].iter().zip(edit.travel.iter_mut()) {
        ui_kit::text_row(ui, &format!("{} {label} (mm)", tr("ui.cam.travel")), value, "");
    }
    if plasma {
        // The torch's cutting data, suggested to each new plasma cut on the table.
        let labels = ["ui.cam.kerf", "ui.cam.feed", "ui.cam.pierce_height", "ui.cam.cut_height", "ui.cam.pierce_delay"];
        for (key, value) in labels.iter().zip(edit.torch.iter_mut()) {
            ui_kit::text_row(ui, tr(key), value, "");
        }
    } else {
        let kinds = [tr("ui.cam.a_none"), tr("ui.cam.a_limited"), tr("ui.cam.a_endless")];
        ui_kit::choice_row(ui, tr("ui.cam.a_axis"), "cam-machine-a", &mut edit.a_kind, &kinds);
        if edit.a_kind == 1 {
            for (label, value) in ["A−", "A+"].iter().zip(edit.a.iter_mut()) {
                ui_kit::text_row(ui, &format!("{} {label} (°)", tr("ui.cam.travel")), value, "");
            }
        }
        let [slow, fast] = &mut edit.spindle;
        ui_kit::text_row(ui, tr("ui.cam.spindle_min"), slow, "");
        ui_kit::text_row(ui, tr("ui.cam.spindle_max"), fast, "");
        ui_kit::text_row(ui, tr("ui.cam.spindle_power"), &mut edit.power, tr("ui.cam.spindle_power_hint"));
    }
    ui_kit::text_row(ui, tr("ui.cam.max_feed"), &mut edit.max_feed, "");
    // The default, the posts for this kind of machine, and the machine's own if it names another.
    let mut posts: Vec<String> =
        Post::BUILTIN.iter().filter(|p| Post::builtin(p).is_some_and(|p| p.machine == kind)).map(|p| (*p).to_string()).collect();
    posts.extend(app.preferences.cam_posts.0.iter().filter(|p| p.machine == kind).map(|p| p.name.clone()));
    if let Some(own) = &edit.post
        && !posts.contains(own)
    {
        posts.push(own.clone());
    }
    let labels: Vec<String> = std::iter::once(tr("ui.cam.post_default").to_string()).chain(posts.iter().map(|p| post_label(p))).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut chosen = edit.post.as_ref().and_then(|p| posts.iter().position(|n| n == p)).map_or(0, |i| i + 1);
    ui_kit::choice_row(ui, tr("ui.cam.post"), "cam-machine-post", &mut chosen, &refs);
    edit.post = chosen.checked_sub(1).and_then(|i| posts.get(i)).cloned();
    // The browser has no folders: there an export is always a download.
    if !cfg!(target_arch = "wasm32") {
        ui_kit::text_row(ui, tr("ui.cam.program_folder"), &mut edit.folder, r"\\linuxcnc\nc_files");
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.program_folder_hint")).color(t.text_dim)).wrap());
    }
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.machine_hint")).color(t.text_dim)).wrap());
    if ui_kit::button(ui, tr("ui.cam.save_machine"), ui_kit::Button::Primary).clicked() {
        let folder_ok = ProgramFolders::default().set("machine", &edit.folder);
        match edit.machine().and_then(|m| folder_ok.map(|()| m)) {
            Ok(machine) => match list.name_free(&machine.name, original.as_deref()) {
                Err(e) => d.error = Some(e),
                Ok(()) => {
                    if let Some(old) = original.clone()
                        && old != machine.name
                    {
                        actions.push(Action::Unmachine(old));
                    }
                    *original = Some(machine.name.clone());
                    actions.push(Action::Folder(machine.name.clone(), edit.folder.clone()));
                    actions.push(Action::Machine(Box::new(machine)));
                }
            },
            Err(e) => d.error = Some(e),
        }
    }
}

fn posts_tab(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    let list = &app.preferences.cam_posts;
    ui.label(RichText::new(tr("ui.cam.posts")).strong());
    if list.0.is_empty() {
        ui.add(egui::Label::new(RichText::new(tr("ui.cam.posts_empty")).color(t.text_dim)).wrap());
    }
    for post in &list.0 {
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                actions.push(Action::Unpost(post.name.clone()));
            }
            if app.session.doc.cam.post(&post.name) != Some(post) && ui_kit::button(ui, tr("ui.cam.to_design"), ui_kit::Button::Small).clicked() {
                actions.push(Action::Run("cam.post.define", post_params(post)));
            }
            let picked = d.post.as_ref().is_some_and(|(n, _)| n.as_deref() == Some(post.name.as_str()));
            let label = format!("{} – .{}", post.name, post.extension);
            if ui_kit::list_picker(ui, &format!("cam-post-{}", post.name), &label, picked).clicked() {
                d.post = Some((Some(post.name.clone()), PostEdit::from_post(post)));
            }
        });
    }
    let labels: Vec<String> = Post::BUILTIN.iter().map(|p| post_label(p)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    ui_kit::choice_row(ui, tr("ui.cam.post_base"), "cam-post-base", &mut d.post_base, &refs);
    if ui_kit::button(ui, tr("ui.cam.new_post"), ui_kit::Button::Normal).clicked() {
        let base = Post::BUILTIN.get(d.post_base).and_then(|b| Post::builtin(b)).unwrap_or_else(Post::linuxcnc);
        let stem = post_label(&base.name);
        let name = (1..=MAX_POSTS).map(|i| format!("{stem} {i}")).find(|n| list.find(n).is_none()).unwrap_or_default();
        d.post = Some((None, PostEdit::from_post(&Post { name, ..base })));
    }
    let design: Vec<&Post> = app.session.doc.cam.posts.iter().filter(|p| list.find(&p.name) != Some(*p)).collect();
    if !design.is_empty() {
        ui.separator();
        ui.label(RichText::new(tr("ui.cam.design_posts")).strong());
        for post in design {
            ui.horizontal(|ui| {
                ui.label(format!("{} – .{}", post.name, post.extension));
                if ui_kit::button(ui, tr("ui.cam.to_library"), ui_kit::Button::Small).clicked() {
                    match list.name_free(&post.name, None) {
                        Err(e) => d.error = Some(e),
                        Ok(()) => actions.push(Action::Post(Box::new(post.clone()))),
                    }
                }
            });
        }
    }
    let Some((original, edit)) = &mut d.post else { return };
    ui.separator();
    ui_kit::text_row(ui, tr("ui.cam.machine_name"), &mut edit.name, "");
    ui_kit::text_row(ui, tr("ui.cam.post_extension"), &mut edit.extension, "ngc");
    let units = [tr("ui.cam.units_mm"), tr("ui.cam.units_inch")];
    ui_kit::choice_row(ui, tr("ui.cam.post_units"), "cam-post-units", &mut edit.units, &units);
    ui_kit::text_row(ui, tr("ui.cam.post_decimals"), &mut edit.decimals, "");
    ui_kit::text_row(ui, tr("ui.cam.post_feed_decimals"), &mut edit.feed_decimals, "");
    let comments = [tr("ui.cam.comments_parentheses"), tr("ui.cam.comments_semicolon"), tr("ui.cam.comments_none")];
    ui_kit::choice_row(ui, tr("ui.cam.post_comments"), "cam-post-comments", &mut edit.comments, &comments);
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_line_numbers"), &mut edit.line_numbers));
    if edit.line_numbers {
        ui_kit::text_row(ui, tr("ui.cam.post_line_start"), &mut edit.line_start, "");
        ui_kit::text_row(ui, tr("ui.cam.post_line_step"), &mut edit.line_step, "");
    }
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_arcs"), &mut edit.arcs));
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_helical_arcs"), &mut edit.helical_arcs));
    ui_kit::text_row(ui, tr("ui.cam.post_arc_tolerance"), &mut edit.arc_tolerance, "");
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_trailing_decimal"), &mut edit.trailing_decimal));
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_two_digit"), &mut edit.two_digit_codes));
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_canned_cycles"), &mut edit.canned_cycles));
    let tapping: Vec<&str> = TAPPING.iter().map(|(_, key)| tr(key)).collect();
    ui_kit::choice_row(ui, tr("ui.cam.post_tapping"), "cam-post-tapping", &mut edit.tapping, &tapping);
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, tr("ui.cam.post_tool_change_first"), &mut edit.tool_change_first));
    ui_kit::text_row(ui, tr("ui.cam.post_dwell"), &mut edit.dwell, "G4 P{seconds}");
    let [left, right, off] = &mut edit.comp;
    ui_kit::text_row(ui, tr("ui.cam.post_comp_left"), left, "G41 D{register}");
    ui_kit::text_row(ui, tr("ui.cam.post_comp_right"), right, "G42 D{register}");
    ui_kit::text_row(ui, tr("ui.cam.post_comp_off"), off, "G40");
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.post_hint")).color(t.text_dim)).wrap());
    for (key, block) in POST_BLOCKS.iter().zip(edit.blocks.iter_mut()) {
        ui_kit::multiline_text_field(ui, tr(key), egui::Id::unique(("cam-post-block", *key)), block);
    }
    if ui_kit::button(ui, tr("ui.cam.save_post"), ui_kit::Button::Primary).clicked() {
        match edit.post().and_then(|post| list.name_free(&post.name, original.as_deref()).map(|()| post)) {
            Ok(post) => {
                if let Some(old) = original.clone()
                    && old != post.name
                {
                    actions.push(Action::Unpost(old));
                }
                *original = Some(post.name.clone());
                actions.push(Action::Post(Box::new(post)));
            }
            Err(e) => d.error = Some(e),
        }
    }
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.cam_window.take() else { return };
    // While a setup's G-code is gone through, only its small panel is shown, at the bottom
    // right, so the model stays free; closing it brings the window back.
    if let Some(view) = d.gcode.as_mut() {
        let t = Tokens::get();
        let title = solidcraft_ui_text::format("ui.cam.gcode_title", &[("setup", &view.setup)]);
        let corner = ctx.content_rect().right_bottom() - egui::vec2(t.space_24, t.space_24);
        let mut actions = Vec::new();
        let closed = ui_kit::Window::new(egui::Id::unique("sc_cam_gcode"), "cut", &title)
            .cards(1.6)
            .resizable(t.form_card_width * 1.4)
            .placed(egui::Align2::RIGHT_BOTTOM, corner)
            .show_body(ctx, |ui| gcode_rows(app, ui, view, &mut actions));
        if closed {
            d.gcode = None;
        }
        app.cam_window = Some(d);
        for action in actions {
            // Hand-edited text is saved as it is, through the same command, never read back.
            if let Action::ExportText(setup, text) = action {
                let extension = program_extension(app, &setup);
                app.pick_save_command(&program_file(&setup, &extension), &[&extension], "cam.export", json!({"setup": setup, "text": text}));
            }
        }
        return;
    }
    let mut actions = Vec::new();
    let t = Tokens::get();
    let height = t.form_card_width * 2.0;
    // At the right edge of the view, below the ribbon, so the model in the middle stays free
    // (the browser has the left); it can still be moved.
    let corner = app
        .viewport
        .rect
        .map(|r| crate::shell::content_rect(app, r).right_top() + egui::vec2(-t.space_16, t.space_16))
        .unwrap_or_else(|| ctx.content_rect().right_top() + egui::vec2(-t.space_24, t.space_24 * 6.0));
    if d.folded {
        // Folded: a narrow card with the setups' eyes and the way back; the toolpaths stay drawn.
        let setups: Vec<String> = app.session.doc.cam.setups.iter().map(|s| s.name.clone()).collect();
        let closed = ui_kit::Window::new(egui::Id::unique("sc_cam_folded"), "cut", tr("ui.cam.panel_title"))
            .cards(1.2)
            .placed(egui::Align2::RIGHT_TOP, corner)
            .show_body(ctx, |ui| {
                if ui_kit::button(ui, tr("ui.cam.unfold"), ui_kit::Button::Small).clicked() {
                    d.folded = false;
                }
                for name in &setups {
                    ui.horizontal(|ui| {
                        let shown = !d.hidden.contains(name);
                        let side = ui_kit::row_height();
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                        let (icon, label) = if shown { ("eye", tr("ui.cam.hide_paths")) } else { ("suppress", tr("ui.cam.show_paths")) };
                        if ui.push_id(("cam-eye-folded", name), |ui| ui_kit::quiet_icon_button_at(ui, rect, icon, label)).inner.clicked() {
                            toggle_paths(&mut d, name);
                        }
                        ui.label(RichText::new(name).color(if shown { t.text } else { t.text_dim }));
                    });
                }
            });
        if !closed {
            app.cam_window = Some(d);
        }
        return;
    }
    let closed = ui_kit::Window::new(egui::Id::unique("sc_cam"), "cut", tr("ui.cam.panel_title"))
        .cards(3.0)
        .resizable(height)
        .placed(egui::Align2::RIGHT_TOP, corner)
        .show_body(ctx, |ui| {
            let tabs = [tr("ui.cam.tab_jobs"), tr("ui.cam.tab_tools"), tr("ui.cam.tab_machines"), tr("ui.cam.tab_posts"), tr("ui.cam.tab_manual")];
            if ui_kit::button(ui, tr("ui.cam.fold"), ui_kit::Button::Small).clicked() {
                d.folded = true;
            }
            ui_kit::segmented(ui, &mut d.tab, &tabs, Default::default());
            ui.separator();
            // The window is sized by the user; its tabs scroll inside it.
            // Each field explains itself on hover in the manual's words.
            egui::ScrollArea::vertical().id_salt("cam-body").auto_shrink([false, false]).show(ui, |ui| {
                crate::cam_manual::with_tips(ui, |ui| match d.tab {
                    0 => jobs_tab(app, ui, &mut d, &mut actions),
                    1 => tools_tab(app, ui, &mut d, &mut actions),
                    2 => machines_tab(app, ui, &mut d, &mut actions),
                    3 => posts_tab(app, ui, &mut d, &mut actions),
                    _ => crate::cam_manual::manual_tab(ui),
                });
                if let Some(e) = &d.error {
                    // The window truncates text; a refusal is read in full.
                    ui_kit::text(ui, e, ui_kit::Text::Error("cam"));
                }
            });
        });
    for action in actions {
        let result = match action {
            Action::Run(id, params) => app.run(id, params).map(|_| ()),
            Action::Export(setup) => export(app, &setup),
            Action::Start(id) => {
                app.start(id);
                Ok(())
            }
            Action::EditStep(setup, name) => {
                let found = app.session.doc.cam.setup(&setup).and_then(|s| s.operations.iter().find(|o| o.name() == name).map(|o| (s, o)));
                if let Some((setup, op)) = found {
                    let form = crate::dialogs_milling::Milling::edit(app, setup, op);
                    app.dialog = Some(crate::dialogs::Dialog::new(crate::dialogs::Kind::Milling(Box::new(form)), crate::dialogs_milling::inputs()));
                }
                Ok(())
            }
            Action::EditSetup(name) => {
                if let Some(setup) = app.session.doc.cam.setup(&name) {
                    let form = crate::dialogs_cam_setup::CamSetupForm::edit(app, setup);
                    app.dialog =
                        Some(crate::dialogs::Dialog::new(crate::dialogs::Kind::CamSetup(Box::new(form)), crate::dialogs_cam_setup::inputs()));
                }
                Ok(())
            }
            Action::Library(tool) => {
                let result = app.preferences.cam_tools.put(*tool);
                if result.is_ok() {
                    app.set_status(tr("ui.cam.saved"), false);
                }
                result
            }
            Action::Unlibrary(number) => {
                app.preferences.cam_tools.0.retain(|t| t.number != number);
                if d.editing.as_ref().is_some_and(|(n, _)| *n == Some(number)) {
                    d.editing = None;
                }
                Ok(())
            }
            Action::ExportLibrary => {
                app.cam_window = Some(std::mem::take(&mut d));
                export_library(app);
                d = app.cam_window.take().unwrap_or_default();
                Ok(())
            }
            Action::ImportLibrary => {
                app.cam_window = Some(std::mem::take(&mut d));
                import_library(app);
                d = app.cam_window.take().unwrap_or_default();
                Ok(())
            }
            Action::Sheet(mut params, stem) => {
                params["labels"] = sheet_labels();
                params["title"] = json!(tr("ui.cam.sheet.title"));
                params["overwrite"] = json!(true);
                let file = program_file(&stem, "html");
                app.pick_save_command(&file, &["html", "csv", "json"], "cam.sheet", params);
                Ok(())
            }
            Action::ExportText(setup, text) => {
                let extension = program_extension(app, &setup);
                app.pick_save_command(&program_file(&setup, &extension), &[&extension], "cam.export", json!({"setup": setup, "text": text}));
                Ok(())
            }
            Action::TakeImport(replace) => {
                let tools = d.import.take().unwrap_or_default();
                take_tools(&mut app.preferences.cam_tools, &tools, replace).map(|(added, replaced)| {
                    let (added, replaced) = (added.to_string(), replaced.to_string());
                    app.set_status(solidcraft_ui_text::format("ui.cam.imported", &[("added", &added), ("replaced", &replaced)]), false);
                })
            }
            Action::Machine(machine) => {
                let result = app.preferences.cam_machines.put(*machine);
                if result.is_ok() {
                    app.set_status(tr("ui.cam.saved"), false);
                }
                result
            }
            Action::Folder(name, folder) => app.preferences.cam_folders.set(&name, &folder),
            Action::Post(post) => {
                let result = app.preferences.cam_posts.put(*post);
                if result.is_ok() {
                    app.set_status(tr("ui.cam.saved"), false);
                }
                result
            }
            Action::Unpost(name) => match app.preferences.cam_machines.0.iter().find(|m| m.post.as_deref() == Some(name.as_str())) {
                // A renamed post is taken off after its new name is saved, so its machines are asked about here too.
                Some(m) => Err(solidcraft_ui_text::format("ui.cam.post_in_use", &[("machine", &m.name)])),
                None => {
                    app.preferences.cam_posts.0.retain(|p| p.name != name);
                    if d.post.as_ref().is_some_and(|(n, _)| n.as_deref() == Some(name.as_str())) {
                        d.post = None;
                    }
                    Ok(())
                }
            },
            Action::Unmachine(name) => {
                app.preferences.cam_machines.0.retain(|m| m.name != name);
                app.preferences.cam_folders.0.remove(&name);
                if d.machine.as_ref().is_some_and(|(n, _)| n.as_deref() == Some(name.as_str())) {
                    d.machine = None;
                }
                Ok(())
            }
        };
        d.error = result.err();
        if d.error.is_some() {
            break;
        }
    }
    if !closed {
        app.cam_window = Some(d);
    }
}

/// The simulated stock, translucent over the model: the faces turned to the eye, far ones first,
/// lit from the eye so walls and floors read apart.
fn draw_stock(
    painter: &egui::Painter,
    proj: &crate::viewport::Proj,
    sim: &solidcraft_engine::cmd::CamSimulation,
    colour: egui::Color32,
    shadow: egui::Color32,
) {
    let back = proj.cam.back();
    let eye = proj.cam.eye();
    let perspective = proj.cam.fov > 0.0;
    let mut faces: Vec<(f64, [egui::Pos2; 3], f32)> = Vec::new();
    for tri in sim.indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| i as usize);
        let (Some(pa), Some(pb), Some(pc)) = (sim.positions.get(a), sim.positions.get(b), sim.positions.get(c)) else { continue };
        let Some(n) = sim.normals.get(a) else { continue };
        let to_eye = if perspective { (eye - *pa).normalized().unwrap_or(back) } else { back };
        let facing = n.dot(to_eye);
        if facing <= 0.0 {
            continue;
        }
        let (Some(sa), Some(sb), Some(sc)) = (proj.to_screen(*pa), proj.to_screen(*pb), proj.to_screen(*pc)) else { continue };
        let depth = (*pa + *pb + *pc).dot(back) / 3.0;
        faces.push((depth, [sa, sb, sc], (0.45 + 0.55 * facing) as f32));
    }
    faces.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut mesh = egui::Mesh::default();
    for (_, pts, light) in faces {
        let fill = colour.lerp_to_gamma(shadow, 1.0 - light).gamma_multiply(0.8);
        let base = mesh.vertices.len() as u32;
        for p in pts {
            mesh.colored_vertex(p, fill);
        }
        mesh.add_triangle(base, base + 1, base + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// How far up its axis the playback draws the tool (mm).
const TOOL_MARK_LENGTH: f64 = 25.0;

/// With the window open, every step's toolpath: cutting moves in the preview colour, rapid
/// moves thin, the steps left out of the G-code dimmed. Made again only when the design changes.
/// Whether setup `name` is drawn in the view: going through a setup's G-code shows that setup only;
/// otherwise every setup whose eye in the job list is open.
pub fn setup_shown(app: &SolveApp, name: &str) -> bool {
    app.cam_window.as_ref().is_some_and(|w| match w.gcode.as_ref() {
        Some(g) => g.setup == name,
        None => !w.hidden.iter().any(|h| h == name),
    })
}

pub fn draw_paths(app: &mut SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let key = std::sync::Arc::as_ptr(&app.session.doc) as usize;
    let Some(window) = app.cam_window.as_mut() else { return };
    if window.paths.as_ref().is_none_or(|(k, _)| *k != key) {
        window.paths = Some((key, app.session.cam_toolpaths()));
    }
    let Some((_, paths)) = &window.paths else { return };
    let t = Tokens::get();
    if let Some(Some(Ok(sim))) = window.gcode.as_ref().map(|g| &g.sim) {
        // The stock in the colour of a body still to be made, apart from the finished model.
        draw_stock(painter, proj, sim, t.preview_new, t.body_edge);
    }
    let selected = window.gcode.as_ref().and_then(GcodeView::selected_move);
    // Going through a setup's G-code shows that setup's toolpaths only; otherwise the setups
    // whose eye is closed in the job list are left out.
    let only = window.gcode.as_ref().map(|g| g.setup.as_str());
    let shown = |p: &&solidcraft_engine::cmd::CamToolpath| match only {
        Some(s) => s == p.setup,
        None => !window.hidden.contains(&p.setup),
    };
    for path in paths.iter().filter(shown) {
        let Ok(points) = &path.points else { continue };
        let cut = if path.skipped { egui::Stroke::new(1.0, t.text_dim) } else { egui::Stroke::new(2.0, t.preview_sketch) };
        let rapid = egui::Stroke::new(1.0, t.text_dim);
        for pair in points.windows(2) {
            if let [(a, _), (b, cutting)] = pair
                && let (Some(a), Some(b)) = (proj.to_screen(*a), proj.to_screen(*b))
            {
                painter.line_segment([a, b], if *cutting { cut } else { rapid });
            }
        }
    }
    // Playback: the way the tool has come drawn over the toolpaths, and the tool where it is,
    // a line up its axis for the shank and a ring at its tip.
    if let Some(view) = window.gcode.as_ref()
        && let (Ok(program), Some((index, tip))) = (&view.program, view.tool_at())
    {
        let done = egui::Stroke::new(1.5, t.accent);
        for m in program.moves.iter().take(index) {
            let points: Vec<egui::Pos2> = m.points.iter().filter_map(|p| proj.to_screen(*p)).collect();
            painter.add(egui::Shape::line(points, done));
        }
        let shank = tip + program.axis * TOOL_MARK_LENGTH;
        if let (Some(a), Some(b)) = (proj.to_screen(tip), proj.to_screen(shank)) {
            painter.line_segment([a, b], egui::Stroke::new(5.0, t.text));
            painter.circle_stroke(a, 6.0, egui::Stroke::new(2.5, t.accent));
        }
        return;
    }
    // The G-code line picked in the window: its move drawn over the rest, its end marked.
    if let Some(m) = selected {
        let points: Vec<egui::Pos2> = m.points.iter().filter_map(|p| proj.to_screen(*p)).collect();
        let stroke = egui::Stroke::new(if m.cutting { 4.0 } else { 2.5 }, t.accent);
        painter.add(egui::Shape::line(points.clone(), stroke));
        if let Some(end) = points.last() {
            painter.circle_stroke(*end, 5.0, stroke);
        }
    }
}
