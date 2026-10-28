//! The `schematic` object of a KiCad project file (`.kicad_pro`), the share of `SCHEMATIC_SETTINGS` (`eeschema/schematic_settings.cpp`, a `NESTED_SETTINGS` at
//! "schematic") and of `PROJECT_FILE` (`common/project/project_file.cpp`: `schematic.top_level_sheets`, `schematic.legacy_lib_*`, `schematic.bus_aliases`)
//! an imported schematic keeps (r7 H5, M1).
//!
//! * The settings the studio has a setting of its own for go into the design's root extras: the unit notation (`subpart_id_separator`, `subpart_first_id`), the
//!   inter-sheet references (`drawing.intersheets_ref_*`) and the simulator's settings (`ngspice`). The connection grid is read by `eda import-kicad` itself.
//! * The rest is kept as KiCad wrote it ([`kept`], `SchExtras::project_schematic`), and the derived project starts its `schematic` object from it, so kicad-cli
//!   reads the formatting, annotation, BOM, netlist and simulator keys of the project as KiCad saved them.
//! * Left out: the keys the studio holds itself (above, written back from the extras), the project's list of top-level sheets (the import reads them, and the
//!   derived project has one root), its bus aliases (the design's own, `Design::bus_aliases`), and the keys that name files of the original folder, which the
//!   derived folder does not have (`legacy_lib_dir`, `legacy_lib_list`, `page_layout_descr_file`). **Differs**: a project's own drawing sheet
//!   (`page_layout_descr_file`) is not carried; the derived project draws KiCad's default.

use eda_model::sch_extras::{IntersheetRefs, NgspiceSettings, SchExtras};
use serde_json::{Map, Value};

/// The keys of the `schematic` object [`kept`] leaves out (dotted for those of `drawing`).
const LEFT_OUT: &[&str] = &[
    "subpart_id_separator",
    "subpart_first_id",
    "connection_grid_size",
    "ngspice",
    "top_level_sheets",
    "bus_aliases",
    "legacy_lib_dir",
    "legacy_lib_list",
    "page_layout_descr_file",
    "drawing.intersheets_ref_show",
    "drawing.intersheets_ref_own_page",
    "drawing.intersheets_ref_short",
    "drawing.intersheets_ref_prefix",
    "drawing.intersheets_ref_suffix",
];

/// The project's `schematic` object without the keys the studio holds or that name files of the original folder (see the module doc). `None` when the
/// object is not there or keeps nothing.
pub fn kept(schematic: &Value) -> Option<Map<String, Value>> {
    let mut out = schematic.as_object()?.clone();
    for key in LEFT_OUT {
        match key.split_once('.') {
            Some((outer, inner)) => {
                if let Some(o) = out.get_mut(outer).and_then(Value::as_object_mut) {
                    o.remove(inner);
                }
            }
            None => {
                out.remove(*key);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The inter-sheet references of a project's `schematic.drawing` (`SCHEMATIC_SETTINGS`' `drawing.intersheets_ref_*` `PARAM`s, each its default when the key is
/// missing or of another type), `None` when the project has none of the keys or they are all KiCad's defaults.
fn intersheet_refs(drawing: &Value) -> Option<IntersheetRefs> {
    let keys = ["intersheets_ref_show", "intersheets_ref_own_page", "intersheets_ref_short", "intersheets_ref_prefix", "intersheets_ref_suffix"];
    if !keys.iter().any(|k| drawing.get(k).is_some()) {
        return None;
    }
    let d: IntersheetRefs = serde_json::from_value(serde_json::json!({})).ok()?;
    let flag = |k: &str, default: bool| drawing.get(k).and_then(Value::as_bool).unwrap_or(default);
    let text = |k: &str, default: &str| drawing.get(k).and_then(Value::as_str).unwrap_or(default).to_string();
    let r = IntersheetRefs {
        show: flag("intersheets_ref_show", d.show),
        own_page: flag("intersheets_ref_own_page", d.own_page),
        short: flag("intersheets_ref_short", d.short),
        prefix: text("intersheets_ref_prefix", &d.prefix),
        suffix: text("intersheets_ref_suffix", &d.suffix),
    };
    (r != d).then_some(r)
}

/// The simulator's settings of a project's `schematic.ngspice` (`NGSPICE_SETTINGS`: `workbook_filename`, `fix_include_paths`, `model_mode`, each its default when
/// missing), `None` when the project has no `ngspice` object.
fn ngspice(schematic: &Value) -> Option<NgspiceSettings> {
    let ng = schematic.get("ngspice")?.as_object()?;
    let d = NgspiceSettings::default();
    Some(NgspiceSettings {
        workbook_filename: ng.get("workbook_filename").and_then(Value::as_str).unwrap_or(&d.workbook_filename).to_string(),
        fix_include_paths: ng.get("fix_include_paths").and_then(Value::as_bool).unwrap_or(d.fix_include_paths),
        model_mode: ng.get("model_mode").and_then(Value::as_u64).map(|m| m as u32).filter(|m| *m <= NgspiceSettings::HSPICE).unwrap_or(d.model_mode),
    })
}

/// Put a project's Schematic Setup (`project_json`, the `.kicad_pro`'s text) into a root sheet's extras: the unit notation, the inter-sheet references, the
/// simulator's settings and the rest of the `schematic` object ([`kept`]). A text that is not JSON, or has no `schematic` object, changes nothing.
pub fn apply(extras: &mut SchExtras, project_json: &str) {
    let Ok(root) = serde_json::from_str::<Value>(project_json) else { return };
    let Some(schematic) = root.get("schematic").filter(|s| s.is_object()) else { return };
    let (separator, first_id) = eda_model::annotate::SubpartIds::from_project(schematic);
    extras.subpart_id_separator = separator;
    extras.subpart_first_id = first_id;
    if let Some(r) = schematic.get("drawing").and_then(intersheet_refs) {
        extras.intersheet_refs = Some(r);
    }
    if let Some(ng) = ngspice(schematic) {
        extras.ngspice = Some(ng);
    }
    extras.project_schematic = kept(schematic);
}

/// The project's top-level sheets (`PROJECT_FILE::GetTopLevelSheets`, `schematic.top_level_sheets`: each a `filename` relative to the project and the `name`
/// the sheet is shown by), in the project's order. Empty when the project lists none (a project from before KiCad 10, which has one root, the file opened).
pub fn top_level_sheets(project_json: &str) -> Vec<(String, String)> {
    let Ok(root) = serde_json::from_str::<Value>(project_json) else { return Vec::new() };
    root.pointer("/schematic/top_level_sheets")
        .and_then(Value::as_array)
        .map(|sheets| {
            sheets
                .iter()
                .filter_map(|s| {
                    let file = s.get("filename").and_then(Value::as_str)?;
                    let name = s.get("name").and_then(Value::as_str).unwrap_or_default();
                    Some((file.to_string(), name.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_projects_unit_notation_and_its_other_settings_reach_the_extras() {
        let pro = r#"{ "schematic": { "subpart_id_separator": 46, "subpart_first_id": 49, "annotate_start_num": 10, "connection_grid_size": 25.0,
            "drawing": { "label_size_ratio": 0.3, "intersheets_ref_show": true }, "page_layout_descr_file": "x.kicad_wks", "legacy_lib_list": [],
            "ngspice": { "model_mode": 0, "fix_include_paths": true, "workbook_filename": "" }, "top_level_sheets": [ { "filename": "a.kicad_sch", "name": "A" } ],
            "meta": { "version": 1 } } }"#;
        let mut e = SchExtras::default();
        apply(&mut e, pro);
        assert_eq!((e.subpart_id_separator, e.subpart_first_id), (Some(46), Some(49)));
        assert!(e.intersheet_refs.as_ref().is_some_and(|r| r.show && r.own_page));
        assert_eq!(e.ngspice.as_ref().map(|n| n.model_mode), Some(0));
        let kept = e.project_schematic.unwrap();
        assert_eq!(kept.get("annotate_start_num"), Some(&serde_json::json!(10)));
        assert_eq!(kept.get("meta"), Some(&serde_json::json!({ "version": 1 })));
        assert_eq!(kept.get("drawing"), Some(&serde_json::json!({ "label_size_ratio": 0.3 })));
        for gone in ["subpart_id_separator", "subpart_first_id", "connection_grid_size", "ngspice", "top_level_sheets", "page_layout_descr_file", "legacy_lib_list"] {
            assert!(!kept.contains_key(gone), "{gone}");
        }
        assert_eq!(top_level_sheets(pro), vec![("a.kicad_sch".to_string(), "A".to_string())]);
    }

    /// `PARAM<int>( "subpart_id_separator", .., 0, 0, 126 )` and `( "subpart_first_id", .., 'A', '1', 'z' )`: a value out of range is the default.
    #[test]
    fn a_unit_notation_out_of_range_is_kicads_default() {
        let mut e = SchExtras::default();
        apply(&mut e, r#"{ "schematic": { "subpart_id_separator": 200, "subpart_first_id": 48 } }"#);
        assert_eq!((e.subpart_id_separator, e.subpart_first_id), (None, None));
        apply(&mut e, r#"{ "schematic": { "subpart_id_separator": 0, "subpart_first_id": 65 } }"#);
        assert_eq!((e.subpart_id_separator, e.subpart_first_id, e.project_schematic), (None, None, None));
    }
}
