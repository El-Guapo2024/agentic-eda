//! Placing something out of KiCad's installed libraries: the definition goes in with the command.
//!
//! A symbol the design does not know (`Amplifier_Operational:LM358`) or a footprint the board does not know
//! (`MountingHole:MountingHole_3.2mm_M3`) is drawn from a library file KiCad.app ships. The ops verbs do not read libraries -- a command
//! has to replay and undo the same on a machine without them -- so the studio's server, which does know where the libraries are,
//! rewrites the command before it is applied (`board::step_quiet`):
//!
//!   * `AddSymbol { lib_id }` becomes one command that first keeps the symbol's definition with the schematic
//!     (`Cmd::EmbedLibSymbol`, KiCad's `SCH_SCREEN::AddLibSymbol`), then adds the instance with the library's own Value and Footprint where the
//!     command left them empty (a new `SCH_SYMBOL` copies its fields from the `LIB_SYMBOL`), then the library's Datasheet. One undo step takes
//!     all of it back.
//!   * `SetSymbolLibIds { changes }` (Edit Symbol Library Links) keeps the definition of every installed symbol it links to first, the same way.
//!   * `PlaceFootprint { footprint }` carries the footprint's pads, graphics and courtyard as `definition`.
//!
//! A symbol the model already resolves (the intent's own, one it already uses, one the design keeps) is left alone: the design's copy wins.

use eda_model::ir::Design;
use eda_model::ConstraintModel;
use eda_ops::Cmd;
use std::path::Path;

/// `cmd` with the definitions of the installed symbols and footprints it names, or `None` when there is nothing to add.
pub(crate) fn with_installed_definitions(cmd: &Cmd, design: &Design, model: &ConstraintModel) -> Option<Cmd> {
    rewrite(cmd, design, model, &eda_kicad::default_symbol_library_root(), &eda_kicad::default_footprint_library_root())
}

/// A library symbol the design has no definition of: a `Lib:Name` the model's explicit symbols (the intent's, a library's it resolved, a published one) and the project
/// symbol library do not hold.
fn lacks_definition(lib_id: &str, design: &Design, model: &ConstraintModel) -> bool {
    lib_id.split_once(':').is_some_and(|(l, n)| !l.is_empty() && !n.is_empty())
        && !eda_model::is_synthetic_lib_id(lib_id)
        && !model.symbols.iter().any(|s| s.lib_id == lib_id)
        && !design.symbol_library.as_ref().is_some_and(|l| l.by_lib_id(lib_id).is_some())
}

fn rewrite(cmd: &Cmd, design: &Design, model: &ConstraintModel, symbols: &Path, footprints: &Path) -> Option<Cmd> {
    match cmd {
        Cmd::Batch { cmds } => {
            let rewritten: Vec<Option<Cmd>> = cmds.iter().map(|c| rewrite(c, design, model, symbols, footprints)).collect();
            rewritten.iter().any(Option::is_some).then(|| Cmd::Batch { cmds: cmds.iter().zip(rewritten).map(|(old, new)| new.unwrap_or_else(|| old.clone())).collect() })
        }
        Cmd::OnSheet { sheet, cmd } => rewrite(cmd, design, model, symbols, footprints).map(|inner| Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(inner) }),
        Cmd::AddSymbol { id, lib_id, at, rot_millideg, value, footprint, unit } => {
            // The design's own definition (the intent's, a published one, one a placed symbol already needed) stands.
            if !lacks_definition(lib_id, design, model) {
                return None;
            }
            let (definition, summary) = crate::library_search::installed_symbol(symbols, lib_id)?;
            let place = Cmd::AddSymbol {
                id: id.clone(),
                lib_id: lib_id.clone(),
                at: *at,
                rot_millideg: *rot_millideg,
                // A new instance starts with the library symbol's fields, unless the caller chose them.
                value: if value.is_empty() { summary.value.clone() } else { value.clone() },
                footprint: if footprint.is_empty() { summary.footprint.clone() } else { footprint.clone() },
                unit: *unit,
            };
            let mut cmds = vec![Cmd::EmbedLibSymbol { symbol: definition }, place];
            let datasheet = summary.datasheet.trim();
            if !datasheet.is_empty() && datasheet != "~" {
                cmds.push(Cmd::EditSymbolFields { id: id.clone(), value: None, footprint: None, datasheet: Some(datasheet.to_string()) });
            }
            Some(Cmd::Batch { cmds })
        }
        Cmd::SetSymbolLibIds { changes, .. } => {
            let mut embeds: Vec<Cmd> = Vec::new();
            for (_, to) in changes {
                if lacks_definition(to, design, model) && !embeds.iter().any(|e| matches!(e, Cmd::EmbedLibSymbol { symbol } if &symbol.lib_id == to)) {
                    if let Some((definition, _)) = crate::library_search::installed_symbol(symbols, to) {
                        embeds.push(Cmd::EmbedLibSymbol { symbol: definition });
                    }
                }
            }
            (!embeds.is_empty()).then(|| {
                embeds.push(cmd.clone());
                Cmd::Batch { cmds: embeds }
            })
        }
        Cmd::PlaceFootprint { footprint, at, reference, value, definition: None } => {
            // The model knows it already (the intent's, a library's it resolved, the built-in table): nothing to bring.
            let probe = eda_model::Part { reference: "?".into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(footprint.trim().to_string()), symbol: None, datasheet: None, pins: vec![], body_um: None, edge: None };
            if model.footprint_of(&probe).is_some() {
                return None;
            }
            let definition = crate::library_search::installed_footprint(footprints, footprint.trim())?;
            Some(Cmd::PlaceFootprint { footprint: footprint.clone(), at: *at, reference: reference.clone(), value: value.clone(), definition: Some(definition) })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Point;

    fn design() -> Design {
        serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
    }

    const LM358: &str = r##"(kicad_symbol_lib (version 20251024)
        (symbol "LM358" (property "Reference" "U") (property "Value" "LM358") (property "Footprint" "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm")
            (property "Datasheet" "http://ti.com/lm358.pdf") (property "Description" "Dual op-amp")
            (symbol "LM358_1_1" (pin input line (at 0 0 0) (length 1) (name "+") (number "3")) (pin input line (at 0 0 0) (length 1) (name "-") (number "2")) (pin output line (at 0 0 0) (length 1) (name "~") (number "1")))
            (symbol "LM358_2_1" (pin input line (at 0 0 0) (length 1) (name "+") (number "5")))
            (symbol "LM358_3_1" (pin power_in line (at 0 0 0) (length 1) (name "V+") (number "8"))))
        (symbol "Plain" (property "Reference" "U") (property "Value" "Plain") (property "Datasheet" "~") (symbol "Plain_1_1" (pin input line (at 0 0 0) (length 1) (name "A") (number "1")))))"##;

    fn add(lib_id: &str, value: &str, footprint: &str) -> Cmd {
        Cmd::AddSymbol { id: "U1".into(), lib_id: lib_id.into(), at: Point { x: 10_000, y: 10_000 }, rot_millideg: 0, value: value.into(), footprint: footprint.into(), unit: 1 }
    }

    fn root_with_lm358(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("eda-library-place-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Amplifier_Operational.kicad_sym"), LM358).unwrap();
        root
    }

    #[test]
    fn adding_an_installed_symbol_keeps_its_definition_and_takes_its_fields() {
        let root = root_with_lm358("add");
        let model = ConstraintModel::default();
        let out = rewrite(&add("Amplifier_Operational:LM358", "", ""), &design(), &model, &root, &root).expect("rewritten");
        let Cmd::Batch { cmds } = out else { panic!("{out:?}") };
        assert_eq!(cmds.len(), 3);
        match &cmds[0] {
            Cmd::EmbedLibSymbol { symbol } => assert_eq!((symbol.lib_id.as_str(), symbol.unit_count, symbol.pins.len()), ("Amplifier_Operational:LM358", 3, 5)),
            other => panic!("{other:?}"),
        }
        match &cmds[1] {
            Cmd::AddSymbol { value, footprint, lib_id, .. } => assert_eq!((value.as_str(), footprint.as_str(), lib_id.as_str()), ("LM358", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "Amplifier_Operational:LM358")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(&cmds[2], Cmd::EditSymbolFields { datasheet: Some(d), .. } if d == "http://ti.com/lm358.pdf"));

        // what the caller chose stays
        let out = rewrite(&add("Amplifier_Operational:LM358", "10k", "Foo:Bar"), &design(), &model, &root, &root).unwrap();
        let Cmd::Batch { cmds } = out else { panic!() };
        assert!(matches!(&cmds[1], Cmd::AddSymbol { value, footprint, .. } if value == "10k" && footprint == "Foo:Bar"));
        // a "~" datasheet is none
        let out = rewrite(&add("Amplifier_Operational:Plain", "", ""), &design(), &model, &root, &root).unwrap();
        let Cmd::Batch { cmds } = out else { panic!() };
        assert_eq!(cmds.len(), 2, "no datasheet to set");
        // no such symbol in the library: left for the model to resolve (or not)
        assert!(rewrite(&add("Amplifier_Operational:Nope", "", ""), &design(), &model, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_symbol_the_design_already_resolves_is_left_alone() {
        let root = root_with_lm358("known");
        let mut model = ConstraintModel::default();
        model.symbols.push(eda_model::LibSymbol { lib_id: "Amplifier_Operational:LM358".into(), graphics: vec![], pins: vec![], power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), reference_prefix: "U".into(), unit_count: 1, pin_names_hidden: false, pin_numbers_hidden: false, pin_name_offset_mm: 0.0, alternate: None });
        assert!(rewrite(&add("Amplifier_Operational:LM358", "", ""), &design(), &model, &root, &root).is_none());
        // nor are a synthetic id, a bare name or a library that is not installed rewritten
        let empty = ConstraintModel::default();
        for id in ["eda:U1", "LM358", "Nope:LM358", ":", "Amplifier_Operational:"] {
            assert!(rewrite(&add(id, "", ""), &design(), &empty, &root, &root).is_none(), "{id}");
        }
        // a definition the design keeps stands
        let mut d = design();
        d.symbol_library = Some(Default::default());
        d.symbol_library.as_mut().unwrap().symbols.push(eda_model::ir::LibrarySymbol::new_empty("Amplifier_Operational:LM358"));
        assert!(rewrite(&add("Amplifier_Operational:LM358", "", ""), &d, &empty, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_rewrite_reaches_into_a_batch_and_a_sheet() {
        let root = root_with_lm358("nested");
        let model = ConstraintModel::default();
        let wrapped = Cmd::OnSheet { sheet: "root".into(), cmd: Box::new(add("Amplifier_Operational:LM358", "", "")) };
        let Some(Cmd::OnSheet { cmd, sheet }) = rewrite(&wrapped, &design(), &model, &root, &root) else { panic!("not rewritten") };
        assert_eq!(sheet, "root");
        assert!(matches!(*cmd, Cmd::Batch { .. }));
        let batch = Cmd::Batch { cmds: vec![Cmd::DeleteWire { id: "w".into() }, add("Amplifier_Operational:LM358", "", "")] };
        let Some(Cmd::Batch { cmds }) = rewrite(&batch, &design(), &model, &root, &root) else { panic!() };
        assert!(matches!(cmds[0], Cmd::DeleteWire { .. }) && matches!(cmds[1], Cmd::Batch { .. }));
        assert!(rewrite(&Cmd::Batch { cmds: vec![Cmd::DeleteWire { id: "w".into() }] }, &design(), &model, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn linking_a_symbol_to_an_installed_one_keeps_its_definition_first() {
        let root = root_with_lm358("links");
        let model = ConstraintModel::default();
        let link = Cmd::SetSymbolLibIds { changes: vec![("Device:R".into(), "Amplifier_Operational:LM358".into()), ("Device:C".into(), "Amplifier_Operational:LM358".into())], update_fields: true };
        let Some(Cmd::Batch { cmds }) = rewrite(&link, &design(), &model, &root, &root) else { panic!("not rewritten") };
        assert_eq!(cmds.len(), 2, "one copy of the definition for both links, then the command");
        assert!(matches!(&cmds[0], Cmd::EmbedLibSymbol { symbol } if symbol.lib_id == "Amplifier_Operational:LM358"));
        assert_eq!(cmds[1], link);
        // a target the design resolves, or no library has, is left alone
        let known = Cmd::SetSymbolLibIds { changes: vec![("Device:R".into(), "Device:C".into())], update_fields: false };
        let mut with_c = ConstraintModel::default();
        with_c.symbols.push(eda_model::LibSymbol { lib_id: "Device:C".into(), graphics: vec![], pins: vec![], power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), reference_prefix: "C".into(), unit_count: 1, pin_names_hidden: false, pin_numbers_hidden: false, pin_name_offset_mm: 0.0, alternate: None });
        assert!(rewrite(&known, &design(), &with_c, &root, &root).is_none());
        let nowhere = Cmd::SetSymbolLibIds { changes: vec![("Device:R".into(), "Nope:Nothing".into())], update_fields: false };
        assert!(rewrite(&nowhere, &design(), &model, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_footprint_placed_from_a_library_carries_its_pads() {
        let root = std::env::temp_dir().join(format!("eda-library-place-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("MountingHole.pretty")).unwrap();
        std::fs::write(root.join("MountingHole.pretty").join("MH.kicad_mod"), r#"(footprint "MH" (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask")))"#).unwrap();
        let model = ConstraintModel::default();
        let place = Cmd::PlaceFootprint { footprint: "MountingHole:MH".into(), at: Point { x: 1000, y: 2000 }, reference: String::new(), value: String::new(), definition: None };
        let Some(Cmd::PlaceFootprint { definition: Some(def), at, .. }) = rewrite(&place, &design(), &model, &root, &root) else { panic!("not rewritten") };
        assert_eq!((def.name.as_str(), def.pads.len(), at), ("MountingHole:MH", 1, Point { x: 1000, y: 2000 }));
        // already carrying one, or not installed: left as it is
        let carrying = Cmd::PlaceFootprint { footprint: "MountingHole:MH".into(), at: Point { x: 0, y: 0 }, reference: String::new(), value: String::new(), definition: Some(def) };
        assert!(rewrite(&carrying, &design(), &model, &root, &root).is_none());
        let missing = Cmd::PlaceFootprint { footprint: "MountingHole:Nope".into(), at: Point { x: 0, y: 0 }, reference: String::new(), value: String::new(), definition: None };
        assert!(rewrite(&missing, &design(), &model, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
