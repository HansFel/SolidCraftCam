//! The CAM window: the job list (each setup's material, highest spindle speed and machining
//! steps in order, with skip, move and remove), the personal tool library with cutting data
//! by material, and the personal machine list (travel, spindle speeds, fastest feed). Both
//! lists live in the preferences and are shared by every design; a tool or machine used in a
//! design is copied into it (`cam.tool.define`, `cam.machine.define`), so a shared file stays
//! complete. The ribbon's Job list (`cam.list`), Tool library (`cam.tool.define`) and Machine
//! list (`cam.machine.define`) open it.

use egui::RichText;
use serde::{Deserialize, Serialize, de};
use serde_json::{Value, json};
use solidcraft_engine::cam::{CuttingData, MATERIALS, Machine, RotaryAxis, Tool, ToolKind};
use solidcraft_engine::doc::cam::{CamOperation, CamSetup};

use crate::SolveApp;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::ui_kit;

/// Most tools the library keeps.
const MAX_TOOLS: usize = 500;
/// Most machines the machine list keeps.
const MAX_MACHINES: usize = 100;

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

/// One line naming a machine and its limits.
pub fn machine_label(m: &Machine) -> String {
    let span = |[lo, hi]: [f64; 2]| number(hi - lo);
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
    }
}

const KINDS: [ToolKind; 3] = [ToolKind::FlatEndmill, ToolKind::BallEndmill, ToolKind::Drill];

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
    travel: [String; 6],
    a_kind: usize,
    a: [String; 2],
    spindle: [String; 2],
    max_feed: String,
}

impl MachineEdit {
    fn from_machine(m: &Machine) -> MachineEdit {
        let (a_kind, a) = match m.a {
            None => (0, ["-360".into(), "360".into()]),
            Some(RotaryAxis::Limited { min, max }) => (1, [number(min), number(max)]),
            Some(RotaryAxis::Endless) => (2, ["-360".into(), "360".into()]),
        };
        MachineEdit {
            name: m.name.clone(),
            travel: [number(m.x[0]), number(m.x[1]), number(m.y[0]), number(m.y[1]), number(m.z[0]), number(m.z[1])],
            a_kind,
            a,
            spindle: [number(m.spindle_min), number(m.spindle_max)],
            max_feed: number(m.max_feed),
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
        let machine = Machine {
            name: self.name.trim().to_string(),
            x: [t(0)?, t(1)?],
            y: [t(2)?, t(3)?],
            z: [t(4)?, t(5)?],
            a,
            spindle_min: parse(&self.spindle[0])?,
            spindle_max: parse(&self.spindle[1])?,
            max_feed: parse(&self.max_feed)?,
        };
        machine.validate().map_err(|e| e.to_string())?;
        Ok(machine)
    }
}

/// The window's state.
#[derive(Default)]
pub struct CamPanel {
    /// 0: job list, 1: tool library, 2: machines.
    pub tab: usize,
    /// The highest spindle speed as typed, by setup.
    rpm: Vec<(String, String)>,
    /// The library tool being edited (its number before the edit), or a new one.
    editing: Option<(Option<u32>, ToolEdit)>,
    /// The listed machine being edited (its name before the edit), or a new one.
    machine: Option<(Option<String>, MachineEdit)>,
    error: Option<String>,
    /// Every step's toolpath for the view, and the design they were made for.
    paths: Option<(usize, Vec<solidcraft_engine::cmd::CamToolpath>)>,
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
    /// Puts a machine on the personal list.
    Machine(Box<Machine>),
    /// Takes a machine off the personal list.
    Unmachine(String),
}

fn op_kind(op: &CamOperation) -> &'static str {
    match op {
        CamOperation::Contour { .. } => "cmd.cam.op.contour",
        CamOperation::Pocket { .. } => "cmd.cam.op.pocket",
        CamOperation::Drill { .. } => "cmd.cam.op.drill",
        CamOperation::Face { .. } => "cmd.cam.op.face",
    }
}

fn setup_rows(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, setup: &CamSetup, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&setup.name).strong());
        if ui_kit::button(ui, tr("ui.cam.edit_setup"), ui_kit::Button::Small).clicked() {
            actions.push(Action::EditSetup(setup.name.clone()));
        }
        if ui_kit::button(ui, tr("ui.cam.delete_setup"), ui_kit::Button::Small).clicked() {
            actions.push(Action::Run("cam.setup", json!({"name": setup.name, "delete": true})));
        }
    });
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
    if let Some(name) = &setup.machine {
        let line = app.session.doc.cam.machine(name).map_or_else(|| name.clone(), machine_label);
        ui.add(egui::Label::new(RichText::new(format!("{}: {line}", tr("ui.cam.machine"))).color(t.text_dim)).wrap());
    }
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
            ui.label(RichText::new(&name).color(if on { t.text } else { t.text_dim }));
            ui.label(RichText::new(tr(op_kind(op))).color(t.text_dim));
            let tool = app.session.doc.cam.tool(op.tool());
            let rpm = op.spindle_rpm().or(tool.and_then(|t| t.spindle_rpm));
            let mut info = tool.map_or(format!("T{}", op.tool()), |t| format!("T{} Ø{}", t.number, number(t.diameter)));
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
                if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                    actions.push(Action::Run("cam.op.remove", json!({"setup": setup.name, "name": name})));
                }
            });
            ui.end_row();
        }
    });
    if count > 0 && ui_kit::button(ui, tr("ui.cam.export_setup"), ui_kit::Button::Normal).clicked() {
        actions.push(Action::Export(setup.name.clone()));
    }
}

fn jobs_tab(app: &SolveApp, ui: &mut egui::Ui, d: &mut CamPanel, actions: &mut Vec<Action>) {
    let t = Tokens::get();
    ui.horizontal_wrapped(|ui| {
        for id in ["cam.setup", "cam.op.face", "cam.op.contour", "cam.op.pocket", "cam.op.drill"] {
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
    egui::ScrollArea::vertical().id_salt("cam-jobs").max_height(420.0).show(ui, |ui| {
        for setup in &setups {
            ui.separator();
            setup_rows(app, ui, d, setup, actions);
        }
    });
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
                if ui_kit::list_picker(ui, &format!("cam-tool-{}", tool.number), &tool_label(tool), picked).clicked() {
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
    let Some((original, edit)) = &mut d.editing else { return };
    ui.separator();
    let kinds: Vec<&str> = KINDS.iter().map(|k| tr(kind_key(*k))).collect();
    ui_kit::text_row(ui, tr("ui.cam.tool"), &mut edit.number, "");
    ui_kit::choice_row(ui, tr("ui.cam.kind"), "cam-tool-kind", &mut edit.kind, &kinds);
    ui_kit::text_row(ui, tr("ui.cam.diameter"), &mut edit.diameter, "");
    ui_kit::text_row(ui, tr("ui.cam.flutes"), &mut edit.flutes, "");
    ui_kit::text_row(ui, tr("ui.cam.flute"), &mut edit.flute_length, "");
    ui_kit::text_row(ui, tr("ui.cam.description"), &mut edit.description, "");
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
                    actions.push(Action::Run("cam.machine.define", serde_json::to_value(machine).unwrap_or(Value::Null)));
                }
                let picked = d.machine.as_ref().is_some_and(|(n, _)| n.as_deref() == Some(machine.name.as_str()));
                if ui_kit::list_picker(ui, &format!("cam-machine-{}", machine.name), &machine_label(machine), picked).clicked() {
                    d.machine = Some((Some(machine.name.clone()), MachineEdit::from_machine(machine)));
                }
            });
        }
    });
    if ui_kit::button(ui, tr("ui.cam.new_machine"), ui_kit::Button::Normal).clicked() {
        let name = (1..=MAX_MACHINES).map(|i| format!("{} {i}", tr("ui.cam.machine"))).find(|n| list.find(n).is_none()).unwrap_or_default();
        d.machine = Some((None, MachineEdit::from_machine(&Machine::new(name))));
    }
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
    for (label, value) in ["X−", "X+", "Y−", "Y+", "Z−", "Z+"].iter().zip(edit.travel.iter_mut()) {
        ui_kit::text_row(ui, &format!("{} {label} (mm)", tr("ui.cam.travel")), value, "");
    }
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
    ui_kit::text_row(ui, tr("ui.cam.max_feed"), &mut edit.max_feed, "");
    ui.add(egui::Label::new(RichText::new(tr("ui.cam.machine_hint")).color(t.text_dim)).wrap());
    if ui_kit::button(ui, tr("ui.cam.save_machine"), ui_kit::Button::Primary).clicked() {
        match edit.machine() {
            Ok(machine) => match list.name_free(&machine.name, original.as_deref()) {
                Err(e) => d.error = Some(e),
                Ok(()) => {
                    if let Some(old) = original.clone()
                        && old != machine.name
                    {
                        actions.push(Action::Unmachine(old));
                    }
                    *original = Some(machine.name.clone());
                    actions.push(Action::Machine(Box::new(machine)));
                }
            },
            Err(e) => d.error = Some(e),
        }
    }
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.cam_window.take() else { return };
    let mut actions = Vec::new();
    let closed = ui_kit::Window::new(egui::Id::new("sc_cam"), "cut", tr("ui.cam.panel_title")).cards(4.0).show_body(ctx, |ui| {
        let tabs = [tr("ui.cam.tab_jobs"), tr("ui.cam.tab_tools"), tr("ui.cam.tab_machines")];
        ui_kit::segmented(ui, &mut d.tab, &tabs, Default::default());
        ui.separator();
        match d.tab {
            0 => jobs_tab(app, ui, &mut d, &mut actions),
            1 => tools_tab(app, ui, &mut d, &mut actions),
            _ => machines_tab(app, ui, &mut d, &mut actions),
        }
        if let Some(e) = &d.error {
            // The window truncates text; a refusal is read in full.
            ui_kit::text(ui, e, ui_kit::Text::Error("cam"));
        }
    });
    for action in actions {
        let result = match action {
            Action::Run(id, params) => app.run(id, params).map(|_| ()),
            Action::Export(setup) => {
                app.pick_save_command(&format!("{setup}.ngc"), &["ngc"], "cam.export", json!({"setup": setup}));
                Ok(())
            }
            Action::Start(id) => {
                app.start(id);
                Ok(())
            }
            Action::EditStep(setup, name) => {
                let found = app.session.doc.cam.setup(&setup).and_then(|s| s.operations.iter().find(|o| o.name() == name).map(|o| (s, o)));
                if let Some((setup, op)) = found {
                    let form = crate::dialogs_milling::Milling::edit(app, setup, op);
                    app.dialog = Some(crate::dialogs::Dialog::new(crate::dialogs::Kind::Milling(form), crate::dialogs_milling::inputs()));
                }
                Ok(())
            }
            Action::EditSetup(name) => {
                if let Some(setup) = app.session.doc.cam.setup(&name) {
                    let form = crate::dialogs_cam_setup::CamSetupForm::edit(app, setup);
                    app.dialog = Some(crate::dialogs::Dialog::new(crate::dialogs::Kind::CamSetup(form), crate::dialogs_cam_setup::inputs()));
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
            Action::Machine(machine) => {
                let result = app.preferences.cam_machines.put(*machine);
                if result.is_ok() {
                    app.set_status(tr("ui.cam.saved"), false);
                }
                result
            }
            Action::Unmachine(name) => {
                app.preferences.cam_machines.0.retain(|m| m.name != name);
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

/// With the window open, every step's toolpath: cutting moves in the preview colour, rapid
/// moves thin, the steps left out of the G-code dimmed. Made again only when the design changes.
pub fn draw_paths(app: &mut SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let key = std::sync::Arc::as_ptr(&app.session.doc) as usize;
    let Some(window) = app.cam_window.as_mut() else { return };
    if window.paths.as_ref().is_none_or(|(k, _)| *k != key) {
        window.paths = Some((key, app.session.cam_toolpaths()));
    }
    let Some((_, paths)) = &window.paths else { return };
    let t = Tokens::get();
    for path in paths {
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
}
