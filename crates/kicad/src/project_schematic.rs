//! The `schematic` object of a KiCad project file (`.kicad_pro`), the share of `SCHEMATIC_SETTINGS` (`eeschema/schematic_settings.cpp`, a `NESTED_SETTINGS` at
//! "schematic") and of `PROJECT_FILE` (`common/project/project_file.cpp`: `schematic.top_level_sheets`, `schematic.legacy_lib_*`, `schematic.bus_aliases`)
//! an imported schematic keeps (r7 H5, M1).
//!
//! * The settings the studio has a setting of its own for go into the design's root extras: the unit notation (`subpart_id_separator`, `subpart_first_id`), the
//!   inter-sheet references (`drawing.intersheets_ref_*`), the simulator's settings (`ngspice`), Schematic Setup's Formatting (the rest of `drawing.*`:
//!   [`formatting`]), Annotation (`annotation.*`, `annotate_start_num`, `reuse_designators`: [`annotation`]) and Field Name Templates (`drawing.field_names`:
//!   [`field_names`]). The connection grid is read by `eda import-kicad` itself.
//! * The rest is kept as KiCad wrote it ([`kept`], `SchExtras::project_schematic`), and the derived project starts its `schematic` object from it, so kicad-cli
//!   reads the formatting, annotation, BOM, netlist and simulator keys of the project as KiCad saved them.
//! * Left out: the keys the studio holds itself (above, written back from the extras), the project's list of top-level sheets (the import reads them, and the
//!   derived project has one root), its bus aliases (the design's own, `Design::bus_aliases`), and the keys that name files of the original folder, which the
//!   derived folder does not have (`legacy_lib_dir`, `legacy_lib_list`, `page_layout_descr_file`). **Differs**: a project's own drawing sheet
//!   (`page_layout_descr_file`) is not carried; the derived project draws KiCad's default.

use eda_model::sch_extras::{IntersheetRefs, NgspiceSettings, SchExtras};
use eda_model::sch_setup::{AnnotationSettings, SchFormatting, TemplateFieldName};
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
    "variants",
    "drawing.intersheets_ref_show",
    "drawing.intersheets_ref_own_page",
    "drawing.intersheets_ref_short",
    "drawing.intersheets_ref_prefix",
    "drawing.intersheets_ref_suffix",
    "drawing.default_text_size",
    "drawing.default_line_thickness",
    "drawing.pin_symbol_size",
    "drawing.text_offset_ratio",
    "drawing.label_size_ratio",
    "drawing.overbar_offset_ratio",
    "drawing.junction_size_choice",
    "drawing.hop_over_size_choice",
    "drawing.dashed_lines_dash_length_ratio",
    "drawing.dashed_lines_gap_length_ratio",
    "drawing.operating_point_overlay_v_precision",
    "drawing.operating_point_overlay_v_range",
    "drawing.operating_point_overlay_i_precision",
    "drawing.operating_point_overlay_i_range",
    "drawing.field_names",
    "annotate_start_num",
    "annotation.sort_order",
    "annotation.method",
    "reuse_designators",
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
                    // an object the studio took every key of is not kept empty (`annotation`, `drawing`)
                    if o.is_empty() {
                        out.remove(outer);
                    }
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

/// Schematic Setup > Formatting's sizes and ratios as `SCHEMATIC_SETTINGS`' `PARAM`s read a project's `schematic` object (`schematic_settings.cpp`): each key
/// its default when missing or not a number; the sizes in mils (`PARAM_SCALED<int>( .., 1 / schIUScale.IU_PER_MILS )`, rounded to the schematic's 100 nm unit;
/// out of range is kept as written and read as KiCad reads it by [`SchFormatting::effective`]); a range that is not one of the list is the default (the panel's
/// `SetStringSelection` finds none). `registerMigration( 0, 1 )`: a `schematic` at schema 0 takes its label margin ratio from `text_offset_ratio`. `None` when
/// the project's are KiCad's defaults.
pub fn formatting(schematic: &Value) -> Option<SchFormatting> {
    let drawing = schematic.get("drawing")?;
    let d = SchFormatting::default();
    let num = |k: &str| drawing.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    let size = |k: &str, default: eda_model::ir::Nm| num(k).map_or(default, |mils| eda_model::ir::Nm::new(eda_model::units::kiround(mils * 254.0) * 100));
    let choice = |k: &str, default: u32| drawing.get(k).and_then(Value::as_u64).filter(|c| (*c as usize) < eda_model::sch_setup::JUNCTION_SIZE_MULT.len()).map_or(default, |c| c as u32);
    let precision = |k: &str, default: u32| drawing.get(k).and_then(Value::as_i64).map_or(default, |p| p.clamp(1, 10) as u32);
    let range = |k: &str, list: &[&str], default: &str| drawing.get(k).and_then(Value::as_str).filter(|r| list.contains(r)).unwrap_or(default).to_string();
    let at_schema_0 = schematic.pointer("/meta/version").and_then(Value::as_i64) == Some(0);
    let text_offset = num("text_offset_ratio").unwrap_or(d.text_offset_ratio);
    let f = SchFormatting {
        default_text_size_nm: size("default_text_size", d.default_text_size_nm),
        default_line_width_nm: size("default_line_thickness", d.default_line_width_nm),
        pin_symbol_size_nm: size("pin_symbol_size", d.pin_symbol_size_nm),
        overbar_offset_ratio: num("overbar_offset_ratio").unwrap_or(d.overbar_offset_ratio),
        text_offset_ratio: text_offset,
        label_size_ratio: if at_schema_0 && num("text_offset_ratio").is_some() { text_offset } else { num("label_size_ratio").unwrap_or(d.label_size_ratio) },
        junction_size_choice: choice("junction_size_choice", d.junction_size_choice),
        hop_over_size_choice: choice("hop_over_size_choice", d.hop_over_size_choice),
        dash_ratio: num("dashed_lines_dash_length_ratio").unwrap_or(d.dash_ratio),
        gap_ratio: num("dashed_lines_gap_length_ratio").unwrap_or(d.gap_ratio),
        opo_v_precision: precision("operating_point_overlay_v_precision", d.opo_v_precision),
        opo_v_range: range("operating_point_overlay_v_range", &eda_model::sch_setup::OPO_V_RANGES, &d.opo_v_range),
        opo_i_precision: precision("operating_point_overlay_i_precision", d.opo_i_precision),
        opo_i_range: range("operating_point_overlay_i_range", &eda_model::sch_setup::OPO_I_RANGES, &d.opo_i_range),
    };
    (f != d).then_some(f)
}

/// Schematic Setup > Annotation as `SCHEMATIC_SETTINGS` reads it: `annotation.sort_order` (0 to 1) and `annotation.method` (0 to 2), each its default out of
/// range (`PARAM<int>`'s min and max), `annotate_start_num`, and `reuse_designators` (true when missing). `None` when they are KiCad's defaults.
pub fn annotation(schematic: &Value) -> Option<AnnotationSettings> {
    let d = AnnotationSettings::default();
    let ranged = |k: &str, max: u64| schematic.pointer(k).and_then(Value::as_u64).filter(|v| *v <= max).map_or(0, |v| v as u32);
    let a = AnnotationSettings {
        sort_order: ranged("/annotation/sort_order", 1),
        method: ranged("/annotation/method", 2),
        start_num: schematic.get("annotate_start_num").and_then(Value::as_i64).unwrap_or(d.start_num),
        reuse_designators: schematic.get("reuse_designators").and_then(Value::as_bool).unwrap_or(d.reuse_designators),
    };
    (a != d).then_some(a)
}

/// Schematic Setup > Field Name Templates as `SCHEMATIC_SETTINGS`' `drawing.field_names` reader takes it: an entry lacking any of `name`, `url` and `visible`
/// is skipped, and an empty list leaves none.
pub fn field_names(schematic: &Value) -> Vec<TemplateFieldName> {
    let Some(list) = schematic.pointer("/drawing/field_names").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|e| {
            let name = e.get("name")?.as_str()?;
            Some(TemplateFieldName { name: name.to_string(), url: e.get("url")?.as_bool()?, visible: e.get("visible")?.as_bool()? })
        })
        .collect()
}

/// Put a project's Schematic Setup (`project_json`, the `.kicad_pro`'s text) into a root sheet's extras: the unit notation, the inter-sheet references, the
/// simulator's settings, Formatting, Annotation, Field Name Templates and the rest of the `schematic` object ([`kept`]). A text that is not JSON, or has no
/// `schematic` object, changes nothing.
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
    extras.formatting = formatting(schematic);
    extras.annotation = annotation(schematic);
    extras.field_name_templates = field_names(schematic);
    extras.variants = variants(schematic);
    extras.project_schematic = kept(schematic);
}

/// The design variants of a project's `schematic.variants` (KiCad 25ec2335 `SCHEMATIC_SETTINGS`' `PARAM_LAMBDA<nlohmann::json>( "variants" )` loader: an
/// array, each entry with a `name` kept with its `description`, empty when it has none; anything else is no variant), name -> description.
pub fn variants(schematic: &Value) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for entry in schematic.get("variants").and_then(Value::as_array).into_iter().flatten() {
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            out.insert(name.to_string(), entry.get("description").and_then(Value::as_str).unwrap_or_default().to_string());
        }
    }
    out
}

/// The `schematic.variants` array the derived project gets (the same `PARAM_LAMBDA`'s writer: `{ "name" }` per variant, `"description"` only when it is not
/// empty, in the map's order): every variant of the design ([`eda_model::variants::names`], so a name only a symbol's record carries is listed too, as
/// `SCHEMATIC::LoadVariants` registers it) with the description the root keeps. Empty for a design with none.
pub fn variants_json(design: &eda_model::ir::Design) -> Vec<Value> {
    let descriptions = design.schematic.as_ref().map(|s| &s.extras.variants);
    eda_model::variants::names(design)
        .into_iter()
        .map(|name| {
            let mut entry = Map::new();
            let description = descriptions.and_then(|d| d.get(&name)).cloned().unwrap_or_default();
            entry.insert("name".into(), Value::String(name));
            if !description.is_empty() {
                entry.insert("description".into(), Value::String(description));
            }
            Value::Object(entry)
        })
        .collect()
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

/// The simulation model files the design names by a path inside its project folder (r7 M6): each `Sim.Library` field of a placed symbol (`user_fields`, on every
/// screen), of a library symbol (`LibConnectivity::user_fields`) and of the simulation fields the studio writes (`ConstraintModel::symbol_sim_fields`), as a
/// path relative to the project -- `${KIPRJMOD}/` taken off -- that stays inside it. KiCad finds such a file in the project's folder
/// (`SIM_LIB_MGR::ResolveLibraryPath`: `m_project->AbsolutePath( expandedPath )`), so `eda import-kicad` copies it into the board folder and the derived files
/// get it beside them (`eda_kicad_engine`'s project writer). An absolute path, a path with another variable, and one that climbs out of the folder are
/// left to KiCad's own search and not carried (**Differs**: such a file is found only on the machine it is on).
pub fn sim_library_files(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Vec<std::path::PathBuf> {
    let mut names: Vec<&str> = Vec::new();
    let screens = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()));
    for sch in screens {
        names.extend(sch.user_fields.values().filter_map(|f| f.get("Sim.Library")).map(String::as_str));
    }
    names.extend(model.symbols.iter().flat_map(|s| s.connectivity.user_fields.iter()).filter(|(n, _)| n == "Sim.Library").map(|(_, t)| t.as_str()));
    names.extend(model.symbol_sim_fields.values().flat_map(|f| f.iter()).filter(|(n, _)| n == "Sim.Library").map(|(_, t)| t.as_str()));
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for name in names {
        let name = name.trim().replace('\\', "/");
        let rel = name.strip_prefix("${KIPRJMOD}/").unwrap_or(&name);
        let path = std::path::Path::new(rel);
        if rel.is_empty() || rel.contains("${") || !path.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
            continue;
        }
        let path = path.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect::<std::path::PathBuf>();
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
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
        assert_eq!(e.annotation.as_ref().map(|a| a.start_num), Some(10), "Annotation's start number is the studio's own");
        assert_eq!(e.formatting.as_ref().map(|f| f.label_size_ratio), Some(0.3), "and so is Formatting's label margin ratio");
        let kept = e.project_schematic.unwrap();
        assert_eq!(kept.get("meta"), Some(&serde_json::json!({ "version": 1 })));
        assert!(!kept.contains_key("annotate_start_num") && !kept.contains_key("drawing"), "the studio's settings are not kept twice: {kept:?}");
        for gone in ["subpart_id_separator", "subpart_first_id", "connection_grid_size", "ngspice", "top_level_sheets", "page_layout_descr_file", "legacy_lib_list"] {
            assert!(!kept.contains_key(gone), "{gone}");
        }
        assert_eq!(top_level_sheets(pro), vec![("a.kicad_sch".to_string(), "A".to_string())]);
    }

    /// Formatting, Annotation and Field Name Templates are read as `SCHEMATIC_SETTINGS` reads them: sizes in mils, a choice out of the list and a range KiCad's
    /// list lacks are the defaults, an entry of `field_names` without its three keys is skipped, and a schema-0 project takes its margin ratio from the offset.
    #[test]
    fn formatting_annotation_and_field_name_templates_are_read_as_kicad_reads_them() {
        let pro = r#"{ "schematic": { "drawing": { "default_text_size": 60.0, "default_line_thickness": 8.0, "pin_symbol_size": 30.0, "junction_size_choice": 5, "hop_over_size_choice": 9,
            "text_offset_ratio": 0.2, "label_size_ratio": 0.4, "dashed_lines_dash_length_ratio": 10.0, "operating_point_overlay_v_range": "mV", "operating_point_overlay_i_range": "kA",
            "operating_point_overlay_v_precision": 4,
            "field_names": [ { "name": "MPN", "visible": false, "url": false }, { "name": "Link", "visible": true, "url": true }, { "name": "Broken", "visible": true } ] },
            "annotation": { "method": 2, "sort_order": 3 }, "annotate_start_num": 100, "reuse_designators": false, "meta": { "version": 1 } } }"#;
        let mut e = SchExtras::default();
        apply(&mut e, pro);
        let f = e.formatting.clone().expect("formatting");
        assert_eq!((f.default_text_size_nm.get(), f.default_line_width_nm.get(), f.pin_symbol_size_nm.get()), (1_524_000, 203_200, 762_000));
        assert_eq!((f.junction_size_choice, f.hop_over_size_choice), (5, 0), "a hop-over choice past the six is the default");
        assert_eq!((f.text_offset_ratio, f.label_size_ratio, f.dash_ratio, f.gap_ratio), (0.2, 0.4, 10.0, 3.0));
        assert_eq!((f.opo_v_range.as_str(), f.opo_i_range.as_str(), f.opo_v_precision), ("mV", "~A", 4), "KiCad's list says KA");
        let a = e.annotation.clone().expect("annotation");
        assert_eq!((a.sort_order, a.method, a.start_num, a.reuse_designators), (0, 2, 100, false));
        assert_eq!(e.field_name_templates.iter().map(|t| (t.name.as_str(), t.visible, t.url)).collect::<Vec<_>>(), vec![("MPN", false, false), ("Link", true, true)]);
        assert!(e.project_schematic.is_none_or(|k| !k.contains_key("drawing") && !k.contains_key("annotation")));
        let mut old = SchExtras::default();
        apply(&mut old, r#"{ "schematic": { "drawing": { "text_offset_ratio": 0.25, "label_size_ratio": 0.375 }, "meta": { "version": 0 } } }"#);
        assert_eq!(old.formatting.map(|f| f.label_size_ratio), Some(0.25), "registerMigration( 0, 1 )");
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
