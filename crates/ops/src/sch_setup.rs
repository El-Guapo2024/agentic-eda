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
}
