//! `POST /api/sch/import_settings`: Schematic Setup's "Import Settings from Another Project..." (KiCad 25ec2335 `DIALOG_SCHEMATIC_SETUP::onAuxiliaryAction`,
//! `DIALOG_SCH_IMPORT_SETTINGS`), the half that reads the other project (UI-LEFT-2 BB4).
//!
//! KiCad reads everything it imports from the other project's file alone (`SCRATCH_PROJECT( .., aRequireProjectFile = true )`: its `SCHEMATIC_SETTINGS`,
//! `ERC_SETTINGS`, `m_TemplateFieldNames`, `m_NetSettings`, `m_BusAliases` and text variables), never its schematic, so the page posts the `.kicad_pro`'s text:
//! `{ "project": "<text>", "project_name": "other.kicad_pro" }`. A text that is no project is refused as KiCad refuses it ("Error importing settings from
//! project:\nProject file %s could not be loaded."). The answer is what each page reads, in the shapes of the endpoints the pages load from --
//! `formatting` (`GET /api/sch/formatting`), `setup` (`GET /api/sch/setup`), `severities` (`GET /api/sch/erc_severities`), `pin_map` (`GET
//! /api/sch/erc_pin_map`) -- and `board` (`/api/state`'s `board_rules` for the Net Classes and Text Variables pages). Nothing on the open design changes.

use serde_json::{json, Value};
use std::path::Path;

/// `POST /api/sch/import_settings`.
pub(crate) fn import_settings(_dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let name = req.get("project_name").and_then(Value::as_str).unwrap_or("project.kicad_pro");
    let refused = || json!({ "ok": false, "message": format!("Error importing settings from project:\nProject file {name} could not be loaded.") });
    let Some(text) = req.get("project").and_then(Value::as_str) else { return refused() };
    let Ok(pro) = serde_json::from_str::<Value>(text) else { return refused() };
    if !pro.is_object() {
        return refused();
    }
    let mut design = crate::import_kicad::empty_board();
    let mut sch = eda_model::ir::SchematicSection::default();
    eda_kicad::project_schematic::apply(&mut sch.extras, text);
    let (severities, pin_map, grid) = crate::import_kicad::project_erc_settings(&pro);
    sch.extras.erc_severities = severities;
    sch.erc_pin_map = pin_map.map(|matrix| eda_model::ir::ErcPinMap { matrix });
    sch.extras.connection_grid_mils = grid;
    design.schematic = Some(sch);
    design.bus_aliases = eda_kicad::project_bus_aliases(text);
    // the net classes with their schematic columns, as the importer reads a project's
    let mut model = eda_model::ConstraintModel::default();
    eda_kicad::merge_project_net_classes(&mut model, text);
    eda_kicad::merge_project_net_classes_for_schematic(&mut model, text);
    let b = &model.board;
    let text_variables: std::collections::BTreeMap<String, String> = pro.get("text_variables").and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()).unwrap_or_default();
    json!({
        "ok": true,
        "formatting": crate::sch_api::formatting_of(&design),
        "setup": crate::sch_api::setup_of(&design),
        "severities": crate::sch_api::erc_severities_of(&design),
        "pin_map": crate::sch_api::erc_pin_map_of(&design),
        "board": {
            "layers": [],
            "board_rules": {
                "track_width": b.track_width,
                "via_drill": b.via_drill,
                "via_diameter": b.via_diameter,
                "clearance": b.clearance,
                "net_classes": b.net_classes,
                "default_class": eda_model::rules::net_class_settings_of(b).default,
                "net_class_schematic": b.net_class_schematic,
                "net_class_pcb_colors": b.net_class_pcb_colors,
                "tuning_profiles": b.tuning_profiles,
                "text_variables": text_variables,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `onAuxiliaryAction`: a text that is no project is refused with KiCad's words; a project's Schematic Setup comes back in the shapes the pages read.
    #[test]
    fn another_projects_schematic_settings_come_back_for_the_pages() {
        let dir = std::env::temp_dir();
        let refused = import_settings(&dir, json!({ "project": "not json", "project_name": "other.kicad_pro" }).to_string().as_bytes());
        assert_eq!(refused["message"], "Error importing settings from project:\nProject file other.kicad_pro could not be loaded.");
        let pro = json!({
            "schematic": { "connection_grid_size": 25.0, "drawing": { "default_text_size": 60.0 }, "annotation": { "method": 1, "sort_order": 1 },
                "bus_aliases": { "DATA": ["D0", "D1"] }, "bom_presets": [{ "name": "Mine" }] },
            "erc": { "rule_severities": { "pin_not_connected": "warning" } },
            "net_settings": { "classes": [{ "name": "Default" }, { "name": "Power", "wire_width": 12.0 }], "meta": { "version": 3 } },
            "text_variables": { "REV": "B" }
        });
        let got = import_settings(&dir, json!({ "project": pro.to_string(), "project_name": "other.kicad_pro" }).to_string().as_bytes());
        assert_eq!(got["ok"], true, "{got}");
        assert_eq!(got["formatting"]["connection_grid_mils"], 25);
        assert_eq!(got["setup"]["annotation"]["method"], 1);
        assert_eq!(got["setup"]["bus_aliases"][0]["name"], "DATA");
        assert_eq!(got["setup"]["bom_presets"], json!(["Mine"]));
        assert_eq!(got["severities"]["severities"]["pin_not_connected"], "warning");
        assert!(got["board"]["board_rules"]["net_classes"].as_array().unwrap().iter().any(|c| c["name"] == "Power"));
        assert_eq!(got["board"]["board_rules"]["text_variables"]["REV"], "B");
    }
}
