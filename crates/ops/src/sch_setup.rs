//! Schematic Setup's verbs for the pages that keep project settings (`eeschema/dialogs/dialog_schematic_setup.cpp`): General > Formatting's sizes and
//! ratios, General > Annotation, General > Field Name Templates, General > BOM Presets and Project > Bus Alias Definitions. Each is the page's OK
//! (`TransferDataFromWindow`): the whole page in, the whole page replaced, one undo step, refused when it changes nothing. The settings sit on the root
//! sheet's extras (`eda_model::sch_setup`) and the design's bus aliases; the derived project carries them to kicad-cli (`eda_kicad::write_schematic_setup`).
//! Net Classes is Board Setup's verb (`Cmd::SetNetClasses`), the two dialogs editing one table as KiCad's edit the project's one `NET_SETTINGS`.

use eda_model::ir::{BusAlias, Design};
use eda_model::sch_setup::{AnnotationSettings, SchFormatting, TemplateFieldName};
use eda_model::CheckResult;

type Done = Result<(), Vec<CheckResult>>;

fn no_change(what: &str, why: &str) -> Done {
    Err(vec![CheckResult::fail("ops_no_change", what, why)])
}

/// `Cmd::SetSchFormatting`: Formatting's settings but the connection grid and the inter-sheet references (`PANEL_SETUP_FORMATTING::TransferDataFromWindow`
/// takes the controls as they are; [`SchFormatting::validate`] refuses only what the controls cannot hold). KiCad's defaults are kept as none.
pub(crate) fn set_formatting(design: &mut Design, settings: &SchFormatting) -> Done {
    settings.validate().map_err(|(what, why)| vec![CheckResult::fail("ops_bad_sch_formatting", what, why)])?;
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    let now = (*settings != SchFormatting::default()).then(|| settings.clone());
    if sch.extras.formatting == now {
        return no_change("formatting", "the formatting is already set so");
    }
    sch.extras.formatting = now;
    Ok(())
}

/// `Cmd::SetAnnotationSettings`: `PANEL_EESCHEMA_ANNOTATION_OPTIONS::TransferDataFromWindow` -- the order, the numbering, the start number, "Allow reference
/// reuse" and the symbol unit notation (`m_SubpartFirstId`, `m_SubpartIdSeparator`; one of the panel's seven, [`eda_model::sch_setup::UNIT_NOTATIONS`]).
/// KiCad's defaults are kept as none.
pub(crate) fn set_annotation(design: &mut Design, settings: &AnnotationSettings, first_id: u32, separator: u32) -> Done {
    settings.validate().map_err(|(what, why)| vec![CheckResult::fail("ops_bad_annotation", what, why)])?;
    if !eda_model::sch_setup::UNIT_NOTATIONS.iter().any(|(_, f, s)| *f == first_id && *s == separator) {
        return Err(vec![CheckResult::fail("ops_bad_annotation", "unit_notation", format!("first unit {first_id} with separator {separator} is not one of A, .A, -A, _A, .1, -1, _1"))]);
    }
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    let now = (*settings != AnnotationSettings::default()).then(|| settings.clone());
    let first = (first_id != 'A' as u32).then_some(first_id);
    let sep = (separator != 0).then_some(separator);
    if sch.extras.annotation == now && sch.extras.subpart_first_id == first && sch.extras.subpart_id_separator == sep {
        return no_change("annotation", "the annotation settings are already set so");
    }
    sch.extras.annotation = now;
    sch.extras.subpart_first_id = first;
    sch.extras.subpart_id_separator = sep;
    Ok(())
}

/// `Cmd::SetFieldNameTemplates`: `PANEL_TEMPLATE_FIELDNAMES::TransferDataFromWindow` -- `DeleteAllFieldNameTemplates` and every row added again as
/// `AddTemplateFieldName` adds it ([`eda_model::sch_setup::kept_template_fields`]).
pub(crate) fn set_field_name_templates(design: &mut Design, fields: &[TemplateFieldName]) -> Done {
    let kept = eda_model::sch_setup::kept_template_fields(fields);
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    if sch.extras.field_name_templates == kept {
        return no_change("field_name_templates", "the field name templates are already so");
    }
    sch.extras.field_name_templates = kept;
    Ok(())
}

/// `Cmd::SetBomPresets`: `PANEL_BOM_PRESETS::TransferDataFromWindow` -- the project's saved BOM presets and BOM formatting presets (`m_BomPresets`,
/// `m_BomFmtPresets`, the `.kicad_pro`'s `schematic.bom_presets` / `bom_fmt_presets`), of which the page can only delete some: `bom_presets` and
/// `bom_fmt_presets` are the names of those kept, each the first preset of its name the project has. A name the project lacks is refused.
pub(crate) fn set_bom_presets(design: &mut Design, bom_presets: &[String], bom_fmt_presets: &[String]) -> Done {
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    let mut project = sch.extras.project_schematic.clone().unwrap_or_default();
    let mut changed = false;
    for (key, names) in [("bom_presets", bom_presets), ("bom_fmt_presets", bom_fmt_presets)] {
        let list: Vec<serde_json::Value> = project.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let name_of = |p: &serde_json::Value| p.get("name").and_then(|n| n.as_str()).map(str::to_string);
        if let Some(missing) = names.iter().find(|n| !list.iter().any(|p| name_of(p).as_deref() == Some(n.as_str()))) {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", key, format!("the project has no preset named {missing:?}"))]);
        }
        let kept: Vec<serde_json::Value> = list.iter().filter(|p| name_of(p).is_some_and(|n| names.contains(&n))).cloned().collect();
        if kept.len() != list.len() {
            changed = true;
            project.insert(key.to_string(), serde_json::Value::Array(kept));
        }
    }
    if !changed {
        return no_change("bom_presets", "no preset was deleted");
    }
    sch.extras.project_schematic = (!project.is_empty()).then_some(project);
    Ok(())
}

/// What the Symbol Fields Table keeps when it closes (`savePresetsToSchematic`, UI-LEFT-2 SS13): the project's view presets and format presets as the dialog holds
/// them (each replacing the project's list), and the format it closes on (`m_BomFmtSettings`).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct BomClose<'a> {
    pub presets: Option<&'a [serde_json::Value]>,
    pub fmt_presets: Option<&'a [serde_json::Value]>,
    pub fmt_view: Option<&'a serde_json::Value>,
}

/// `Cmd::SetBomView`: the Symbol Fields Table's "Save preset..." (`onBomPresetChanged`: `wxTextEntryDialog( .., _( "BOM preset name:" ), _( "Save BOM Preset" ) )`, a
/// built-in name refused, an existing one of the project's overwritten after "Overwrite existing preset?"), "Delete preset..." and the view the table closes on
/// (`savePresetsToSchematic`: `m_BomSettings`), in the project's `schematic.bom_presets` and `schematic.bom_settings` (`SCHEMATIC_SETTINGS`). A preset is
/// `bom_settings.cpp`'s JSON as the studio sends it.
pub(crate) fn set_bom_view(design: &mut Design, save: Option<&serde_json::Value>, delete: Option<&str>, view: Option<&serde_json::Value>, close: &BomClose<'_>) -> Done {
    /// `BOM_PRESET::BuiltInPresets`' names (read-only, `m_readOnly`).
    const BUILT_IN: [&str; 4] = ["Default Editing", "Grouped By Value", "Grouped By Value and Footprint", "Attributes"];
    /// `BOM_FMT_PRESET::BuiltInPresets`' names.
    const BUILT_IN_FMT: [&str; 3] = ["CSV", "TSV", "Semicolons"];
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    let mut project = sch.extras.project_schematic.clone().unwrap_or_default();
    let before = project.clone();
    let mut list: Vec<serde_json::Value> = project.get("bom_presets").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let name_of = |p: &serde_json::Value| p.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    if let Some(p) = save {
        let name = name_of(p);
        if BUILT_IN.contains(&name.as_str()) {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", name.as_str(), "Default presets cannot be modified.\nPlease use a different name.")]);
        }
        if p.get("fields_ordered").and_then(|f| f.as_array()).is_none() {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", name.as_str(), "a preset needs its columns (fields_ordered)")]);
        }
        match list.iter_mut().find(|q| name_of(q) == name) {
            Some(q) => *q = p.clone(),
            None => list.push(p.clone()),
        }
    }
    if let Some(name) = delete {
        if BUILT_IN.contains(&name) {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", name, "Default presets cannot be modified.\nPlease use a different name.")]);
        }
        let n = list.len();
        list.retain(|q| name_of(q) != name);
        if list.len() == n {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", name, format!("the project has no preset named {name:?}"))]);
        }
    }
    if save.is_some() || delete.is_some() {
        project.insert("bom_presets".into(), serde_json::Value::Array(list));
    }
    if let Some(v) = view {
        project.insert("bom_settings".into(), v.clone());
    }
    // `savePresetsToSchematic`: the presets that are not read-only, in the dialog's (name) order, each list only when it changed
    let read_only = |p: &serde_json::Value, names: &[&str]| names.contains(&name_of(p).as_str());
    if let Some(all) = close.presets {
        if let Some(bad) = all.iter().find(|p| read_only(p, &BUILT_IN) || p.get("fields_ordered").and_then(|f| f.as_array()).is_none()) {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", name_of(bad).as_str(), "a project preset needs a name that is not a built-in one and its columns (fields_ordered)")]);
        }
        project.insert("bom_presets".into(), serde_json::Value::Array(all.to_vec()));
    }
    if let Some(all) = close.fmt_presets {
        if let Some(bad) = all.iter().find(|p| read_only(p, &BUILT_IN_FMT) || serde_json::from_value::<crate::fields_table::BomFmt>((*p).clone()).is_err()) {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", name_of(bad).as_str(), "a project format preset needs a name that is not a built-in one and its delimiters")]);
        }
        project.insert("bom_fmt_presets".into(), serde_json::Value::Array(all.to_vec()));
    }
    if let Some(v) = close.fmt_view {
        if serde_json::from_value::<crate::fields_table::BomFmt>(v.clone()).is_err() {
            return Err(vec![CheckResult::fail("ops_bad_bom_presets", "bom_fmt_settings", "a format needs its delimiters")]);
        }
        project.insert("bom_fmt_settings".into(), v.clone());
    }
    if project == before {
        return no_change("bom_settings", "the table's presets and view are already so");
    }
    sch.extras.project_schematic = (!project.is_empty()).then_some(project);
    Ok(())
}

/// `Cmd::SetBusAliases`: `PANEL_SETUP_BUSES::TransferDataFromWindow` over `SCHEMATIC::SetBusAliases` and the project's `schematic.bus_aliases`. The grid's own
/// vetoes are refused with its words: two aliases of one name ("Alias name '%s' already in use.", `OnAliasesGridCellChanging`, names compared as typed) and an
/// empty member ("Member net/alias name cannot be empty.", `OnMemberGridCellChanging`). A member typed as a space-separated list is several members, as the
/// grid splits it (`wxStringTokenizer( name, " " )`). Then as the project keeps them: a name and each member stripped, an alias with no name dropped, sorted
/// by name (the project's map; the page lists them without regard to case).
pub(crate) fn set_bus_aliases(design: &mut Design, aliases: &[BusAlias]) -> Done {
    let mut out: Vec<BusAlias> = Vec::new();
    for (i, a) in aliases.iter().enumerate() {
        // (a row never named is never vetoed: the grid checks a name as it is typed, and an alias with no name is dropped below)
        if !a.name.is_empty() && aliases[..i].iter().any(|b| b.name == a.name) {
            return Err(vec![CheckResult::fail("ops_bad_bus_alias", a.name.as_str(), format!("Alias name '{}' already in use.", a.name))]);
        }
        let mut members: Vec<String> = Vec::new();
        for m in &a.members {
            if m.is_empty() {
                return Err(vec![CheckResult::fail("ops_bad_bus_alias", a.name.as_str(), "Member net/alias name cannot be empty.")]);
            }
            members.extend(m.split(' ').map(str::trim).filter(|t| !t.is_empty()).map(String::from));
        }
        let name = a.name.trim();
        if name.is_empty() || out.iter().any(|b| b.name == name) {
            continue;
        }
        out.push(BusAlias { name: name.to_string(), members });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    if design.bus_aliases == out {
        return no_change("bus_aliases", "the bus aliases are already so");
    }
    design.bus_aliases = out;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Nm;

    fn design() -> Design {
        let mut d: Design = serde_json::from_value(serde_json::json!({ "schema": eda_model::ir::SCHEMA_VERSION, "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0, "stage_hashes": [] } })).unwrap();
        d.schematic = Some(crate::empty_schematic_section());
        d
    }

    fn alias(name: &str, members: &[&str]) -> BusAlias {
        BusAlias { name: name.into(), members: members.iter().map(|m| m.to_string()).collect() }
    }

    #[test]
    fn formatting_is_stored_and_kicads_defaults_are_none() {
        let mut d = design();
        let f = SchFormatting { junction_size_choice: 5, default_text_size_nm: Nm::new(1_524_000), ..SchFormatting::default() };
        set_formatting(&mut d, &f).unwrap();
        assert_eq!(d.schematic.as_ref().unwrap().extras.formatting.as_ref(), Some(&f));
        assert_eq!(set_formatting(&mut d, &f).unwrap_err()[0].check, "ops_no_change");
        set_formatting(&mut d, &SchFormatting::default()).unwrap();
        assert!(d.schematic.as_ref().unwrap().extras.formatting.is_none());
        assert_eq!(set_formatting(&mut d, &SchFormatting { hop_over_size_choice: 6, ..SchFormatting::default() }).unwrap_err()[0].check, "ops_bad_sch_formatting");
    }

    #[test]
    fn annotation_keeps_the_unit_notation_with_it() {
        let mut d = design();
        let a = AnnotationSettings { method: 1, start_num: 100, ..AnnotationSettings::default() };
        set_annotation(&mut d, &a, '1' as u32, '.' as u32).unwrap();
        let e = &d.schematic.as_ref().unwrap().extras;
        assert_eq!((e.annotation.as_ref(), e.subpart_first_id, e.subpart_id_separator), (Some(&a), Some('1' as u32), Some('.' as u32)));
        assert!(set_annotation(&mut d, &a, '1' as u32, 0).is_err(), "\"1\" with no separator is not one of the seven");
        set_annotation(&mut d, &AnnotationSettings::default(), 'A' as u32, 0).unwrap();
        let e = &d.schematic.as_ref().unwrap().extras;
        assert_eq!((e.annotation.as_ref(), e.subpart_first_id, e.subpart_id_separator), (None, None, None));
    }

    #[test]
    fn bus_aliases_are_refused_with_the_grids_words_and_kept_as_the_project_keeps_them() {
        let mut d = design();
        let e = set_bus_aliases(&mut d, &[alias("DATA", &["D0"]), alias("DATA", &["D1"])]).unwrap_err();
        assert_eq!(e[0].hint.as_deref().unwrap_or_default(), "Alias name 'DATA' already in use.");
        let e = set_bus_aliases(&mut d, &[alias("DATA", &["D0", ""])]).unwrap_err();
        assert_eq!(e[0].hint.as_deref().unwrap_or_default(), "Member net/alias name cannot be empty.");
        set_bus_aliases(&mut d, &[alias("ZED", &["Z"]), alias(" DATA ", &["D0 D1", "D2"]), alias("", &["X"]), alias("", &[])]).unwrap();
        assert_eq!(d.bus_aliases, vec![alias("DATA", &["D0", "D1", "D2"]), alias("ZED", &["Z"])], "split, stripped, the unnamed dropped, sorted by name");
        let same = d.bus_aliases.clone();
        assert_eq!(set_bus_aliases(&mut d, &same).unwrap_err()[0].check, "ops_no_change");
    }

    #[test]
    fn field_name_templates_and_bom_presets() {
        let mut d = design();
        set_field_name_templates(&mut d, &[TemplateFieldName { name: "MPN".into(), visible: true, url: false }, TemplateFieldName { name: String::new(), visible: false, url: false }]).unwrap();
        assert_eq!(d.schematic.as_ref().unwrap().extras.field_name_templates.len(), 1);
        let presets = serde_json::json!({ "bom_presets": [ { "name": "Mine" }, { "name": "Other" } ], "bom_fmt_presets": [ { "name": "Tabs" } ] });
        d.schematic.as_mut().unwrap().extras.project_schematic = presets.as_object().cloned();
        set_bom_presets(&mut d, &["Other".into()], &["Tabs".into()]).unwrap();
        let kept = d.schematic.as_ref().unwrap().extras.project_schematic.clone().unwrap();
        assert_eq!(kept["bom_presets"], serde_json::json!([ { "name": "Other" } ]));
        assert_eq!(set_bom_presets(&mut d, &["Nope".into()], &[]).unwrap_err()[0].check, "ops_bad_bom_presets");
    }

    #[test]
    fn the_fields_tables_presets_are_saved_overwritten_and_deleted_in_the_project_and_a_built_in_one_is_not() {
        let mut d = design();
        let preset = |name: &str, filter: &str| serde_json::json!({ "name": name, "fields_ordered": [{ "name": "Reference", "label": "Reference", "show": true, "group_by": false }], "sort_field": "Reference", "sort_asc": true, "filter_string": filter, "group_symbols": true, "exclude_dnp": false, "include_excluded_from_bom": false });
        set_bom_view(&mut d, Some(&preset("Mine", "")), None, None, &BomClose::default()).unwrap();
        set_bom_view(&mut d, Some(&preset("Mine", "R*")), None, None, &BomClose::default()).unwrap();
        let project = d.schematic.as_ref().unwrap().extras.project_schematic.clone().unwrap();
        let list = project["bom_presets"].as_array().unwrap();
        assert_eq!(list.len(), 1, "saved under a name it has is an overwrite");
        assert_eq!(list[0]["filter_string"], "R*");
        let e = set_bom_view(&mut d, Some(&preset("Attributes", "")), None, None, &BomClose::default()).unwrap_err();
        assert_eq!(e[0].hint.as_deref().unwrap_or_default(), "Default presets cannot be modified.\nPlease use a different name.");
        set_bom_view(&mut d, None, None, Some(&preset("", "C*")), &BomClose::default()).unwrap();
        assert_eq!(d.schematic.as_ref().unwrap().extras.project_schematic.as_ref().unwrap()["bom_settings"]["filter_string"], "C*");
        set_bom_view(&mut d, None, Some("Mine"), None, &BomClose::default()).unwrap();
        assert!(d.schematic.as_ref().unwrap().extras.project_schematic.as_ref().unwrap()["bom_presets"].as_array().unwrap().is_empty());
        assert_eq!(set_bom_view(&mut d, None, Some("Mine"), None, &BomClose::default()).unwrap_err()[0].check, "ops_bad_bom_presets");
    }

    /// UI-LEFT-2 SS13: what the table keeps when it closes (`savePresetsToSchematic`): the view presets and the format presets as the dialog held them, the view
    /// and the format it closes on, in one step; a built-in name is no project preset; nothing new is refused.
    #[test]
    fn the_fields_table_keeps_its_presets_its_view_and_its_format_when_it_closes() {
        let mut d = design();
        let preset = |name: &str| serde_json::json!({ "name": name, "fields_ordered": [{ "name": "Reference", "label": "Ref", "show": true, "group_by": false }], "sort_field": "Reference", "sort_asc": true, "filter_string": "", "group_symbols": true, "exclude_dnp": false, "include_excluded_from_bom": false });
        let fmt = |name: &str| serde_json::json!({ "name": name, "field_delimiter": "|", "string_delimiter": "", "ref_delimiter": ",", "ref_range_delimiter": "-", "keep_tabs": false, "keep_line_breaks": false });
        let (presets, fmts) = (vec![preset("A"), preset("B")], vec![fmt("Pipes")]);
        let view = preset("");
        let fmt_view = fmt("");
        set_bom_view(&mut d, None, None, Some(&view), &BomClose { presets: Some(&presets), fmt_presets: Some(&fmts), fmt_view: Some(&fmt_view) }).unwrap();
        let project = d.schematic.as_ref().unwrap().extras.project_schematic.clone().unwrap();
        assert_eq!(project["bom_presets"].as_array().unwrap().len(), 2);
        assert_eq!(project["bom_fmt_presets"][0]["field_delimiter"], "|");
        assert_eq!(project["bom_fmt_settings"]["ref_range_delimiter"], "-");
        assert_eq!(project["bom_settings"]["fields_ordered"][0]["label"], "Ref");
        // the same again changes nothing
        assert_eq!(set_bom_view(&mut d, None, None, Some(&view), &BomClose { presets: Some(&presets), fmt_presets: Some(&fmts), fmt_view: Some(&fmt_view) }).unwrap_err()[0].check, "ops_no_change");
        let bad = vec![fmt("CSV")];
        assert_eq!(set_bom_view(&mut d, None, None, None, &BomClose { fmt_presets: Some(&bad), ..BomClose::default() }).unwrap_err()[0].check, "ops_bad_bom_presets");
        // the command as the dialog sends it
        let cmd: crate::Cmd = serde_json::from_value(serde_json::json!({ "op": "set_bom_view", "view": view, "presets": [preset("A")], "fmt_presets": [], "fmt_view": fmt_view })).unwrap();
        assert!(matches!(cmd, crate::Cmd::SetBomView { presets: Some(ref p), fmt_presets: Some(ref f), .. } if p.len() == 1 && f.is_empty()));
    }
}
