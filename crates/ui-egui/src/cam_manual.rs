//! The CAM manual: the Manual tab of the CAM window, and the explanation each CAM form field
//! shows on hover. Both read the same messages, so a field explains itself in the words the
//! manual uses.

use crate::i18n::tr;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;

/// The manual's sections in reading order: heading and text.
const SECTIONS: &[(&str, &str)] = &[
    ("ui.cam.manual.overview", "ui.cam.manual.overview_text"),
    ("ui.cam.manual.parts", "ui.cam.manual.parts_text"),
    ("ui.cam.manual.machines", "ui.cam.manual.machines_text"),
    ("ui.cam.manual.setup", "ui.cam.manual.setup_text"),
    ("ui.cam.manual.tools", "ui.cam.manual.tools_text"),
    ("ui.cam.manual.steps", "ui.cam.manual.steps_text"),
    ("ui.cam.manual.milling", "ui.cam.manual.milling_text"),
    ("ui.cam.manual.holes", "ui.cam.manual.holes_text"),
    ("ui.cam.manual.plasma", "ui.cam.manual.plasma_text"),
    ("ui.cam.manual.check", "ui.cam.manual.check_text"),
    ("ui.cam.manual.output", "ui.cam.manual.output_text"),
    ("ui.cam.manual.sheets", "ui.cam.manual.sheets_text"),
];

/// Each explained field: its label's message and its explanation's message.
pub(crate) const TIPS: &[(&str, &str)] = &[
    ("ui.cam.a_axis", "ui.cam.tip.a_axis"),
    ("ui.cam.allowance", "ui.cam.tip.allowance"),
    ("ui.cam.bottom", "ui.cam.tip.bottom"),
    ("ui.cam.chain", "ui.cam.tip.chain"),
    ("ui.cam.chamfer_width", "ui.cam.tip.chamfer_width"),
    ("ui.cam.clearance", "ui.cam.tip.clearance"),
    ("ui.cam.cut_height", "ui.cam.tip.cut_height"),
    ("ui.cam.description", "ui.cam.tip.description"),
    ("ui.cam.diameter", "ui.cam.tip.diameter"),
    ("ui.cam.direction", "ui.cam.tip.direction"),
    ("ui.cam.dwell", "ui.cam.tip.dwell"),
    ("ui.cam.extend_end", "ui.cam.tip.extend"),
    ("ui.cam.extend_start", "ui.cam.tip.extend"),
    ("ui.cam.feed", "ui.cam.tip.feed"),
    ("ui.cam.finish_z", "ui.cam.tip.finish_z"),
    ("ui.cam.flip_x", "ui.cam.tip.flip"),
    ("ui.cam.flip_z", "ui.cam.tip.flip"),
    ("ui.cam.floor_stock", "ui.cam.tip.floor_stock"),
    ("ui.cam.flute", "ui.cam.tip.flute"),
    ("ui.cam.flutes", "ui.cam.tip.flutes"),
    ("ui.cam.height", "ui.cam.tip.rectangle"),
    ("ui.cam.helix_internal", "ui.cam.tip.helix_internal"),
    ("ui.cam.helix_pitch", "ui.cam.tip.helix_pitch"),
    ("ui.cam.holder", "ui.cam.tip.holder"),
    ("ui.cam.holder_diameter", "ui.cam.tip.holder"),
    ("ui.cam.holder_length", "ui.cam.tip.holder"),
    ("ui.cam.hole_diameter", "ui.cam.tip.hole_diameter"),
    ("ui.cam.kerf", "ui.cam.tip.kerf"),
    ("ui.cam.kind", "ui.cam.tip.kind"),
    ("ui.cam.lead_in", "ui.cam.tip.lead"),
    ("ui.cam.lead_in_size", "ui.cam.tip.lead_size"),
    ("ui.cam.lead_out", "ui.cam.tip.lead"),
    ("ui.cam.lead_out_size", "ui.cam.tip.lead_size"),
    ("ui.cam.library_tool", "ui.cam.tip.library_tool"),
    ("ui.cam.machine", "ui.cam.tip.machine"),
    ("ui.cam.machine_kind", "ui.cam.tip.machine_kind"),
    ("ui.cam.machine_name", "ui.cam.tip.machine_name"),
    ("ui.cam.material", "ui.cam.tip.material"),
    ("ui.cam.max_feed", "ui.cam.tip.max_feed"),
    ("ui.cam.max_rpm", "ui.cam.tip.max_rpm"),
    ("ui.cam.name", "ui.cam.tip.name"),
    ("ui.cam.operation", "ui.cam.tip.operation"),
    ("ui.cam.output", "ui.cam.tip.output"),
    ("ui.cam.overlap", "ui.cam.tip.overlap"),
    ("ui.cam.overrun", "ui.cam.tip.overrun"),
    ("ui.cam.part", "ui.cam.tip.part"),
    ("ui.cam.part_name", "ui.cam.tip.part"),
    ("ui.cam.pierce_delay", "ui.cam.tip.pierce_delay"),
    ("ui.cam.pierce_height", "ui.cam.tip.pierce_height"),
    ("ui.cam.pitch", "ui.cam.tip.pitch"),
    ("ui.cam.plunge", "ui.cam.tip.plunge"),
    ("ui.cam.post", "ui.cam.tip.post"),
    ("ui.cam.program_folder", "ui.cam.tip.program_folder"),
    ("ui.cam.retract", "ui.cam.tip.retract"),
    ("ui.cam.rpm", "ui.cam.tip.rpm"),
    ("ui.cam.setup", "ui.cam.tip.setup"),
    ("ui.cam.setup_type", "ui.cam.tip.setup_type"),
    ("ui.cam.side", "ui.cam.tip.side"),
    ("ui.cam.slot_width", "ui.cam.tip.slot_width"),
    ("ui.cam.slow_hole_diameter", "ui.cam.tip.slow_holes"),
    ("ui.cam.slow_hole_share", "ui.cam.tip.slow_holes"),
    ("ui.cam.spindle_max", "ui.cam.tip.spindle_range"),
    ("ui.cam.spindle_min", "ui.cam.tip.spindle_range"),
    ("ui.cam.stepdown", "ui.cam.tip.stepdown"),
    ("ui.cam.stepover", "ui.cam.tip.stepover"),
    ("ui.cam.stock", "ui.cam.tip.stock"),
    ("ui.cam.tab_count", "ui.cam.tip.tabs"),
    ("ui.cam.tab_height", "ui.cam.tip.tabs"),
    ("ui.cam.tab_width", "ui.cam.tip.tabs"),
    ("ui.cam.tabs", "ui.cam.tip.tabs"),
    ("ui.cam.thread_internal", "ui.cam.tip.thread_internal"),
    ("ui.cam.thread_left", "ui.cam.tip.thread_left"),
    ("ui.cam.tip_offset", "ui.cam.tip.tip_offset"),
    ("ui.cam.tool", "ui.cam.tip.tool"),
    ("ui.cam.top", "ui.cam.tip.top"),
    ("ui.cam.wall_stock", "ui.cam.tip.wall_stock"),
    ("ui.cam.wcs", "ui.cam.tip.work_offset"),
    ("ui.cam.width", "ui.cam.tip.rectangle"),
    ("ui.cam.work_offset", "ui.cam.tip.work_offset"),
    ("ui.cam.x", "ui.cam.tip.rectangle"),
    ("ui.cam.y", "ui.cam.tip.rectangle"),
    ("ui.cam.zero", "ui.cam.tip.zero"),
    ("ui.cam.zero_follows", "ui.cam.tip.zero_follows"),
];

/// The explanations as the rows show their labels: translated label, translated explanation.
fn tips() -> Vec<(String, String)> {
    TIPS.iter().map(|(label, tip)| (tr(label).to_string(), tr(tip).to_string())).collect()
}

/// Draws a CAM form with `add`: each of its rows labelled as in `TIPS` explains itself on hover.
pub(crate) fn with_tips<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui_kit::with_row_tips(ui, tips(), add)
}

/// A checkbox (which has no row label) with its explanation on hover.
pub(crate) fn checkbox(ui: &mut egui::Ui, label: &str, value: &mut bool) -> egui::Response {
    let response = ui_kit::checkbox(ui, tr(label), value);
    match TIPS.iter().find(|(l, _)| *l == label) {
        Some((_, tip)) => response.backing_on_hover_text(tr(tip)),
        None => response,
    }
}

/// The Manual tab: the sections, then every field with its explanation, in the order of `TIPS`
/// (one line per explanation, under the labels that share it).
pub(crate) fn manual_tab(ui: &mut egui::Ui) {
    for (title, text) in SECTIONS {
        ui_kit::text(ui, tr(title), ui_kit::Text::Heading("cam-manual"));
        for paragraph in tr(text).split("\n\n") {
            ui.add(egui::Label::new(paragraph).wrap());
        }
        ui.add_space(crate::theme::Tokens::get().space_8);
    }
    ui_kit::text(ui, tr("ui.cam.manual.fields"), ui_kit::Text::Heading("cam-manual"));
    let mut done: Vec<&str> = Vec::new();
    for (_, tip) in TIPS {
        if done.contains(tip) {
            continue;
        }
        done.push(tip);
        let labels: Vec<&str> = TIPS.iter().filter(|(_, t)| t == tip).map(|(l, _)| tr(l)).collect();
        ui.add(egui::Label::new(egui::RichText::new(labels.join(" · ")).strong()).wrap());
        ui.add(egui::Label::new(tr(tip)).wrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every section and every explanation has its English words, and no explanation is unused.
    #[test]
    fn the_cam_manual_and_every_field_explanation_have_their_words() {
        for (title, text) in SECTIONS {
            for id in [title, text] {
                assert_ne!(tr(id), *id, "{id} has no message");
            }
        }
        assert_ne!(tr("ui.cam.manual.fields"), "ui.cam.manual.fields");
        for (label, tip) in TIPS {
            assert_ne!(tr(label), *label, "{label} has no message");
            assert_ne!(tr(tip), *tip, "{tip} has no message");
        }
        let mut labels: Vec<&str> = TIPS.iter().map(|(l, _)| *l).collect();
        assert!(labels.is_sorted(), "TIPS stays sorted by label");
        labels.dedup();
        assert_eq!(labels.len(), TIPS.len(), "each label once");
    }

    /// A row inside a CAM form finds its explanation by its label; outside the form none.
    #[test]
    fn a_cam_row_finds_its_explanation_by_its_label() {
        let ctx = crate::fonts::test_context();
        let mut checked = false;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            with_tips(ui, |ui| {
                assert_eq!(ui_kit::tip_for(ui, tr("ui.cam.stepdown")).as_deref(), Some(tr("ui.cam.tip.stepdown")));
                assert!(ui_kit::tip_for(ui, "no such row").is_none());
            });
            assert!(ui_kit::tip_for(ui, tr("ui.cam.stepdown")).is_none(), "only inside the form");
            checked = true;
        });
        output.textures_delta.clear();
        assert!(checked);
    }
}
