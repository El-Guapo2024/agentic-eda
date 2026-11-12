//! Rescue Symbols' change to the schematic (`eeschema.EditorControl.rescueSymbols`, UI-LEFT A2): [`Cmd::RescueSymbols`], the part of
//! `RESCUER::RescueProject` that is the schematic's own -- each chosen candidate's `PerformAction` (`RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::PerformAction`,
//! eeschema/project_rescue.cpp at KiCad 8303b2ad): every symbol that names the rescued library identifier names the rescue library's copy instead
//! (`eachSymbol->SetLibId( m_new_id )`), on every screen (`getSymbols` takes `SCH_SCREENS( Root )`, power symbols included: they are `SCH_SYMBOL`s too),
//! and `SCH_SCREENS::UpdateSymbolLinks` then links them to the copy the rescue library holds. Here the copy is kept with the design under its new
//! identifier, as placing a library symbol keeps one (`Cmd::EmbedLibSymbol`), so the symbols draw, trace and export from it at once.
//!
//! The rescue library file (`<project>-rescue.kicad_sym`) and its row in the project's symbol library table are written by the server before this verb
//! runs (`crates/cli/src/rescue_api.rs`, `SYMBOL_LIB_TABLE_RESCUER::WriteRescueLibrary`). **Differs**: KiCad clears the undo list after a rescue
//! (`SCH_EDITOR_CONTROL::rescueProject`: `ClearUndoRedoList`); here the relink is one undoable command like every other edit, and undoing it leaves the
//! rescue library and its table row in place (as KiCad leaves them when the rescued schematic is not saved).

use eda_model::ir::LibrarySymbol;
use eda_model::CheckResult;

use super::sch_control::lib_id_is_valid;
use super::Board;

fn fail(check: &str, subject: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, msg.into())]
}

impl<'a> Board<'a> {
    /// [`super::Cmd::RescueSymbols`]: `changes` pairs each rescued identifier with its identifier in the rescue library; `symbols` are the rescue
    /// library's copies (named by the new identifiers); a new identifier with no copy among them (Remap Legacy Library Symbols, which gives none) takes the
    /// copy the design kept under the old one. Refused as a whole when a new identifier is not `Library:Name`, or when no symbol of any sheet names an old one.
    pub(crate) fn rescue_symbols(&mut self, changes: &[(String, String)], symbols: &[LibrarySymbol]) -> Result<(), Vec<CheckResult>> {
        if changes.is_empty() {
            return Err(fail("ops_nothing_to_change", "rescue", "No symbols were rescued."));
        }
        let screens = || self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.values()));
        for (from, to) in changes {
            // the legacy rescuer renames a bare name to another (`RESCUE_CASE_CANDIDATE::PerformAction`: `SetLibItemName( m_new_name )`)
            let legacy_rename = !from.contains(':') && !to.is_empty() && !to.contains(':');
            if !lib_id_is_valid(to) && !legacy_rename {
                return Err(fail("ops_bad_lib_id", to, format!("Symbol library identifier {to} is not valid.")));
            }
            if !screens().any(|s| s.symbols.iter().any(|x| &x.lib_id == from) || s.power_symbols.iter().any(|p| &p.lib_id == from)) {
                return Err(fail("ops_unknown_lib_id", from, "no placed symbol uses this library identifier"));
            }
        }
        // the rescue library's copies, kept with the schematic (`AddSymbol` replaces a symbol of the same name: a rescue made again replaces the old copy)
        {
            let lib = self.symbol_library_mut();
            for s in symbols.iter().filter(|s| changes.iter().any(|(_, to)| *to == s.lib_id)) {
                let mut entry = s.clone();
                entry.published = true;
                entry.extends = None;
                entry.unit_count = entry.unit_count.max(1);
                for p in &mut entry.pins {
                    p.id.clear();
                }
                for g in &mut entry.graphics {
                    g.set_id(String::new());
                }
                entry.assign_missing_ids();
                lib.symbols.retain(|x| x.lib_id != entry.lib_id);
                lib.symbols.push(entry);
            }
            // Remap Legacy Library Symbols gives no copy (`remapSymbolToLibTable` only sets the nickname, and `UpdateSymbolLinks` links the library's): the copy
            // the schematic kept under the old identifier is its copy under the new one
            for (from, to) in changes {
                if symbols.iter().any(|s| &s.lib_id == to) || lib.symbols.iter().any(|s| &s.lib_id == to) {
                    continue;
                }
                if let Some(kept) = lib.symbols.iter().find(|s| &s.lib_id == from).cloned() {
                    lib.symbols.push(LibrarySymbol { lib_id: to.clone(), ..kept });
                }
            }
            lib.symbols.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
        }
        // `eachSymbol->SetLibId( m_new_id )` on every screen; a power symbol's local / global kind follows its identifier
        let all = self.design.schematic.iter_mut().chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.values_mut()));
        for sch in all {
            for (from, to) in changes {
                for s in sch.symbols.iter_mut().filter(|s| &s.lib_id == from) {
                    s.lib_id = to.clone();
                }
                for p in sch.power_symbols.iter_mut().filter(|p| &p.lib_id == from) {
                    p.lib_id = to.clone();
                }
                if sch.extras.local_power.remove(from) {
                    sch.extras.local_power.insert(to.clone());
                }
                if sch.extras.global_power.remove(from) {
                    sch.extras.global_power.insert(to.clone());
                }
            }
        }
        Ok(())
    }
}
