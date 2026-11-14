//! `POST /api/import_settings`: Board Setup's "Import Settings from Another Board..." (KiCad 25ec2335 `DIALOG_BOARD_SETUP::onAuxiliaryAction`,
//! `DIALOG_IMPORT_SETTINGS`), the half that reads the other board (UI-LEFT-2 BB4).
//!
//! A browser hands the page a file's text and not its path, so the page posts the files the person picked: `{ "board": "<.kicad_pcb text>", "board_name":
//! "other.kicad_pcb", "project": "<.kicad_pro text>" | null, "rules": "<.kicad_dru text>" | null }`. KiCad loads the board with its project
//! (`SCRATCH_PROJECT( .., aRequireProjectFile = true )`) and refuses a board without one; so does this ("Error importing settings from board:\nAssociated
//! project file %s could not be loaded"). The board is read with the importer `eda import-kicad` uses and its project merged the same way, and the answer is
//! what Board Setup's pages read of a board, in `/api/state`'s shapes: `{ "ok": true, "board": { "layers", "board_rules": { .. }, "routing": { the
//! pre-defined sizes, "teardrop_settings" }, "drawings": { "zone_defaults", "dimension_settings" } } }`. Nothing on the open board changes: the page puts the
//! chosen parts into its pages (`ImportSettingsFrom`), which the person then applies, as KiCad's OK does.

use serde_json::{json, Value};
use std::path::Path;

/// `POST /api/import_settings`.
pub(crate) fn import_settings(_dir: &Path, body: &[u8]) -> Value {
    let fail = |message: String| json!({ "ok": false, "message": message });
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(text) = req.get("board").and_then(Value::as_str) else { return fail("there is no board to import settings from".into()) };
    let name = req.get("board_name").and_then(Value::as_str).unwrap_or("board.kicad_pcb");
    let Some(pro) = req.get("project").and_then(Value::as_str).filter(|p| !p.trim().is_empty()) else {
        let project = format!("{}.kicad_pro", name.strip_suffix(".kicad_pcb").unwrap_or(name));
        return fail(format!("Error importing settings from board:\nAssociated project file {project} could not be loaded"));
    };
    let (mut design, mut model, _) = match eda_kicad::import_kicad_pcb(text) {
        Ok(r) => r,
        Err(e) => return fail(format!("Error loading board file:\n{name}\n{}", e.iter().filter_map(|c| c.hint.clone()).collect::<Vec<_>>().join("; "))),
    };
    crate::import_foreign::merge_project(&mut model, pro);
    eda_kicad::merge_project_predefined_sizes(&mut design, pro);
    eda_kicad::merge_project_teardrops(&mut design, pro);
    eda_kicad::merge_project_dimension_defaults(&mut design, pro);
    eda_kicad::merge_project_zone_defaults(&mut design, pro);
    eda_kicad::merge_project_via_stack_presets(&mut design, pro);
    if let Some(dru) = req.get("rules").and_then(Value::as_str) {
        eda_kicad::merge_custom_rules(&mut model, dru);
    }
    json!({ "ok": true, "board": settings_json(&design, &model) })
}

/// What Board Setup's pages read of a board (`/api/state`'s `layers`, `board_rules`, `routing` and `drawings`, the parts the pages use).
fn settings_json(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Value {
    let b = &model.board;
    let routing = design.routing.as_ref();
    json!({
        "layers": b.layers,
        "board_rules": {
            "track_width": b.track_width,
            "via_drill": b.via_drill,
            "via_diameter": b.via_diameter,
            "clearance": b.clearance,
            "net_classes": b.net_classes,
            "default_class": eda_model::rules::net_class_settings_of(b).default,
            "net_class_schematic": b.net_class_schematic,
            "net_class_pcb_colors": b.net_class_pcb_colors,
            "component_classes": b.component_classes,
            "stackup": model.stackup,
            "constraints": eda_model::rules::Constraints::of(b),
            "constraints_explicit": b.constraints_explicit,
            "mask_paste": b.solder_mask,
            "text_graphics": b.text_graphics,
            "board_thickness_nm": b.board_thickness_nm,
            "copper_layers": b.layers.len(),
            "layer_names": b.layers,
            "layer_setup": b.layer_setup.canonical(),
            "formatting": b.formatting,
            "zone_hatch_offsets": b.hatch_offsets,
            "tuning_patterns": b.tuning_patterns,
            "tuning_profiles": b.tuning_profiles,
            "severities": eda_kicad::effective_rule_severities(model),
            "custom_rules_text": b.custom_rules_text,
        },
        "routing": {
            "track_width_presets": routing.map(|r| r.track_width_presets.clone()).unwrap_or_default(),
            "via_presets": routing.map(|r| r.via_presets.clone()).unwrap_or_default(),
            "diff_pair_presets": routing.map(|r| r.diff_pair_presets.clone()).unwrap_or_default(),
            "teardrop_settings": routing.map(|r| r.teardrop_settings).unwrap_or_default(),
        },
        "drawings": {
            "zone_defaults": design.drawings.as_ref().and_then(|d| d.zone_defaults.clone()),
            "zone_defaults_locked": design.drawings.as_ref().is_some_and(|d| d.zone_defaults_locked),
            "via_stack_presets": design.drawings.as_ref().map(|d| d.via_stack_presets.clone()).unwrap_or_default(),
            "dimension_settings": design.drawings.as_ref().map(|d| d.dimension_settings).unwrap_or_default(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (general (thickness 1.6)) (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0.05))
  (net 0 ""))"#;

    /// `onAuxiliaryAction`: a board without its project is refused with KiCad's words; with it, the board's and the project's settings come back in the
    /// shapes the pages read, and the open board is not touched.
    #[test]
    fn another_boards_settings_come_back_for_the_pages_and_a_board_without_its_project_is_refused() {
        let dir = std::env::temp_dir();
        let refused = import_settings(&dir, json!({ "board": BOARD, "board_name": "other.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["message"], "Error importing settings from board:\nAssociated project file other.kicad_pro could not be loaded");
        let pro = json!({ "board": { "design_settings": {
            "rules": { "min_clearance": 0.3 },
            "track_widths": [0.0, 0.25, 0.5],
            "defaults": { "zones": { "min_clearance": 0.7 } }
        } } });
        let got = import_settings(&dir, json!({ "board": BOARD, "board_name": "other.kicad_pcb", "project": pro.to_string(), "rules": "(version 1)\n(rule a (constraint clearance (min 1mm)))\n" }).to_string().as_bytes());
        assert_eq!(got["ok"], true, "{got}");
        let b = &got["board"];
        assert_eq!(b["layers"], json!(["F.Cu", "B.Cu"]));
        assert_eq!(b["board_rules"]["constraints"]["min_clearance_nm"], 300_000);
        assert_eq!(b["routing"]["track_width_presets"], json!([250_000, 500_000]));
        assert_eq!(b["drawings"]["zone_defaults"]["clearance"], 700_000);
        assert!(b["board_rules"]["custom_rules_text"].as_str().is_some_and(|t| t.contains("rule a")), "{}", b["board_rules"]["custom_rules_text"]);
    }
}
