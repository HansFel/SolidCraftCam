//! Dense ports of the common feature forms. Commands and parameter construction stay in dialogs.
use super::*;
use crate::ui_kit;
use crate::ui_kit::{choice_row, expr_row, hint_row};
use solidcraft_engine::kernel::Continuity;

pub(super) fn command(kind: &Kind) -> Option<&'static str> {
    match kind {
        Kind::Milling(_) => Some("cam.job.create"),
        Kind::CamSetup(_) => Some("cam.setup"),
        Kind::Material { .. } => Some("material.assign"),
        Kind::Appearance(_) => Some("appearance.assign"),
        Kind::Motion(k) => Some(match k {
            crate::dialogs_motion::Mo::ContactSet { .. } => "contact.create",
            crate::dialogs_motion::Mo::MotionStudy { .. } => "motion.study",
            crate::dialogs_motion::Mo::Explode { .. } => "explode.create",
            crate::dialogs_motion::Mo::Configurations { .. } => "config.table",
        }),
        Kind::Part(_) => Some("parts.insert"),
        Kind::PartSize(_) => Some("parts.resize"),
        Kind::SaveMesh(k) => Some(crate::dialogs_save_mesh::command_id(k)),
        Kind::ViewDxf { .. } => Some("file.export_view_dxf"),
        Kind::ViewNavigator(_) => Some("view.navigator"),
        Kind::InsertDrawing(p) => Some(if p.command.ends_with("dxf") { "sketch.insert_dxf" } else { "sketch.insert_svg" }),
        Kind::Sketch => Some("sketch.create"),
        Kind::SketchIntersect { .. } => Some("sketch.intersect"),
        Kind::SketchProject { .. } => Some("sketch.project"),
        Kind::SketchModify(k) => Some(crate::sketch_modify_ui::command(k)),
        Kind::Text(_) => Some("sketch.edit_text"),
        Kind::TextMove(_) => Some("sketch.move"),
        Kind::EditParam { .. } => Some("parameters.change"),
        Kind::Measure { .. } => Some("inspect.measure"),
        Kind::Section { .. } => Some("inspect.section"),
        Kind::Rename { .. } => Some("timeline.rename"),
        Kind::ConfirmDelete { .. } => Some("timeline.delete"),
        Kind::Extrude { .. } => Some("solid.extrude"),
        Kind::Fillet { chamfer, .. } => Some(if *chamfer { "solid.chamfer" } else { "solid.fillet" }),
        Kind::Hole { .. } => Some("solid.hole"),
        Kind::PatternRect { .. } => Some("solid.pattern.rectangular"),
        Kind::PatternCirc { .. } => Some("solid.pattern.circular"),
        Kind::Revolve { .. } => Some("solid.revolve"),
        Kind::Mirror => Some("solid.mirror"),
        Kind::Loft { .. } => Some("solid.loft"),
        Kind::Sweep { .. } => Some("solid.sweep"),
        Kind::Midplane => Some("construct.plane.midplane"),
        Kind::MeshConvert { .. } => Some("mesh.convert"),
        Kind::Construct { command, .. } => Some(command),
        Kind::PathPlane { .. } => Some("construct.plane.along_path"),
        Kind::Split => Some("solid.split_body"),
        Kind::DeleteFaces => Some("face.delete"),
        Kind::Move { .. } => Some("solid.move"),
        Kind::Thread { .. } => Some("solid.thread"),
        Kind::ReplaceFace => Some("solid.replace_face"),
        Kind::BoundaryFill { .. } => Some("solid.boundary_fill"),
        Kind::Align { .. } => Some("solid.align"),
        Kind::Remove => Some("solid.remove"),
        Kind::PathPattern { .. } => Some("solid.pattern.path"),
        Kind::Combine { .. } => Some("solid.combine"),
        Kind::Rib { web, .. } => Some(if *web { "solid.web" } else { "solid.rib" }),
        Kind::Primitive { cmd, .. } => Some(cmd),
        Kind::Surface(k) => Some(match k {
            crate::dialogs_surface::Surf::Extrude { .. } => "surface.extrude",
            crate::dialogs_surface::Surf::Revolve { .. } => "surface.revolve",
            crate::dialogs_surface::Surf::Sweep(_) => "surface.sweep",
            crate::dialogs_surface::Surf::Loft { .. } => "surface.loft",
            crate::dialogs_surface::Surf::Blend { .. } => "surface.blend",
            crate::dialogs_surface::Surf::Boundary { .. } => "surface.boundary",
            crate::dialogs_surface::Surf::Patch { .. } => "surface.patch",
            crate::dialogs_surface::Surf::Unstitch => "surface.unstitch",
            crate::dialogs_surface::Surf::ReverseNormal => "surface.reverse_normal",
            crate::dialogs_surface::Surf::Ruled { .. } => "surface.ruled",
            crate::dialogs_surface::Surf::Trim { .. } => "surface.trim",
            crate::dialogs_surface::Surf::Untrim { .. } => "surface.untrim",
            crate::dialogs_surface::Surf::SplitFace => "solid.split_face",
            _ => k.simple_metadata().map_or("", |(command, _)| command),
        }),
        Kind::Sheet(k) => match k {
            crate::dialogs_sheet::Sm::Flange { .. } => Some("sheet.flange"),
            crate::dialogs_sheet::Sm::Unfold { refold } => Some(if *refold { "sheet.refold" } else { "sheet.unfold" }),
            crate::dialogs_sheet::Sm::Convert { .. } => Some("sheet.convert"),
            crate::dialogs_sheet::Sm::Rules { .. } => Some("sheet.manage_rules"),
            crate::dialogs_sheet::Sm::FlatPattern { .. } => Some("sheet.flat_pattern"),
            _ => k.simple_metadata().map(|(command, _)| command),
        },
        Kind::Frame(_) => Some("frame.member"),
        Kind::Plastic(k) => match k {
            crate::dialogs_plastic::Pl::SnapFit { .. } => Some("plastic.snap_fit"),
            crate::dialogs_plastic::Pl::Assign { .. } => Some("plastic.assign_rule"),
            crate::dialogs_plastic::Pl::Rules { .. } => Some("plastic.manage_rules"),
            _ => k.simple_metadata().map(|(command, _)| command),
        },
        Kind::Assembly(k) => match k {
            crate::dialogs_assembly::Asm::FromBodies => Some("component.from_bodies"),
            crate::dialogs_assembly::Asm::NewComponent { .. } => Some("component.create"),
            crate::dialogs_assembly::Asm::Joint(f) => Some(if f.as_built { "joint.as_built" } else { "joint.create" }),
            crate::dialogs_assembly::Asm::JointOrigin { .. } => Some("joint.origin"),
            crate::dialogs_assembly::Asm::RigidGroup => Some("joint.rigid_group"),
            crate::dialogs_assembly::Asm::MotionLink { .. } => Some("joint.motion_link"),
            crate::dialogs_assembly::Asm::Coupling(f) => Some(f.command),
            crate::dialogs_assembly::Asm::Cam(_) => Some("joint.cam"),
            crate::dialogs_assembly::Asm::Trace(_) => Some("motion.trace"),
            crate::dialogs_assembly::Asm::Drive { .. } => Some("joint.drive"),
            crate::dialogs_assembly::Asm::Interference { .. } => Some("inspect.interference"),
        },
        _ => kind.simple_metadata().map(|(command, _)| command),
    }
}

const FILLET_TYPES: [(&str, &str); 5] = [
    ("Constant radius", "constant"),
    ("Chord length", "chord"),
    ("Variable radius", "variable"),
    ("ui.fillet_type_curvature", "curvature"),
    ("Spline", "spline"),
];

/// How a fillet's corners where three or more rounded edges meet continue the rounds, with the
/// hover line that says what each builds: tangent (a rolling ball, the `corner` default) or
/// curvature continuous (`corner: g2`, a smooth patch set back along each edge).
const FILLET_CORNERS: [(Continuity, &str); 2] =
    [(Continuity::G1, "ui.fillet_corner_tangent_tip"), (Continuity::G2, "ui.fillet_corner_curvature_tip")];

/// The default setback of a smooth-patch corner, as a multiple of the radius (the kernel's), as
/// the messages show it.
fn setback_factor() -> String {
    solidcraft_engine::doc::kernel::DEFAULT_SETBACK_FACTOR.to_string()
}

/// The Corners row's hover text: a line for each choice.
fn fillet_corner_tip() -> String {
    let factor = setback_factor();
    FILLET_CORNERS.map(|(_, tip)| crate::i18n::format(tip, &[("factor", &factor)])).join("\n")
}

/// What `solid.fillet` does with a radius too large for the faces beside an edge (`fit`): stop
/// and say so (the default), round every edge with the largest radius that fits, or keep the
/// radius and narrow it only where the faces run out.
const FILLET_FITS: [(&str, &str); 3] = [("ui.fillet_fit_exact", "exact"), ("ui.fillet_fit_reduce", "reduce"), ("ui.fillet_fit_taper", "taper")];

/// How two rounds turn where they meet at a sharp corner edge (`turn`): in a mitre (the
/// default) or swept round the edge.
const FILLET_TURNS: [(&str, &str); 2] = [("ui.fillet_turn_mitre", "mitre"), ("ui.fillet_turn_swept", "swept")];

/// Which fillet options apply together, exactly as `solid.fillet` takes them (`fillet` in
/// crates/engine/src/cmd/features.rs):
///
/// | Type          | Face limits (`fit`)     | Corners (`corner`)                  | Setback (`setbacks`) | Turns (`turn`)                  |
/// |---------------|-------------------------|-------------------------------------|----------------------|---------------------------------|
/// | Constant      | exact, reduce or taper  | tangent; curvature with exact only  | with curvature only  | mitre; swept without taper, g2  |
/// | any other     | exact (not sent)        | tangent (not sent)                  | none                 | mitre (not sent)                |
///
/// The dialog shows an option's row only where it applies and sends the option only then, and a
/// choice that rules another out puts that one back to its default ([`settle_fillet_options`]).
pub(super) fn fillet_option_applies(extra: &serde_json::Map<String, Value>, key: &str) -> bool {
    let constant = extra.get("type").and_then(Value::as_str).is_none_or(|t| t == "constant");
    let exact = extra.get("fit").and_then(Value::as_str).is_none_or(|fit| fit == "exact");
    match key {
        "fit" => constant,
        "corner" | "setbacks" => constant && exact,
        // A swept turn takes an exact or reduced radius and rolling-ball corners.
        "turn" => constant && extra.get("fit").and_then(Value::as_str) != Some("taper") && extra.get("corner").and_then(Value::as_str) != Some("g2"),
        _ => true,
    }
}

/// After `changed` (`fit` or `corner`) took a new value, the options it rules out back to their
/// defaults: a fitted radius (`fit` reduce or taper) takes tangent corners, a curvature corner
/// takes the exact radius. A type other than Constant clears both ([`choose_fillet_type`]).
fn settle_fillet_options(extra: &mut serde_json::Map<String, Value>, changed: &str, setback: &mut Option<String>) {
    let fitted = extra.get("fit").and_then(Value::as_str).is_some_and(|fit| fit != "exact");
    let tapered = extra.get("fit").and_then(Value::as_str) == Some("taper");
    let patch = extra.get("corner").and_then(Value::as_str) == Some("g2");
    let swept = extra.get("turn").and_then(Value::as_str) == Some("swept");
    match changed {
        "fit" if fitted => {
            extra.remove("corner");
            extra.remove("setbacks");
            *setback = None;
            if tapered {
                extra.remove("turn");
            }
        }
        "corner" if patch => {
            extra.remove("fit");
            extra.remove("turn");
        }
        "turn" if swept => {
            extra.remove("corner");
            extra.remove("setbacks");
            *setback = None;
            if tapered {
                extra.remove("fit");
            }
        }
        _ => {}
    }
}

/// A fillet's Corners row (`corner`) and, with a smooth patch, its Setback (`setbacks`, one
/// distance for every corner), for a Constant radius fillet. Shown only while the edges have a
/// corner where three or more rounded edges meet, or the fillet already has a smooth patch.
fn fillet_corner_rows(
    ui: &mut egui::Ui,
    s: &solidcraft_engine::Session,
    extra: &mut serde_json::Map<String, Value>,
    inputs: &[selection::SelInput],
    radius: &str,
    setback: &mut Option<String>,
) -> bool {
    let items = selection::items(inputs, 0);
    let corners = fillet_corners(s, items);
    let patch = extra.get("corner").and_then(Value::as_str) == Some("g2");
    if !patch && corners.is_empty() {
        return false;
    }
    let mut enter = false;
    let mut corner = if patch { Continuity::G2 } else { Continuity::G1 };
    ui_kit::continuity_row_explained(ui, "ui.corners", "fi_corner", &mut corner, &FILLET_CORNERS.map(|(g, _)| g), &fillet_corner_tip());
    if (corner == Continuity::G2) != patch {
        if corner == Continuity::G2 {
            extra.insert("corner".into(), json!(Continuity::G2.as_str()));
        } else {
            extra.remove("corner");
        }
        settle_fillet_options(extra, "corner", setback);
    }
    if extra.get("corner").and_then(Value::as_str) == Some("g2") {
        let factor = setback_factor();
        let length = |text: &str| crate::length_input::eval(&s.doc, text, solidcraft_engine::doc::expr::Kind::Length).ok().filter(|v| v.is_finite());
        let r = length(radius).filter(|r| *r > 0.0);
        // Not typed, the field shows the command's default as a length, following the radius.
        let default = r.map(|r| crate::length_input::shown(&s.doc, solidcraft_engine::doc::kernel::DEFAULT_SETBACK_FACTOR * r));
        let shown = setback.clone().or(default).unwrap_or_default();
        // A setback at or below the least the kernel takes there is flagged before OK.
        let least = r.and_then(|r| crate::dialogs::fillet_least_setback(ui.ctx(), s, items, &corners, r));
        let warning = least
            .filter(|least| length(&shown).is_some_and(|v| v <= least * (1.0 + 1e-9)))
            .map(|least| crate::i18n::format("ui.fillet_setback_least", &[("least", &crate::length_input::shown(&s.doc, least))]));
        let mut text = shown.clone();
        let row = ui.scope(|ui| {
            let hint = crate::i18n::format("ui.fillet_setback_hint", &[("factor", &factor)]);
            ui_kit::checked_row(ui, "ui.fillet_setback", &mut text, &hint, warning.as_deref())
        });
        row.response.backing_on_hover_text(crate::i18n::format("ui.fillet_setback_tip", &[("factor", &factor)]));
        let (accepted, field) = row.inner;
        enter |= accepted;
        if text != shown {
            *setback = Some(text);
        }
        // Emptied and left, the field goes back to the default.
        if setback.as_deref().is_some_and(|t| t.trim().is_empty()) && !field.has_focus() {
            *setback = None;
        }
    }
    enter
}

/// A Constant radius fillet's "Face limits" row (`fit`), with the radius a reduced fillet used.
fn fillet_fit_rows(app: &SolveApp, ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, setback: &mut Option<String>) {
    let t = Tokens::get();
    let before = extra.get("fit").cloned();
    let row = ui.scope(|ui| form_row(ui, "ui.fillet_fit", |ui| extra_choice(ui, extra, "fi_fit", "fit", &FILLET_FITS)));
    row.response.backing_on_hover_text(tr("ui.fillet_fit_tip"));
    let fit = row.inner;
    if extra.get("fit") != before.as_ref() {
        settle_fillet_options(extra, "fit", setback);
    }
    if fit == 1
        && let Some((lo, hi)) = app.preview.applied
    {
        form_row(ui, "Applied Radius", |ui| {
            let shown = if hi > lo {
                format!("{} – {}", crate::prefs::show_mm_bare(app, lo), crate::prefs::show_mm(app, hi, 1))
            } else {
                crate::prefs::show_mm(app, lo, 1)
            };
            ui.label(RichText::new(shown).color(t.warning));
        });
    }
}

/// A Constant radius fillet's Turns row (`turn`): how two rounds turn where they meet at a sharp
/// corner edge.
fn fillet_turn_row(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, setback: &mut Option<String>) {
    let before = extra.get("turn").cloned();
    let row = ui.scope(|ui| form_row(ui, "ui.fillet_turn", |ui| extra_choice(ui, extra, "fi_turn", "turn", &FILLET_TURNS)));
    row.response.on_hover_text(tr("ui.fillet_turn_tip"));
    if extra.get("turn") != before.as_ref() {
        settle_fillet_options(extra, "turn", setback);
    }
}

/// The most instances a solid pattern makes in one direction (the document's own limit).
const MAX_PATTERN_INSTANCES: i64 = 1000;

/// A solid pattern's quantity row, with step arrows from 1 to `MAX_PATTERN_INSTANCES`. The
/// count goes to the command as typed, so it is evaluated as the document evaluates it.
fn quantity_row(ui: &mut egui::Ui, doc: &solidcraft_engine::doc::Document, label: &str, value: &mut String) -> bool {
    count_row_to(ui, doc, label, value, MAX_PATTERN_INSTANCES)
}

/// The most revolutions a coil makes (the helix's own limit).
const MAX_COIL_REVOLUTIONS: i64 = 200;

/// A count row with step arrows by whole numbers from 1 to `max`, evaluated as the document
/// evaluates it. A fraction still types in (a coil's revolutions); an arrow steps it to the next
/// whole number.
fn count_row_to(ui: &mut egui::Ui, doc: &solidcraft_engine::doc::Document, label: &str, value: &mut String, max: i64) -> bool {
    let eval = |text: &str| doc.eval(text.trim(), solidcraft_engine::doc::expr::Kind::Unitless).ok();
    ui_kit::count_row(ui, label, value, &ui_kit::Count { min: 1, max, eval: &eval })
}

fn fillet_type(extra: &serde_json::Map<String, Value>) -> usize {
    let value = extra.get("type").and_then(Value::as_str).unwrap_or("constant");
    if value == "g2" { 3 } else { FILLET_TYPES.iter().position(|(_, key)| *key == value).unwrap_or(0) }
}

/// Defaults are introduced only for an explicit new type choice, never for a no-op edit.
fn choose_fillet_type(extra: &mut serde_json::Map<String, Value>, index: usize, radius: &str) {
    let Some((_, key)) = FILLET_TYPES.get(index) else { return };
    if index == 0 {
        extra.remove("type");
    } else {
        extra.insert("type".into(), json!(key));
    }
    if index != 0 {
        extra.remove("fit");
        extra.remove("corner");
        extra.remove("setbacks");
        extra.remove("turn");
    }
    if index == 2 {
        extra.entry("radius2").or_insert_with(|| json!("1 mm"));
        extra.entry("mid").or_insert_with(|| json!([]));
    }
    if index == 4 {
        let radii = spline_radii(extra, radius);
        extra.insert("radii".into(), json!(radii));
    }
}

pub(super) fn rows(app: &mut SolveApp, ctx: &egui::Context, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    if matches!(d.kind, Kind::Appearance(_)) {
        ui.spacing_mut().item_spacing.y = t.space_10;
    }
    if matches!(d.kind, Kind::Sketch | Kind::SketchModify(_) | Kind::Text(_) | Kind::TextMove(_) | Kind::EditParam { .. }) {
        let prompt = if let Some(input) = d.active_input() {
            tr(hint(input)).to_owned()
        } else {
            let key = match d.kind {
                Kind::Text(_) => "sketch_panel.text_hint",
                Kind::TextMove(_) => "sketch_panel.move_hint",
                Kind::EditParam { .. } => "sketch_panel.dimension_hint",
                _ => "dialog.selection_guidance",
            };
            tr(key).to_owned()
        };
        ui_kit::text(ui, &prompt, ui_kit::Text::Hint("tool")).backing_on_hover_text(&prompt);
        if matches!(&d.kind, Kind::SketchModify(k) if k.previews()) {
            let label = d.active_input().map(|input| tr(input.label)).unwrap_or_else(|| tr("Objects"));
            let step = crate::i18n::format(
                "dialog.selection_step",
                &[("step", &(d.active + 1).to_string()), ("total", &d.inputs.len().to_string()), ("input", label)],
            );
            ui_kit::text(ui, &step, ui_kit::Text::Hint("step")).backing_on_hover_text(tr("dialog.selection_guidance"));
        }
    }
    let manage_hint = match &d.kind {
        Kind::Material { .. } => Some("manage.material_hint"),
        Kind::Appearance(_) => Some("manage.appearance_hint"),
        Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }) => Some("manage.config_hint"),
        Kind::Sheet(crate::dialogs_sheet::Sm::Rules { .. }) | Kind::Plastic(crate::dialogs_plastic::Pl::Rules { .. }) => Some("manage.rules_hint"),
        Kind::Plastic(crate::dialogs_plastic::Pl::Assign { .. }) => Some("manage.assign_hint"),
        _ => None,
    };
    if let Some(key) = manage_hint {
        ui_kit::text(ui, tr(key), ui_kit::Text::Hint("manage"));
    }
    placement_row(d, ui, app);
    if crate::hole_refs::takes(d) {
        // Each reference edge with its distance (#37, #6380).
        enter |= crate::hole_refs::input_rows(d, ui, &app.session, app.preferences.origin_plane_names, app.preferences.length_decimals);
    } else if let Kind::CamSetup(form) = &mut d.kind {
        // The part-zero mode chooses which pick rows show; a hidden row takes no picks.
        crate::dialogs_cam_setup::head_rows(app, ui, form);
        let shown = crate::dialogs_cam_setup::shown_inputs(form);
        if !shown.iter().any(|r| r.contains(&d.active))
            && let Some(first) = shown.first()
        {
            d.active = first.start;
        }
        for range in shown {
            input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, range);
        }
    } else {
        input_rows(d, ui, &app.session, app.preferences.origin_plane_names);
    }
    d.sync_material(&app.session);
    // The Hole dialog's dropdowns preview each choice on a copy of the form as it stands.
    let hole_form = matches!(d.kind, Kind::Hole { .. }).then(|| (d.kind.clone(), to_object_on(&d.inputs)));
    match &mut d.kind {
        Kind::Milling(form) => crate::cam_manual::with_tips(ui, |ui| crate::dialogs_milling::rows(app, ui, form, &mut d.inputs)),
        Kind::CamSetup(form) => crate::cam_manual::with_tips(ui, |ui| crate::dialogs_cam_setup::rows(app, ui, form)),
        Kind::Material { index, editor } => crate::material_editor::rows(app, ui, index, editor),
        Kind::Appearance(k) => enter |= crate::dialogs_appearance::rows(app, ui, k, &mut d.inputs),
        Kind::Motion(k) => enter |= crate::dialogs_motion::rows(app, ui, k, &mut d.inputs),
        Kind::Part(k) => enter |= crate::dialogs_parts::rows(app, ui, k, &d.inputs),
        Kind::PartSize(k) => enter |= crate::dialogs_part_size::rows(app, ui, k),
        Kind::SaveMesh(k) => enter |= crate::dialogs_save_mesh::rows(app, ui, k, &mut d.inputs),
        Kind::ViewDxf { dashed, tolerance } => crate::dialogs_view_dxf::rows(app, ui, dashed, tolerance),
        Kind::ViewNavigator(form) => crate::view_gizmo::rows(app, ui, form, &d.inputs),
        Kind::InsertDrawing(p) => crate::dialogs_insert_drawing::rows(ui, p),
        Kind::Sketch => {
            if d.error.is_some() && matches!(d.inputs.first().and_then(|i| i.items.first()), Some(Sel::Face { .. })) {
                ui_kit::text(
                    ui,
                    tr("This face cannot provide a durable attachment. Choose another face or explicitly use a fixed plane."),
                    ui_kit::Text::Hint("attachment"),
                );
                if ui_kit::button(ui, tr("Use fixed plane"), ui_kit::Button::Normal).clicked() {
                    d.extra.insert("fixed_plane".into(), json!(true));
                }
            }
        }
        Kind::SketchIntersect { bodies, projection_link } => {
            let mut selected = if *bodies { 1 } else { 0 };
            let changed = ui_kit::row(ui, tr("ui.sketch_intersect_selection_filter"), |ui| {
                ui_kit::segmented(ui, &mut selected, &[tr("Geometry"), tr("Bodies")], Default::default())
            });
            if changed && let Some(input) = d.inputs.first_mut() {
                *bodies = selected == 1;
                input.accept = if *bodies { BODIES } else { super::INTERSECT_GEOMETRY };
                input.items.clear();
            }
            ui_kit::row(ui, tr("ui.sketch_intersect_projection_link"), |ui| ui_kit::checkbox(ui, "", projection_link));
        }
        Kind::SketchProject { bodies, projection_link } => {
            let mut selected = if *bodies { 1 } else { 0 };
            let changed = ui_kit::row(ui, tr("ui.sketch_intersect_selection_filter"), |ui| {
                ui_kit::segmented(ui, &mut selected, &[tr("Geometry"), tr("Bodies")], Default::default())
            });
            if changed && let Some(input) = d.inputs.first_mut() {
                *bodies = selected == 1;
                input.accept = if *bodies { BODIES } else { super::PROJECT_GEOMETRY };
                input.items.clear();
            }
            ui_kit::row(ui, tr("ui.sketch_intersect_projection_link"), |ui| ui_kit::checkbox(ui, "", projection_link));
        }
        Kind::SketchModify(k) => enter |= crate::sketch_modify_ui::rows(ui, k, &app.session.doc),
        Kind::Text(k) => enter |= crate::sketch_text::rows(ui, k, &mut d.inputs, &mut d.active),
        Kind::TextMove(k) => enter |= crate::sketch_text_move::rows(ui, k),
        Kind::EditParam { name, expr } => {
            enter |= ui_kit::row(ui, name, |ui| ui_kit::focused_field(ui, expr));
        }
        Kind::Extrude {
            distance,
            direction,
            operation,
            auto_op,
            smart,
            distance2,
            taper,
            taper_present,
            to_offset,
            start,
            all,
            thin,
            wall,
            wall_side,
            wall2,
        } => {
            form_row(ui, "Direction", |ui| {
                // One Side, Two Sides, Symmetric (a flip is a negative distance).
                let order = [0usize, TWO_SIDES, 2];
                let reversed = *direction == 1 || *direction == TWO_SIDES_REVERSED;
                let shown = match *direction {
                    1 => 0,
                    TWO_SIDES_REVERSED => TWO_SIDES,
                    d => d,
                };
                let mut pos = order.iter().position(|o| *o == shown).unwrap_or(0);
                combo(ui, "ex_dir", &["One Side", "Two Sides", "Symmetric"], &mut pos);
                if pos != order.iter().position(|o| *o == shown).unwrap_or(0) {
                    *direction = match (order.get(pos).copied().unwrap_or(0), reversed) {
                        (0, true) => 1,
                        (TWO_SIDES, true) => TWO_SIDES_REVERSED,
                        (d, _) => d,
                    };
                    // Selecting Symmetric applies the current total-distance semantics.
                    d.extra.insert("convention".into(), json!(solidcraft_engine::doc::ExtrudeConvention::TotalDistance));
                    // An explicit direction ends the automatic choices (#1257).
                    smart.manual_dir = true;
                    *auto_op = false;
                }
            });
            let cur = if to_object_on(&d.inputs) { 1 } else { usize::from(*all) * 2 };
            let mut ext = cur;
            form_row(ui, "Extent Type", |ui| {
                combo(ui, "ex_ext", &["Distance", "To Object", "All"], &mut ext);
                *all = ext == 2;
                set_to_object(&mut d.inputs, &mut d.active, ext == 1);
            });
            if ext == 0 {
                enter |= expr_row(ui, "Distance", distance);
            }
            if ext == 1 {
                enter |= expr_row(ui, "To Offset", to_offset);
            }
            if two_sided(*direction) {
                enter |= expr_row(ui, "Distance 2", distance2);
            }
            // A choice made by hand sticks, the shown one included (#6901): Extrude's own choice
            // stops before the next frame makes it.
            if crate::op_auto::op_row(ui, "ex_op", operation) {
                *auto_op = false;
            }
            ui_kit::section(ui, "extrude_options", "Options", true, |ui| {
                let was_thin = *thin;
                let was_direction = *direction;
                let mut reverse = *direction == 1 || *direction == TWO_SIDES_REVERSED;
                if *direction < 2 || two_sided(*direction) {
                    ui_kit::checkbox_grid(ui, &mut [("Thin Extrude", thin), ("Reverse Direction", &mut reverse)], 2);
                    *direction = match (two_sided(*direction), reverse) {
                        (true, true) => TWO_SIDES_REVERSED,
                        (true, false) => TWO_SIDES,
                        (false, r) => usize::from(r),
                    };
                    if *direction != was_direction {
                        smart.manual_dir = true;
                        *auto_op = false;
                    }
                } else {
                    ui_kit::checkbox_grid(ui, &mut [("Thin Extrude", thin)], 2);
                }
                if *thin != was_thin {
                    set_thin(&mut d.inputs, *thin);
                }
                if *thin {
                    choice_row(ui, "Wall Location", "ex_wall", wall_side, &WALL_LABELS);
                    enter |= expr_row(ui, "Wall Thickness", wall);
                    if *wall_side == 3 {
                        enter |= expr_row(ui, "Thickness Outside", wall2);
                    }
                }
            });
            let advanced = !start.is_empty() || object_on(&d.inputs, FROM_OBJECT) || *taper_present;
            ui_kit::section(ui, "extrude_advanced", "Advanced", advanced, |ui| {
                let old = if object_on(&d.inputs, FROM_OBJECT) { 2 } else { usize::from(!start.is_empty()) };
                let mut st = old;
                form_row(ui, "Start", |ui| {
                    combo(ui, "ex_start", &["Profile Plane", "Offset", "From Object"], &mut st);
                    if st != old {
                        set_object(&mut d.inputs, &mut d.active, FROM_OBJECT, st == 2);
                        if st == 1 && start.is_empty() {
                            *start = length_default(&app.session, "5");
                        }
                        if st == 0 {
                            start.clear();
                        }
                    }
                });
                if st != 0 {
                    enter |= expr_row(ui, "Offset", start);
                }
                if *direction < 2 {
                    form_row(ui, "Taper Angle", |ui| {
                        let before = taper.clone();
                        enter |= field(ui, taper);
                        if *taper != before {
                            *taper_present = !taper.trim().is_empty();
                        }
                    });
                }
            });
        }
        Kind::Fillet { radius, chamfer, chain, ctype, distance2, angle, flip, setback } => {
            if *chamfer {
                choice_row(ui, "Chamfer Type", "ch_type", ctype, &CHAMFER_TYPES);
            }
            enter |= expr_row(ui, if *chamfer { if *ctype == 0 { "Distance" } else { "Distance 1" } } else { "Radius" }, radius);
            let mut rtype = fillet_type(&d.extra);
            if !*chamfer {
                form_row(ui, "Type", |ui| {
                    let previous = rtype;
                    let response = ui_kit::dropdown(ui, "fi_type", &mut rtype, &FILLET_TYPES.map(|(label, _)| label));
                    if previous != rtype {
                        choose_fillet_type(&mut d.extra, rtype, radius);
                    }
                    response
                })
                .backing_on_hover_text(tr("ui.fillet_type_choices_tip"));
                if rtype == 0 {
                    enter |= fillet_corner_rows(ui, &app.session, &mut d.extra, &d.inputs, radius, setback);
                    fillet_fit_rows(app, ui, &mut d.extra, setback);
                    fillet_turn_row(ui, &mut d.extra, setback);
                }
            }
            if *chamfer && *ctype == 1 {
                enter |= expr_row(ui, "Distance 2", distance2);
            }
            if *chamfer && *ctype == 2 {
                enter |= expr_row(ui, "Angle", angle);
            }
            if *chamfer && *ctype > 0 {
                form_row(ui, "Flip", |ui| check(ui, flip));
            }
            ui_kit::section(ui, "fillet_options", "Options", true, |ui| {
                ui.scope(|ui| ui_kit::checkbox_grid(ui, &mut [("Tangent Chain", chain), ("Preview", &mut d.preview_enabled)], 2))
                    .response
                    .backing_on_hover_text(tr("ui.fillet_options_tip"));
            });
            // The radii along the edge, for the two types that take them.
            if !*chamfer && matches!(rtype, 2 | 4) {
                ui_kit::section(ui, "variable_radius", "Variable Radius", true, |ui| {
                    if rtype == 2 {
                        form_row(ui, "End Radius", |ui| {
                            enter |= extra_field(ui, &mut d.extra, "radius2", &length_default(&app.session, "1"));
                        });
                        // Which end of the first edge takes the start radius.
                        form_row(ui, "Swap Ends", |ui| {
                            extra_check(ui, &mut d.extra, "swap_ends");
                        });
                        // Radii between the ends: position along the edge (from the start end)
                        // and radius.
                        let mut mids = mid_points(&d.extra);
                        let mut remove = None;
                        for (k, (position, r)) in mids.iter_mut().enumerate() {
                            form_row(ui, "Mid Radius", |ui| {
                                ui.horizontal(|ui| {
                                    ui_kit::fraction_field(ui, position)
                                        .backing_on_hover_text(tr("Position along the edge, 0 at the start end, 1 at the other"));
                                    let width = (ui.available_width() - t.space_24 - t.space_4).max(0.0);
                                    ui.allocate_ui_with_layout(vec2(width, t.space_24), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        enter |= field(ui, r);
                                    });
                                    if ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("Remove this point")).clicked() {
                                        remove = Some(k);
                                    }
                                });
                            });
                        }
                        if let Some(k) = remove {
                            mids.remove(k);
                        }
                        form_row(ui, "", |ui| {
                            if ui_kit::button(ui, tr("Add Mid Radius"), ui_kit::Button::Normal).clicked() && mids.len() < 20 {
                                let t = if mids.is_empty() { 0.5 } else { (mids.iter().map(|m| m.0).fold(0.0, f64::max) + 1.0) * 0.5 };
                                mids.push((t.min(0.99), radius.clone()));
                            }
                        });
                        d.extra.insert("mid".into(), json!(mids.iter().map(|(t, r)| json!([t, r])).collect::<Vec<_>>()));
                        // How the radius runs between the radii: smoothly, or straight.
                        let was = d.extra.get("smooth").and_then(Value::as_bool).unwrap_or(false);
                        let mut smooth = was;
                        ui_kit::checkbox_grid(ui, &mut [("ui.fillet_smooth_radius", &mut smooth)], 1);
                        if smooth != was {
                            d.extra.insert("smooth".into(), json!(smooth));
                        }
                    }
                    if rtype == 4 {
                        // The radius along the edge as a B-spline: its control radii from the
                        // start end, also dragged by their handles on the edge.
                        form_row(ui, "Swap Ends", |ui| {
                            extra_check(ui, &mut d.extra, "swap_ends");
                        });
                        let mut radii = spline_radii(&mut d.extra, radius);
                        let n = radii.len();
                        for (k, r) in radii.iter_mut().enumerate() {
                            enter |= expr_row(ui, if k + 1 == n { "End Radius" } else { "Control Radius" }, r);
                        }
                        d.extra.insert("radii".into(), json!(radii));
                    }
                });
            }
        }
        Kind::PatternRect { count, spacing, count2, spacing2 } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, "Spacing", spacing);
            enter |= quantity_row(ui, &app.session.doc, "Quantity 2", count2);
            enter |= expr_row(ui, "Spacing 2", spacing2);
            if d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                feature_pattern_alignment_row(ui, &mut d.extra, 0);
                feature_pattern_alignment_row(ui, &mut d.extra, 1);
            }
        }
        Kind::PatternCirc { count, angle } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, "Total angle", angle);
            if d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                feature_pattern_alignment_row(ui, &mut d.extra, 0);
            }
        }
        Kind::Hole { kind, opts, .. } => {
            let doc = &app.session.doc;
            let eval = |e: &str, k| doc.eval(e, k).ok();
            // Each option shows the hole it makes while hovered (dialogs_hole::preview).
            let preview = |ui: &mut egui::Ui, choice| {
                if let Some((form, to_object)) = &hole_form {
                    crate::dialogs_hole::preview(ui, form, *to_object, choice, &eval);
                }
            };
            use crate::dialogs_hole::Choice;
            form_row(ui, "Extents", |ui| {
                let cur = if to_object_on(&d.inputs) { 1 } else { usize::from(opts.all) * 2 };
                let mut ext = cur;
                ui_kit::dropdown_previews(ui, "hole_ext", &mut ext, &["Distance", "To Object", "All"], |ui, i| preview(ui, Choice::Extents(i)));
                opts.all = ext == 2;
                set_to_object(&mut d.inputs, &mut d.active, ext == 1);
            });
            if opts.multiple {
                // Every point of the picked points' sketches (standalone points and
                // circle centres), not just the picked ones.
                form_row(ui, "All Sketch Points", |ui| {
                    extra_check(ui, &mut d.extra, "all_points");
                });
                // The holes go into the part the sketch touches; Flip drills the
                // other way.
                form_row(ui, "Flip", |ui| {
                    extra_check(ui, &mut d.extra, "flip");
                });
            }
            form_row(ui, "Hole Type", |ui| {
                ui_kit::dropdown_previews(ui, "hole_kind", kind, &HOLE_LABELS, |ui, i| preview(ui, Choice::Type(i)));
            });
            form_row(ui, "Hole Tap Type", |ui| {
                let mut tap = usize::from(opts.tapped);
                ui_kit::dropdown_previews(ui, "hole_tap", &mut tap, &["Simple", "Tapped"], |ui, i| preview(ui, Choice::Tapped(i == 1)));
                opts.tapped = tap == 1;
            });
            if opts.tapped {
                enter |= crate::dialogs_thread::hole_rows(ui, &app.session.doc, &mut opts.thread, &mut opts.custom_thread, &mut opts.drill);
                crate::dialogs_thread::hole_hand_row(ui, &mut opts.hand);
                ui_kit::checkbox_grid(ui, &mut [("Full thread depth", &mut opts.thread_full_depth), ("Modeled", &mut opts.modeled)], 1);
                // A modelled thread's inner end, towards the bottom, as the thread dialog's.
                if opts.modeled {
                    enter |= crate::dialogs_thread::end_rows(ui, &mut opts.ease_out);
                }
            }
            // Through all, the tip lies past the part.
            if !opts.all {
                form_row(ui, "Drill Point", |ui| {
                    let mut tip = usize::from(opts.angled);
                    ui_kit::dropdown_previews(ui, "hole_tip", &mut tip, &["Flat", "Angle"], |ui, i| preview(ui, Choice::Angled(i == 1)));
                    opts.angled = tip == 1;
                });
            }
            // Counterbore only: the thickness of the layers a 3D printer bridges across its
            // floor; empty for none.
            if *kind == 1 {
                enter |= hint_row(ui, "Bridge", &mut opts.bridge, tr("None"));
            }
            // The section through the hole, each size beside its dimension (#2171).
            let to_object = to_object_on(&d.inputs);
            enter |= crate::dialogs_hole::rows(ui, &mut d.kind, to_object, &eval);
        }
        _ => enter |= additional_rows(app, d, ui),
    }
    error_rows(app, ctx, d, ui);
    enter
}

fn extra_segments(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, key: &str, choices: &[(&str, &str)]) {
    let current = extra.get(key).and_then(Value::as_str).and_then(|value| choices.iter().position(|(_, key)| *key == value)).unwrap_or(0);
    let mut selected = current;
    let labels: Vec<_> = choices.iter().map(|(label, _)| *label).collect();
    if ui_kit::segmented(ui, &mut selected, &labels, Default::default()) && selected != current {
        if selected == 0 {
            extra.remove(key);
        } else if let Some((_, value)) = choices.get(selected) {
            extra.insert(key.into(), json!(value));
        }
    }
}

/// Only user changes add optional JSON keys, preserving no-op feature edits.
fn extra_options(ui: &mut egui::Ui, id: &str, extra: &mut serde_json::Map<String, Value>, choices: &[(&str, &str)]) {
    let mut values: Vec<bool> = choices.iter().map(|(_, key)| extra.get(*key).and_then(Value::as_bool).unwrap_or(false)).collect();
    let before = values.clone();
    let mut items: Vec<_> = choices.iter().zip(values.iter_mut()).map(|((label, _), value)| (*label, value)).collect();
    ui_kit::options(ui, id, &mut items);
    for (((_, key), value), before) in choices.iter().zip(values).zip(before) {
        if value != before {
            extra.insert((*key).into(), json!(value));
        }
    }
}

/// The message a primitive's size field is labelled with, by its parameter name: "Section size",
/// not `section_size`.
pub(crate) fn primitive_label(key: &str) -> &str {
    match key {
        "length" => "ui.length",
        "width" => "ui.width",
        "height" => "ui.height",
        "diameter" => "ui.diameter",
        "radius" => "ui.radius",
        "top_diameter" => "ui.top_diameter",
        "top_radius" => "ui.top_radius",
        "major" => "ui.major_radius",
        "minor" => "ui.minor_radius",
        "revolutions" => "ui.revolutions",
        "pitch" => "ui.pitch",
        "section_size" => "ui.section_size",
        _ => key,
    }
}

/// The coil's section, where the section sits on the diameter, the way it turns and where it
/// starts (#3862). A default stays out of the parameters, so OK on an unchanged edit keeps the coil.
fn coil_rows(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>) -> bool {
    const POSITIONS: [(&str, &str); 3] = [("Inside", "inside"), ("Center", "center"), ("Outside", "outside")];
    form_row(ui, "Section", |ui| extra_segments(ui, extra, "section", &[("Circular", "circular"), ("Square", "square")]));
    form_row(ui, "Section position", |ui| {
        let current =
            extra.get("section_position").and_then(Value::as_str).and_then(|v| POSITIONS.iter().position(|(_, key)| *key == v)).unwrap_or(1);
        let mut selected = current;
        if ui_kit::segmented(ui, &mut selected, &POSITIONS.map(|(label, _)| label), Default::default()) && selected != current {
            match POSITIONS.get(selected) {
                Some((_, value)) if *value != "center" => {
                    extra.insert("section_position".into(), json!(value));
                }
                _ => {
                    extra.remove("section_position");
                }
            }
        }
    });
    let mut angle = extra.get("start_angle").and_then(Value::as_str).unwrap_or_default().to_string();
    let before = angle.clone();
    let enter = hint_row(ui, "Start angle", &mut angle, "0 deg");
    if angle != before {
        if angle.trim().is_empty() {
            extra.remove("start_angle");
        } else {
            extra.insert("start_angle".into(), json!(angle));
        }
    }
    extra_options(ui, "coil_options", extra, &[("Clockwise", "clockwise")]);
    enter
}

fn additional_rows(app: &mut SolveApp, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let mut enter = false;
    let through_point = offset_through_point(d).is_some();
    match &mut d.kind {
        Kind::Revolve { angle, operation, .. } => {
            let cur = usize::from(to_object_on(&d.inputs));
            let mut ty = cur;
            form_row(ui, "Type", |ui| {
                ui_kit::segmented(ui, &mut ty, &["Angle", "To Object"], Default::default());
                set_to_object(&mut d.inputs, &mut d.active, ty == 1);
            });
            if ty == 0 {
                // One Side, Two Sides (a second angle), Symmetric (the angle split).
                let cur = if d.extra.contains_key("angle2") {
                    1
                } else if d.extra.get("direction").is_some() {
                    2
                } else {
                    0
                };
                let mut dir = cur;
                form_row(ui, "Direction", |ui| {
                    ui_kit::segmented(ui, &mut dir, &["One Side", "Two Sides", "Symmetric"], Default::default());
                    if dir != cur {
                        d.extra.remove("angle2");
                        d.extra.remove("direction");
                        match dir {
                            1 => {
                                d.extra.insert("angle2".into(), json!("90 deg"));
                            }
                            2 => {
                                d.extra.insert("direction".into(), json!("symmetric"));
                            }
                            _ => {}
                        }
                    }
                });
                enter |= expr_row(ui, "Angle", angle);
                if dir == 1 {
                    form_row(ui, "Angle 2", |ui| {
                        enter |= extra_field(ui, &mut d.extra, "angle2", "90 deg");
                    });
                }
            }
            crate::op_auto::op_row(ui, "rv_op", operation);
        }
        Kind::Shell { thickness } => {
            // The direction below says which side the wall grows to; the label stays short enough
            // for the label column.
            enter |= expr_row(ui, "Thickness", thickness);
            form_row(ui, "Direction", |ui| {
                extra_segments(ui, &mut d.extra, "direction", &[("Inside", "inside"), ("Outside", "outside"), ("Both", "both")]);
            });
            extra_options(ui, "shell_options", &mut d.extra, &[("Tangent Chain", "tangent_chain")]);
        }
        Kind::Draft { angle } => {
            enter |= expr_row(ui, "Angle", angle);
        }
        Kind::Mirror => {
            // Feature replay retains the source operation; Join is for body copies.
            if !d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                form_row(ui, "Operation", |ui| {
                    let mut op = usize::from(d.extra.get("combine").and_then(Value::as_bool).unwrap_or(false));
                    ui_kit::segmented(ui, &mut op, &["New Body", "Join"], Default::default());
                    d.extra.insert("combine".into(), json!(op == 1));
                });
            }
        }
        Kind::OffsetPlane { offset } => {
            if !through_point {
                enter |= expr_row(ui, "Distance", offset);
            }
        }
        Kind::AnglePlane { angle, direction_only } => {
            enter |= expr_row(ui, "Angle", angle);
            ui_kit::options(ui, "plane_angle_options", &mut [("Use axis direction only", direction_only)]);
            const PIVOT: Accept = selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS;
            set_object_accepting(&mut d.inputs, &mut d.active, "Pivot", PIVOT, *direction_only);
        }
        // The angle turns the plane from its reference, so it shows once one is picked.
        Kind::TangentPlane { angle } => {
            if d.inputs.get(1).is_some_and(|inp| !inp.items.is_empty()) {
                enter |= expr_row(ui, "Angle", angle);
            }
        }
        Kind::PathPlane { t, .. } => {
            enter |= expr_row(ui, "Position (0 to 1)", t);
        }
        Kind::MeshConvert { tolerance } => {
            form_row(ui, "Tolerance", |ui| enter |= field(ui, tolerance));
        }
        Kind::Construct { t: Some(t), .. } => {
            form_row(ui, "Position (0 to 1)", |ui| enter |= field(ui, t));
        }
        Kind::Split | Kind::ReplaceFace | Kind::Remove | Kind::Midplane | Kind::ChatContext | Kind::Construct { t: None, .. } => {}
        Kind::Rib { thickness, depth, flip, normal, .. } => {
            form_row(ui, "Direction", |ui| {
                let mut i = usize::from(*normal);
                ui_kit::segmented(ui, &mut i, &["In plane", "Normal"], Default::default());
                *normal = i == 1;
            });
            enter |= expr_row(ui, "Thickness", thickness);
            enter |= hint_row(ui, "Depth", depth, tr("to the next face"));
            ui_kit::options(ui, "feature_options", &mut [("Flip", flip)]);
        }
        Kind::Emboss { depth, deboss } => {
            enter |= expr_row(ui, "Depth", depth);
            form_row(ui, "Type", |ui| {
                let mut i = usize::from(*deboss);
                ui_kit::segmented(ui, &mut i, &["Emboss", "Deboss"], Default::default());
                *deboss = i == 1;
            });
            ui.add(egui::Label::new(crate::i18n::tr("ui.emboss_wrap_note")).wrap());
        }
        Kind::Align { flip, components, axes: [ax, ay, az], angle } => {
            // Where on the picked face or edge to snap: as picked (a face turns to meet
            // the other, an edge at its middle), its center, a round one's center, or, for bodies,
            // the joint origin the Joint dialog makes of it (found again as the part changes, #6257).
            let snaps: &[(&str, &str)] = if *components {
                &[("As Picked", "picked"), ("Center", "center"), ("Circle Center", "circle")]
            } else {
                &[("As Picked", "picked"), ("Center", "center"), ("Circle Center", "circle"), ("Joint Origin", "origin")]
            };
            for (label, key) in [("From Snap", "from_snap"), ("To Snap", "to_snap")] {
                form_row(ui, label, |ui| {
                    extra_choice(ui, &mut d.extra, key, key, snaps);
                });
            }
            if *components {
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip)]);
            } else {
                // A turn about the target's direction after the move (#6257).
                enter |= hint_row(ui, "Angle", angle, tr("none"));
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip), ("X", ax), ("Y", ay), ("Z", az)]);
            }
            // A part inside a subassembly placed more than once sits where that shared
            // definition says: aligning it moves it in every placement.
            if *components && align_moves_shared(&app.session, d.inputs.first().map_or(&[], |i| i.items.as_slice())) {
                let t = Tokens::get();
                ui.add(egui::Label::new(RichText::new(tr(ALIGN_SHARED_WARNING)).color(t.warning).size(t.font_small)).wrap());
            }
        }
        Kind::BoundaryFill { operation, cells, .. } => {
            form_row(ui, "Cells", |_| {});
            if cells.is_empty() {
                ui_kit::status_line(ui, "Pick the tool bodies", ui_kit::StatusTone::Dim);
            }
            for (i, (_, label, on)) in cells.iter_mut().enumerate() {
                form_row(ui, &format!("Cell {}", i + 1), |ui| {
                    ui_kit::checkbox(ui, label.as_str(), on);
                });
            }
            choice_row(ui, "Operation", "bf_op", operation, &OP_LABELS);
            extra_options(ui, "boundary_options", &mut d.extra, &[("Remove Tools", "remove_tools")]);
        }
        Kind::PathPattern { count, spacing } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, if d.extra.contains_key("distance") { "Distance" } else { "Spacing" }, spacing);
        }
        Kind::Pipe { diameter, wall, operation } => {
            enter |= expr_row(ui, "Diameter", diameter);
            enter |= hint_row(ui, "Wall", wall, tr("solid"));
            choice_row(ui, "Operation", "pp_op", operation, &OP_LABELS);
        }
        Kind::Stock { margin } => {
            enter |= expr_row(ui, "Margin", margin);
        }
        Kind::Scale { factor, per_axis, factor_y, factor_z } => {
            form_row(ui, "Type", |ui| {
                let mut i = usize::from(*per_axis);
                ui_kit::segmented(ui, &mut i, &["scale.uniform", "scale.per_axis"], Default::default());
                if (i == 1) != *per_axis {
                    *per_axis = i == 1;
                    // Y and Z start at the factor, so nothing changes until one of them does.
                    if *per_axis {
                        factor_y.clone_from(factor);
                        factor_z.clone_from(factor);
                    }
                }
            });
            if *per_axis {
                enter |= expr_row(ui, "X", factor);
                enter |= expr_row(ui, "Y", factor_y);
                enter |= expr_row(ui, "Z", factor_z);
            } else {
                enter |= expr_row(ui, "Scale factor", factor);
            }
        }
        Kind::OffsetFaces { distance } => {
            enter |= expr_row(ui, "Distance", distance);
        }
        Kind::Thread { designation, length, suggested, of, options } => {
            let mut modeled = d.extra.get("modeled").and_then(Value::as_bool).unwrap_or(false);
            let before = modeled;
            let (submitted, reset) = crate::dialogs_thread::rows(ui, designation, suggested, length, options, &mut modeled);
            if modeled != before {
                d.extra.insert("modeled".into(), json!(modeled));
            }
            enter |= submitted;
            if reset {
                of.clear();
            }
            crate::dialogs_thread::sync_inputs(&mut d.inputs, &mut d.active, options);
        }
        Kind::Loft { operation, rail_fit } => {
            crate::op_auto::op_row(ui, "lf_op", operation);
            crate::dialogs_sweep::rail_fit_row(ui, "lf", rail_fit);
        }
        Kind::Sweep { operation, opts } => {
            enter |= crate::dialogs_sweep::rows_with(ui, opts, "sw", |ui| {
                crate::op_auto::op_row(ui, "sw_op", operation);
            });
        }
        k @ Kind::Move { .. } => {
            enter |= crate::dialogs_move::rows(ui, k);
            extra_options(ui, "move_options", &mut d.extra, &[("Create Copy", "copy")]);
        }
        Kind::Primitive { cmd, fields, operation, at, .. } => {
            let from_sketch = *cmd == "solid.coil" && d.inputs.first().is_some_and(|i| i.accept & selection::PROFILES != 0 && !i.items.is_empty());
            for (k, v) in fields.iter_mut() {
                if from_sketch && (*k == "diameter" || *k == "section_size") {
                    continue;
                }
                // Revolutions steps by whole turns like the other counts (#7481).
                enter |= if *k == "revolutions" {
                    count_row_to(ui, &app.session.doc, primitive_label(k), v, MAX_COIL_REVOLUTIONS)
                } else {
                    expr_row(ui, primitive_label(k), v)
                };
            }
            if *cmd == "solid.coil" {
                enter |= coil_rows(ui, &mut d.extra);
            }
            if at.is_none() {
                enter |= crate::primitive_placement::rows(ui, &app.session, cmd, &mut d.extra);
            }
            if let Some((p, _)) = *at {
                form_row(ui, "Position", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z));
                        // A cylinder's place is its Face pick, cleared there.
                        if *cmd != "solid.cylinder"
                            && ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("Place at the origin instead")).clicked()
                        {
                            *at = None;
                        }
                    });
                });
            }
            crate::op_auto::op_row(ui, "pr_op", operation);
        }
        Kind::Combine { operation, keep_tools } => {
            form_row(ui, "Operation", |ui| {
                let mut op = operation.saturating_sub(1);
                combo(ui, "cb_op", &OP_LABELS[1..], &mut op);
                *operation = op + 1;
            });
            ui_kit::section(ui, "combine_options", "Options", true, |ui| {
                let mut new_component = d.extra.get("new_component").and_then(Value::as_bool).unwrap_or(false);
                let before = new_component;
                ui_kit::checkbox_grid(ui, &mut [("New Component", &mut new_component), ("Keep Tools", keep_tools)], 2);
                if new_component != before {
                    d.extra.insert("new_component".into(), json!(new_component));
                }
            });
        }
        Kind::Surface(k) => enter |= crate::dialogs_surface::rows(ui, k),
        Kind::Sheet(k) => enter |= crate::dialogs_sheet::rows(app, ui, k, &mut d.inputs),
        Kind::Plastic(k) => enter |= crate::dialogs_plastic::rows(app, ui, k, &d.inputs),
        Kind::Frame(k) => enter |= crate::dialogs_frame::rows(app, ui, k, &d.inputs),
        Kind::Assembly(k) => enter |= crate::dialogs_assembly::rows(app, ui, k, &mut d.inputs, &mut d.active),
        Kind::Measure { result, show, .. } => measure_rows(app, ui, result.as_ref(), show),
        Kind::Section { offset, flip, hatch } => {
            enter |= expr_row(ui, "Distance", offset);
            ui_kit::options(ui, "feature_options", &mut [("Flip", flip), ("Hatch", hatch)]);
            if section_turns_off(&app.session, &d.inputs) && !d.extra.contains_key("plane") {
                ui_kit::text(ui, tr("section.off_hint"), ui_kit::Text::Hint("section_off"));
            }
        }
        Kind::Rename { name, .. } => {
            form_row(ui, "Name", |ui| {
                enter |= ui_kit::focused_text_field(ui, name, "");
            });
        }
        _ => {}
    }
    enter
}

fn fmt_mm(app: &SolveApp, v: &Value) -> String {
    v.as_f64().map(|x| crate::prefs::show_mm(app, x, 1)).unwrap_or_default()
}

/// The measurement in the dialog: each item's size, then the distance (a pair of round items
/// offers centre to centre, closest and farthest), its ΔX/Y/Z and the angle between two items.
fn measure_rows(app: &SolveApp, ui: &mut egui::Ui, r: Option<&Value>, show: &mut Option<Shown>) {
    let Some(r) = r else {
        ui_kit::text(ui, tr("pick one or two vertices, edges or faces"), ui_kit::Text::Hint("measure"));
        return;
    };
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        ui_kit::text(ui, e, ui_kit::Text::Report("measure_error", true));
        return;
    }
    let mm = |v: &Value| fmt_mm(app, v);
    let value = |ui: &mut egui::Ui, label: &str, text: &str| {
        form_row(ui, label, |ui| {
            ui_kit::text(ui, text, ui_kit::Text::Cell);
        });
    };
    for (i, it) in r["items"].as_array().into_iter().flatten().enumerate() {
        let lines = crate::quick_measure::item_lines(app, it);
        for (k, line) in lines.into_iter().enumerate() {
            let label = if k == 0 { crate::i18n::format("measure.item", &[("number", &(i + 1).to_string())]) } else { String::new() };
            // Coordinates need all three values; a fixed-height form cell elides the end.
            let text = if label.is_empty() { line } else { format!("{label}  {line}") };
            ui_kit::text(ui, &text, ui_kit::Text::Report(&format!("measure_item_{i}_{k}"), false));
        }
    }
    if let Some((now, d, a, b)) = shown_distance(r, *show) {
        if r.get("center_distance").is_some() {
            // Centre, min and max all listed; the chosen one is drawn and broken into ΔX/Y/Z.
            let centre_label = crate::quick_measure::centre_distance_label(r);
            for (which, label, key) in
                [(Shown::Centre, centre_label, "center_distance"), (Shown::Min, tr("Min"), "distance_mm"), (Shown::Max, tr("Max"), "max")]
            {
                let Some(v) = r.get(key).filter(|v| v.is_number()) else { continue };
                if ui_kit::list_picker(ui, key, &format!("{}  {}", label, mm(v)), now == which).clicked() {
                    *show = Some(which);
                }
            }
        } else {
            let text = crate::prefs::show_mm(app, d, 1);
            form_row(ui, crate::i18n::tr_label("Distance"), |ui| {
                let size = crate::theme::Tokens::get().form_close_size;
                // The value keeps the room left of the button, so the button stays inside the row.
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - size).max(0.0));
                    ui_kit::text(ui, &text, ui_kit::Text::Cell);
                });
                let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                let copy = ui_kit::icon_button_at(ui, rect, "copy", crate::i18n::tr_label("Copy"));
                crate::scenario::publish_handle("measure:copy:distance", copy.rect.center());
                if copy.clicked() {
                    ui.ctx().copy_text(text.clone());
                }
            });
        }
        let delta = b - a;
        for (l, v) in [("ΔX", delta.x), ("ΔY", delta.y), ("ΔZ", delta.z)] {
            value(ui, l, &crate::prefs::show_mm(app, v.abs(), 1));
        }
    }
    if let Some(a) = r["angle_deg"].as_f64()
        && r["items"].as_array().is_some_and(|v| v.len() == 2)
    {
        value(ui, crate::i18n::tr_label("Angle"), &format!("{a:.3}°"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{self, Harness};

    const COMMANDS: [&str; 6] =
        ["solid.extrude", "solid.fillet", "solid.chamfer", "solid.hole", "solid.pattern.rectangular", "solid.pattern.circular"];

    #[test]
    fn linuxcnc_cam_form_opens_and_saves_a_job_without_opening_a_file_dialog() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        h.step(&json!({"start":"cam.job.create"})).unwrap();
        h.frames(3);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Milling(_))));
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.doc.cam.setups.len(), 1);
        assert_eq!(h.app.session.doc.cam.setups[0].operations.len(), 1);
        assert!(h.app.file_picker.is_none());
        // The new step shows in the job list.
        assert!(h.app.cam_window.is_some());
    }

    #[test]
    fn linuxcnc_cam_form_takes_library_cutting_data_and_the_job_list_shows_the_steps() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        // The library drops broken and duplicate rows and keeps the rest in tool order.
        h.app.preferences.cam_tools = serde_json::from_value(json!([
            {"number": 3, "kind": "flat_endmill", "diameter": 6, "flute_length": 20, "cutting": {"wood": {"vc": 300, "fz": 0.1, "ap": 0.5, "ae": 0.5}}},
            null,
            {"number": 3, "kind": "drill", "diameter": 5, "flute_length": 30},
            {"number": 1, "kind": "flat_endmill", "diameter": -1, "flute_length": 20},
            {"number": 2, "kind": "drill", "diameter": 5, "flute_length": 30},
        ]))
        .unwrap();
        assert_eq!(h.app.preferences.cam_tools.0.iter().map(|t| t.number).collect::<Vec<_>>(), [2, 3]);
        h.step(&json!({"start":"cam.op.contour"})).unwrap();
        h.frames(3);
        let tool = h.app.preferences.cam_tools.find(3).cloned();
        let mut d = h.app.dialog.take().unwrap();
        let Kind::Milling(form) = &mut d.kind else { panic!("the contour form") };
        // No material: nothing to take.
        assert!(form.take_cutting(&h.app).is_err());
        form.choose_tool(tool);
        form.material = Some("wood".into());
        form.take_cutting(&h.app).unwrap();
        // n = 300 · 1000 / (π · 6), vf = n · 2 · 0.1.
        let p = form.params().unwrap();
        assert_eq!(
            (p["tool"].as_u64(), p["spindle_rpm"].as_f64(), p["feed"].as_f64(), p["material"].as_str()),
            (Some(3), Some(15915.0), Some(3183.0), Some("wood"))
        );
        h.app.dialog = Some(d);
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        // The tool came into the design with its cutting data, the setup took the material.
        let cam = &h.app.session.doc.cam;
        assert_eq!(cam.tool(3).map(|t| t.cutting.len()), Some(1));
        assert_eq!(cam.setups[0].material.as_deref(), Some("wood"));
        // The next step goes into the same setup; the job list shows both in order.
        h.step(&json!({"start":"cam.op.pocket"})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.cam.setups.len(), 1);
        // A second pocket gets a free name and never replaces the first.
        h.step(&json!({"start":"cam.op.pocket"})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        let names: Vec<String> = h.app.session.doc.cam.setups[0].operations.iter().map(|o| o.name().to_string()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert_eq!(names.get(2).map(String::as_str), names.get(1).map(|n| format!("{n} 2")).as_deref());
        // A typed name that is taken is refused, not replaced.
        h.step(&json!({"start":"cam.op.pocket"})).unwrap();
        h.frames(2);
        if let Some(Kind::Milling(form)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            form.set_name(&names[0]);
        }
        h.frames(2);
        let Some(Kind::Milling(form)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the pocket form") };
        assert!(form.params().is_err());
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.cam.setups[0].operations.len(), 3);
        h.app.dialog = None;
        h.step(&json!({"start":"cam.list"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.cam_window.as_ref().map(|w| w.tab), Some(0));
        let names: Vec<String> = h.app.session.doc.cam.setups[0].operations.iter().map(|o| o.name().to_string()).collect();
        for name in &names {
            assert!(h.widget(name).is_some(), "{name} is listed");
        }
        assert!(h.widget("Export G-code…").is_some());
        // Editing a step fills the form with its values and geometry; OK replaces it in place.
        let setup = h.app.session.doc.cam.setups[0].clone();
        let first = setup.operations.first().cloned().unwrap();
        let form = crate::dialogs_milling::Milling::edit(&h.app, &setup, &first);
        let p = form.params().unwrap();
        assert_eq!((p["name"].as_str(), p["tool"].as_u64(), p["spindle_rpm"].as_f64()), (Some(first.name()), Some(3), Some(15915.0)));
        assert!(p.get("geometry").is_some());
        h.app.run("cam.job.create", p).unwrap();
        assert_eq!(h.app.session.doc.cam.setups[0].operations, setup.operations);
        assert!(h.widget("Edit").is_some());
        h.step(&json!({"start":"cam.tool.define"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.cam_window.as_ref().map(|w| w.tab), Some(1));
        assert!(h.widget("New tool").is_some());
    }

    #[test]
    fn cam_export_writes_into_the_machine_folder_or_asks_when_it_cannot() {
        // Folders are personal: a broken row is dropped, an empty folder removes the entry.
        let mut folders: crate::cam_panel::ProgramFolders =
            serde_json::from_value(json!({"Portal": r"\\linuxcnc\nc_files", "": "x", "Bell": "a\u{7}b", "Count": 3})).unwrap();
        assert_eq!(folders.0.keys().collect::<Vec<_>>(), ["Portal"]);
        folders.set("Portal", "  ").unwrap();
        assert!(folders.get("Portal").is_none());
        assert!(folders.set("Portal", &"x".repeat(1025)).is_err());
        let mut h = Harness::new();
        h.app.home.open = false;
        let mill = json!({"name": "Portal", "x": [0, 300], "y": [0, 200], "z": [-100, 0],
            "spindle_min": 1000, "spindle_max": 24000, "max_feed": 5000, "post": "grbl"});
        h.app.run("cam.tool.define", json!({"number": 1, "diameter": 4, "flute_length": 12})).unwrap();
        h.app.run("cam.setup", json!({"name": "Top: front/back", "machine": mill})).unwrap();
        h.app
            .run("cam.op.contour", json!({"setup": "Top: front/back", "name": "C", "tool": 1, "outer": [[0,0],[20,0],[20,10],[0,10]], "bottom": -1}))
            .unwrap();
        // The machine's post names the file (GRBL: .nc); a name's slash and colon cannot be in it.
        let folder = h.dir.join("machine-share");
        std::fs::create_dir_all(&folder).unwrap();
        h.app.preferences.cam_folders.set("Portal", &folder.to_string_lossy()).unwrap();
        crate::cam_panel::export(&mut h.app, "Top: front/back").unwrap();
        let file = folder.join("Top_ front_back.nc");
        assert!(std::fs::read_to_string(&file).is_ok_and(|g| g.contains("G1")));
        // Exported again, the program in the folder is replaced.
        crate::cam_panel::export(&mut h.app, "Top: front/back").unwrap();
        // A folder that cannot be written says so and asks where instead: nothing is lost.
        let unreachable = file.join("not a folder");
        h.app.preferences.cam_folders.set("Portal", &unreachable.to_string_lossy()).unwrap();
        let e = crate::cam_panel::export(&mut h.app, "Top: front/back").unwrap_err();
        assert!(e.contains("not a folder"), "{e}");
        h.frames(3);
        assert!(h.dir.join("Top_ front_back.nc").is_file(), "the save dialog took the program");
    }

    #[test]
    fn cam_form_shows_the_cut_load_and_warns_what_the_machine_cannot_give() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        let mill = json!({"name": "Mini", "x": [0, 300], "y": [0, 200], "z": [-100, 0],
            "spindle_min": 1000, "spindle_max": 24000, "max_feed": 5000, "spindle_power": 0.05});
        h.app.run("cam.setup", json!({"name": "Top", "material": "aluminium", "machine": mill})).unwrap();
        // 10000 rev/min, two flutes at 0.05 mm: 1000 mm/min, 6 mm deep, 0.6 mm sideways.
        h.app.preferences.cam_tools = serde_json::from_value(json!([{"number": 3, "kind": "flat_endmill", "diameter": 6, "flute_length": 20,
            "cutting": {"aluminium": {"vc": 188.4955592153876, "fz": 0.05, "ap": 1.0, "ae": 0.1}}}]))
        .unwrap();
        h.step(&json!({"start":"cam.op.pocket"})).unwrap();
        h.frames(2);
        let tool = h.app.preferences.cam_tools.find(3).cloned();
        let mut d = h.app.dialog.take().unwrap();
        let Kind::Milling(form) = &mut d.kind else { panic!("the pocket form") };
        form.choose_tool(tool);
        form.take_cutting(&h.app).unwrap();
        assert_eq!(form.params().unwrap()["feed"].as_f64(), Some(1000.0));
        // The thin chip is a fifth thinner than fz, and the cut needs more than the spindle has.
        let (load, ap, ae, over) = form.load(&h.app).unwrap();
        assert!((load.chip - 0.03).abs() < 1e-9 && (ap, ae) == (6.0, 0.6), "{load:?}");
        assert_eq!(load.thinned_feed, 1667.0);
        assert!(matches!(over.as_slice(), [solidcraft_engine::cam::Overload::Power { have, .. }] if *have == 0.05), "{over:?}");
        // Ticked, the feed rises until the chip is fz again.
        form.thin_chips = true;
        form.take_cutting(&h.app).unwrap();
        assert_eq!(form.params().unwrap()["feed"].as_f64(), Some(1667.0));
        h.app.dialog = Some(d);
        h.frames(2);
        for label in ["Load", "Make up for thin chips"] {
            assert!(h.widget(label).is_some(), "{label} is shown");
        }
        // The machine form keeps the spindle power.
        assert_eq!(h.app.session.doc.cam.machine("Mini").and_then(|m| m.spindle_power), Some(0.05));
    }

    #[test]
    fn cam_contour_form_lets_the_control_compensate_and_edits_keep_it() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        h.app.run("cam.tool.define", json!({"number": 2, "diameter": 6, "flute_length": 20})).unwrap();
        h.app.run("cam.setup", json!({"name": "Top"})).unwrap();
        h.step(&json!({"start":"cam.op.contour"})).unwrap();
        h.frames(2);
        // The choice is there; holding tabs are offered only while the CAM compensates.
        assert!(h.widget("Radius compensation").is_some() && h.widget("Holding tabs").is_some());
        let mut d = h.app.dialog.take().unwrap();
        let Kind::Milling(form) = &mut d.kind else { panic!("the contour form") };
        form.compensation = 1;
        assert_eq!(form.params().unwrap()["compensation"], "control");
        h.app.dialog = Some(d);
        h.frames(2);
        assert!(h.widget("Holding tabs").is_none(), "no tabs with compensation on the control");
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let setup = h.app.session.doc.cam.setups[0].clone();
        let op = setup.operations.first().cloned().unwrap();
        let solidcraft_engine::doc::cam::CamOperation::Contour { params, .. } = &op else { panic!("a contour") };
        assert_eq!(params.compensation, solidcraft_engine::doc::cam::CamCompensation::Control);
        // Editing brings the choice back; the CAM's own compensation is not written at all.
        let again = crate::dialogs_milling::Milling::edit(&h.app, &setup, &op);
        assert_eq!(again.compensation, 1);
        let mut cam = again.clone();
        cam.compensation = 0;
        assert!(cam.params().unwrap().get("compensation").is_none());
    }

    #[test]
    fn cam_slot_form_takes_a_width_and_mills_around_the_rectangle() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        h.app.run("cam.setup", json!({"name": "Top"})).unwrap();
        h.step(&json!({"start":"cam.op.slot"})).unwrap();
        h.frames(2);
        // A slot has a width and wall stock, no floor stock and no climb or conventional choice.
        assert!(h.widget("Slot width (mm)").is_some() && h.widget("Wall stock to leave (mm)").is_some());
        assert!(h.widget("Floor stock to leave (mm)").is_none() && h.widget("Climb").is_none());
        let Some(Kind::Milling(form)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the slot form") };
        let p = form.params().unwrap();
        assert_eq!((p["kind"].as_str(), p.get("slot_width")), (Some("slot"), None), "empty: the tool's diameter");
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let setup = h.app.session.doc.cam.setup("Top").cloned().unwrap();
        let op = setup.operations.first().cloned().unwrap();
        assert!(matches!(&op, solidcraft_engine::doc::cam::CamOperation::Slot { .. }), "{op:?}");
        assert!(h.app.session.cam_program("Top").is_ok());
        // Editing brings it back as a slot, a typed width kept.
        let mut form = crate::dialogs_milling::Milling::edit(&h.app, &setup, &op);
        assert_eq!(form.params().unwrap()["kind"].as_str(), Some("slot"));
        form.set_field("slot_width", "10");
        assert_eq!(form.params().unwrap()["slot_width"].as_f64(), Some(10.0));
    }

    #[test]
    fn cam_drill_form_takes_a_sketch_circle_and_a_sketch_point_as_holes() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [10, 10], "radius": 2})).unwrap();
        h.app.run("sketch.point", json!({"point": [30, 25], "id": "p"})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("cam.tool.define", json!({"number": 2, "kind": "drill", "diameter": 4, "flute_length": 30})).unwrap();
        h.app.run("cam.setup", json!({"name": "Top"})).unwrap();
        h.step(&json!({"start":"cam.op.drill"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        d.pick(&h.app.session, Sel::Profile { sketch, index: 0 });
        d.pick(&h.app.session, Sel::ExternalSketchPoint { sketch, id: "p".into() });
        assert_eq!(d.inputs[0].items.len(), 2);
        h.app.dialog = Some(d);
        h.frames(2);
        assert!(h.widget("1. Sketch profile 1").is_some() && h.widget("2. Sketch point p").is_some());
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let op = h.app.session.doc.cam.setup("Top").and_then(|s| s.operations.first().cloned()).unwrap();
        let solidcraft_engine::doc::cam::CamOperation::Drill { geometry: solidcraft_engine::doc::cam::CamGeometry::Points { points }, .. } = op
        else {
            panic!("{op:?}")
        };
        assert_eq!(points, [[10.0, 10.0], [30.0, 25.0]]);
    }

    /// A library drill picked in a pocket form, while T1 is the design's end mill that a contour
    /// uses: the step becomes a drill on a free number and the contour still posts.
    #[test]
    fn cam_a_library_drill_picked_for_a_circle_drills_without_taking_the_mills_number() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [10, 10], "radius": 2})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20})).unwrap();
        h.app.run("cam.setup", json!({"name": "Top", "stock": {"min": [0, 0, -10], "max": [40, 30, 0]}})).unwrap();
        h.app
            .run(
                "cam.op.contour",
                json!({"setup": "Top", "name": "Outside", "tool": 1, "outer": [[0,0],[40,0],[40,30],[0,30]], "side": "outside", "bottom": -3}),
            )
            .unwrap();
        h.app.preferences.cam_tools = serde_json::from_value(json!([{"number": 1, "kind": "drill", "diameter": 4, "flute_length": 30}])).unwrap();
        h.step(&json!({"start":"cam.op.pocket"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        d.pick(&h.app.session, Sel::Profile { sketch, index: 0 });
        let Kind::Milling(form) = &mut d.kind else { panic!("the pocket form") };
        form.pick_library_tool(&h.app, h.app.preferences.cam_tools.find(1).cloned());
        let p = form.params().unwrap();
        assert_eq!((p["kind"].as_str(), p["tool"].as_u64()), (Some("drill"), Some(2)), "{p}");
        h.app.dialog = Some(d);
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let cam = &h.app.session.doc.cam;
        assert_eq!(
            (cam.tool(1).map(|t| t.kind), cam.tool(2).map(|t| t.kind)),
            (Some(solidcraft_engine::cam::ToolKind::FlatEndmill), Some(solidcraft_engine::cam::ToolKind::Drill))
        );
        let ops = cam.setup("Top").map(|s| s.operations.len());
        assert_eq!(ops, Some(2));
        h.app.run("cam.post", json!({"setup": "Top"})).unwrap();
    }

    #[test]
    fn cam_typed_hole_centres_take_decimal_commas_and_separators() {
        let mut h = Harness::new();
        h.app.run("cam.tool.define", json!({"number": 2, "kind": "drill", "diameter": 5, "flute_length": 30})).unwrap();
        h.app.run("cam.setup", json!({"name": "Top"})).unwrap();
        h.app.run("cam.op.drill", json!({"setup": "Top", "name": "H", "tool": 2, "points": [[1, 2]], "bottom": -5})).unwrap();
        let setup = h.app.session.doc.cam.setup("Top").cloned().unwrap();
        let op = setup.operations.first().cloned().unwrap();
        let mut form = crate::dialogs_milling::Milling::edit(&h.app, &setup, &op);
        assert_eq!(form.params().unwrap()["points"], json!([[1.0, 2.0]]));
        form.set_holes(
            "10 20
10.5; 20
10,5 20,25
10, 20
10,20

  7	8  ",
        );
        let points = form.params().unwrap()["points"].clone();
        assert_eq!(points, json!([[10.0, 20.0], [10.5, 20.0], [10.5, 20.25], [10.0, 20.0], [10.0, 20.0], [7.0, 8.0]]));
        for bad in ["10", "10 20 30", "10,5,20", "x 20", "10;;"] {
            form.set_holes(bad);
            assert!(form.params().is_err(), "{bad}");
        }
    }

    #[test]
    fn cam_tool_library_goes_to_a_file_and_comes_back_asking_before_it_replaces() {
        use solidcraft_engine::cam::{Tool, ToolKind};
        let mut h = Harness::new();
        h.app.home.open = false;
        for (n, d) in [(1, 6.0), (2, 10.0)] {
            h.app.preferences.cam_tools.put(Tool::new(n, ToolKind::FlatEndmill, d, 20.0)).unwrap();
        }
        crate::cam_panel::open(&mut h.app, 1);
        h.frames(2);
        // Exported into the folder the save dialog answers with.
        crate::cam_panel::export_library(&mut h.app);
        h.frames(3);
        let file = h.dir.join("tool-library.json");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("solidcraft-tools"), "{text}");
        // At home tool 2 is another one and tool 3 is missing.
        h.app.preferences.cam_tools.0.clear();
        h.app.preferences.cam_tools.put(Tool::new(2, ToolKind::BallEndmill, 8.0, 16.0)).unwrap();
        *h.next_file.borrow_mut() = Some(file.to_string_lossy().into_owned());
        crate::cam_panel::import_library(&mut h.app);
        h.frames(3);
        assert!(h.widget("The file holds 2 tools, 1 of them new. Different from the library: T2.").is_some());
        h.step(&json!({"widget": "Take only new ones"})).unwrap();
        h.frames(2);
        let kept = h.app.preferences.cam_tools.find(2).map(|t| t.kind);
        assert_eq!((h.app.preferences.cam_tools.0.len(), kept), (2, Some(ToolKind::BallEndmill)));
        // Taking all replaces tool 2 as well.
        let tools = vec![Tool::new(2, ToolKind::FlatEndmill, 10.0, 20.0)];
        let mut library = h.app.preferences.cam_tools.clone();
        assert_eq!(crate::cam_panel::take_tools(&mut library, &tools, true), Ok((0, 1)));
        // A file that is not a tool library is refused and nothing changes.
        let other = h.dir.join("other.json");
        std::fs::write(&other, r#"{"format":"something-else","tools":[]}"#).unwrap();
        *h.next_file.borrow_mut() = Some(other.to_string_lossy().into_owned());
        crate::cam_panel::import_library(&mut h.app);
        h.frames(3);
        let error = h.app.cam_window.as_ref().and_then(|w| w.error.clone()).unwrap_or_default();
        assert!(error.contains("solidcraft-tools"), "{error}");
        assert_eq!(h.app.preferences.cam_tools.0.len(), 2);
    }

    #[test]
    fn cam_window_shows_a_setups_gcode_and_draws_the_selected_lines_move() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        let mill = json!({"name": "Portal", "x": [0, 300], "y": [0, 200], "z": [-100, 0],
            "spindle_min": 1000, "spindle_max": 24000, "max_feed": 5000, "post": "grbl"});
        h.app.run("cam.tool.define", json!({"number": 1, "diameter": 4, "flute_length": 12})).unwrap();
        h.app.run("cam.setup", json!({"name": "Top", "machine": mill})).unwrap();
        h.app.run("cam.op.contour", json!({"setup": "Top", "name": "C", "tool": 1, "outer": [[0,0],[20,0],[20,10],[0,10]], "bottom": -1})).unwrap();
        // Every move belongs to a motion line of the program as it is exported.
        let program = h.app.session.cam_program("Top").unwrap();
        let lines: Vec<&str> = program.gcode.lines().collect();
        assert!(!program.moves.is_empty());
        for m in &program.moves {
            let text = lines[m.line - 1];
            assert!(text.contains('X') || text.contains('Y') || text.contains('Z'), "line {}: {text}", m.line);
        }
        h.step(&json!({"start":"cam.list"})).unwrap();
        h.frames(3);
        h.step(&json!({"widget": "Show G-code"})).unwrap();
        h.frames(3);
        let summary = format!("{} lines, post GRBL", lines.len());
        assert!(h.widget(&summary).is_some(), "{summary}");
        // The estimated run time is shown under it.
        let time = program.time.as_ref().unwrap();
        assert!(time.total > 0.0 && time.tool_changes == 0.0, "{time:?}");
        let clock = crate::cam_panel::clock;
        let shown = format!(
            "Run time about {} (cutting {}, rapids {}, tool changes {}).",
            clock(time.total),
            clock(time.cutting),
            clock(time.rapid),
            clock(time.tool_changes)
        );
        assert!(h.widget(&shown).is_some(), "{shown}");
        assert_eq!((clock(59.6), clock(61.0), clock(3725.0), clock(f64::NAN)), ("1:00".into(), "1:01".into(), "1:02:05".into(), "0:00".into()));
        // Stepping through the moves selects their lines; the cuts run round the outline.
        fn view(h: &Harness) -> Option<&crate::cam_panel::GcodeView> {
            h.app.cam_window.as_ref().and_then(|w| w.gcode.as_ref())
        }
        let mut cuts = 0;
        for _ in 0..program.moves.len() {
            h.app.cam_window.as_mut().and_then(|w| w.gcode.as_mut()).unwrap().step(1);
            let m = view(&h).and_then(|v| v.selected_move()).unwrap();
            if m.cutting {
                cuts += 1;
                assert!(m.points.iter().all(|p| (-2.1..=22.1).contains(&p.x) && (-2.1..=12.1).contains(&p.y)), "{:?}", m.points);
            }
        }
        assert!(cuts >= 4);
        // Playback: from the start the tool runs along the program, line by line, and stops at
        // its end; Play again starts over, Pause holds it, a step puts it at that move's end.
        fn view_mut(h: &mut Harness) -> &mut crate::cam_panel::GcodeView {
            h.app.cam_window.as_mut().and_then(|w| w.gcode.as_mut()).unwrap()
        }
        view_mut(&mut h).rewind();
        assert_eq!(view(&h).and_then(|v| v.tool_at()).map(|(i, _)| i), Some(0));
        h.step(&json!({"widget": "Play"})).unwrap();
        assert!(view(&h).is_some_and(|v| v.playing));
        assert!(h.widget("Pause").is_some());
        let mut lines_seen = Vec::new();
        for _ in 0..10_000 {
            view_mut(&mut h).advance(0.05);
            let v = view(&h).unwrap();
            if lines_seen.last() != v.selected.as_ref() {
                lines_seen.extend(v.selected);
            }
            if !v.playing {
                break;
            }
        }
        assert!(view(&h).is_some_and(|v| !v.playing), "the playback ends");
        assert!(lines_seen.windows(2).all(|w| w[0] < w[1]) && lines_seen.len() >= 4, "{lines_seen:?}");
        let end = program.moves.last().and_then(|m| m.points.last()).copied().unwrap();
        let (_, tip) = view(&h).and_then(|v| v.tool_at()).unwrap();
        assert!(tip.dist(end) < 1e-6, "{tip:?} {end:?}");
        view_mut(&mut h).play();
        assert_eq!(view(&h).and_then(|v| v.travelled), Some(0.0));
        view_mut(&mut h).advance(0.1);
        view_mut(&mut h).play();
        let held = view(&h).and_then(|v| v.travelled);
        view_mut(&mut h).advance(1.0);
        assert_eq!(view(&h).and_then(|v| v.travelled), held);
        view_mut(&mut h).step(1);
        let (index, tip) = view(&h).and_then(|v| v.tool_at()).unwrap();
        assert_eq!(view(&h).and_then(|v| v.selected), program.moves.get(index).map(|m| m.line));
        assert!(program.moves.get(index).and_then(|m| m.points.last()).is_some_and(|p| p.dist(tip) < 1e-6));
        h.frames(2);
        let last = view(&h).and_then(|v| v.selected);
        h.frames(2);
        // A changed design is posted again; the selected line stays.
        h.app.run("cam.op.contour", json!({"setup": "Top", "name": "C", "tool": 1, "outer": [[0,0],[30,0],[30,10],[0,10]], "bottom": -2})).unwrap();
        h.frames(2);
        assert_eq!(view(&h).and_then(|v| v.selected), last);
        let summary = format!("{} lines, post GRBL", h.app.session.cam_program("Top").unwrap().gcode.lines().count());
        assert!(h.widget(&summary).is_some(), "{summary}");
        // The simulation cuts the stock, says what it removed and found, follows a changed
        // design while shown, and goes again on Hide.
        h.app.run("cam.setup", json!({"name": "Top", "stock": {"min": [-5, -5, -5], "max": [35, 15, 0]}})).unwrap();
        h.frames(2);
        h.step(&json!({"widget": "Simulate material removal"})).unwrap();
        h.frames(2);
        let removed = |h: &Harness| match view(h).and_then(|v| v.sim.as_ref()) {
            Some(Ok(sim)) => Ok((sim.removed, sim.indices.len())),
            Some(Err(e)) => Err(e.clone()),
            None => Err("not simulated".into()),
        };
        let (volume, triangles) = removed(&h).unwrap();
        assert!(volume > 0.0 && triangles > 0);
        assert!(h.widget("No rapid move goes through the stock and no cut reaches above the flutes.").is_some());
        h.app.run("cam.op.contour", json!({"setup": "Top", "name": "C", "tool": 1, "outer": [[0,0],[30,0],[30,10],[0,10]], "bottom": -3})).unwrap();
        h.frames(2);
        assert!(removed(&h).is_ok_and(|(v, _)| v > volume));
        h.step(&json!({"widget": "Hide simulation"})).unwrap();
        h.frames(2);
        assert!(view(&h).is_some_and(|v| v.sim.is_none()));
        // A long program edited by hand scrolls inside the panel, which stays on the screen.
        view_mut(&mut h).edited = Some(
            (1..=5000)
                .map(|n| {
                    format!(
                        "G1 X{n} Y0 F600
"
                    )
                })
                .collect(),
        );
        h.frames(4);
        let panel = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_cam_gcode"))).unwrap();
        assert!(panel.max.y <= h.ctx.content_rect().max.y + 1.0, "the panel runs off the screen: {panel:?}");
        view_mut(&mut h).edited = None;
        h.frames(2);
        // Only the small G-code panel is shown meanwhile; closing it brings the window back.
        assert!(h.widget("G-code: Top").is_some());
        assert!(h.widget("Show G-code").is_none());
        h.app.cam_window.as_mut().unwrap().gcode = None;
        h.frames(2);
        assert!(h.widget("Show G-code").is_some());
    }

    #[test]
    fn cam_posts_tab_edits_own_posts_and_a_machine_takes_its_post_into_the_design() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h);
        // The post list drops broken rows and built-in names, and keeps the rest by name.
        let mut own = solidcraft_engine::cam::Post::grbl();
        own.name = "Werkstatt".into();
        own.line_numbers = Some(solidcraft_engine::cam::LineNumbers { start: 100, step: 5 });
        let mut builtin_name = own.clone();
        builtin_name.name = "LinuxCNC".into();
        let mut broken = serde_json::to_value(&own).unwrap();
        broken["name"] = json!("Broken");
        broken["decimals"] = json!(9);
        h.app.preferences.cam_posts = serde_json::from_value(json!([own, builtin_name, broken])).unwrap();
        let listed: Vec<&str> = h.app.preferences.cam_posts.0.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(listed, ["Werkstatt"]);
        let list = &h.app.preferences.cam_posts;
        let builtin = Err(crate::i18n::tr("ui.cam.post_builtin_name").to_string());
        let taken = Err(crate::i18n::tr("ui.cam.post_exists").to_string());
        assert_eq!((list.name_free("grbl", None), list.name_free("Werkstatt", None)), (builtin, taken));
        assert_eq!((list.name_free("Werkstatt", Some("Werkstatt")), list.name_free("Fräse", None)), (Ok(()), Ok(())));
        // The tab lists it; its form shows every setting with the post's values.
        crate::cam_panel::open(&mut h.app, 3);
        h.frames(3);
        assert!(h.widget("New post").is_some() && h.widget("Start from").is_some());
        h.step(&json!({"widget": "Werkstatt – .nc"})).unwrap();
        h.frames(2);
        for label in
            ["File extension", "Line numbers (N)", "First line number", "Arcs as G2/G3", "Program start (safe start)", "Program end", "Save post"]
        {
            assert!(h.widget(label).is_some(), "{label} is shown");
        }
        // A machine names it, so its machine choice lists it; a setup copies both into the design.
        h.app.preferences.cam_machines = serde_json::from_value(json!([
            {"name": "Portal", "x": [0, 600], "y": [0, 400], "z": [-120, 0], "spindle_min": 3000, "spindle_max": 12000, "max_feed": 2000, "post": "Werkstatt"},
        ]))
        .unwrap();
        let portal = h.app.preferences.cam_machines.find("Portal").cloned();
        let mut setup = crate::dialogs_cam_setup::CamSetupForm::new(&h.app);
        setup.set_machine(&h.app, portal.clone());
        let p = setup.params(&crate::dialogs_cam_setup::inputs()).unwrap();
        h.app.run("cam.setup", p).unwrap();
        let cam = &h.app.session.doc.cam;
        assert_eq!((cam.machine("Portal"), cam.post("Werkstatt")), (portal.as_ref(), h.app.preferences.cam_posts.find("Werkstatt")));
        // The design's machine names it, so the design keeps it.
        assert!(h.app.run("cam.post.remove", json!({"name": "Werkstatt"})).is_err());
        // The parameters the tab sends to the design define the same post.
        let params = crate::cam_panel::post_params(h.app.preferences.cam_posts.find("Werkstatt").unwrap());
        let defined = h.app.run("cam.post.define", params).unwrap();
        assert_eq!(
            serde_json::from_value::<solidcraft_engine::cam::Post>(defined["post"].clone()).ok().as_ref(),
            h.app.session.doc.cam.post("Werkstatt")
        );
    }

    #[test]
    fn cam_machine_list_holds_cutting_data_and_the_form_sets_side_direction_and_stock() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h);
        // The machine list drops broken and duplicate rows and keeps the rest by name.
        h.app.preferences.cam_machines = serde_json::from_value(json!([
            {"name": "Portal", "x": [0, 600], "y": [0, 400], "z": [-120, 0], "a": {"kind": "endless"}, "spindle_min": 3000, "spindle_max": 12000, "max_feed": 2000, "post": "rs274"},
            {"name": "Portal", "x": [0, 1], "y": [0, 1], "z": [0, 1], "spindle_min": 1, "spindle_max": 2, "max_feed": 1},
            {"name": "Broken", "x": [5, 0], "y": [0, 1], "z": [0, 1], "spindle_min": 1, "spindle_max": 2, "max_feed": 1},
            {"name": "Mini", "x": [0, 200], "y": [0, 150], "z": [-60, 0], "spindle_min": 1000, "spindle_max": 24000, "max_feed": 3000},
        ]))
        .unwrap();
        let listed: Vec<&str> = h.app.preferences.cam_machines.0.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(listed, ["Mini", "Portal"]);
        h.step(&json!({"start":"cam.machine.define"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.cam_window.as_ref().map(|w| w.tab), Some(2));
        let portal = h.app.preferences.cam_machines.find("Portal").cloned();
        let label = crate::cam_panel::machine_label(portal.as_ref().unwrap());
        assert!(label.ends_with(", RS-274"), "{label}");
        // Its form offers the post-processor, the default first.
        h.step(&json!({"widget": label})).unwrap();
        h.frames(2);
        assert!(h.widget("Post-processor").is_some() && h.widget("RS-274").is_some());
        assert!(h.widget("Program folder").is_some(), "the machine says where its programs go");
        // Each machine is made once: a new one, or a renamed one, may not take an existing name.
        let list = &h.app.preferences.cam_machines;
        let taken = Err(crate::i18n::tr("ui.cam.machine_exists").to_string());
        assert_eq!((list.name_free("Portal", None), list.name_free("Portal", Some("Mini"))), (taken.clone(), taken));
        assert_eq!((list.name_free("Portal", Some("Portal")), list.name_free("Fräse", None)), (Ok(()), Ok(())));
        // The setup form copies the chosen machine into the design.
        let mut setup = crate::dialogs_cam_setup::CamSetupForm::new(&h.app);
        setup.set_machine(&h.app, portal.clone());
        let mut p = setup.params(&crate::dialogs_cam_setup::inputs()).unwrap();
        p["material"] = json!("wood");
        h.app.run("cam.setup", p).unwrap();
        let cam = &h.app.session.doc.cam;
        assert_eq!((cam.setups[0].machine.as_deref(), cam.machine("Portal")), (Some("Portal"), portal.as_ref()));
        // Cutting data are held to the machine: 15915 rev/min becomes 12000, the feed stops at 2000.
        h.app.preferences.cam_tools = serde_json::from_value(json!([
            {"number": 3, "kind": "flat_endmill", "diameter": 6, "flute_length": 20, "cutting": {"wood": {"vc": 300, "fz": 0.1, "ap": 0.5, "ae": 0.5}}},
        ]))
        .unwrap();
        h.step(&json!({"start":"cam.op.contour"})).unwrap();
        h.frames(3);
        let tool = h.app.preferences.cam_tools.find(3).cloned();
        let mut d = h.app.dialog.take().unwrap();
        let Kind::Milling(form) = &mut d.kind else { panic!("the contour form") };
        form.choose_tool(tool);
        form.take_cutting(&h.app).unwrap();
        // An inside contour, milled conventionally, leaving 0.5 mm on the wall and 0.2 mm on the floor.
        form.set_cutting(2, false, "0.5", "0.2");
        let p = form.params().unwrap();
        assert_eq!((p["spindle_rpm"].as_f64(), p["feed"].as_f64()), (Some(12000.0), Some(2000.0)));
        assert_eq!(
            (p["side"].as_str(), p["climb"].as_bool(), p["stock_to_leave"].as_f64(), p["floor_stock"].as_f64()),
            (Some("inside"), Some(false), Some(0.5), Some(0.2))
        );
        h.app.dialog = Some(d);
        h.frames(2);
        // The form shows the choices.
        for label in ["Inside the contour", "Conventional", "Wall stock to leave (mm)", "Floor stock to leave (mm)", "Holding tabs"] {
            assert!(h.widget(label).is_some(), "{label} is shown");
        }
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let setup = h.app.session.doc.cam.setups[0].clone();
        let op = setup.operations.first().cloned().unwrap();
        let solidcraft_engine::doc::cam::CamOperation::Contour { params, .. } = &op else { panic!("a contour") };
        assert_eq!((params.side, params.climb), (solidcraft_engine::doc::cam::CamSide::Inside, false));
        // Editing brings the choices back unchanged.
        let again = crate::dialogs_milling::Milling::edit(&h.app, &setup, &op).params().unwrap();
        assert_eq!(
            (again["side"].as_str(), again["climb"].as_bool(), again["stock_to_leave"].as_f64(), again["floor_stock"].as_f64()),
            (Some("inside"), Some(false), Some(0.5), Some(0.2))
        );
        let edited = crate::dialogs_cam_setup::CamSetupForm::edit(&h.app, &setup).params(&crate::dialogs_cam_setup::inputs()).unwrap();
        assert_eq!(edited["machine"]["name"].as_str(), Some("Portal"));
    }

    #[test]
    fn cam_form_starts_at_the_stock_top_takes_shares_of_the_diameter_and_faces_the_stock() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h);
        // Part zero on the model's top, 2 mm of stock above it and 3 mm at the sides.
        let stock =
            json!({"kind": "relative_box", "mode": "sides_and_top_bottom", "side_offset": 3, "top_offset": 2, "bottom_offset": 0, "round_up": 0});
        h.app
            .run(
                "cam.setup",
                json!({"name": "Top", "wcs_origin_mode": "model_box_point",
                "box_point": {"x": "negative", "y": "negative", "z": "positive"}, "stock_spec": stock}),
            )
            .unwrap();
        assert_eq!(crate::dialogs_milling::stock_top(&h.app.session, "Top"), Some(2.0));
        // A new step starts at the stock's top; the retract height moves above it.
        h.step(&json!({"start":"cam.op.contour"})).unwrap();
        h.frames(2);
        let Some(Kind::Milling(form)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the contour form") };
        let p = form.params().unwrap();
        // Stepdown 100 % and stepover 50 % of the 6 mm cutter.
        assert_eq!(
            (p["top"].as_f64(), p["retract"].as_f64(), p["stepdown"].as_f64(), p["stepover"].as_f64()),
            (Some(2.0), Some(4.0), Some(6.0), Some(3.0))
        );
        assert!(h.widget("Start depth").is_some() && h.widget("Start depth Z (mm)").is_some());
        h.app.dialog = None;
        // Facing: the whole stock down to part zero, the 2 mm the stock has above it taken off; the
        // start and safety heights follow from the finished height and the allowance.
        h.step(&json!({"start":"cam.op.face"})).unwrap();
        h.frames(2);
        let Some(Kind::Milling(form)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the facing form") };
        let p = form.params().unwrap();
        assert_eq!(
            (p["kind"].as_str(), p["top"].as_f64(), p["bottom"].as_f64(), p["retract"].as_f64(), p["clearance"].as_f64(), p["overrun"].as_f64()),
            (Some("face"), Some(2.0), Some(0.0), Some(4.0), Some(12.0), Some(2.0))
        );
        assert!(p.get("allowance").is_none());
        assert!(h.widget("Finished height Z (mm)").is_some() && h.widget("Allowance Z (mm)").is_some() && h.widget("Overrun (mm)").is_some());
        assert!(h.widget("Start depth Z (mm)").is_none() && h.widget("Retract Z (mm)").is_none());
        assert!(h.widget("Wall stock to leave (mm)").is_none());
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let setup = h.app.session.doc.cam.setup("Top").cloned().unwrap();
        let op = setup.operations.first().cloned().unwrap();
        assert!(matches!(&op, solidcraft_engine::doc::cam::CamOperation::Face { .. }), "{op:?}");
        let paths = h.app.session.cam_toolpaths();
        assert!(paths.first().is_some_and(|p| p.points.as_ref().is_ok_and(|v| v.iter().any(|(_, c)| *c))));
        // Editing brings it back as facing.
        let again = crate::dialogs_milling::Milling::edit(&h.app, &setup, &op).params().unwrap();
        assert_eq!((again["kind"].as_str(), again["overrun"].as_f64(), again["top"].as_f64()), (Some("face"), Some(2.0), Some(2.0)));
    }

    #[test]
    fn cam_facing_form_has_no_entry_point_input() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h);
        let stock =
            json!({"kind": "relative_box", "mode": "sides_and_top_bottom", "side_offset": 3, "top_offset": 2, "bottom_offset": 0, "round_up": 0});
        h.app
            .run(
                "cam.setup",
                json!({"name": "Top", "wcs_origin_mode": "model_box_point",
            "box_point": {"x": "negative", "y": "negative", "z": "positive"}, "stock_spec": stock}),
            )
            .unwrap();
        h.step(&json!({"start":"cam.op.face"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        d.active = 4;
        h.app.dialog = Some(d);
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap();
        assert_ne!(d.active, 4, "facing must not allow activating entry point input");
        let Kind::Milling(form) = &d.kind else { panic!("the facing form") };
        assert_eq!(form.entry, None);
        assert!(form.params().unwrap().get("entry").is_none());
    }

    #[test]
    fn cam_adaptive_form_edit_preserves_adaptive_kind_and_entry() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h); // a 10 × 10 × 5 plate at the model's corner
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.point", json!({"point": [16, 5], "id": "enter"})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.step(&json!({"start":"cam.op.adaptive"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        d.active = 4;
        d.pick(&h.app.session, Sel::ExternalSketchPoint { sketch, id: "enter".into() });
        assert_eq!(d.inputs[4].items.len(), 1);
        h.app.dialog = Some(d);
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.cam.setups[0].operations.len(), 1);
        let setup = &h.app.session.doc.cam.setups[0];
        let op = &setup.operations[0];
        // Now edit this step
        let form = crate::dialogs_milling::Milling::edit(&h.app, setup, op);
        assert_eq!(form.kind, crate::dialogs_milling::ADAPTIVE, "must be ADAPTIVE kind (12), not Trochoidal Slot (11)");
        assert!(form.adaptive, "adaptive flag must be true");
        assert_eq!(form.entry, Some([16.0, 5.0, 0.0]), "entry point must be retained");
        assert!(form.is_editing(), "form must report editing");

        // Verify title when editing
        let edit_dialog = crate::dialogs::Dialog::new(crate::dialogs::Kind::Milling(Box::new(form)), crate::dialogs_milling::inputs());
        h.app.dialog = Some(edit_dialog);
        h.frames(2);
        assert!(h.widget("EDIT ADAPTIVE CLEARING").is_some(), "dialog title must be EDIT ADAPTIVE CLEARING");
    }

    #[test]
    fn cam_adaptive_form_enters_at_a_picked_point_and_gives_up_the_direction() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h); // a 10 × 10 × 5 plate at the model's corner
        // A sketch point in the air beside the plate, for the entry pick.
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.point", json!({"point": [16, 5], "id": "enter"})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.step(&json!({"start":"cam.op.adaptive"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        d.active = 4;
        d.pick(&h.app.session, Sel::ExternalSketchPoint { sketch, id: "enter".into() });
        assert_eq!(d.inputs[4].items.len(), 1);
        h.app.dialog = Some(d);
        h.frames(2);
        let Some(Kind::Milling(form)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the adaptive form") };
        // The entry travels with the parameters, and the way each pass feeds follows from
        // it: every pass climbs, so the form stops asking for a direction.
        let p = form.params().unwrap();
        assert_eq!(p["entry"].as_array().map(|v| (v[0].as_f64(), v[1].as_f64())), Some((Some(16.0), Some(5.0))), "{p}");
        assert_eq!(p["climb"].as_bool(), Some(true), "{p}");
        assert_eq!(p["adaptive"].as_bool(), Some(true), "{p}");
        assert!(h.widget("Entry point").is_some());
        assert!(h.widget("Direction").is_none(), "the open side fixes the direction");
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.doc.cam.setups[0].operations.len(), 1);
        let op = &h.app.session.doc.cam.setups[0].operations[0];
        if let solidcraft_engine::doc::cam::CamOperation::Pocket { params, .. } = op {
            assert!(params.adaptive, "must be adaptive");
            assert_eq!(params.entry, Some([16.0, 5.0, 0.0]), "entry point must be saved with operation");
        } else {
            panic!("expected pocket operation");
        }
    }

    /// The Entry point pick reaches the form only if the viewport offers it a hit: a sketch
    /// point of another sketch is hit-tested only while the active input accepts POINTS
    /// (`viewport::pick_hits`), while EXTERNAL_POINTS alone only maps a hit that never came.
    /// Found by hand: a visible sketch point outside the faced area was not clickable.
    #[test]
    fn cam_entry_point_input_enables_cross_sketch_picking() {
        let inputs = crate::dialogs_milling::inputs();
        let entry = inputs.iter().find(|i| i.label == "Entry point").expect("the Entry point input");
        assert_ne!(entry.accept & crate::selection::POINTS, 0, "POINTS is what lets the viewport hit-test other sketches' points");
        assert_ne!(entry.accept & crate::selection::EXTERNAL_POINTS, 0, "EXTERNAL_POINTS keeps the pick's sketch and point id");
    }

    #[test]
    fn cam_contour_pick_of_a_round_rim_counts_once() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        // The real sample plate: its holes are round rims of two halves.
        let script = solidcraft_engine::sample::script();
        for c in script["commands"].as_array().unwrap() {
            h.app.run(c["command"].as_str().unwrap(), c["params"].clone()).unwrap();
        }
        h.app.run("cam.setup", json!({"name": "Top"})).unwrap();
        h.step(&json!({"start":"cam.op.contour"})).unwrap();
        h.frames(2);
        // A round rim of the sample: one contour, whether the kernel gives it as one edge or
        // as halves (every piece of the rim is picked with it).
        let st = h.app.session.world_state().unwrap();
        let body = st.bodies.first().unwrap();
        let mesh = body.mesh();
        let index = mesh
            .edges
            .iter()
            .position(|e| e.len() > 3 && e.iter().all(|p| (p.z - 8.0).abs() < 1e-6 && (p.x - 12.0).hypot(p.y - 25.0) < 6.0))
            .unwrap();
        let edge = &mesh.edges[index];
        let item = Sel::Edge { body: body.name.clone(), index, point: edge[edge.len() / 2] };
        let mut d = h.app.dialog.take().unwrap();
        d.pick(&h.app.session, item);
        assert_eq!(d.inputs[0].items.len(), mesh.circular_rim(index).len());
        assert_eq!(crate::dialogs_milling::pick_count(&h.app.session, &d.inputs[0].items), 1);
        h.app.dialog = Some(d);
        h.frames(2);
        // Side and direction are offered for the contour.
        assert!(h.widget("Automatic").is_some() && h.widget("Climb").is_some());
    }

    #[test]
    fn cam_setup_form_puts_part_zero_on_the_model_and_the_job_list_edits_it() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h);
        let body = h.app.session.world_state().unwrap().bodies.first().map(|b| b.name.clone()).unwrap();
        // Unpicked, a new setup's part zero is the top corner of the model's box at low X and Y.
        h.step(&json!({"start":"cam.setup"})).unwrap();
        h.frames(3);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::CamSetup(_))));
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let first = h.app.session.doc.cam.setups.first().cloned().unwrap();
        assert_eq!((first.name.as_str(), first.work_origin), ("Setup 1", [0.0, 0.0, 5.0]));
        // A picked final depth is a height in the setup's frame: the box's bottom is 5 below zero.
        let floor = solidcore_record::Vec3::new(3.0, 3.0, 0.0);
        assert_eq!(crate::dialogs_milling::work_z(&h.app.session, "Setup 1", floor), -5.0);
        assert_eq!(crate::dialogs_milling::work_z(&h.app.session, "Not yet", floor), 0.0);
        assert!(h.app.session.cam_setup_outline("Setup 1").is_some_and(|o| o.stock.is_some()));
        // A picked straight edge puts part zero at its middle.
        h.step(&json!({"start":"cam.setup"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        let Kind::CamSetup(form) = &mut d.kind else { panic!("the setup form") };
        form.zero = 0;
        // Without a pick a new setup has no part zero to take.
        assert!(form.params(&d.inputs).is_err());
        let edge = Sel::Edge { body: body.clone(), index: 0, point: solidcore_record::Vec3::new(5.0, 0.0, 5.0) };
        d.inputs.first_mut().unwrap().items = vec![edge];
        let p = form.params(&d.inputs).unwrap();
        assert_eq!(p["zero_edge"]["body"].as_str(), Some(body.as_str()));
        h.app.run("cam.setup", p).unwrap();
        let second = h.app.session.doc.cam.setup("Setup 2").cloned().unwrap();
        assert!(second.work_origin.iter().zip([5.0, 0.0, 5.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{:?}", second.work_origin);
        // Zero axis by axis: X from a corner, Z from the top face; Y from the model's box (low Y).
        h.step(&json!({"start":"cam.setup"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        let Kind::CamSetup(form) = &mut d.kind else { panic!("the setup form") };
        form.zero = 1;
        assert_eq!(form.params(&d.inputs).ok().and_then(|p| p["wcs_origin_mode"].as_str().map(str::to_owned)).as_deref(), Some("model_box_point"));
        let at = solidcore_record::Vec3::new;
        d.inputs[1].items = vec![Sel::Vertex { body: body.clone(), point: at(10.0, 0.0, 0.0) }];
        d.inputs[3].items = vec![Sel::Face { body: body.clone(), index: 0, point: at(5.0, 5.0, 5.0) }];
        let p = form.params(&d.inputs).unwrap();
        assert!(p["zero_axes"].get("y").is_none());
        h.app.run("cam.setup", p).unwrap();
        assert_eq!(h.app.session.doc.cam.setup("Setup 3").map(|s| s.work_origin), Some([10.0, 0.0, 5.0]));
        // Editing a setup shows its name; a changed name renames it and keeps its part zero.
        let third = h.app.session.doc.cam.setup("Setup 3").cloned().unwrap();
        let mut form = crate::dialogs_cam_setup::CamSetupForm::edit(&h.app, &third);
        form.name = "Clamped on the side".into();
        let p = form.params(&crate::dialogs_cam_setup::inputs()).unwrap();
        assert_eq!((p["name"].as_str(), p["rename"].as_str()), (Some("Setup 3"), Some("Clamped on the side")));
        h.app.run("cam.setup", p).unwrap();
        assert!(h.app.session.doc.cam.setup("Setup 3").is_none());
        assert_eq!(h.app.session.doc.cam.setup("Clamped on the side").map(|s| s.work_origin), Some([10.0, 0.0, 5.0]));
        // The job list offers each setup for editing and deleting.
        h.step(&json!({"start":"cam.list"})).unwrap();
        h.frames(3);
        assert!(h.widget("Edit").is_some() && h.widget("Delete").is_some());
    }

    #[test]
    fn cam_parts_group_setups_and_a_plasma_setup_takes_only_plasma_tables_posts_and_cuts() {
        use solidcraft_engine::doc::cam::{CamOperation, SetupOperationType};
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":1100}));
        sample(&mut h);
        let table = solidcraft_engine::cam::Machine::plasma("Table");
        h.app.preferences.cam_machines = serde_json::from_value(json!([
            {"name": "Portal", "x": [0, 600], "y": [0, 400], "z": [-120, 0], "spindle_min": 3000, "spindle_max": 12000, "max_feed": 2000},
            table,
        ]))
        .unwrap();
        let portal = h.app.preferences.cam_machines.find("Portal").cloned();
        // A plasma setup of part "Bracket": a mill picked for it is dropped as the form is shown.
        h.step(&json!({"start":"cam.setup"})).unwrap();
        h.frames(2);
        let mut d = h.app.dialog.take().unwrap();
        let Kind::CamSetup(form) = &mut d.kind else { panic!("the setup form") };
        form.kind = Some(1);
        form.part = "Bracket".into();
        form.set_machine(&h.app, portal.clone());
        h.app.dialog = Some(d);
        h.frames(2);
        assert!(h.widget("Plasma cutting").is_some() && h.widget("Part").is_some());
        let mut d = h.app.dialog.take().unwrap();
        let Kind::CamSetup(form) = &mut d.kind else { panic!("the setup form") };
        assert_eq!(form.operation_type(&h.app), SetupOperationType::Plasma);
        let mut p = form.params(&d.inputs).unwrap();
        assert!(p["machine"].is_null(), "a mill does not run a plasma setup");
        form.set_machine(&h.app, h.app.preferences.cam_machines.find("Table").cloned());
        p = form.params(&d.inputs).unwrap();
        assert_eq!((p["part"].as_str(), p["operation_type"].as_str()), (Some("Bracket"), Some("plasma")));
        h.app.run("cam.setup", p).unwrap();
        let cut_setup = h.app.session.doc.cam.setups[0].clone();
        assert_eq!((cut_setup.machine.as_deref(), cut_setup.part.as_deref()), (Some("Table"), Some("Bracket")));
        assert_eq!(crate::cam_panel::setup_post(&h.app.session.doc.cam, &cut_setup), "plasma");
        // Only posts for plasma tables are offered; a milling setup of the same part takes milling posts.
        let plasma_posts: Vec<String> = crate::cam_panel::posts_for(&h.app, solidcraft_engine::cam::MachineKind::Plasma).into_iter().map(|(n, _)| n).collect();
        assert_eq!(plasma_posts, ["plasma"]);
        assert!(crate::cam_panel::posts_for(&h.app, solidcraft_engine::cam::MachineKind::Mill).iter().all(|(n, _)| n != "plasma"));
        // A plasma cut goes into the plasma setup with the table's torch data and no tool.
        let form = crate::dialogs_milling::Milling::for_operation(&h.app, "cam.op.plasma");
        let p = form.params().unwrap();
        assert_eq!((p["kind"].as_str(), p["setup"].as_str(), p["kerf"].as_f64()), (Some("plasma"), Some(cut_setup.name.as_str()), Some(1.5)));
        assert!(p.get("material").is_none());
        h.app.run("cam.job.create", p).unwrap();
        let cut = h.app.session.doc.cam.setups[0].operations.first().cloned().unwrap();
        assert!(matches!(cut, CamOperation::Plasma { .. }) && h.app.session.doc.cam.tools.is_empty());
        let again = crate::dialogs_milling::Milling::edit(&h.app, &h.app.session.doc.cam.setups[0], &cut).params().unwrap();
        assert_eq!((again["kind"].as_str(), again["pierce_delay"].as_f64()), (Some("plasma"), Some(0.5)));
        // A contour is not offered the plasma setup: it starts a new one.
        let contour = crate::dialogs_milling::Milling::for_operation(&h.app, "cam.op.contour").params().unwrap();
        assert_ne!(contour["setup"].as_str(), Some(cut_setup.name.as_str()));
        // The job list heads each part with its sheets, and each setup has its own sheet.
        h.app.run("cam.setup", json!({"name": "Drill first", "part": "Bracket"})).unwrap();
        h.step(&json!({"start":"cam.list"})).unwrap();
        h.frames(3);
        for label in ["Part: Bracket", "Sheets for the part", "Setup sheet"] {
            assert!(h.widget(label).is_some(), "{label} is shown");
        }
        assert_eq!(crate::cam_panel::parts(&h.app.session.doc.cam), [Some("Bracket".to_string())]);
        // A closed eye hides the setup's part zero with its toolpaths; the others stay.
        assert!(crate::cam_panel::setup_shown(&h.app, &cut_setup.name) && crate::cam_panel::setup_shown(&h.app, "Drill first"));
        h.app.cam_window.as_mut().unwrap().hidden.push(cut_setup.name.clone());
        assert!(!crate::cam_panel::setup_shown(&h.app, &cut_setup.name) && crate::cam_panel::setup_shown(&h.app, "Drill first"));
        // The Manual tab reads the CAM manual, and its fields explain themselves in its words.
        h.app.cam_window.as_mut().unwrap().tab = 4;
        h.frames(3);
        for label in ["How CAM is organised", "Parts", "Plasma cutting", "Fields"] {
            assert!(h.widget(label).is_some(), "{label} is shown");
        }
    }

    #[test]
    fn cam_forms_show_their_texts_in_full() {
        fn elided(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) if t.galley.elided => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| elided(s, out)),
                _ => {}
            }
        }
        // The CAM texts (the CAM catalog's, not the rest of the screen's) of every CAM form and the
        // CAM window, in every complete language, at two sizes.
        let catalog: serde_json::Value = serde_json::from_str(include_str!("../../../assets/locales/pending/cam-linuxcnc.json")).unwrap();
        let mut cut = Vec::new();
        for (w, ht, language) in [
            (1400.0, 1000.0, "en"),
            (1400.0, 1000.0, "de"),
            (1920.0, 1080.0, "de"),
            (1400.0, 1000.0, "ja"),
            (1400.0, 1000.0, "zh"),
            (1400.0, 1000.0, "ar"),
            (1400.0, 1000.0, "he"),
            (1400.0, 1000.0, "vi"),
        ] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":w,"height":ht}));
            h.call("ui.prefs", json!({"language":language}));
            sample(&mut h);
            h.app.run("cam.setup", json!({"name": "Setup A"})).unwrap();
            let mut show = |h: &mut Harness, what: &str| {
                h.frames(4);
                let mut output =
                    h.ctx.run_ui(egui::RawInput { screen_rect: Some(h.ctx.content_rect()), focused: true, ..Default::default() }, |ui| {
                        h.app.logic(ui.ctx());
                        h.app.ui(ui);
                    });
                output.textures_delta.clear();
                let mut out = Vec::new();
                output.shapes.iter().for_each(|s| elided(&s.shape, &mut out));
                let ours = catalog[language].as_object().unwrap();
                out.retain(|text| ours.values().any(|v| v.as_str() == Some(text.as_str())));
                if !out.is_empty() {
                    cut.push(format!("{language} {w}: {what} cuts off {out:?}"));
                }
            };
            for cmd in ["cam.setup", "cam.op.contour", "cam.op.pocket", "cam.op.drill", "cam.op.face", "cam.op.slot"] {
                h.app.dialog = None;
                h.step(&json!({"start":cmd})).unwrap();
                show(&mut h, cmd);
                if cmd == "cam.setup" {
                    if let Some(Kind::CamSetup(form)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
                        form.zero = 1;
                    }
                    show(&mut h, "setup per axis");
                }
            }
            h.app.dialog = None;
            for tab in 0..3 {
                crate::cam_panel::open(&mut h.app, tab);
                show(&mut h, &format!("panel {tab}"));
            }
        }
        assert!(cut.is_empty(), "{cut:#?}");
    }

    fn sample(h: &mut Harness) {
        h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":"5 mm"})).unwrap();
    }

    #[test]
    fn common_forms_fit_both_layouts_and_scales_and_pin_footer_at_small_heights() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for scale in [1.0, 2.0] {
                for height in [950.0, 720.0] {
                    let mut h = Harness::new();
                    h.call("ui.set", json!({"layout": mode}));
                    h.app.preferences.ui_scale = scale;
                    h.call("ui.resize", json!({"width": 1400.0 / scale, "height": height / scale}));
                    sample(&mut h);
                    for command in COMMANDS {
                        h.step(&json!({"start":command})).unwrap();
                        // Force the long Hole variant too: counterbore, tapping and multiple picks.
                        if let Some(Dialog { kind: Kind::Hole { kind, opts, .. }, .. }) = h.app.dialog.as_mut() {
                            *kind = 1;
                            opts.tapped = true;
                            opts.multiple = true;
                        }
                        h.frames(8);
                        let card = scenario::rect_of("dialog:card").unwrap();
                        let viewport = h.app.viewport.rect.unwrap();
                        assert!(
                            viewport.expand(1.0).contains_rect(card),
                            "{command} {mode:?} scale={scale} height={height}: {card:?} outside {viewport:?}"
                        );
                        for action in ["dialog:ok", "dialog:cancel"] {
                            let rect = scenario::rect_of(action).unwrap();
                            assert!(
                                card.contains_rect(rect),
                                "{command} {mode:?} scale={scale} height={height}: clipped {action} {rect:?} card={card:?}"
                            );
                        }
                        assert!(crate::shell::covered_rects(&h.app).contains(&card));
                        assert!(card.width() <= 301.0, "{card:?}");
                        h.step(&json!({"widget":"Cancel"})).unwrap();
                        assert!(h.app.dialog.is_none(), "{command}");
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_windows_do_not_panic_or_publish_non_finite_cards() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for size in [(1.0, 1.0), (40.0, 80.0), (180.0, 240.0)] {
                let mut h = Harness::new();
                h.call("ui.set", json!({"layout": mode}));
                sample(&mut h);
                h.call("ui.resize", json!({"width":size.0,"height":size.1}));
                for command in COMMANDS {
                    h.app.start(command);
                    assert!(h.app.dialog.is_some(), "{command} must actually open");
                    h.frames(3);
                    if let Some(rect) = h.app.viewport.dialog_rect {
                        assert!(rect.is_finite() && rect.width() >= 0.0 && rect.height() >= 0.0, "{command}: {rect:?}");
                    }
                    cancel(&mut h.app);
                }
            }
        }
    }

    #[test]
    fn fillet_options_keep_the_existing_command_parameters() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        choose(&mut h, "Type", "ui.fillet_type_curvature");
        assert_eq!(h.app.dialog.as_ref().unwrap().extra.get("type"), Some(&json!("curvature")));
        h.step(&json!({"click":{"handle":"dialog:Tangent Chain"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::Fillet { chain: false, .. }));
        h.step(&json!({"click":{"handle":"dialog:section:Options"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Tangent Chain").is_none());
        h.step(&json!({"click":{"handle":"dialog:section:Options"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Tangent Chain").is_some());
    }
    fn fillet_harness() -> Harness {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        let edge = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 5.0)).unwrap();
        h.app.session.selection = vec![edge];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        h
    }

    fn parameters(h: &Harness) -> Value {
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands.first().unwrap().0, "solid.fillet");
        commands.first().unwrap().1.clone()
    }

    fn choose(h: &mut Harness, row: &str, option: &str) {
        h.step(&json!({"click":{"handle":format!("dialog:{row}")}})).unwrap();
        h.step(&json!({"click":{"handle":format!("dialog:{row}:{option}")}})).unwrap();
        h.frames(2);
    }

    #[test]
    fn fillet_and_chamfer_name_the_body_of_their_edges() {
        // Named, the feature looks for its edges on that body alone instead of meshing every
        // body of the design on each preview and apply.
        let h = fillet_harness();
        let body = h.app.session.world_state().unwrap().bodies.first().unwrap().name.clone();
        let p = parameters(&h);
        assert_eq!(p["body"], json!(body), "{p}");
        // Edges of two bodies name none: each point finds its own.
        let mut h = Harness::new();
        sample(&mut h);
        h.app.run("solid.box", json!({"length":4,"width":4,"height":4,"corner":[20,0,0],"operation":"new"})).unwrap();
        let a = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 5.0)).unwrap();
        let b = edge_sel(&h.app.session, Vec3::new(22.0, 0.0, 4.0)).unwrap();
        h.app.session.selection = vec![a, b];
        h.step(&json!({"start":"solid.chamfer"})).unwrap();
        h.frames(4);
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let (cmd, p) = commands.first().unwrap();
        assert_eq!(*cmd, "solid.chamfer");
        assert!(p.get("body").is_none(), "{p}");
    }

    #[test]
    fn press_pull_on_a_bore_inside_another_part_moves_that_bore() {
        // A pin standing inside a hollow housing that comes first: by its point alone the face
        // was looked for on the housing, whose box holds it, and the command failed with
        // "no face at …". The dialog now names the pin.
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":100,"width":100,"height":100,"body_name":"Housing"})).unwrap();
        h.app.run("solid.box", json!({"length":80,"width":80,"height":100,"corner":[10,10,10],"operation":"cut","targets":["Housing"]})).unwrap();
        h.app.run("solid.cylinder", json!({"base":[50,50,20],"radius":5,"height":20,"operation":"new","body_name":"Pin"})).unwrap();
        let face = face_sel(&h.app.session, Vec3::new(55.0, 50.0, 30.0)).unwrap();
        assert!(matches!(&face, Sel::Face { body, .. } if body == "Pin"), "{face:?}");
        h.app.session.selection = vec![face];
        h.step(&json!({"start":"solid.press_pull"})).unwrap();
        h.frames(4);
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let (cmd, mut p) = commands.first().cloned().unwrap();
        assert_eq!(cmd, "solid.offset_face");
        assert_eq!(p["body"], json!("Pin"), "{p}");
        p["distance"] = json!(1);
        h.app.run(&cmd, p).unwrap();
        let st = h.app.session.world_state().unwrap();
        let pin = st.body("Pin").unwrap().measure().unwrap();
        let expected = std::f64::consts::PI * 36.0 * 20.0;
        assert!((pin.volume - expected).abs() < expected * 1e-3, "pin volume {} (expected {expected})", pin.volume);
        let housing = st.body("Housing").unwrap().measure().unwrap();
        assert!((housing.volume - (1e6 - 80.0 * 80.0 * 90.0)).abs() < 1.0, "the housing is unchanged: {}", housing.volume);
    }

    #[test]
    fn every_type_choice_produces_its_supported_command_without_changing_defaults() {
        for (label, key) in FILLET_TYPES {
            let mut h = fillet_harness();
            let original = parameters(&h);
            h.frames(3);
            assert_eq!(parameters(&h), original, "merely drawing the default form is lossless");
            choose(&mut h, "Type", label);
            let p = parameters(&h);
            assert_eq!(p.get("type").and_then(Value::as_str).unwrap_or("constant"), key, "{p}");
            assert_eq!(p["radius"], original["radius"]);
            assert_eq!(p["edges"], original["edges"]);
            assert!(p.get("preview").is_none() && p.get("preview_enabled").is_none());
            if key == "variable" {
                assert_eq!(p["radius2"], json!("1 mm"));
                assert!(p["start"].is_array());
            }
            if key == "spline" {
                assert_eq!(p["radii"].as_array().unwrap().len(), 4);
                assert_eq!(p["knots"].as_array().unwrap().len(), 9);
                assert!(p["start"].is_array());
            }
            // Type is the one place the section is chosen: no second Continuity control.
            assert!(scenario::handle_at("dialog:Continuity").is_none());
        }
    }

    #[test]
    fn if_too_large_maps_to_fit() {
        let mut h = fillet_harness();
        assert!(scenario::handle_at("dialog:section:Advanced").is_none(), "no Advanced section: its one row sits with Type");
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_reduce");
        assert_eq!(parameters(&h)["fit"], json!("reduce"));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_taper");
        assert_eq!(parameters(&h)["fit"], json!("taper"));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_exact");
        assert!(parameters(&h).get("fit").is_none());
        choose(&mut h, "Type", "Chord length");
        assert!(scenario::handle_at("dialog:ui.fillet_fit").is_none(), "a fitted radius is for Constant radius only");
    }

    /// Turns sends `turn: swept` with a Constant radius only, and a choice it rules out (a
    /// narrowed radius, a curvature corner) puts the other back: Swept and those never go
    /// together.
    #[test]
    fn turns_maps_to_turn_and_rules_out_what_a_swept_corner_does_not_take() {
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        assert!(scenario::handle_at("dialog:ui.fillet_turn").is_some());
        assert!(parameters(&h).get("turn").is_none(), "Mitre is the default, not sent");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        assert_eq!(parameters(&h)["turn"], json!("swept"));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_reduce");
        assert_eq!((parameters(&h)["turn"].clone(), parameters(&h)["fit"].clone()), (json!("swept"), json!("reduce")));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_taper");
        assert!(parameters(&h).get("turn").is_none(), "a narrowed radius puts Turns back to Mitre");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        assert!(parameters(&h).get("fit").is_none(), "Swept puts Face limits back to Exact radius");
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        assert!(parameters(&h).get("turn").is_none(), "a curvature corner puts Turns back to Mitre");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        assert!(parameters(&h).get("corner").is_none(), "Swept puts Corners back to Tangent");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_mitre");
        assert!(parameters(&h).get("turn").is_none(), "Mitre is not sent");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        choose(&mut h, "Type", "Chord length");
        assert!(parameters(&h).get("turn").is_none() && scenario::handle_at("dialog:ui.fillet_turn").is_none(), "Turns is for Constant radius only");
    }

    /// The dialog follows the option model (`fillet_option_applies`): through every type, fit and
    /// corner choice, in either order, each option it shows is one the others allow, and the
    /// command it builds is never refused for its options (`solid.fillet` runs it on a copy of
    /// the design). A choice that rules another out resets that one, visibly.
    #[test]
    fn no_reachable_fillet_option_state_is_refused_for_its_options() {
        let refused_for_options = |e: &str| {
            e.contains("take a constant radius")
                || e.contains("is built for")
                || e.contains("are built for")
                || e.contains(solidcraft_engine::doc::SWEPT_TAKES)
        };
        // Each distinct command is run once.
        let ran = std::cell::RefCell::new(std::collections::BTreeSet::new());
        let check = |h: &Harness, what: &str| {
            let p = parameters(h);
            let constant = p.get("type").is_none_or(|t| t == "constant");
            let exact = p.get("fit").is_none_or(|f| f == "exact");
            assert!(constant || p.get("fit").is_none(), "{what}: fit without Constant radius: {p}");
            assert!(constant && exact || (p.get("corner").is_none() && p.get("setbacks").is_none()), "{what}: a curvature corner with {p}");
            assert_eq!(scenario::handle_at("dialog:ui.fillet_fit").is_some(), constant, "{what}: Face limits shows exactly with Constant radius");
            assert_eq!(scenario::handle_at("dialog:ui.corners").is_some(), constant, "{what}: Corners shows exactly with Constant radius");
            if !ran.borrow_mut().insert(p.to_string()) {
                return;
            }
            let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
            if let Err(e) = copy.execute("solid.fillet", &p) {
                assert!(!refused_for_options(&e.to_string()), "{what}: {p} refused for its options: {e}");
            }
        };
        for (label, _) in FILLET_TYPES {
            for (fit, _) in FILLET_FITS {
                for corner_first in [false, true] {
                    let mut h = fillet_harness();
                    set_edges(&mut h, 3);
                    let what = format!("{label}, {fit}, corner first: {corner_first}");
                    if corner_first {
                        choose(&mut h, "ui.corners", Continuity::G2.label_id());
                        check(&h, &what);
                        choose(&mut h, "ui.fillet_fit", fit);
                        if fit != "ui.fillet_fit_exact" {
                            assert!(parameters(&h).get("corner").is_none(), "{what}: a fitted radius puts Corners back to Tangent");
                        }
                    } else {
                        choose(&mut h, "ui.fillet_fit", fit);
                        check(&h, &what);
                        choose(&mut h, "ui.corners", Continuity::G2.label_id());
                        assert!(parameters(&h).get("fit").is_none(), "{what}: a curvature corner puts Face limits back to Exact radius");
                    }
                    check(&h, &what);
                    choose(&mut h, "Type", label);
                    check(&h, &what);
                }
            }
        }
    }

    /// The three edges of the sample's corner at (10, 0, 5): top front, top right and upright.
    fn corner_edges(h: &Harness, count: usize) -> Vec<Sel> {
        [Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(10.0, 0.0, 2.5)]
            .into_iter()
            .take(count)
            .map(|p| edge_sel(&h.app.session, p).unwrap())
            .collect()
    }

    fn set_edges(h: &mut Harness, count: usize) {
        let edges = corner_edges(h, count);
        h.app.dialog.as_mut().unwrap().inputs.first_mut().unwrap().items = edges;
        h.frames(3);
    }

    /// Corners shows only once three rounded edges meet at a corner; Curvature (a smooth patch) writes
    /// `corner: g2`, its Setback (untouched: the command's 1.5 × radius) goes to that corner as
    /// `setbacks`, and Tangent (a rolling ball) writes neither.
    #[test]
    fn corners_row_follows_the_selection_and_maps_to_corner_and_setbacks() {
        let mut h = fillet_harness();
        for count in [1, 2] {
            set_edges(&mut h, count);
            assert!(scenario::handle_at("dialog:ui.corners").is_none(), "{count} edges: no corner where three rounded edges meet");
        }
        set_edges(&mut h, 3);
        let corners = fillet_corners(&h.app.session, &h.app.dialog.as_ref().unwrap().inputs[0].items);
        assert_eq!(corners.len(), 1, "{corners:?}");
        assert!(corners[0].dist(Vec3::new(10.0, 0.0, 5.0)) < 1e-6, "{corners:?}");
        assert!(scenario::handle_at("dialog:ui.corners").is_some(), "three rounded edges meet at (10, 0, 5)");
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_none(), "Setback goes with a smooth patch");
        assert!(parameters(&h).get("corner").is_none(), "Ball is the default and writes nothing");
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        let p = parameters(&h);
        assert_eq!(p["corner"], json!("g2"));
        assert!(p.get("setbacks").is_none(), "an empty Setback leaves the command's default: {p}");
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_some());
        if let Some(Dialog { kind: Kind::Fillet { setback, .. }, .. }) = h.app.dialog.as_mut() {
            *setback = Some("3 mm".into());
        }
        assert_eq!(parameters(&h)["setbacks"], json!([{"vertex": [10.0, 0.0, 5.0], "distance": "3 mm"}]));
        choose(&mut h, "ui.corners", Continuity::G1.label_id());
        let p = parameters(&h);
        assert!(p.get("corner").is_none() && p.get("setbacks").is_none(), "{p}");
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_none());
        // Another type clears the patch and hides the row: its corners are the rolling ball's.
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        choose(&mut h, "Type", "Chord length");
        let p = parameters(&h);
        assert!(p.get("corner").is_none() && p.get("setbacks").is_none(), "{p}");
        assert!(scenario::handle_at("dialog:ui.corners").is_none());
    }

    /// Setback shows the command's default as an editable length that follows the radius until
    /// one is typed (and sends none), keeps a typed one through radius changes, and flags one at
    /// or below the least setback the kernel takes there (the radius, at this square corner)
    /// before OK; a setback past it builds.
    #[test]
    fn setback_shows_its_default_keeps_a_typed_value_and_flags_one_too_small() {
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        let radius = |h: &mut Harness, r: &str| {
            if let Some(Dialog { kind: Kind::Fillet { radius, .. }, .. }) = h.app.dialog.as_mut() {
                *radius = r.into();
            }
            h.frames(3);
        };
        let shown = |h: &Harness| {
            let tree = h.output.accesskit_update.as_ref().unwrap();
            let label = crate::i18n::tr_label("ui.fillet_setback");
            tree.nodes.iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.value()).unwrap_or_default().to_string()
        };
        let typed = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
            Kind::Fillet { setback, .. } => setback.clone(),
            other => panic!("{other:?}"),
        };
        let flagged = || scenario::rect_of("dialog:caption:ui.fillet_setback").is_some();
        let type_setback = |h: &mut Harness, text: &str| {
            h.step(&json!({"click":{"handle":"dialog:ui.fillet_setback"}})).unwrap();
            h.step(&json!({"key":"A","cmd":true})).unwrap();
            h.step(&json!({"text": text})).unwrap();
            h.frames(3);
        };
        radius(&mut h, "2 mm");
        assert_eq!((shown(&h), typed(&h)), ("3 mm".to_string(), None), "the default, 1.5 × the radius, as a length");
        assert!(!flagged() && parameters(&h).get("setbacks").is_none(), "an untouched default sends none");
        radius(&mut h, "3 mm");
        assert_eq!(shown(&h), "4.5 mm", "untouched, it follows the radius");
        radius(&mut h, "2 mm");
        type_setback(&mut h, "1.5 mm");
        assert_eq!(typed(&h).as_deref(), Some("1.5 mm"));
        radius(&mut h, "2.5 mm");
        assert_eq!(shown(&h), "1.5 mm", "typed, it stays as typed");
        radius(&mut h, "2 mm");
        assert!(flagged(), "1.5 mm is not past the least, the 2 mm radius");
        let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        let refused = copy.execute("solid.fillet", &parameters(&h)).unwrap_err().to_string();
        assert!(refused.contains("setback must be more than the radius (2 mm)"), "the kernel refuses what the field flags: {refused}");
        assert_eq!(
            crate::dialogs::fillet_plain_error(&refused, true),
            Some(crate::dialogs::FilletRefusal::InField),
            "the field says it, not again under the dialog"
        );
        let least = crate::dialogs::fillet_least_setback(
            &h.ctx,
            &h.app.session,
            &h.app.dialog.as_ref().unwrap().inputs[0].items,
            &[Vec3::new(10.0, 0.0, 5.0)],
            2.0,
        );
        assert!(least.is_some_and(|l| (l - 2.0).abs() < 1e-9), "{least:?}");
        assert_eq!(crate::length_input::shown(&h.app.session.doc, least.unwrap()), "2 mm", "the message's least");
        type_setback(&mut h, "2 mm");
        assert!(flagged(), "the least itself is refused too");
        type_setback(&mut h, "3 mm");
        assert!(!flagged(), "past the least");
        let p = parameters(&h);
        assert_eq!(p["setbacks"], json!([{"vertex": [10.0, 0.0, 5.0], "distance": "3 mm"}]));
        let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        copy.execute("solid.fillet", &p).unwrap();
        let st = crate::open_meshing::session_world(&copy);
        assert!(st.bodies.iter().all(|b| b.body.validity().is_empty()), "a 3 mm setback on 2 mm rounds builds");
    }

    /// No visible label of the fillet dialog names a continuity by its code (G0 … G3): the words
    /// are on the controls, the codes only in hover texts. Every section open, the corner row shown.
    #[test]
    fn fillet_labels_use_words_not_continuity_codes() {
        let coded = |text: &str| ["G0", "G1", "G2", "G3"].iter().any(|code| text.contains(code));
        for label in FILLET_TYPES.map(|(label, _)| label).into_iter().chain(FILLET_CORNERS.map(|(g, _)| g.label_id())) {
            assert!(!coded(crate::i18n::tr_label(label)), "{label}: {}", crate::i18n::tr_label(label));
        }
        for (g, tip) in FILLET_CORNERS {
            assert!(crate::i18n::tr(tip).contains(g.code()), "the hover text keeps the code for those who know it: {tip}");
        }
        let tip = fillet_corner_tip();
        assert!(tip.contains("rolling") && tip.contains("1.5") && tip.contains("Setback"), "what each corner builds: {tip}");
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        h.frames(3);
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_some() && scenario::handle_at("dialog:ui.fillet_fit").is_some());
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let labels: Vec<String> = tree.nodes.iter().flat_map(|(_, node)| [node.label(), node.value()]).flatten().map(str::to_string).collect();
        assert!(labels.iter().any(|l| l == "Curvature"), "{labels:?}");
        let shown: Vec<&String> = labels.iter().filter(|l| coded(l)).collect();
        assert!(shown.is_empty(), "labels with a continuity code: {shown:?}");
    }

    /// A smooth-patch fillet reopens with its corner and setback, and OK writes them as saved; a
    /// setback list the dialog cannot show as one distance is kept as it is.
    /// A setback too short for SMLib's corner fill (past the least the field flags) is refused by
    /// the kernel naming the smallest that builds; the dialog says it in plain words, that length
    /// in them.
    #[test]
    fn a_setback_too_short_for_the_corner_says_the_least_that_builds() {
        let refused = format!(
            "invalid parameters for `solid.fillet`: the setback is too short for its corner: {}5.9620 mm (the corner's patch is convex only from there)",
            solidcraft_engine::kernel::SETBACK_AT_LEAST
        );
        match crate::dialogs::fillet_plain_error(&refused, true) {
            Some(crate::dialogs::FilletRefusal::Plain(plain)) => assert!(plain.contains("5.9620 mm"), "{plain}"),
            other => panic!("{other:?}"),
        }
        // A blend too large for its faces keeps its own words.
        assert_eq!(
            crate::dialogs::fillet_plain_error("the fillet is larger than the faces beside its edges allow", true),
            Some(crate::dialogs::FilletRefusal::Plain(crate::i18n::tr("ui.fillet_error_too_large").to_string()))
        );
    }

    #[test]
    fn a_saved_smooth_patch_reopens_with_its_setback() {
        for (setbacks, field) in [
            (json!([{"vertex": [0, 0, 0], "distance": 7}]), Some("7")),
            // Named near the vertex, as the command allows, rather than at it.
            (json!([{"vertex": [0.1, 0.1, 0], "distance": 7}]), None),
        ] {
            let mut h = Harness::new();
            // The command's own smooth-patch corner: a 20 mm box, its three edges at the origin.
            h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "B"})).unwrap();
            let edges = json!([[10, 0, 0], [0, 10, 0], [0, 0, 10]]);
            h.app.run("solid.fillet", json!({"edges": edges, "radius": 4, "corner": "g2", "setbacks": setbacks})).unwrap();
            let id = h.app.session.doc.features.last().unwrap().id;
            let corner_of = |h: &Harness| match &h.app.session.doc.feature(id).unwrap().kind {
                FeatureKind::Fillet { corner, setbacks, .. } => (*corner, setbacks.clone()),
                other => panic!("{other:?}"),
            };
            let saved = corner_of(&h);
            assert!(!saved.0.is_rolling() && saved.1.len() == 1, "{saved:?}");
            h.app.edit_feature(id);
            h.frames(3);
            let d = h.app.dialog.as_ref().unwrap();
            assert!(matches!(&d.kind, Kind::Fillet { setback, .. } if setback.as_deref() == field), "{:?}", d.kind);
            assert_eq!(d.extra.get("corner"), Some(&json!("g2")));
            for (command, params) in apply_commands(&h.app, d).unwrap() {
                crate::dialogs::roll_marker(&mut h.app, None).unwrap();
                h.app.run(&command, params).unwrap();
            }
            assert_eq!(corner_of(&h), saved, "OK keeps the corner and its setbacks");
        }
    }

    #[test]
    fn variable_controls_toggle_keep_mid_radii_and_preview_is_ui_only() {
        let mut h = fillet_harness();
        let original = parameters(&h);
        h.step(&json!({"click":{"handle":"dialog:Preview"}})).unwrap();
        h.frames(3);
        assert!(!h.app.dialog.as_ref().unwrap().preview_enabled);
        assert!(!h.app.preview.active && !h.app.preview.busy);
        assert_eq!(parameters(&h), original);
        h.step(&json!({"click":{"handle":"dialog:Preview"}})).unwrap();
        assert!(h.app.dialog.as_ref().unwrap().preview_enabled);
        assert_eq!(parameters(&h), original);
        assert!(scenario::handle_at("dialog:End Radius").is_none());
        choose(&mut h, "Type", "Variable radius");
        assert!(scenario::handle_at("dialog:End Radius").is_some(), "the section shows, open, once its type is chosen");
        assert!(
            scenario::handle_at("dialog:ui.fillet_smooth_radius").is_some(),
            "Smooth radius change is a supported variable-radius option even before adding a middle radius"
        );
        h.step(&json!({"widget":"Add Mid Radius"})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:ui.fillet_smooth_radius"}})).unwrap();
        let saved = parameters(&h);
        assert_eq!(saved["mid"].as_array().unwrap().len(), 1);
        assert_eq!(saved["smooth"], json!(true));
        h.step(&json!({"click":{"handle":"dialog:section:Variable Radius"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Mid Radius").is_none());
        assert_eq!(parameters(&h), saved, "folding keeps all values and commands");
        assert!(scenario::handle_at("dialog:section:Setbacks").is_none());
        assert!(scenario::handle_at("dialog:Result").is_none());
    }

    #[test]
    fn edges_field_paints_zero_one_and_many_and_footer_follows_validity() {
        fn contains_text(shape: &egui::Shape, expected: &str) -> bool {
            match shape {
                egui::Shape::Text(text) => text.galley.job.text == expected,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains_text(shape, expected)),
                _ => false,
            }
        }
        let mut h = fillet_harness();
        let picks: Vec<_> = [Vec3::new(5.0, 0.0, 5.0), Vec3::new(0.0, 5.0, 5.0), Vec3::new(10.0, 5.0, 5.0)]
            .into_iter()
            .map(|p| edge_sel(&h.app.session, p).unwrap())
            .collect();
        for (count, text) in [(0, "Select edges"), (1, "1 selected"), (3, "3 selected")] {
            h.app.dialog.as_mut().unwrap().inputs.first_mut().unwrap().items = picks.iter().take(count).cloned().collect();
            h.frames(3);
            if count <= 1 {
                let tree = h.output.accesskit_update.as_ref().unwrap();
                let disabled = tree
                    .nodes
                    .iter()
                    .find_map(|(_, node)| (node.role() == egui::accesskit::Role::Button && node.label() == Some("OK")).then(|| node.is_disabled()))
                    .unwrap();
                assert_eq!(disabled, count == 0, "OK follows the actual required edge selection");
            }
            let mut output = h.ctx.run_ui(
                egui::RawInput {
                    time: Some(h.ctx.input(|input| input.time)),
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 1000.0))),
                    ..Default::default()
                },
                |ui| h.app.ui(ui),
            );
            let painted = output.shapes.iter().any(|shape| contains_text(&shape.shape, text));
            output.textures_delta.clear();
            assert!(painted, "the actual field must paint {text}");
            let field = scenario::rect_of("dialog:field:Edges").unwrap();
            assert!(
                (field.width() - scenario::rect_of("dialog:control:Edges").unwrap().width()).abs() < 0.02,
                "clear is inside the full field (allow egui subpixel rounding)"
            );
            assert!(h.widget("OK").is_some());
            if count == 0 {
                h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
                assert!(h.app.dialog.is_some(), "invalid OK stays disabled");
            }
        }
    }
}

#[cfg(test)]
#[path = "dialogs_all_tests.rs"]
mod all_tests;

#[cfg(test)]
#[path = "dialogs_sketch_tests.rs"]
mod sketch_tests;
