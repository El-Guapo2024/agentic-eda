//! Placing something out of the libraries of the library tables: the definition goes in with the command.
//!
//! A symbol the design does not know (`Amplifier_Operational:LM358`) or a footprint the board does not know
//! (`MountingHole:MountingHole_3.2mm_M3`) is drawn from a library file: one KiCad.app ships (the global table) or one of the board's own (its project
//! table, `MyParts:Chip`). The ops verbs do not read libraries -- a command has to replay and undo the same on a machine without them -- so the studio's
//! server, which does know where the libraries are, rewrites the command before it is applied (`board::step_quiet`):
//!
//!   * `AddSymbol { lib_id }` becomes one command that first keeps the symbol's definition with the schematic
//!     (`Cmd::EmbedLibSymbol`, KiCad's `SCH_SCREEN::AddLibSymbol`), then adds the instance with the library's own Value and Footprint where the
//!     command left them empty (a new `SCH_SYMBOL` copies its fields from the `LIB_SYMBOL`), then the library's Datasheet. One undo step takes
//!     all of it back.
//!   * `AddPowerSymbol { lib_id }` (Place Power's chooser) keeps the power symbol's definition first, the same way.
//!   * `SetSymbolLibIds { changes }` (Edit Symbol Library Links) keeps the definition of every installed symbol it links to first, the same way.
//!   * `sch_edit change_symbol { lib_id }` (Change Symbols, `DIALOG_CHANGE_SYMBOLS::processSymbol` with the `LIB_SYMBOL` `GetLibSymbol( newId )` reads from the
//!     library -- one the Symbol Chooser found in any library of the tables) keeps the new symbol's definition first, the same way.
//!   * `PlaceFootprint { footprint }` carries the footprint as `definition`, the library's file as the Footprint Editor reads it: its pads, graphics and courtyard where the
//!     model does not know the footprint, and always what the board's footprint has of it beyond those (its pads' layers, its record and items: UI-LEFT PP5).
//!   * `ExchangeFootprints` carries the library copy of each footprint it puts a part on (`library`, and in `library_items` the footprint as the Footprint Editor reads
//!     it: the exchange gives the board footprint the library's pads' layers, record and items, UI-LEFT P30, PP5), and a symbol's Footprint field that is edited
//!     (`EditSymbolFields`, the Symbol Fields Table's `SetSymbolFields`) is followed by the board in the same step: the exchange of the part's footprint is
//!     added to the command, `board_netlist_updater.cpp`'s `replaceFootprint`.
//!   * `OpenFootprintForEdit { name }` of a `Lib:Name` the design has no copy of becomes the `PutLibraryFootprint` of the library's footprint
//!     ([`with_opened_footprint`]), so the Footprint Editor opens what the file holds and never a blank footprint under its name; `OpenSymbolForEdit` the
//!     `PutLibrarySymbol` of the library's symbol ([`with_opened_symbol`]), the same for the Symbol Editor.
//!
//! A symbol the model already resolves (the intent's own, one it already uses, one the design keeps) is left alone: the design's copy wins.

use crate::library_index::{Kind, LibSource};
use eda_model::ir::Design;
use eda_model::ConstraintModel;
use eda_ops::{Cmd, ExchangeOptions};
use std::collections::BTreeMap;
use std::path::Path;

/// Whether `cmd` can name a library symbol or footprint at all: the library tables are read only for a command that does.
fn places_from_libraries(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::Batch { cmds } => cmds.iter().any(places_from_libraries),
        Cmd::OnSheet { cmd, .. } | Cmd::InGroup { cmd, .. } => places_from_libraries(cmd),
        Cmd::AddSymbol { .. } | Cmd::AddPowerSymbol { .. } | Cmd::SetSymbolLibIds { .. } | Cmd::PlaceFootprint { .. } | Cmd::ExchangeFootprints { .. } => true,
        Cmd::SchEdit(eda_ops::sch_edit::SchCmd::ChangeSymbol { .. }) => true,
        // Import Netlist: the footprints it adds or puts parts on (`crate::netlist_api::filled`)
        Cmd::ImportNetlist(_) => true,
        Cmd::EditSymbolFields { footprint: Some(f), .. } => !f.trim().is_empty(),
        Cmd::SetSymbolFields { edits, .. } => edits.iter().any(|e| e.field == "Footprint"),
        _ => false,
    }
}

/// `cmd` with the definitions of the library symbols and footprints it names, or `None` when there is nothing to add. `dir` is the board's folder: its
/// library tables say where the libraries are.
pub(crate) fn with_installed_definitions(dir: &Path, cmd: &Cmd, design: &Design, model: &ConstraintModel) -> Option<Cmd> {
    if !places_from_libraries(cmd) {
        return None;
    }
    rewrite(cmd, design, model, &*crate::library_tables::libs(dir, Kind::Symbol), &*crate::library_tables::libs(dir, Kind::Footprint))
}

/// `open_footprint_for_edit` of a footprint the design has no copy of, out of a library of the library tables (`Lib:Name`: `Name.kicad_mod` in the folder
/// the row nicknamed `Lib` names): the library's footprint goes in with the command, as the `put_library_footprint` (`overwrite`, the model may resolve the
/// same name) that the Footprint Editor sends before it opens one (`libraryClient.ts::ensureFootprintInProject`), but in the same step as the open.
///
/// The editor asks first (`GET /api/library/footprint`: is the design's copy there?) and opens after, in requests of their own, and a command can land in
/// between: another client forgetting the design's copy (the libsave model keeps it as the library's buffer), an undo. The open verb reads no library,
/// so it used to start a blank footprint under the library's name, and that blank stood in for the file from then on: every later open found it "in the
/// project", the tree marked the library modified, and Save would have written it over the file. `FOOTPRINT_EDIT_FRAME::LoadFootprintFromLibrary`
/// reads the library when it opens. A file that is there and cannot be read refuses the open (`PCB_BASE_FRAME::LoadFootprint` gives nothing on the
/// `IO_ERROR`, and `LoadFootprintFromLibrary` returns with the editor as it was). A name no library has is left to the verb (a blank one, KiCad's New
/// Footprint).
pub(crate) fn with_opened_footprint(dir: &Path, cmd: &Cmd, design: &Design) -> Result<Option<Cmd>, Vec<eda_model::CheckResult>> {
    let Cmd::OpenFootprintForEdit { name } = cmd else { return Ok(None) };
    if !name.contains(':') || design.footprint_library.as_ref().is_some_and(|l| l.by_name(name).is_some()) {
        return Ok(None);
    }
    match crate::library_search::read_installed_footprint(&*crate::library_tables::libs(dir, Kind::Footprint), name) {
        Ok(Some(footprint)) => Ok(Some(Cmd::PutLibraryFootprint { footprint, overwrite: true })),
        Ok(None) => Ok(None),
        Err(why) => Err(vec![eda_model::CheckResult::fail("ops_bad_footprint", name, why)]),
    }
}

/// `open_symbol_for_edit` of a symbol the design has no copy of, out of a library of the library tables (`Lib:Name` in the file, or the folder, the row
/// nicknamed `Lib` names): the library's symbol goes in with the command, as the `put_library_symbol` (`overwrite`, the model may resolve the same id) that
/// the Symbol Editor sends before it opens one (`libraryClient.ts::ensureSymbolInProject`), but in the same step as the open -- the symbol sibling of
/// [`with_opened_footprint`].
///
/// The editor asks first (`GET /api/library/symbol`: is the design's copy there?), reads the project's names, and opens after, in requests of their own; a
/// command can land in between (another client forgetting the design's copy, an undo), and the open verb, which reads no library file, used to start from
/// the model's pins-and-graphics copy or a blank symbol under the library's id, which then stood in for the file: every later open found it "in the
/// project", and Save would have written it over the file. `SYMBOL_EDIT_FRAME::LoadSymbol` reads the library when it opens. A library or a symbol's text
/// that is there and cannot be read refuses the open ("Error loading symbol %s from library '%s'." and `return false`: the editor as it was). An id no
/// library has is left to the verb (the model's symbol, the builtin table, else a blank one, KiCad's New Symbol).
pub(crate) fn with_opened_symbol(dir: &Path, cmd: &Cmd, design: &Design) -> Result<Option<Cmd>, Vec<eda_model::CheckResult>> {
    let Cmd::OpenSymbolForEdit { lib_id } = cmd else { return Ok(None) };
    if !lib_id.contains(':') || design.symbol_library.as_ref().is_some_and(|l| l.by_lib_id(lib_id).is_some()) {
        return Ok(None);
    }
    match crate::library_search::read_installed_symbol(&*crate::library_tables::libs(dir, Kind::Symbol), lib_id) {
        Ok(Some((symbol, _))) => Ok(Some(Cmd::PutLibrarySymbol { symbol, overwrite: true })),
        Ok(None) => Ok(None),
        Err(why) => Err(vec![eda_model::CheckResult::fail("ops_bad_symbol", lib_id, why)]),
    }
}

/// A library symbol the design has no definition of: a `Lib:Name` the model's explicit symbols (the intent's, a library's it resolved, a published one) and the project
/// symbol library do not hold.
fn lacks_definition(lib_id: &str, design: &Design, model: &ConstraintModel) -> bool {
    lib_id.split_once(':').is_some_and(|(l, n)| !l.is_empty() && !n.is_empty())
        && !eda_model::is_synthetic_lib_id(lib_id)
        && !model.symbols.iter().any(|s| s.lib_id == lib_id)
        && !design.symbol_library.as_ref().is_some_and(|l| l.by_lib_id(lib_id).is_some())
}

fn rewrite<S: LibSource + ?Sized, F: LibSource + ?Sized>(cmd: &Cmd, design: &Design, model: &ConstraintModel, symbols: &S, footprints: &F) -> Option<Cmd> {
    match cmd {
        Cmd::Batch { cmds } => {
            let rewritten: Vec<Option<Cmd>> = cmds.iter().map(|c| rewrite(c, design, model, symbols, footprints)).collect();
            rewritten.iter().any(Option::is_some).then(|| Cmd::Batch { cmds: cmds.iter().zip(rewritten).map(|(old, new)| new.unwrap_or_else(|| old.clone())).collect() })
        }
        Cmd::OnSheet { sheet, cmd } => rewrite(cmd, design, model, symbols, footprints).map(|inner| Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(inner) }),
        Cmd::InGroup { group, cmd } => rewrite(cmd, design, model, symbols, footprints).map(|inner| Cmd::InGroup { group: group.clone(), cmd: Box::new(inner) }),
        Cmd::AddSymbol { id, lib_id, at, rot_millideg, value, footprint, unit, annotate } => {
            // The design's own definition (the intent's, a published one, one a placed symbol already needed) stands.
            if !lacks_definition(lib_id, design, model) {
                return None;
            }
            let (definition, summary) = crate::library_search::installed_symbol(symbols, lib_id)?;
            let place = Cmd::AddSymbol { annotate: annotate.clone(),
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
        // Place Power (`PlaceSymbol` with the power filter): a power symbol chosen out of an installed library (`power:+3V3`) is an `SCH_SYMBOL` like any
        // other, made from its `LIB_SYMBOL`: its definition goes in with it, as `AddSymbol`'s does, so it draws as the library draws it. The design's own
        // definition, and the built-in rail where no library has the name, stand.
        Cmd::AddPowerSymbol { lib_id, .. } => {
            if !lacks_definition(lib_id, design, model) {
                return None;
            }
            let (definition, _) = crate::library_search::installed_symbol(symbols, lib_id)?;
            Some(Cmd::Batch { cmds: vec![Cmd::EmbedLibSymbol { symbol: definition }, cmd.clone()] })
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
        // Change Symbols to a symbol of a library the design does not use yet: its definition first (a second change to it in the same batch finds it there).
        Cmd::SchEdit(eda_ops::sch_edit::SchCmd::ChangeSymbol { lib_id, .. }) => {
            if !lacks_definition(lib_id.trim(), design, model) {
                return None;
            }
            let (definition, _) = crate::library_search::installed_symbol(symbols, lib_id.trim())?;
            Some(Cmd::Batch { cmds: vec![Cmd::EmbedLibSymbol { symbol: definition }, cmd.clone()] })
        }
        Cmd::ExchangeFootprints { references, footprint, options, library, library_items } => {
            // the library copy of each footprint it needs: the one the parts change to, or the one each is on
            let mut names: Vec<String> = match footprint {
                Some(f) => vec![f.trim().to_string()],
                // (the footprint each is on, by the name the libraries know it by: a built-in `0603` is `Resistor_SMD:R_0603_1608Metric`, `Lib:X#2` is `Lib:X`)
                None => references.iter().filter_map(|r| model.part(r)).map(|p| eda_model::footprint::library_lookup_name(p, &model.footprints)).collect(),
            };
            names.sort();
            names.dedup();
            let (mut library, mut library_items) = (library.clone(), library_items.clone());
            let mut added = false;
            for name in names {
                if library.contains_key(&name) {
                    continue;
                }
                if let Some(copy) = crate::library_search::installed_engine_footprint(footprints, &name) {
                    if let Some(lib) = installed_library_footprint(footprints, &name) {
                        library_items.insert(name.clone(), lib);
                    }
                    library.insert(name, copy);
                    added = true;
                }
            }
            added.then(|| Cmd::ExchangeFootprints { references: references.clone(), footprint: footprint.clone(), options: *options, library, library_items })
        }
        // The symbol's Footprint field is the board's footprint (`replaceFootprint`): the exchange follows the edit
        Cmd::EditSymbolFields { id, footprint: Some(f), .. } => follow_footprint_field(cmd, id, f, design, model, footprints),
        Cmd::SetSymbolFields { edits, .. } => {
            let follows: Vec<Cmd> = edits.iter().filter(|e| e.field == "Footprint").filter_map(|e| exchange_for_field(&e.id, &e.value, design, model, footprints)).collect();
            (!follows.is_empty()).then(|| {
                let mut cmds = vec![cmd.clone()];
                cmds.extend(follows);
                Cmd::Batch { cmds }
            })
        }
        Cmd::ImportNetlist(req) => {
            let filled = crate::netlist_api::filled(req, model, footprints);
            (filled != *req).then_some(Cmd::ImportNetlist(filled))
        }
        Cmd::PlaceFootprint { footprint, at, reference, value, definition: None } => {
            // The library's footprint (`PCB_BASE_FRAME::LoadFootprint`): the verb takes the pads of one the model knows already (the intent's, a library's it resolved, the
            // built-in table) from the model, and the rest -- what the engine footprint does not carry -- from this (UI-LEFT PP5). A name no library has: nothing to bring.
            let definition = crate::library_search::installed_footprint(footprints, footprint.trim())?;
            Some(Cmd::PlaceFootprint { footprint: footprint.clone(), at: *at, reference: reference.clone(), value: value.clone(), definition: Some(definition) })
        }
        _ => None,
    }
}

/// The installed footprint `name` as the Footprint Editor reads it: what an exchange gives the board footprint beyond the engine's footprint (`ExchangeFootprint` keeps
/// `aNew`'s own children and record: its pads' layer sets, its items, its description ..., UI-LEFT P30, PP5).
fn installed_library_footprint<F: LibSource + ?Sized>(footprints: &F, name: &str) -> Option<eda_model::ir::LibraryFootprint> {
    crate::library_search::installed_footprint(footprints, name)
}

/// Does something have the footprint `wanted` for the part `id`: a library of the tables, the Footprint Editor, or the model itself (another part's, a built-in package)?
/// `Some(library entry)` where a library has it (the command carries that copy), `Some(None)` where the model or the editor has it, `None` where nothing does.
fn find_footprint<F: LibSource + ?Sized>(wanted: &str, id: &str, design: &Design, model: &ConstraintModel, footprints: &F) -> Option<Option<eda_model::Footprint>> {
    if let Some(copy) = crate::library_search::installed_engine_footprint(footprints, wanted) {
        return Some(Some(copy));
    }
    let value = model.part(id).and_then(|p| p.value.clone());
    let probe = eda_model::Part { reference: id.to_string(), mpn: None, lcsc: None, value, package: None, footprint: Some(wanted.to_string()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
    let in_editor = design.footprint_library.as_ref().is_some_and(|l| l.by_name(wanted).is_some());
    (model.footprint_of(&probe).is_some() || in_editor).then_some(None)
}

/// The library copy of footprint `name` for the compare tools: the footprint the board is compared with (or drawn beside), and what the library
/// sets on it and its pads -- its `LibraryFootprint`'s margins and zone facts, which the engine `Footprint` does not carry and `FootprintNeedsUpdate`
/// compares the board's overrides with.
pub(crate) struct LibraryCopy {
    pub footprint: eda_model::Footprint,
    pub facts: eda_model::fp_compare::LibraryFacts,
}

/// The library copy of footprint `name`: the Footprint Editor's entry for it where the project edited one (the newest copy there is), else the file of the library tables.
pub(crate) fn library_copy<F: LibSource + ?Sized>(name: &str, design: &Design, footprints: &F) -> Option<LibraryCopy> {
    if let Some(entry) = design.footprint_library.as_ref().and_then(|l| l.by_name(name)) {
        let mut copy = entry.to_engine_footprint();
        copy.name = name.to_string();
        return Some(LibraryCopy { footprint: copy, facts: eda_model::fp_compare::LibraryFacts::of_library_footprint(entry) });
    }
    let footprint = crate::library_search::installed_engine_footprint(footprints, name)?;
    // The installed file as the editor reads it, for the values the engine copy drops. A file the editor refuses keeps no facts: every override of the
    // board's is then told, as before.
    let facts = crate::library_search::installed_footprint(footprints, name).map(|fp| eda_model::fp_compare::LibraryFacts::of_library_footprint(&fp)).unwrap_or_default();
    Some(LibraryCopy { footprint, facts })
}

/// Is the footprint `wanted` one the studio can put on the part `id` (Update PCB from Schematic: "Cannot update %s (footprint '%s' not found).")?
pub(crate) fn footprint_found<F: LibSource + ?Sized>(wanted: &str, id: &str, design: &Design, model: &ConstraintModel, footprints: &F) -> bool {
    find_footprint(wanted, id, design, model, footprints).is_some()
}

/// The exchange that puts the board's part `id` on the footprint `wanted`, the one its symbol's Footprint field now names -- `None` when the board is on it already, when
/// there is no such part, when its footprint is locked, or when no library has the footprint (the schematic keeps what was typed and the board stays as it was: Update PCB
/// from Schematic lists it).
fn exchange_for_field<F: LibSource + ?Sized>(id: &str, wanted: &str, design: &Design, model: &ConstraintModel, footprints: &F) -> Option<Cmd> {
    let wanted = wanted.trim();
    let part = model.part(id)?;
    if wanted.is_empty() || eda_ops::names_footprint_of(part, wanted) {
        return None;
    }
    // `replaceFootprint` does not change a locked footprint ("Could not change %s footprint from '%s' to '%s' (footprint is locked)."): Update PCB lists it
    if design.drawings.as_ref().is_some_and(|d| d.locked_ids.iter().any(|l| l == id)) {
        return None;
    }
    let (mut library, mut library_items) = (BTreeMap::new(), BTreeMap::new());
    if let Some(copy) = find_footprint(wanted, id, design, model, footprints)? {
        library.insert(wanted.to_string(), copy);
        if let Some(lib) = installed_library_footprint(footprints, wanted) {
            library_items.insert(wanted.to_string(), lib);
        }
    }
    Some(Cmd::ExchangeFootprints { references: vec![id.to_string()], footprint: Some(wanted.to_string()), options: ExchangeOptions::replace(), library, library_items })
}

/// `cmd` (an edit of the symbol `id`'s Footprint field) and the exchange that makes the board follow it, in one command.
fn follow_footprint_field<F: LibSource + ?Sized>(cmd: &Cmd, id: &str, wanted: &str, design: &Design, model: &ConstraintModel, footprints: &F) -> Option<Cmd> {
    let exchange = exchange_for_field(id, wanted, design, model, footprints)?;
    Some(Cmd::Batch { cmds: vec![cmd.clone(), exchange] })
}

/// (For the tests of the modules that build sources of their own.)
#[cfg(test)]
pub(crate) fn rewrite_for_tests(cmd: &Cmd, design: &Design, model: &ConstraintModel, symbols: &dyn LibSource, footprints: &dyn LibSource) -> Option<Cmd> {
    rewrite(cmd, design, model, symbols, footprints)
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
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
        Cmd::AddSymbol { annotate: None, id: "U1".into(), lib_id: lib_id.into(), at: Point { x: mm(10.0), y: mm(10.0) }, rot_millideg: 0, value: value.into(), footprint: footprint.into(), unit: 1 }
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
        model.symbols.push(eda_model::LibSymbol { connectivity: Default::default(), lib_id: "Amplifier_Operational:LM358".into(), graphics: vec![], pins: vec![], power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), reference_prefix: "U".into(), unit_count: 1, pin_names_hidden: false, pin_numbers_hidden: false, pin_name_offset_mm: 0.0, alternate: None });
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

    /// Place Power: a power symbol out of an installed library goes in with its definition, as Place Symbol's does (`PlaceSymbol` makes both from
    /// their `LIB_SYMBOL`); one the design draws already, or a rail no library has, is left as it is.
    #[test]
    fn placing_an_installed_power_symbol_keeps_its_definition_first() {
        let root = std::env::temp_dir().join(format!("eda-library-place-power-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("power.kicad_sym"),
            r##"(kicad_symbol_lib (version 20251024)
        (symbol "+3V3" (power) (property "Reference" "#PWR") (property "Value" "+3V3") (property "Description" "Power symbol creates a global label with name \"+3V3\"")
            (symbol "+3V3_0_1" (polyline (pts (xy -0.762 1.27) (xy 0 2.54)) (stroke (width 0) (type default)) (fill (type none))))
            (symbol "+3V3_1_1" (pin power_in line (at 0 0 90) (length 0) hide (name "+3V3") (number "1")))))"##,
        )
        .unwrap();
        let place = |lib_id: &str| Cmd::AddPowerSymbol { lib_id: lib_id.into(), at: Point { x: mm(10.0), y: mm(10.0) }, rot_millideg: 90_000, net: "+3V3".into(), pin: String::new(), annotate: None };
        let model = ConstraintModel::default();
        let out = rewrite(&place("power:+3V3"), &design(), &model, &root, &root).expect("rewritten");
        let Cmd::Batch { cmds } = out else { panic!("{out:?}") };
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], Cmd::EmbedLibSymbol { symbol } if symbol.lib_id == "power:+3V3" && symbol.power), "{:?}", cmds[0]);
        assert!(matches!(&cmds[1], Cmd::AddPowerSymbol { lib_id, rot_millideg: 90_000, net, .. } if lib_id == "power:+3V3" && net == "+3V3"));
        // no library has it: the built-in rail draws it
        assert!(rewrite(&place("power:VBAT"), &design(), &model, &root, &root).is_none());
        // the design keeps a definition of it
        let mut d = design();
        d.symbol_library = Some(Default::default());
        d.symbol_library.as_mut().unwrap().symbols.push(eda_model::ir::LibrarySymbol::new_empty("power:+3V3"));
        assert!(rewrite(&place("power:+3V3"), &d, &model, &root, &root).is_none());
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
        with_c.symbols.push(eda_model::LibSymbol { connectivity: Default::default(), lib_id: "Device:C".into(), graphics: vec![], pins: vec![], power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), reference_prefix: "C".into(), unit_count: 1, pin_names_hidden: false, pin_numbers_hidden: false, pin_name_offset_mm: 0.0, alternate: None });
        assert!(rewrite(&known, &design(), &with_c, &root, &root).is_none());
        let nowhere = Cmd::SetSymbolLibIds { changes: vec![("Device:R".into(), "Nope:Nothing".into())], update_fields: false };
        assert!(rewrite(&nowhere, &design(), &model, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Change Symbols to a symbol of a library the design does not use (the browse button's Symbol Chooser over every library): `processSymbol` takes the
    /// `LIB_SYMBOL` from the library, so its definition goes in first; one the design has, or no library has, is left to the verb.
    #[test]
    fn changing_a_symbol_to_an_installed_one_keeps_its_definition_first() {
        use eda_ops::sch_edit::SchCmd;
        let root = root_with_lm358("change");
        let model = ConstraintModel::default();
        let change = Cmd::sch(SchCmd::ChangeSymbol { id: "R1".into(), lib_id: "Amplifier_Operational:LM358".into() });
        let Some(Cmd::Batch { cmds }) = rewrite(&change, &design(), &model, &root, &root) else { panic!("not rewritten") };
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], Cmd::EmbedLibSymbol { symbol } if symbol.lib_id == "Amplifier_Operational:LM358" && symbol.unit_count == 3));
        assert_eq!(cmds[1], change);
        // in a batch of changes (the dialog's Apply), each is rewritten; the second embed of the same symbol is a no-op in the verb
        let batch = Cmd::Batch { cmds: vec![change.clone(), Cmd::sch(SchCmd::ChangeSymbol { id: "R2".into(), lib_id: "Amplifier_Operational:LM358".into() })] };
        let Some(Cmd::Batch { cmds }) = rewrite(&batch, &design(), &model, &root, &root) else { panic!() };
        assert!(cmds.iter().all(|c| matches!(c, Cmd::Batch { .. })));
        assert!(rewrite(&Cmd::sch(SchCmd::ChangeSymbol { id: "R1".into(), lib_id: "Nope:Nothing".into() }), &design(), &model, &root, &root).is_none());
        let mut d = design();
        d.symbol_library = Some(Default::default());
        d.symbol_library.as_mut().unwrap().symbols.push(eda_model::ir::LibrarySymbol::new_empty("Amplifier_Operational:LM358"));
        assert!(rewrite(&change, &d, &model, &root, &root).is_none(), "the design's copy stands");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_footprint_placed_from_a_library_carries_its_pads() {
        let root = std::env::temp_dir().join(format!("eda-library-place-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("MountingHole.pretty")).unwrap();
        std::fs::write(root.join("MountingHole.pretty").join("MH.kicad_mod"), r#"(footprint "MH" (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask")))"#).unwrap();
        let model = ConstraintModel::default();
        let place = Cmd::PlaceFootprint { footprint: "MountingHole:MH".into(), at: Point { x: mm(1.0), y: mm(2.0) }, reference: String::new(), value: String::new(), definition: None };
        let Some(Cmd::PlaceFootprint { definition: Some(def), at, .. }) = rewrite(&place, &design(), &model, &root, &root) else { panic!("not rewritten") };
        assert_eq!((def.name.as_str(), def.pads.len(), at), ("MountingHole:MH", 1, Point { x: mm(1.0), y: mm(2.0) }));
        // already carrying one, or not installed: left as it is
        let carrying = Cmd::PlaceFootprint { footprint: "MountingHole:MH".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, reference: String::new(), value: String::new(), definition: Some(def) };
        assert!(rewrite(&carrying, &design(), &model, &root, &root).is_none());
        let missing = Cmd::PlaceFootprint { footprint: "MountingHole:Nope".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, reference: String::new(), value: String::new(), definition: None };
        assert!(rewrite(&missing, &design(), &model, &root, &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
