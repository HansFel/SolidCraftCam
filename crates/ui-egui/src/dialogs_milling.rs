//! Guided rectangle milling and hole-pattern CAM. The form invokes the same engine commands
//! as scripts; the preview worker never changes the open document.
use crate::selection::{EDGES, PLANAR_FACES, PROFILES, SelInput, SelectionExt as _, VERTICES};
use crate::viewport::Proj;
use crate::{SolveApp, theme::Tokens, ui_kit};
use egui::{Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::cam::{MATERIALS, Tool, ToolKind};
use solidcraft_engine::geom::Vec3;
use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Debug)]
pub struct Milling {
    kind: usize,
    setup: String,
    name: String,
    values: Vec<String>,
    holes: String,
    output: String,
    preview: Option<Value>,
    error: Option<String>,
    pending: Option<Arc<Mutex<mpsc::Receiver<Result<Value, String>>>>>,
    submitted: Option<Value>,
    source: Option<Arc<solidcraft_engine::doc::Document>>,
    /// Picked geometry (`sketch` and `profiles`, or `faces` and `edges`); null: the rectangle.
    geometry: Value,
    /// Where a contour starts (model coordinates): a picked corner or the middle of an edge.
    start: Option<[f64; 3]>,
    /// The library tool chosen: copied into the design (with its cutting data) when the job runs.
    library_tool: Option<Tool>,
    /// The setup's material group; None keeps the setup's own.
    pub(crate) material: Option<String>,
    /// An edited step's stored geometry (`{"geometry": …}`) and start point, used while nothing
    /// new is picked.
    kept: Value,
    kept_start: Option<[f64; 3]>,
    /// The last final-depth pick taken into the field; a new pick sets the field once.
    bottom_pick: Option<[f64; 3]>,
    /// The same for the start depth.
    top_pick: Option<[f64; 3]>,
    /// An edit: OK replaces the step of this name, which stays fixed.
    editing: bool,
    /// A new step's name is already a step of its setup: OK would replace that one.
    taken: bool,
    /// A contour's side, as [`SIDES`] lists them.
    side: usize,
    /// Climb milling; false: conventional.
    climb: bool,
}

/// A contour's sides: where the part stays whole, outside or inside each picked contour, on it.
const SIDES: [&str; 4] = ["auto", "outside", "inside", "on"];

/// `base`, or `base 2`, `base 3`, … : the first name no step of `setup` has.
fn free_name(app: &SolveApp, setup: &str, base: &str) -> String {
    let used = |n: &str| app.session.doc.cam.setup(setup).is_some_and(|s| s.operations.iter().any(|o| o.name() == n));
    if !used(base) {
        return base.to_string();
    }
    (2..=10_000).map(|i| format!("{base} {i}")).find(|n| !used(n)).unwrap_or_else(|| base.to_string())
}

/// The form's picks: contours to machine (edges, planar faces, sketch profiles; a click adds
/// or removes one), the contour's start point, and the heights the final and the start depth
/// are taken from.
pub fn inputs() -> Vec<SelInput> {
    vec![
        SelInput::new("Contours", PROFILES | PLANAR_FACES | EDGES, true),
        SelInput::new("Start point", VERTICES | EDGES, false),
        SelInput::new("Final depth", VERTICES | EDGES | PLANAR_FACES, false),
        SelInput::new("Start depth", VERTICES | EDGES | PLANAR_FACES, false),
    ]
}

/// The top of setup `setup`'s stock in its work frame: where cutting starts unless an earlier
/// step took material off. None for a setup not made yet.
pub(crate) fn stock_top(session: &solidcraft_engine::Session, setup: &str) -> Option<f64> {
    let outline = session.cam_setup_outline(setup)?;
    let corners = outline.stock?;
    let top = corners.iter().map(|p| (*p - outline.origin).dot(outline.axes[2])).fold(f64::NEG_INFINITY, f64::max);
    top.is_finite().then_some((top * 1000.0).round() / 1000.0)
}

/// A field's millimetres: a number, or a share of the tool diameter (`50 %`).
fn millimetres(text: &str, diameter: f64) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    match text.strip_suffix('%') {
        Some(share) => share.trim().parse::<f64>().ok().map(|p| p / 100.0 * diameter),
        None => text.parse().ok(),
    }
}

fn plain_number(v: f64) -> String {
    format!("{:.3}", v).trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A model point's height in setup `setup`'s work frame: what a picked final depth becomes. A
/// setup not made yet has the model's frame.
pub(crate) fn work_z(session: &solidcraft_engine::Session, setup: &str, p: Vec3) -> f64 {
    match session.cam_setup_outline(setup) {
        Some(o) => (p - o.origin).dot(o.axes[2]),
        None => p.z,
    }
}

const FIELDS: &[(&str, &str, &str)] = &[
    ("tool", "ui.cam.tool", "1"),
    ("diameter", "ui.cam.diameter", "6"),
    ("flute_length", "ui.cam.flute", "20"),
    ("x", "ui.cam.x", "0"),
    ("y", "ui.cam.y", "0"),
    ("width", "ui.cam.width", "40"),
    ("height", "ui.cam.height", "30"),
    ("top", "ui.cam.top", "0"),
    ("bottom", "ui.cam.bottom", "-3"),
    ("clearance", "ui.cam.clearance", "10"),
    ("retract", "ui.cam.retract", "2"),
    ("stepdown", "ui.cam.stepdown", "100 %"),
    ("stepover", "ui.cam.stepover", "50 %"),
    ("stock_to_leave", "ui.cam.wall_stock", "0"),
    ("floor_stock", "ui.cam.floor_stock", "0"),
    ("overrun", "ui.cam.overrun", "2"),
    ("feed", "ui.cam.feed", "600"),
    ("plunge_feed", "ui.cam.plunge", "150"),
    ("spindle_rpm", "ui.cam.rpm", "6000"),
    ("work_offset", "ui.cam.wcs", "1"),
    ("dwell", "ui.cam.dwell", "0"),
];

impl Milling {
    pub fn computing(&self) -> bool {
        self.pending.is_some()
    }

    pub fn new(app: &SolveApp) -> Self {
        let mut values: Vec<_> = FIELDS.iter().map(|(_, _, value)| (*value).into()).collect();
        let next_tool = (1..=9999).find(|n| app.session.doc.cam.tool(*n).is_none()).unwrap_or(1);
        if let Some(tool) = values.first_mut() {
            *tool = next_tool.to_string();
        }
        // A new step goes into the last setup; the first one makes a setup.
        let setup = app.session.doc.cam.setups.last();
        let setup_name = setup.map_or_else(|| app.session.doc.cam.unused_setup_name(), |s| s.name.clone());
        Self {
            kind: 1,
            name: free_name(app, &setup_name, app.tr("ui.cam.pocket")),
            setup: setup_name,
            values,
            holes: "10 10\n30 10".into(),
            output: String::new(),
            preview: None,
            error: None,
            pending: None,
            submitted: None,
            source: None,
            geometry: Value::Null,
            start: None,
            library_tool: None,
            material: setup.and_then(|s| s.material.clone()),
            kept: Value::Null,
            kept_start: None,
            bottom_pick: None,
            top_pick: None,
            editing: false,
            taken: false,
            side: 0,
            climb: true,
        }
        .starting_at_stock(app)
    }

    /// Start depth at the setup's stock top, the safety heights kept above it.
    fn starting_at_stock(mut self, app: &SolveApp) -> Self {
        let top = stock_top(&app.session, &self.setup).unwrap_or(0.0);
        self.set_top(top);
        self
    }

    fn field(&mut self, key: &str) -> Option<&mut String> {
        FIELDS.iter().position(|(k, _, _)| *k == key).and_then(|i| self.values.get_mut(i))
    }

    /// Sets the start depth; retract and clearance heights below it move up above it.
    fn set_top(&mut self, top: f64) {
        if let Some(field) = self.field("top") {
            *field = plain_number(top);
        }
        let number = |s: &mut Self, key: &str| s.field(key).and_then(|v| v.trim().parse::<f64>().ok());
        let retract = number(self, "retract").unwrap_or(2.0);
        let retract = if retract < top + 1.0 { top + 2.0 } else { retract };
        if let Some(field) = self.field("retract") {
            *field = plain_number(retract);
        }
        let clearance = number(self, "clearance").unwrap_or(10.0);
        if clearance < retract
            && let Some(field) = self.field("clearance")
        {
            *field = plain_number(retract + 8.0);
        }
    }

    /// The form for an existing step: its values, tool and geometry; OK replaces it in place.
    pub fn edit(app: &SolveApp, setup: &solidcraft_engine::doc::cam::CamSetup, op: &solidcraft_engine::doc::cam::CamOperation) -> Self {
        use solidcraft_engine::doc::cam::{CamGeometry, CamOperation};
        let mut form = Self::new(app);
        let (kind, cut, rpm) = match op {
            CamOperation::Contour { cut, spindle_rpm, .. } => (0, cut, spindle_rpm),
            CamOperation::Pocket { cut, spindle_rpm, .. } => (1, cut, spindle_rpm),
            CamOperation::Drill { cut, spindle_rpm, .. } => (2, cut, spindle_rpm),
            CamOperation::Face { cut, spindle_rpm, .. } => (3, cut, spindle_rpm),
        };
        let tool = app.session.doc.cam.tool(op.tool());
        let number = |v: f64| format!("{:.4}", v).trim_end_matches('0').trim_end_matches('.').to_string();
        // The fields take millimetres: a stored "0 mm" shows as 0.
        let plain = |e: &solidcraft_engine::doc::Expr| e.as_str().trim().strip_suffix("mm").map_or(e.as_str(), str::trim_end).to_string();
        let mut set = |key: &str, value: String| {
            if let Some(i) = FIELDS.iter().position(|(k, _, _)| *k == key)
                && let Some(field) = form.values.get_mut(i)
            {
                *field = value;
            }
        };
        set("tool", op.tool().to_string());
        if let Some(tool) = tool {
            set("diameter", number(tool.diameter));
            set("flute_length", number(tool.flute_length));
        }
        for (key, expr) in [
            ("top", &cut.top),
            ("bottom", &cut.bottom),
            ("clearance", &cut.clearance),
            ("retract", &cut.retract),
            ("stepdown", &cut.stepdown),
            ("feed", &cut.feed),
            ("plunge_feed", &cut.plunge_feed),
        ] {
            set(key, plain(expr));
        }
        if let Some(rpm) = rpm.or(tool.and_then(|t| t.spindle_rpm)) {
            set("spindle_rpm", number(rpm));
        }
        set("work_offset", setup.work_offset.to_string());
        match op {
            CamOperation::Pocket { params, .. } => {
                set("stepover", plain(&params.stepover));
                set("stock_to_leave", plain(&params.stock_to_leave));
                set("floor_stock", plain(&params.floor_stock));
            }
            CamOperation::Drill { dwell, .. } => set("dwell", number(*dwell)),
            CamOperation::Contour { params, .. } => {
                set("stock_to_leave", plain(&params.stock_to_leave));
                set("floor_stock", plain(&params.floor_stock));
            }
            CamOperation::Face { params, .. } => {
                set("stepover", plain(&params.stepover));
                set("overrun", plain(&params.overrun));
            }
        }
        match op {
            CamOperation::Contour { params, .. } => {
                use solidcraft_engine::doc::cam::CamSide;
                form.side = match params.side {
                    CamSide::Auto => 0,
                    CamSide::Outside => 1,
                    CamSide::Inside => 2,
                    CamSide::On => 3,
                };
                form.climb = params.climb;
            }
            CamOperation::Pocket { params, .. } => form.climb = params.climb,
            CamOperation::Drill { .. } | CamOperation::Face { .. } => {}
        }
        match op {
            CamOperation::Drill { geometry: CamGeometry::Points { points }, .. } => {
                form.holes = points.iter().map(|[x, y]| format!("{} {}", number(*x), number(*y))).collect::<Vec<_>>().join(
                    "
",
                );
            }
            CamOperation::Contour { geometry, .. } | CamOperation::Pocket { geometry, .. } => {
                form.kept = serde_json::to_value(geometry).map(|g| json!({"geometry": g})).unwrap_or(Value::Null);
            }
            CamOperation::Drill { .. } | CamOperation::Face { .. } => {}
        }
        if let CamOperation::Contour { params, .. } = op {
            form.kept_start = params.start;
        }
        (form.geometry, form.start) = (form.kept.clone(), form.kept_start);
        form.kind = kind;
        form.setup = setup.name.clone();
        form.name = op.name().to_string();
        form.editing = true;
        form.material = setup.material.clone();
        form
    }

    /// The form opened from an operation's own button: contour, pocket or drill chosen.
    pub fn for_operation(app: &SolveApp, id: &str) -> Self {
        let (kind, name) = match id {
            "cam.op.contour" => (0, "ui.cam.contour"),
            "cam.op.drill" => (2, "ui.cam.drill"),
            "cam.op.face" => (3, "ui.cam.face"),
            _ => (1, "ui.cam.pocket"),
        };
        let form = Self::new(app);
        let mut form = Self { kind, name: free_name(app, &form.setup, app.tr(name)), ..form };
        // Facing ends where the part's top is: at part zero.
        if kind == 3
            && let Some(bottom) = form.field("bottom")
        {
            *bottom = "0".into();
        }
        form
    }

    #[cfg(test)]
    pub(crate) fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    #[cfg(test)]
    pub(crate) fn set_cutting(&mut self, side: usize, climb: bool, wall: &str, floor: &str) {
        (self.side, self.climb) = (side, climb);
        for (key, value) in [("stock_to_leave", wall), ("floor_stock", floor)] {
            if let Some(i) = FIELDS.iter().position(|(k, _, _)| *k == key)
                && let Some(field) = self.values.get_mut(i)
            {
                *field = value.to_string();
            }
        }
    }

    pub fn params(&self) -> Result<Value, String> {
        if self.taken {
            return Err(crate::i18n::tr("ui.cam.name_taken").into());
        }
        let mut p = json!({"setup": self.setup, "name": self.name,
            "kind": match self.kind { 0 => "contour", 2 => "drill", 3 => "face", _ => "pocket" }});
        let diameter = self.values.get(1).and_then(|v| v.trim().replace(',', ".").parse::<f64>().ok()).unwrap_or(0.0);
        for ((key, _, _), value) in FIELDS.iter().zip(&self.values) {
            // Stepdown and stepover can be a share of the tool diameter.
            let number =
                if matches!(*key, "stepdown" | "stepover") { millimetres(value, diameter) } else { value.trim().replace(',', ".").parse().ok() };
            let number: f64 = number.ok_or_else(|| crate::i18n::tr("ui.cam.invalid_number").to_string())?;
            if !number.is_finite() {
                return Err(crate::i18n::tr("ui.cam.invalid_number").into());
            }
            p[*key] = if *key == "tool" || *key == "work_offset" {
                if number < 1.0 || number.fract() != 0.0 || number > 9999.0 {
                    return Err(crate::i18n::tr("ui.cam.invalid_number").into());
                }
                json!(number as u32)
            } else {
                json!(number)
            };
        }
        let mut points = Vec::new();
        if self.kind == 2 {
            for line in self.holes.lines().filter(|line| !line.trim().is_empty()) {
                let values = line
                    .split_whitespace()
                    .map(str::parse::<f64>)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| crate::i18n::tr("ui.cam.holes_hint").to_string())?;
                let [x, y] = values.as_slice() else {
                    return Err(crate::i18n::tr("ui.cam.holes_hint").into());
                };
                if !x.is_finite() || !y.is_finite() {
                    return Err(crate::i18n::tr("ui.cam.invalid_number").into());
                }
                points.push([*x, *y]);
            }
        }
        p["points"] = json!(points);
        if let Some(geometry) = self.geometry.as_object() {
            for (key, value) in geometry {
                p[key.as_str()] = value.clone();
            }
        }
        if let (0, Some(start)) = (self.kind, self.start) {
            p["start"] = json!(start);
        }
        if self.kind == 0 {
            p["side"] = json!(SIDES.get(self.side).copied().unwrap_or("auto"));
        }
        // Only facing runs past the stock.
        if self.kind != 3
            && let Some(map) = p.as_object_mut()
        {
            map.remove("overrun");
        }
        if self.kind < 2 {
            p["climb"] = json!(self.climb);
            for key in ["stock_to_leave", "floor_stock"] {
                if p[key].as_f64().is_some_and(|v| v < 0.0) {
                    return Err(crate::i18n::tr("ui.cam.stock_negative").into());
                }
            }
        }
        if let Some(material) = &self.material {
            p["material"] = json!(material);
        }
        Ok(p)
    }

    /// The tool the form describes: the library's when one is chosen, else one from the fields.
    fn tool(&self) -> Result<Tool, String> {
        let value = |i: usize| {
            self.values.get(i).and_then(|v| v.trim().parse::<f64>().ok()).ok_or_else(|| crate::i18n::tr("ui.cam.invalid_number").to_string())
        };
        let kind = if self.kind == 2 { ToolKind::Drill } else { ToolKind::FlatEndmill };
        let mut tool = self.library_tool.clone().unwrap_or_else(|| Tool::new(1, kind, 1.0, 1.0));
        tool.number = value(0)? as u32;
        (tool.diameter, tool.flute_length) = (value(1)?, value(2)?);
        Ok(tool)
    }

    /// Takes a library tool (or none): its number, diameter and flute length fill the fields.
    pub(crate) fn choose_tool(&mut self, tool: Option<Tool>) {
        if let Some(tool) = &tool {
            for (i, value) in [(0, f64::from(tool.number)), (1, tool.diameter), (2, tool.flute_length)] {
                if let Some(field) = self.values.get_mut(i) {
                    *field = format!("{value}");
                }
            }
        }
        self.library_tool = tool;
    }

    /// Fills spindle speed, feeds, stepdown and stepover from the tool's cutting data for the material.
    pub(crate) fn take_cutting(&mut self, app: &SolveApp) -> Result<(), String> {
        let tr = crate::i18n::tr;
        let material = self.material.clone().ok_or_else(|| tr("ui.cam.pick_material").to_string())?;
        let tool = self.tool()?;
        let data = tool.cutting_for(&material).ok_or_else(|| tr("ui.cam.no_cutting").to_string())?;
        let max_rpm = app.session.doc.cam.setup(&self.setup).and_then(|s| s.max_rpm);
        let mut s = data.speeds(&tool, max_rpm).map_err(|e| e.to_string())?;
        // Held to the setup's machine: its spindle range and fastest feed.
        if let Some(machine) = app.session.doc.cam.setup(&self.setup).and_then(|x| app.session.doc.cam.setup_machine(x)) {
            s = machine.hold(s);
        }
        for (key, value) in
            [("spindle_rpm", s.spindle_rpm), ("feed", s.feed), ("plunge_feed", s.plunge_feed), ("stepdown", s.stepdown), ("stepover", s.stepover)]
        {
            if let Some(i) = FIELDS.iter().position(|(k, _, _)| *k == key)
                && let Some(field) = self.values.get_mut(i)
            {
                *field = format!("{:.3}", value).trim_end_matches('0').trim_end_matches('.').to_string();
            }
        }
        Ok(())
    }

    fn picked(&self) -> bool {
        !self.geometry.is_null()
    }
}

/// The picks as job parameters: profiles of one sketch, else planar faces by persistent name
/// and the contours through picked edges.
fn geometry(app: &SolveApp, inputs: &[SelInput]) -> Value {
    let picks = crate::selection::items(inputs, 0);
    let profiles = picks.profiles();
    if let Some((sketch, _)) = profiles.first() {
        let indices: Vec<usize> = profiles.iter().filter(|(s, _)| s == sketch).map(|(_, i)| *i).collect();
        return json!({"sketch": sketch, "profiles": indices});
    }
    let state = app.session.world_state();
    let faces: Vec<Value> = picks
        .iter()
        .filter_map(|pick| {
            let Sel::Face { body, index, point } = pick else { return None };
            // A face the model gives no name is found again by the picked point.
            let name = solidcraft_engine::doc::naming::face_names(state.body(body)?).get(*index).cloned().unwrap_or_default();
            Some(json!({"body": body, "name": name, "at": [point.x, point.y, point.z]}))
        })
        .collect();
    let edges: Vec<Value> = picks
        .iter()
        .filter_map(|pick| match pick {
            Sel::Edge { body, point, .. } => Some(json!({"body": body, "at": [point.x, point.y, point.z]})),
            _ => None,
        })
        .collect();
    let mut out = serde_json::Map::new();
    if !faces.is_empty() {
        out.insert("faces".into(), json!(faces));
    }
    if !edges.is_empty() {
        out.insert("edges".into(), json!(edges));
    }
    if out.is_empty() { Value::Null } else { Value::Object(out) }
}

/// The picked start point: a corner, or the middle of an edge.
fn start(inputs: &[SelInput]) -> Option<[f64; 3]> {
    match crate::selection::items(inputs, 1).first()? {
        Sel::Vertex { point, .. } | Sel::Edge { point, .. } => Some([point.x, point.y, point.z]),
        _ => None,
    }
}

/// How many contours `items` are: the pieces of one pick (both halves of a round rim) count once.
pub(crate) fn pick_count(session: &solidcraft_engine::Session, items: &[Sel]) -> usize {
    let mut seen: Vec<Sel> = Vec::new();
    let mut count = 0;
    for pick in items {
        if seen.iter().any(|x| crate::selection::same_item(x, pick)) {
            continue;
        }
        count += 1;
        seen.extend(crate::viewport::grouped_selection(session, pick));
        seen.push(pick.clone());
    }
    count
}

/// One line per picked contour, each with a button that takes it out again. The pieces of one
/// pick (both halves of a round rim) share a line and go together.
fn contour_list(ui: &mut egui::Ui, session: &solidcraft_engine::Session, inputs: &mut [SelInput], drill: bool) {
    let tr = crate::i18n::tr;
    let Some(input) = inputs.first_mut() else { return };
    if input.items.is_empty() {
        ui.add(egui::Label::new(tr(if drill { "ui.cam.pick_bores_hint" } else { "ui.cam.pick_hint" })).wrap());
        return;
    }
    let group = |pick: &Sel| {
        let mut g = crate::viewport::grouped_selection(session, pick);
        if !g.iter().any(|x| crate::selection::same_item(x, pick)) {
            g.push(pick.clone());
        }
        g
    };
    let groups: Vec<Vec<Sel>> = input.items.iter().map(group).collect();
    let mut remove = None;
    let mut line = 0;
    for (n, pick) in input.items.iter().enumerate() {
        if groups.iter().take(n).any(|g| g.iter().any(|x| crate::selection::same_item(x, pick))) {
            continue;
        }
        line += 1;
        let what = match pick {
            Sel::Edge { body, .. } if drill => format!("{} – {body}", tr("ui.cam.bore")),
            Sel::Face { body, .. } if drill => format!("{} – {body}", tr("ui.cam.face_bores")),
            Sel::Edge { body, .. } => format!("{} – {body}", tr("ui.cam.edge_contour")),
            Sel::Face { body, .. } => format!("{} – {body}", tr("ui.cam.face_contours")),
            Sel::Profile { index, .. } => format!("{} {}", tr("ui.cam.sketch_profile"), index + 1),
            _ => continue,
        };
        ui.horizontal(|ui| {
            ui.label(format!("{line}. {what}"));
            if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                remove = Some(n);
            }
        });
    }
    if let Some(group) = remove.and_then(|n| groups.get(n)) {
        input.items.retain(|x| !group.iter().any(|y| crate::selection::same_item(x, y)));
    }
}

pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, form: &mut Milling, inputs: &mut [SelInput]) {
    let tr = crate::i18n::tr;
    let kinds = [tr("ui.cam.contour"), tr("ui.cam.pocket"), tr("ui.cam.drill"), tr("ui.cam.face")];
    ui_kit::choice_row(ui, tr("ui.cam.operation"), "cam-kind", &mut form.kind, &kinds);
    if form.kind == 3 {
        ui.add(egui::Label::new(tr("ui.cam.face_hint")).wrap_mode(egui::TextWrapMode::Wrap));
    } else {
        contour_list(ui, &app.session, inputs, form.kind == 2);
    }
    if !form.kept.is_null() && crate::selection::items(inputs, 0).is_empty() {
        // An edited step keeps its geometry until something new is picked.
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(tr("ui.cam.kept_geometry")).wrap());
            if ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                form.kept = Value::Null;
            }
        });
    }
    if form.kind == 0 {
        let start_label = if form.start.is_some() { tr("ui.cam.start_set") } else { tr("ui.cam.start_hint") };
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(start_label).wrap());
            if form.start.is_some() && ui_kit::button(ui, tr("ui.cam.remove"), ui_kit::Button::Small).clicked() {
                form.kept_start = None;
                if let Some(input) = inputs.get_mut(1) {
                    input.items.clear();
                }
            }
        });
    }
    // Where the tool rides and which way it turns, right with the contours they apply to.
    if form.kind == 0 {
        let sides = [tr("ui.cam.side_auto"), tr("ui.cam.side_outside"), tr("ui.cam.side_inside"), tr("ui.cam.side_on")];
        ui_kit::choice_row(ui, tr("ui.cam.side"), "cam-side", &mut form.side, &sides);
    }
    if form.kind < 2 {
        let directions = [tr("ui.cam.climb"), tr("ui.cam.conventional")];
        let mut direction = usize::from(!form.climb);
        ui_kit::choice_row(ui, tr("ui.cam.direction"), "cam-direction", &mut direction, &directions);
        form.climb = direction == 0;
    }
    let picked = geometry(app, inputs);
    form.geometry = if picked.is_null() { form.kept.clone() } else { picked };
    form.start = start(inputs).or(form.kept_start);
    // A picked face, edge or corner gives the final depth; the field stays free to change.
    let depth = match crate::selection::items(inputs, 2).first() {
        Some(Sel::Vertex { point, .. } | Sel::Edge { point, .. } | Sel::Face { point, .. }) => Some(*point),
        _ => None,
    };
    // A picked face, edge or corner gives the start depth too (the stock's top until then).
    let start_depth = match crate::selection::items(inputs, 3).first() {
        Some(Sel::Vertex { point, .. } | Sel::Edge { point, .. } | Sel::Face { point, .. }) => Some(*point),
        _ => None,
    };
    let top_key = start_depth.map(|p| [p.x, p.y, p.z]);
    if top_key != form.top_pick {
        form.top_pick = top_key;
        if let Some(p) = start_depth {
            let z = work_z(&app.session, &form.setup, p);
            form.set_top((z * 1000.0).round() / 1000.0);
        }
    }
    let depth_key = depth.map(|p| [p.x, p.y, p.z]);
    if depth_key != form.bottom_pick {
        form.bottom_pick = depth_key;
        if let Some(p) = depth
            && let Some(i) = FIELDS.iter().position(|(k, _, _)| *k == "bottom")
            && let Some(field) = form.values.get_mut(i)
        {
            let z = work_z(&app.session, &form.setup, p);
            *field = format!("{:.3}", z).trim_end_matches('0').trim_end_matches('.').to_string();
        }
    }
    setup_row(app, ui, form);
    if form.editing {
        ui.label(format!("{}: {}", tr("ui.cam.name"), form.name));
    } else {
        ui_kit::text_row(ui, tr("ui.cam.name"), &mut form.name, "");
    }
    // A new step never replaces another one silently; Edit is for that.
    form.taken = !form.editing && app.session.doc.cam.setup(&form.setup).is_some_and(|s| s.operations.iter().any(|o| o.name() == form.name.trim()));
    if form.taken {
        ui.add(egui::Label::new(egui::RichText::new(tr("ui.cam.name_taken")).color(Tokens::get().warning)).wrap());
    }
    tool_rows(app, ui, form);
    let picked = form.picked();
    for ((key, label, _), value) in FIELDS.iter().zip(&mut form.values) {
        if (picked || form.kind == 3) && matches!(*key, "x" | "y" | "width" | "height") {
            continue;
        }
        if (form.kind == 3) != (*key == "overrun") && matches!(*key, "overrun" | "stock_to_leave" | "floor_stock") {
            continue;
        }
        if form.kind == 2 && matches!(*key, "x" | "y" | "width" | "height" | "stepover" | "feed" | "stock_to_leave" | "floor_stock") {
            continue;
        }
        if form.kind != 2 && *key == "dwell" {
            continue;
        }
        if form.kind == 0 && *key == "stepover" {
            continue;
        }
        // An existing setup keeps its own work offset.
        if *key == "work_offset" && app.session.doc.cam.setup(&form.setup).is_some() {
            continue;
        }
        ui_kit::text_row(ui, tr(label), value, "");
    }
    if form.kind == 2 && !picked {
        ui.add(egui::Label::new(tr("ui.cam.holes_hint")).wrap());
        ui_kit::text_area(ui, tr("ui.cam.holes_hint"), egui::Id::new("cam-holes"), &mut form.holes, 3, 100_000);
    }
    ui_kit::text_row(ui, tr("ui.cam.output"), &mut form.output, tr("ui.cam.output_hint"));
    let params = form.params();
    let source_current = form.source.as_ref().is_some_and(|doc| Arc::ptr_eq(doc, &app.session.doc));
    if params.as_ref().ok() != form.submitted.as_ref() || !source_current {
        form.preview = None;
    }
    let completed = form.pending.as_ref().and_then(|pending| {
        pending.lock().ok().and_then(|rx| match rx.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(tr("ui.cam.worker_stopped").into())),
            Err(mpsc::TryRecvError::Empty) => None,
        })
    });
    if let Some(result) = completed {
        form.pending = None;
        match result {
            Ok(value) if params.as_ref().ok() == form.submitted.as_ref() && source_current => {
                if value.get("gcode").is_some() {
                    form.preview = Some(value);
                } else {
                    app.set_status(tr("ui.cam.exported"), false);
                }
                form.error = None;
            }
            Ok(_) => {}
            Err(error) => form.error = Some(error),
        }
    }
    let exporting =
        form.pending.is_none() && !form.output.trim().is_empty() && ui_kit::button(ui, tr("ui.cam.export"), ui_kit::Button::Normal).clicked();
    if form.pending.is_some() {
        ui.add(egui::Label::new(tr("ui.cam.computing")).wrap());
        ui.ctx().request_repaint();
    } else if exporting || ui_kit::button(ui, tr("ui.cam.preview"), ui_kit::Button::Normal).clicked() {
        match params {
            Err(error) => form.error = Some(error),
            Ok(params) => {
                form.submitted = Some(params.clone());
                form.source = Some(app.session.doc.clone());
                let mut session = app.session.scratch();
                let output = exporting.then(|| form.output.trim().to_string());
                let tool = form.library_tool.as_ref().map(crate::cam_panel::define_params);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (tx, rx) = mpsc::channel();
                    let context = ui.ctx().clone();
                    match std::thread::Builder::new().name("cam-preview".into()).spawn(move || {
                        let result = compute(&mut session, &params, output, tool);
                        let _ = tx.send(result);
                        context.request_repaint();
                    }) {
                        Ok(_) => form.pending = Some(Arc::new(Mutex::new(rx))),
                        Err(error) => form.error = Some(error.to_string()),
                    }
                }
                #[cfg(target_arch = "wasm32")]
                match compute(&mut session, &params, output, tool) {
                    Ok(value) if value.get("gcode").is_some() => form.preview = Some(value),
                    Ok(_) => app.set_status(tr("ui.cam.exported"), false),
                    Err(error) => form.error = Some(error.to_string()),
                }
            }
        }
    }
    if let Some(error) = &form.error {
        crate::ui_kit::text(ui, error, crate::ui_kit::Text::Error("cam_preview"));
    }
    if let Some(value) = &form.preview {
        paint(ui, value);
        ui.add(egui::Label::new(tr("ui.cam.legend")).wrap());
        if ui_kit::button(ui, tr("ui.cam.copy"), ui_kit::Button::Normal).clicked()
            && let Some(code) = value.get("gcode").and_then(Value::as_str)
        {
            ui.ctx().copy_text(code.into());
        }
    }
}

/// The previewed toolpath on the model: cutting moves in the preview colour, apart from blue
/// selections; rapids dimmed.
pub fn overlay(painter: &egui::Painter, proj: &Proj, form: &Milling) {
    let Some(points) = form.preview.as_ref().and_then(|v| v.get("points")).and_then(Value::as_array) else { return };
    let at = |p: &Value| -> Option<(Vec3, bool)> {
        let m = p.get("model")?.as_array()?;
        Some((Vec3::new(m.first()?.as_f64()?, m.get(1)?.as_f64()?, m.get(2)?.as_f64()?), p.get("cutting")?.as_bool()?))
    };
    let points: Vec<_> = points.iter().filter_map(at).collect();
    let t = Tokens::get();
    if let Some(p) = form.start.and_then(|[x, y, z]| proj.to_screen(Vec3::new(x, y, z))) {
        painter.circle_stroke(p, 6.0, Stroke::new(2.0, t.preview_sketch));
    }
    for pair in points.windows(2) {
        if let [(a, _), (b, cutting)] = pair
            && let (Some(a), Some(b)) = (proj.to_screen(*a), proj.to_screen(*b))
        {
            let stroke = if *cutting { Stroke::new(2.0, t.preview_sketch) } else { Stroke::new(1.0, t.text_dim) };
            painter.line_segment([a, b], stroke);
        }
    }
}

fn paint(ui: &mut egui::Ui, value: &Value) {
    let Some(points) = value.get("points").and_then(Value::as_array) else { return };
    let points: Vec<_> = points
        .iter()
        .filter_map(|p| {
            let p3 = p.get("position")?.as_array()?;
            Some((p3.first()?.as_f64()?, p3.get(1)?.as_f64()?, p.get("cutting")?.as_bool()?))
        })
        .collect();
    let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    if !min_x.is_finite() {
        return;
    }
    let (response, painter) = ui.allocate_painter(vec2(ui.available_width(), 160.0), Sense::hover());
    let rect = response.rect.shrink(8.0);
    let scale = (f64::from(rect.width()) / (max_x - min_x).max(1.0)).min(f64::from(rect.height()) / (max_y - min_y).max(1.0));
    let point = |x: f64, y: f64| pos2(rect.left() + ((x - min_x) * scale) as f32, rect.bottom() - ((y - min_y) * scale) as f32);
    let t = Tokens::get();
    for pair in points.windows(2) {
        if let [a, b] = pair {
            painter.line_segment([point(a.0, a.1), point(b.0, b.1)], Stroke::new(1.0, if b.2 { t.accent } else { t.text_dim }));
        }
    }
    for (x, y, _) in &points {
        painter.circle_filled(point(*x, *y), 1.5, t.accent);
    }
}

/// The setup the step goes into: one of the design's, or a new one named here. Choosing a setup
/// takes its work offset and material.
fn setup_row(app: &SolveApp, ui: &mut egui::Ui, form: &mut Milling) {
    let tr = crate::i18n::tr;
    let cam = &app.session.doc.cam;
    if cam.setups.is_empty() {
        ui_kit::text_row(ui, tr("ui.cam.setup"), &mut form.setup, "");
        return;
    }
    let mut labels: Vec<&str> = cam.setups.iter().map(|s| s.name.as_str()).collect();
    labels.push(tr("ui.cam.new_setup_entry"));
    let current = cam.setups.iter().position(|s| s.name == form.setup).unwrap_or(cam.setups.len());
    let mut chosen = current;
    ui_kit::choice_row(ui, tr("ui.cam.setup_pick"), "cam-form-setup", &mut chosen, &labels);
    if chosen != current {
        match cam.setups.get(chosen) {
            Some(setup) => {
                form.setup = setup.name.clone();
                form.material = setup.material.clone();
                if let Some(top) = stock_top(&app.session, &setup.name) {
                    form.set_top(top);
                }
            }
            None => form.setup = cam.unused_setup_name(),
        }
    }
    if chosen == cam.setups.len() {
        ui_kit::text_row(ui, tr("ui.cam.new_setup_name"), &mut form.setup, "");
    }
    if let Some(setup) = cam.setup(&form.setup)
        && let Some(i) = FIELDS.iter().position(|(k, _, _)| *k == "work_offset")
        && let Some(field) = form.values.get_mut(i)
    {
        *field = setup.work_offset.to_string();
    }
}

/// The library tool to copy, the material and the button that takes their cutting data.
fn tool_rows(app: &SolveApp, ui: &mut egui::Ui, form: &mut Milling) {
    let tr = crate::i18n::tr;
    let library = &app.preferences.cam_tools.0;
    if !library.is_empty() {
        let labels: Vec<String> =
            std::iter::once(tr("ui.cam.own_tool").to_string()).chain(library.iter().map(crate::cam_panel::tool_label)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let current = form.library_tool.as_ref().and_then(|t| library.iter().position(|l| l.number == t.number)).map_or(0, |i| i + 1);
        let mut chosen = current;
        ui_kit::choice_row(ui, tr("ui.cam.library_tool"), "cam-library-tool", &mut chosen, &refs);
        if chosen != current {
            form.choose_tool(chosen.checked_sub(1).and_then(|i| library.get(i)).cloned());
        }
    }
    let labels: Vec<String> =
        std::iter::once(tr("ui.cam.mat_none").to_string()).chain(MATERIALS.iter().map(|m| crate::cam_panel::material_label(m))).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let current = form.material.as_ref().and_then(|m| MATERIALS.iter().position(|x| x == m)).map_or(0, |i| i + 1);
    let mut chosen = current;
    ui_kit::choice_row(ui, tr("ui.cam.material"), "cam-form-material", &mut chosen, &refs);
    if chosen != current {
        form.material = chosen.checked_sub(1).and_then(|i| MATERIALS.get(i)).map(|m| (*m).to_string());
    }
    if ui_kit::button(ui, tr("ui.cam.use_cutting"), ui_kit::Button::Normal).clicked() {
        form.error = form.take_cutting(app).err();
    }
}

pub fn run(app: &mut SolveApp, form: &Milling) -> Result<(), String> {
    let params = form.params()?;
    if let Some(tool) = &form.library_tool {
        app.run("cam.tool.define", crate::cam_panel::define_params(tool))?;
    }
    app.run("cam.job.create", params)?;
    Ok(())
}

fn compute(session: &mut solidcraft_engine::Session, params: &Value, output: Option<String>, tool: Option<Value>) -> Result<Value, String> {
    if let Some(tool) = tool {
        session.execute("cam.tool.define", &tool).map_err(|e| e.to_string())?;
    }
    if let Some(path) = output {
        session.execute("cam.job.create", params).map_err(|e| e.to_string())?;
        session.execute("cam.export", &json!({"setup":params.get("setup"), "path":path})).map_err(|e| e.to_string())
    } else {
        session.execute("cam.job.preview", params).map_err(|e| e.to_string())
    }
}
