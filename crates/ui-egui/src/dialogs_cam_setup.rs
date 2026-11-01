//! A CAM setup: name, work offset, part zero and axes picked on the model, stock, machine,
//! material and highest spindle speed. OK runs `cam.setup` with the picks; the engine works out
//! the frame and copies the chosen machine into the design.
use crate::selection::{EDGES, PLANAR_FACES, SelInput, VERTICES};
use crate::viewport::Proj;
use crate::{theme::Tokens, ui_kit};
use egui::Stroke;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::cam::{MATERIALS, Machine};
use solidcraft_engine::doc::cam::{BoxAxisRef, CamSetup, CamStock, StockOffsetMode, WcsOriginMode};

/// How part zero is found, in the order the form offers them: one picked point, a pick per
/// axis, the model's origin, a point of the model's or the stock's box.
const ZERO: [&str; 5] = ["selected_point", "selected_point", "model_origin", "model_box_point", "stock_box_point"];
/// The first box mode in [`ZERO`].
const BOX_MODES: usize = 3;
/// The picks per axis in [`inputs`]: X, Y and Z zero.
const AXIS_PICKS: std::ops::Range<usize> = 1..4;
const BOX_REFS: [&str; 3] = ["negative", "center", "positive"];

#[derive(Clone, Debug)]
pub struct CamSetupForm {
    pub(crate) name: String,
    /// The setup being changed; None makes a new one.
    editing: Option<String>,
    /// Work offset 1…6 as 0…5 (G54…G59).
    offset: usize,
    pub(crate) zero: usize,
    /// The box point per work axis: low, middle, high.
    corner: [usize; 3],
    /// Picked per axis: where an axis without a pick sits. 0: on the model's box, 1: on the
    /// stock's box (both at `corner`), 2: where it was (editing only).
    rest: usize,
    flip_z: bool,
    flip_x: bool,
    /// Box around the model, box of fixed size, round bar of fixed size.
    stock: usize,
    /// Around the model: allowance at the sides, on top, underneath. Fixed box: X, Y, height.
    /// Round bar: diameter, height.
    values: [String; 3],
    material: Option<String>,
    max_rpm: String,
    /// The machine the setup runs on: one of the personal list's or the design's.
    machine: Option<Machine>,
    /// The personal post that machine names, copied into the design with it.
    machine_post: Option<solidcraft_engine::cam::Post>,
}

/// The picks: part zero (a corner, or an edge: a round edge gives its centre), zero per axis
/// (a corner, an edge or a face each), the face +Z points out of, and the edge +X runs along.
pub fn inputs() -> Vec<SelInput> {
    let any = VERTICES | EDGES | PLANAR_FACES;
    vec![
        SelInput::new("Pick for part zero", VERTICES | EDGES, false),
        SelInput::new("X zero", any, false),
        SelInput::new("Y zero", any, false),
        SelInput::new("Z zero", any, false),
        SelInput::new("Tool axis face", PLANAR_FACES, false),
        SelInput::new("X edge", EDGES, false),
    ]
}

/// The picks for the axes (Z face, X edge), always shown.
pub const AXES_INPUTS: std::ops::Range<usize> = 4..6;

/// The pick rows the chosen part-zero mode uses, then the axis picks.
pub fn shown_inputs(form: &CamSetupForm) -> Vec<std::ops::Range<usize>> {
    let zero = match form.zero {
        0 => Some(0..1),
        1 => Some(AXIS_PICKS),
        _ => None,
    };
    zero.into_iter().chain(std::iter::once(AXES_INPUTS)).collect()
}

fn number(v: f64) -> String {
    format!("{:.3}", v).trim_end_matches('0').trim_end_matches('.').to_string()
}

fn box_ref(r: BoxAxisRef) -> usize {
    match r {
        BoxAxisRef::Negative => 0,
        BoxAxisRef::Center => 1,
        BoxAxisRef::Positive => 2,
    }
}

impl CamSetupForm {
    /// A new setup: the next free name, part zero at the top of the model's box, the model's axes.
    pub fn new(app: &crate::SolveApp) -> Self {
        Self {
            name: app.session.doc.cam.unused_setup_name(),
            editing: None,
            offset: 0,
            zero: 3,
            corner: [0, 0, 2],
            rest: 0,
            flip_z: false,
            flip_x: false,
            stock: 0,
            values: ["0".into(), "0".into(), "0".into()],
            material: app.session.doc.cam.setups.last().and_then(|s| s.material.clone()),
            max_rpm: String::new(),
            machine: app.session.doc.cam.setups.last().and_then(|s| app.session.doc.cam.setup_machine(s)).cloned(),
            machine_post: None,
        }
    }

    /// The form for an existing setup: unpicked, it keeps the setup's axes and part zero.
    pub fn edit(app: &crate::SolveApp, setup: &CamSetup) -> Self {
        let zero = match setup.wcs_origin_mode {
            WcsOriginMode::SelectedPoint => 0,
            WcsOriginMode::ModelOrigin => 2,
            WcsOriginMode::ModelBoxPoint => 3,
            WcsOriginMode::StockBoxPoint => 4,
        };
        let (stock, values) = match &setup.stock_spec {
            CamStock::Box { width, depth, height } => (1, [number(*width), number(*depth), number(*height)]),
            CamStock::Cylinder { diameter, height } => (2, [number(*diameter), number(*height), String::new()]),
            CamStock::RelativeBox { mode: StockOffsetMode::AllSides, side_offset, .. } => {
                (0, [number(*side_offset), number(*side_offset), number(*side_offset)])
            }
            CamStock::RelativeBox { mode: StockOffsetMode::SidesAndTopBottom, side_offset, top_offset, bottom_offset, .. } => {
                (0, [number(*side_offset), number(*top_offset), number(*bottom_offset)])
            }
            _ => (0, ["0".into(), "0".into(), "0".into()]),
        };
        Self {
            name: setup.name.clone(),
            editing: Some(setup.name.clone()),
            offset: usize::from(setup.work_offset.clamp(1, 6) - 1),
            zero,
            corner: [box_ref(setup.box_point.x), box_ref(setup.box_point.y), box_ref(setup.box_point.z)],
            rest: 2,
            flip_z: false,
            flip_x: false,
            stock,
            values,
            material: setup.material.clone(),
            max_rpm: setup.max_rpm.map(number).unwrap_or_default(),
            machine: app.session.doc.cam.setup_machine(setup).cloned(),
            machine_post: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_machine(&mut self, app: &crate::SolveApp, machine: Option<Machine>) {
        self.machine_post = machine.as_ref().and_then(|m| crate::cam_panel::machine_post(&app.preferences.cam_posts, m)).cloned();
        self.machine = machine;
    }

    /// The setup the form shows in the viewport.
    pub fn name(&self) -> &str {
        self.editing.as_deref().unwrap_or(&self.name)
    }

    pub fn params(&self, inputs: &[SelInput]) -> Result<Value, String> {
        let tr = crate::i18n::tr;
        let name = self.name.trim();
        if name.is_empty() {
            return Err(tr("ui.cam.setup_name_needed").into());
        }
        // Picked per axis puts the unpicked axes on a box first, unless they stay where they were.
        let mode = match (self.zero, self.rest) {
            (1, 0) => BOX_MODES,
            (1, 1) => BOX_MODES + 1,
            (zero, _) => zero,
        };
        let mut p = json!({"name": name, "work_offset": self.offset + 1, "wcs_origin_mode": ZERO.get(mode).copied().unwrap_or("selected_point")});
        // An edited setup keeps its name in `name`; a changed name field renames it.
        if let Some(old) = self.editing.as_deref().filter(|old| *old != name) {
            p["name"] = json!(old);
            p["rename"] = json!(name);
        }
        if mode >= BOX_MODES {
            let r = |i: usize| self.corner.get(i).and_then(|c| BOX_REFS.get(*c)).copied().unwrap_or("center");
            p["box_point"] = json!({"x": r(0), "y": r(1), "z": r(2)});
        }
        let at = |v: solidcraft_engine::geom::Vec3| json!([v.x, v.y, v.z]);
        if self.zero == 0 {
            match crate::selection::items(inputs, 0).first() {
                Some(Sel::Vertex { point, .. }) => p["work_origin"] = at(*point),
                Some(Sel::Edge { body, point, .. }) => p["zero_edge"] = json!({"body": body, "at": at(*point)}),
                _ if self.editing.is_none() => return Err(tr("ui.cam.zero_needed").into()),
                _ => {}
            }
        }
        if self.zero == 1 {
            let mut axes = serde_json::Map::new();
            for (key, i) in ["x", "y", "z"].into_iter().zip(AXIS_PICKS) {
                let pick = match crate::selection::items(inputs, i).first() {
                    Some(Sel::Vertex { body, point }) | Some(Sel::Face { body, point, .. }) => json!({"body": body, "at": at(*point)}),
                    Some(Sel::Edge { body, point, .. }) => json!({"body": body, "at": at(*point), "edge": true}),
                    _ => continue,
                };
                axes.insert(key.into(), pick);
            }
            if axes.is_empty() && self.rest == 2 {
                return Err(tr("ui.cam.zero_axes_needed").into());
            }
            if !axes.is_empty() {
                p["zero_axes"] = Value::Object(axes);
            }
        }
        if let Some(Sel::Face { body, point, .. }) = crate::selection::items(inputs, 4).first() {
            p["z_face"] = json!({"body": body, "at": at(*point)});
        }
        if let Some(Sel::Edge { body, point, .. }) = crate::selection::items(inputs, 5).first() {
            p["x_edge"] = json!({"body": body, "at": at(*point)});
        }
        if self.flip_z {
            p["flip_z"] = json!(true);
        }
        if self.flip_x {
            p["flip_x"] = json!(true);
        }
        let value = |i: usize| -> Result<f64, String> {
            let text = self.values.get(i).map(|v| v.trim().replace(',', ".")).unwrap_or_default();
            let v = if text.is_empty() { 0.0 } else { text.parse::<f64>().map_err(|_| tr("ui.cam.invalid_number").to_string())? };
            if v.is_finite() && (0.0..=1.0e6).contains(&v) { Ok(v) } else { Err(tr("ui.cam.invalid_number").into()) }
        };
        p["stock_spec"] = match self.stock {
            1 => json!({"kind": "box", "width": value(0)?, "depth": value(1)?, "height": value(2)?}),
            2 => json!({"kind": "cylinder", "diameter": value(0)?, "height": value(1)?}),
            _ => {
                let (side, top, bottom) = (value(0)?, value(1)?, value(2)?);
                let mode = if side == 0.0 && top == 0.0 && bottom == 0.0 { "no_additional" } else { "sides_and_top_bottom" };
                json!({"kind": "relative_box", "mode": mode, "side_offset": side, "top_offset": top, "bottom_offset": bottom, "round_up": 0})
            }
        };
        p["material"] = self.material.as_ref().map_or(Value::Null, |m| json!(m));
        p["machine"] = self.machine.as_ref().and_then(|m| serde_json::to_value(m).ok()).unwrap_or(Value::Null);
        if let Some(post) = self.machine_post.as_ref().filter(|p| self.machine.as_ref().is_some_and(|m| m.post.as_ref() == Some(&p.name))) {
            p["machine_post"] = serde_json::to_value(post).unwrap_or(Value::Null);
        }
        let rpm = self.max_rpm.trim().replace(',', ".");
        p["max_rpm"] = if rpm.is_empty() { Value::Null } else { json!(rpm.parse::<f64>().map_err(|_| tr("ui.cam.invalid_number").to_string())?) };
        Ok(p)
    }
}

/// A hint the form's width wraps instead of cutting off.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Wrap));
}

/// Above the pick rows: name, work offset and how part zero is found.
pub fn head_rows(ui: &mut egui::Ui, form: &mut CamSetupForm) {
    let tr = crate::i18n::tr;
    ui_kit::text_row(ui, tr("ui.cam.setup"), &mut form.name, "");
    let offsets = ["G54", "G55", "G56", "G57", "G58", "G59"];
    ui_kit::choice_row(ui, tr("ui.cam.work_offset"), "cam-setup-offset", &mut form.offset, &offsets);
    let zero = [tr("ui.cam.zero_point"), tr("ui.cam.zero_axes"), tr("ui.cam.zero_origin"), tr("ui.cam.zero_model"), tr("ui.cam.zero_stock")];
    ui_kit::choice_row(ui, tr("ui.cam.zero"), "cam-setup-zero", &mut form.zero, &zero);
    if form.zero == 0 {
        note(ui, tr("ui.cam.zero_hint"));
    }
    if form.zero == 1 {
        note(ui, tr("ui.cam.zero_axes_hint"));
        let mut rests = vec![tr("ui.cam.zero_model"), tr("ui.cam.zero_stock")];
        if form.editing.is_some() {
            rests.push(tr("ui.cam.zero_keep"));
        }
        form.rest = form.rest.min(rests.len() - 1);
        ui_kit::choice_row(ui, tr("ui.cam.zero_rest"), "cam-setup-rest", &mut form.rest, &rests);
    }
    if form.zero >= BOX_MODES || (form.zero == 1 && form.rest < 2) {
        let refs = [tr("ui.cam.box_low"), tr("ui.cam.box_mid"), tr("ui.cam.box_high")];
        for (i, (key, id)) in
            [("ui.cam.box_x", "cam-setup-bx"), ("ui.cam.box_y", "cam-setup-by"), ("ui.cam.box_z", "cam-setup-bz")].into_iter().enumerate()
        {
            if let Some(corner) = form.corner.get_mut(i) {
                ui_kit::choice_row(ui, tr(key), id, corner, &refs);
            }
        }
    }
}

/// Below the pick rows: the axes, the stock, machine, material and highest spindle speed.
pub fn rows(app: &crate::SolveApp, ui: &mut egui::Ui, form: &mut CamSetupForm) {
    let tr = crate::i18n::tr;
    note(ui, tr("ui.cam.axis_hint"));
    if form.editing.is_some() {
        note(ui, tr("ui.cam.kept_axes"));
    }
    ui_kit::checkbox(ui, tr("ui.cam.flip_z"), &mut form.flip_z);
    ui_kit::checkbox(ui, tr("ui.cam.flip_x"), &mut form.flip_x);
    let stock = [tr("ui.cam.stock_around"), tr("ui.cam.stock_box"), tr("ui.cam.stock_round")];
    ui_kit::choice_row(ui, tr("ui.cam.stock"), "cam-setup-stock", &mut form.stock, &stock);
    let labels: &[&str] = match form.stock {
        1 => &["ui.cam.stock_x", "ui.cam.stock_y", "ui.cam.stock_h"],
        2 => &["ui.cam.stock_dia", "ui.cam.stock_h"],
        _ => &["ui.cam.stock_side", "ui.cam.stock_top", "ui.cam.stock_bottom"],
    };
    for (label, value) in labels.iter().zip(form.values.iter_mut()) {
        ui_kit::text_row(ui, tr(label), value, "");
    }
    machine_row(app, ui, form);
    let names: Vec<String> =
        std::iter::once(tr("ui.cam.mat_none").to_string()).chain(MATERIALS.iter().map(|m| crate::cam_panel::material_label(m))).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let current = form.material.as_ref().and_then(|m| MATERIALS.iter().position(|x| x == m)).map_or(0, |i| i + 1);
    let mut chosen = current;
    ui_kit::choice_row(ui, tr("ui.cam.material"), "cam-setup-material", &mut chosen, &refs);
    if chosen != current {
        form.material = chosen.checked_sub(1).and_then(|i| MATERIALS.get(i)).map(|m| (*m).to_string());
    }
    ui_kit::text_row(ui, tr("ui.cam.max_rpm"), &mut form.max_rpm, tr("ui.cam.max_rpm_hint"));
}

/// The machine: none, one of the personal list, or one only the design has.
fn machine_row(app: &crate::SolveApp, ui: &mut egui::Ui, form: &mut CamSetupForm) {
    let tr = crate::i18n::tr;
    let list = &app.preferences.cam_machines.0;
    let mut machines: Vec<&Machine> = list.iter().collect();
    machines.extend(app.session.doc.cam.machines.iter().filter(|m| !list.iter().any(|l| l.name == m.name)));
    if machines.is_empty() && form.machine.is_none() {
        note(ui, tr("ui.cam.no_machines"));
        return;
    }
    let names: Vec<&str> = std::iter::once(tr("ui.cam.machine_none")).chain(machines.iter().map(|m| m.name.as_str())).collect();
    let current = form.machine.as_ref().and_then(|f| machines.iter().position(|m| m.name == f.name)).map_or(0, |i| i + 1);
    let mut chosen = current;
    ui_kit::choice_row(ui, tr("ui.cam.machine"), "cam-setup-machine", &mut chosen, &names);
    if chosen != current {
        form.machine = chosen.checked_sub(1).and_then(|i| machines.get(i)).map(|m| (*m).clone());
    }
    if let Some(machine) = &form.machine {
        note(ui, &crate::cam_panel::machine_label(machine));
    }
    form.machine_post = form.machine.as_ref().and_then(|m| crate::cam_panel::machine_post(&app.preferences.cam_posts, m)).cloned();
}

/// Part zero with its axes and the stock's edges for setup `name`, as the design holds it.
pub fn draw_setup(session: &solidcraft_engine::Session, painter: &egui::Painter, proj: &Proj, name: &str) {
    let Some(outline) = session.cam_setup_outline(name) else { return };
    let t = Tokens::get();
    if let Some(corners) = outline.stock {
        let stroke = Stroke::new(1.0, t.text_dim);
        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            if let (Some(a), Some(b)) = (corners.get(a).and_then(|p| proj.to_screen(*p)), corners.get(b).and_then(|p| proj.to_screen(*p))) {
                painter.line_segment([a, b], stroke);
            }
        }
    }
    let Some(o) = proj.to_screen(outline.origin) else { return };
    // The axes a fixed size on screen, so part zero reads at any zoom.
    let length = 40.0;
    for (axis, colour) in outline.axes.iter().zip([t.axis_x, t.axis_y, t.axis_z]) {
        let Some(tip) = proj.to_screen(outline.origin + *axis) else { continue };
        let along = tip - o;
        if along.length() < 1e-3 {
            continue;
        }
        painter.arrow(o, along * (length / along.length()), Stroke::new(2.0, colour));
    }
    painter.circle_filled(o, 4.0, t.text);
}

pub fn overlay(session: &solidcraft_engine::Session, painter: &egui::Painter, proj: &Proj, form: &CamSetupForm) {
    draw_setup(session, painter, proj, form.name());
}
