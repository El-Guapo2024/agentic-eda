//! The schematic-control verbs: what eeschema's `SCH_EDITOR_CONTROL`, `SCH_EDIT_TOOL` attribute commands and
//! the Edit Symbol Library Links dialog change in the schematic, each as one `/api/cmd` verb with undo.
//!
//! * [`Cmd::SetSymbolAttrs`] -- `SCH_EDIT_TOOL::SetAttribute` (`eeschema.EditorControl.setDNP`,
//!   `setExcludeFromBOM`, `setExcludeFromBoard`, `setExcludeFromSimulation`).
//! * [`Cmd::SetSheetPage`] -- `SCH_EDIT_TOOL::EditPageNumber` (`eeschema.EditorControl.editPageNumber`).
//! * [`Cmd::SetSchItemText`] -- the text change `SCH_TOOL_BASE::Increment` makes to a label or a text
//!   (`eeschema.Interactive.increment*`).
//! * [`Cmd::SetSymbolLibIds`] -- `DIALOG_EDIT_SYMBOLS_LIBID::TransferDataFromWindow`
//!   (`eeschema.EditorControl.editSymbolLibraryLinks`, and the library mapping of
//!   `exportSymbolsToLibrary`).
//! * [`Cmd::IncrementAnnotations`] -- `SCH_EDITOR_CONTROL::IncrementAnnotations`
//!   (`eeschema.EditorControl.incrementAnnotations`).
//!
//! All of them are `Domain::Schematic` verbs on the schematic the studio edits (the root sheet, like every other
//! schematic verb), except [`Cmd::SetSheetPage`], which finds the placement anywhere in the hierarchy.

use eda_model::CheckResult;
use std::collections::BTreeMap;

use super::{fields_table, Board};

fn fail(check: &str, subject: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, msg.into())]
}

/// `wxFILTER_ALPHANUMERIC` -- the filter `SCH_EDIT_TOOL::EditPageNumber`'s text entry applies ("No white space"): a page number is
/// letters and digits only. The empty string clears the number (the sheet is numbered by its place in the hierarchy again).
pub fn page_number_is_valid(page: &str) -> bool {
    page.chars().all(|c| c.is_alphanumeric())
}

/// `LIB_ID::IsValid`: a library identifier has both a library nickname and an item name (`Device:R`).
pub fn lib_id_is_valid(lib_id: &str) -> bool {
    match lib_id.split_once(':') {
        Some((lib, item)) => !lib.is_empty() && !item.is_empty() && !item.contains(':'),
        None => false,
    }
}

/// The item part of a `Library:Item` identifier (`getName` of `DIALOG_EDIT_SYMBOLS_LIBID`).
fn item_name(lib_id: &str) -> &str {
    lib_id.split_once(':').map_or(lib_id, |(_, item)| item)
}

/// `SCH_EDITOR_CONTROL::IncrementAnnotations`'s plan: every reference whose letters equal the start reference's and whose number is at
/// least the start's moves by `increment`; the rest stay. `refs` are the distinct references of the sheet (every placed unit of one
/// reference carries the same text). A start without a trailing number is not splittable (`IsSplitNeeded`), so nothing is planned.
/// Returns `(old, new)` pairs in reference order.
pub fn increment_plan(refs: &[String], start: &str, increment: i32) -> Result<Vec<(String, String)>, String> {
    let start = fields_table::Ref::parse(start.trim());
    let Some(start_num) = start.num else { return Ok(Vec::new()) };
    let mut plan = Vec::new();
    for r in refs {
        let parsed = fields_table::Ref::parse(r);
        let Some(num) = parsed.num else { continue };
        if parsed.prefix != start.prefix || num < start_num {
            continue;
        }
        let moved = num as i64 + increment as i64;
        if moved < 0 {
            return Err(format!("{r} would become {}{moved}: a reference number cannot go below 0", parsed.prefix));
        }
        plan.push((r.clone(), format!("{}{moved}", parsed.prefix)));
    }
    plan.sort();
    Ok(plan)
}

impl<'a> Board<'a> {
    /// `SCH_EDIT_TOOL::SetAttribute`: `Some(v)` sets the attribute on every placed unit of every listed reference ("The attributes
    /// should be kept in sync in multi-unit parts"), `None` leaves it. Which state a toggle goes to (`new_state`: set when any of the
    /// items does not have it yet, else cleared) is the caller's decision, as in the C++ where it is computed before the commit.
    pub(crate) fn set_symbol_attrs(&mut self, ids: &[String], dnp: Option<bool>, exclude_from_bom: Option<bool>, exclude_from_board: Option<bool>, exclude_from_sim: Option<bool>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "attributes", "select a symbol first"));
        }
        let sch = self.schematic_mut()?;
        for id in ids {
            if !sch.symbols.iter().any(|s| &s.id == id) {
                return Err(fail("ops_unknown_symbol", id, "no symbol instance with this reference"));
            }
        }
        for s in sch.symbols.iter_mut().filter(|s| ids.contains(&s.id)) {
            if let Some(v) = dnp {
                s.dnp = v;
            }
            if let Some(v) = exclude_from_bom {
                s.exclude_from_bom = v;
            }
            if let Some(v) = exclude_from_board {
                s.exclude_from_board = v;
            }
            if let Some(v) = exclude_from_sim {
                s.exclude_from_sim = v;
            }
        }
        // The board's footprint carries the same two flags: the one edited last wins (`Board::sync_footprint_bom_flags`).
        self.sync_footprint_bom_flags(ids, dnp, exclude_from_bom);
        Ok(())
    }

    /// `SCH_EDIT_TOOL::EditPageNumber`: the placement `sheet` (a `SheetInstance::id`, found on the root sheet or on any sheet
    /// screen) gets `page`. Letters and digits only; empty clears it.
    pub(crate) fn set_sheet_page(&mut self, sheet: &str, page: &str) -> Result<(), Vec<CheckResult>> {
        let page = page.trim();
        if !page_number_is_valid(page) {
            return Err(fail("ops_bad_page", sheet, "a page number is letters and digits only (no spaces)"));
        }
        if let Some(s) = self.design.schematic.as_mut().and_then(|root| root.sheets.iter_mut().find(|s| s.id == sheet)) {
            s.page = page.to_string();
            return Ok(());
        }
        if let Some(screens) = self.design.sheet_contents.as_mut() {
            for screen in screens.values_mut() {
                if let Some(s) = screen.sheets.iter_mut().find(|s| s.id == sheet) {
                    s.page = page.to_string();
                    return Ok(());
                }
            }
        }
        Err(fail("ops_unknown_sheet", sheet, "no sheet with this id"))
    }

    /// What `SCH_TOOL_BASE::Increment` does to a label or a text (`label.SetText( *newLabel )`): the item keeps its place and its id,
    /// its text changes. A label's text is the name of its net, so it may not be empty.
    pub(crate) fn set_sch_item_text(&mut self, id: &str, text: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if let Some(l) = sch.labels.iter_mut().find(|l| l.id == id) {
            if text.is_empty() {
                return Err(fail("ops_bad_label", id, "a label needs text"));
            }
            l.net = text.to_string();
            return Ok(());
        }
        if let Some(t) = sch.texts.iter_mut().find(|t| t.id == id) {
            t.content = text.to_string();
            return Ok(());
        }
        Err(fail("ops_unknown_item", id, "no label or text with this id"))
    }

    /// `DIALOG_EDIT_SYMBOLS_LIBID::TransferDataFromWindow`: every placed symbol whose library id is `from` is linked to `to` instead. A
    /// new id must be valid (`LIB_ID::IsValid`) and name a symbol that loads (`LoadSymbol( id )` -- here the model's symbol table, which
    /// holds the library files, the project library and the built-in symbols). A Value that was only the old item's name follows to
    /// the new name ("If value is a proxy for the itemName then make sure it gets updated"). `update_fields` is the dialog's "Update
    /// symbol fields from new library" (`UpdateFields( ..., reset other fields )`): the Value becomes the library symbol's (its name here -- the
    /// library symbol keeps no Value field of its own), the Datasheet the library symbol's, and the Footprint goes back to the library's, which
    /// is none ("Warning: fields Value and Footprints will be therefore replaced").
    pub(crate) fn set_symbol_lib_ids(&mut self, changes: &[(String, String)], update_fields: bool) -> Result<(), Vec<CheckResult>> {
        if changes.is_empty() {
            return Err(fail("ops_nothing_to_change", "library links", "no library link changes"));
        }
        // each target's datasheet, from the symbol it names: the project library first (a symbol drawn in the Symbol Editor, or one an export put there), then the model's
        // library files and built-in symbols
        let mut resolved: BTreeMap<String, String> = BTreeMap::new();
        for (from, to) in changes {
            if !lib_id_is_valid(to) {
                return Err(fail("ops_bad_lib_id", to, format!("Symbol library identifier {to} is not valid.")));
            }
            if self.schematic()?.symbols.iter().all(|s| &s.lib_id != from) {
                return Err(fail("ops_unknown_lib_id", from, "no placed symbol uses this library identifier"));
            }
            let datasheet = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(to)).map(|s| s.datasheet.clone()).or_else(|| self.model.symbol_of(to).map(|s| s.datasheet.clone()));
            match datasheet {
                Some(d) => {
                    resolved.insert(to.clone(), d);
                }
                None => return Err(fail("ops_unknown_symbol", to, format!("Error loading symbol {}: the library has no such symbol.", item_name(to)))),
            }
        }
        let sch = self.schematic_mut()?;
        for (from, to) in changes {
            if from == to {
                continue;
            }
            let (old_name, new_name) = (item_name(from).to_string(), item_name(to).to_string());
            for s in sch.symbols.iter_mut().filter(|s| &s.lib_id == from) {
                if s.value == old_name {
                    s.value = new_name.clone();
                }
                if update_fields {
                    if let Some(datasheet) = resolved.get(to) {
                        s.value = new_name.clone();
                        s.footprint = String::new();
                        s.datasheet = datasheet.clone();
                    }
                }
                s.lib_id = to.clone();
            }
        }
        Ok(())
    }

    /// `SCH_EDITOR_CONTROL::IncrementAnnotations` -- see [`increment_plan`]. The references move together: a plan like `R5 -> R6, R6 -> R7`
    /// would trip over itself one rename at a time, so every moved reference first takes a temporary name (never a real one) and then its
    /// final one. Each rename carries what `Cmd::RenameSymbol` carries (the wires', power symbols' and no-connects' `REF.PIN` strings, the
    /// user fields). Refused as a whole when a new reference is already another symbol's.
    pub(crate) fn increment_annotations(&mut self, start: &str, increment: i32) -> Result<(), Vec<CheckResult>> {
        let refs: Vec<String> = {
            let mut r: Vec<String> = self.schematic()?.symbols.iter().map(|s| s.id.clone()).collect();
            r.sort();
            r.dedup();
            r
        };
        let plan = increment_plan(&refs, start, increment).map_err(|m| fail("ops_bad_increment", start, m))?;
        if plan.is_empty() {
            return Ok(());
        }
        let moving: Vec<&String> = plan.iter().map(|(old, _)| old).collect();
        let elsewhere: Vec<String> = self.symbols_elsewhere().into_iter().map(|(r, _)| r.to_string()).collect();
        for (_, new) in &plan {
            if refs.iter().any(|r| r == new && !moving.contains(&r)) {
                return Err(fail("ops_duplicate_symbol", new, "a symbol with this reference is already on the sheet"));
            }
            if elsewhere.contains(new) {
                return Err(fail("ops_duplicate_symbol", new, "a symbol with this reference is already on another sheet"));
            }
        }
        for (i, (old, _)) in plan.iter().enumerate() {
            self.rename_symbol(old, &format!("\u{1}tmp{i}"))?;
        }
        for (i, (_, new)) in plan.iter().enumerate() {
            self.rename_symbol(&format!("\u{1}tmp{i}"), new)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_number_is_letters_and_digits() {
        assert!(page_number_is_valid("12"));
        assert!(page_number_is_valid("A3"));
        assert!(page_number_is_valid(""), "empty clears the number");
        assert!(!page_number_is_valid("1 2"));
        assert!(!page_number_is_valid("1-2"));
    }

    #[test]
    fn a_library_id_has_a_nickname_and_an_item() {
        assert!(lib_id_is_valid("Device:R"));
        assert!(!lib_id_is_valid("R"));
        assert!(!lib_id_is_valid(":R"));
        assert!(!lib_id_is_valid("Device:"));
    }

    fn refs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_increment_plan_moves_the_same_letters_from_the_start_number_up() {
        let all = refs(&["C1", "R1", "R10", "R2", "R3", "RN1", "U1"]);
        let plan = increment_plan(&all, "R2", 3).unwrap();
        assert_eq!(plan, vec![("R10".to_string(), "R13".to_string()), ("R2".to_string(), "R5".to_string()), ("R3".to_string(), "R6".to_string())], "R1 is below the start; RN1, C1 and U1 have other letters");
    }

    #[test]
    fn a_start_without_a_number_plans_nothing_and_a_negative_result_is_refused() {
        assert!(increment_plan(&refs(&["R1", "R2"]), "R?", 1).unwrap().is_empty());
        assert!(increment_plan(&refs(&["R1", "R2"]), "R", 1).unwrap().is_empty());
        assert!(increment_plan(&refs(&["R1", "R2"]), "R1", -2).is_err(), "R1 would become R-1");
        let down = increment_plan(&refs(&["R3", "R4"]), "R3", -1).unwrap();
        assert_eq!(down, vec![("R3".to_string(), "R2".to_string()), ("R4".to_string(), "R3".to_string())]);
    }

    #[test]
    fn the_item_name_is_the_part_after_the_colon() {
        assert_eq!(item_name("Device:R_Small"), "R_Small");
        assert_eq!(item_name("R"), "R");
    }
}
