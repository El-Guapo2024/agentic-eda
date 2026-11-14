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
use eda_model::sheet_list;
use std::collections::{BTreeMap, BTreeSet};

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

/// [`Board::set_symbol_lib_ids`] on one screen: every symbol linked to a `from` id is linked to its `to` (`SetLibId`), its Value following the item name
/// when it was the old one's, and with `update_fields` the library symbol's Value, Datasheet and an empty Footprint.
fn relink(sch: &mut eda_model::ir::SchematicSection, changes: &[(String, String)], resolved: &BTreeMap<String, String>, update_fields: bool) {
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
}

impl<'a> Board<'a> {
    /// The screens of the hierarchy (`""` the root, then each sheet file some sheet path shows), each once.
    fn hierarchy_screens(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in sheet_list::walk(&self.design) {
            if !out.contains(&e.file) {
                out.push(e.file);
            }
        }
        out
    }

    /// `SCH_EDIT_TOOL::SetAttribute`'s `collectedItems`: the symbols `ids` of the sheet in view (every placed unit of each), and -- "The attributes should be kept
    /// in sync in multi-unit parts. Of course the symbol must be annotated to collect other units." (`symbol->IsAnnotated( &GetCurrentSheet() )`, then
    /// `CollectOtherUnits( ref, unit, libId, sheet, &otherUnits )` for every sheet of `Hierarchy()`) -- every symbol of the same library symbol to which some
    /// sheet path gives the reference the sheet path in view gives it (`GetRef( &GetCurrentSheet() )`). A screen shown on two paths names a part per path:
    /// the units collected are those of the reference in view, on whatever screen they are (UI-LEFT S9). As (screen, index), each once; and the references
    /// the collected symbols have on every path (the board's parts that take the flags).
    fn attribute_targets(&self, ids: &[String]) -> Result<(Vec<(String, usize)>, BTreeSet<String>), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "attributes", "select a symbol first"));
        }
        let here = self.focus.clone().unwrap_or_default();
        let path = self.focus_sheet_path();
        let mut wanted: Vec<(String, String)> = Vec::new();
        let mut out: Vec<(String, usize)> = Vec::new();
        {
            let sch = self.schematic()?;
            for id in ids {
                if !sch.symbols.iter().any(|s| &s.id == id) {
                    return Err(fail("ops_unknown_symbol", id, "no symbol instance with this reference"));
                }
                for (i, s) in sch.symbols.iter().enumerate().filter(|(_, s)| &s.id == id) {
                    out.push((here.clone(), i));
                    let reference = sheet_list::reference_under(sch, s, &path);
                    if !eda_model::annotate::is_unannotated(&reference) {
                        wanted.push((reference, s.lib_id.clone()));
                    }
                }
            }
        }
        for e in sheet_list::walk(&self.design) {
            let Some(sch) = sheet_list::screen_of(&self.design, &e.path) else { continue };
            for (i, s) in sch.symbols.iter().enumerate() {
                let reference = sheet_list::reference_under(sch, s, &e.path);
                if wanted.iter().any(|(r, lib)| *r == reference && *lib == s.lib_id) && !out.contains(&(e.file.clone(), i)) {
                    out.push((e.file.clone(), i));
                }
            }
        }
        let paths_of = sheet_list::paths_of_screens(&self.design);
        let mut refs: BTreeSet<String> = ids.iter().cloned().collect();
        for (file, i) in &out {
            let Some(sch) = sheet_list::screen_of_file(&self.design, file) else { continue };
            let Some(s) = sch.symbols.get(*i) else { continue };
            for p in paths_of.get(file).into_iter().flatten() {
                refs.insert(sheet_list::reference_under(sch, s, p));
            }
        }
        Ok((out, refs))
    }

    /// `SCH_EDIT_TOOL::SetAttribute`: `Some(v)` sets the attribute on what [`Self::attribute_targets`] collects, `None` leaves it. Which state a toggle goes to
    /// (`new_state`) is the caller's here; [`Self::toggle_symbol_attribute`] works it out as the C++ does.
    pub(crate) fn set_symbol_attrs(&mut self, ids: &[String], dnp: Option<bool>, exclude_from_bom: Option<bool>, exclude_from_board: Option<bool>, exclude_from_sim: Option<bool>) -> Result<(), Vec<CheckResult>> {
        let (targets, refs) = self.attribute_targets(ids)?;
        for (file, i) in &targets {
            let Some(s) = sheet_list::screen_of_file_mut(&mut self.design, file).and_then(|sch| sch.symbols.get_mut(*i)) else { continue };
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
        // The board's footprint carries the same two flags: the one edited last wins (`Board::sync_footprint_bom_flags`); a screen shown on two paths
        // gives the flags (the symbol's own, `m_DNP`) to the part of each path.
        let refs: Vec<String> = refs.into_iter().collect();
        self.sync_footprint_bom_flags(&refs, dnp, exclude_from_bom);
        Ok(())
    }

    /// [`crate::sch_edit::SchCmd::ToggleSymbolAttribute`]: `SetAttribute`'s `new_state` -- set when any collected item does not have the attribute
    /// (`!item->GetDNP( sheet, variant )`, ...), else cleared -- over everything [`Self::attribute_targets`] collects, the units on other sheets included
    /// (r5 D5-M2), then [`Self::set_symbol_attrs`].
    ///
    /// `RequestSelection( { SCH_SYMBOL_T, SCH_SHEET_T, SCH_RULE_AREA_T } )`: `ids` may also name sheets of the sheet in view (their own flags,
    /// `SchExtras::sheet_props`: `SCH_SHEET::SetDNP( new_state, sheet, variant )` in the default variant) and rule areas (`SCH_RULE_AREA`'s flags, which no
    /// variant has); `new_state` is worked out over all of them together, as `collectedItems` holds them all. A power symbol is an `SCH_SYMBOL` too, taken as
    /// any symbol (its reference is its own, so it has no other units): its flags are `SchExtras::power_props`' (the writer's `(in_bom)`, `(on_board)`,
    /// `(exclude_from_sim)`, `(dnp)`), in the default variant as a sheet's.
    ///
    /// In a design variant (`variant` not empty; KiCad 25ec2335 `SetAttribute`: `GetDNP( sheet, variant )`, `SetDNP( new_state, sheet, variant )`), a symbol's
    /// state is its record of the variant, else its own flag, read and set on every record [`Self::set_symbol_variant`] edits -- each unit of the reference
    /// on every sheet, at its own sheet path (UI-LEFT-3 S3-18) -- a sheet's its record at the path in view, a rule area's and a power symbol's their own (no
    /// variant has them). **Differs**: KiCad passes the sheet in view for every collected unit, so a unit on another sheet has no instance there: its own flag
    /// counts for `new_state` (`GetDNP` falls back to `m_DNP`) and `setVariantAttribute` leaves it alone (`wxCHECK_MSG( instance, .. )`, an assertion); here
    /// its record is read and set as the units of the sheet in view are, so the units of one part never part in a variant.
    pub(crate) fn toggle_symbol_attribute(&mut self, ids: &[String], attribute: &str, variant: &str) -> Result<(), Vec<CheckResult>> {
        use eda_model::sch_extras::SchGraphicKind;
        if !matches!(attribute, "dnp" | "exclude_from_bom" | "exclude_from_board" | "exclude_from_sim") {
            return Err(fail("ops_bad_attribute", attribute, "the attribute is dnp, exclude_from_bom, exclude_from_board or exclude_from_sim"));
        }
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "attributes", "select a symbol first"));
        }
        let (sheet_ids, area_ids, power_ids, symbol_ids) = {
            let sch = self.schematic()?;
            let is_sheet = |id: &String| sch.sheets.iter().any(|s| &s.id == id);
            let is_area = |id: &String| sch.extras.graphics.iter().any(|g| &g.id == id && matches!(g.shape, SchGraphicKind::RuleArea { .. }));
            let is_power = |id: &String| sch.power_symbols.iter().any(|p| &p.id == id);
            let sheets: Vec<String> = ids.iter().filter(|id| is_sheet(id)).cloned().collect();
            let areas: Vec<String> = ids.iter().filter(|id| !is_sheet(id) && is_area(id)).cloned().collect();
            let powers: Vec<String> = ids.iter().filter(|id| !is_sheet(id) && !is_area(id) && is_power(id)).cloned().collect();
            let symbols: Vec<String> = ids.iter().filter(|id| !is_sheet(id) && !is_area(id) && !is_power(id)).cloned().collect();
            (sheets, areas, powers, symbols)
        };
        let pick = |dnp: bool, bom: bool, board: bool, sim: bool| match attribute {
            "dnp" => dnp,
            "exclude_from_bom" => bom,
            "exclude_from_board" => board,
            _ => sim,
        };
        let variant = if variant.is_empty() { String::new() } else { self.existing_variant(variant)? };
        // a sheet's record of the variant: the path in view's where its screen is shown on several paths (`set_sheet_variant`'s)
        let sheet_parent: Vec<String> = if self.focus.as_ref().is_some_and(|f| eda_model::sheet_list::placement_counts(&self.design).get(f).copied().unwrap_or(0) > 1) { self.focus_sheet_path() } else { Vec::new() };
        let mut new_state = false;
        if variant.is_empty() {
            let targets = if symbol_ids.is_empty() { Vec::new() } else { self.attribute_targets(&symbol_ids)?.0 };
            for (file, i) in &targets {
                let Some(s) = sheet_list::screen_of_file(&self.design, file).and_then(|sch| sch.symbols.get(*i)) else { continue };
                new_state |= !pick(s.dnp, s.exclude_from_bom, s.exclude_from_board, s.exclude_from_sim);
            }
        } else {
            if !symbol_ids.is_empty() {
                // refused as the default variant's is: a symbol the sheet in view does not have
                self.attribute_targets(&symbol_ids)?;
            }
            for (key, id, path) in self.variant_symbol_records(&symbol_ids) {
                let Some(sch) = self.section_of_key(key.as_deref()) else { continue };
                for sym in sch.symbols.iter().filter(|s| s.id == id) {
                    let f = eda_model::variants::resolved_flags_on(sch, sym, path.as_deref().unwrap_or(&[]), &variant);
                    new_state |= !pick(f.dnp, f.exclude_from_bom, f.exclude_from_board, f.exclude_from_sim);
                }
            }
        }
        {
            let sch = self.schematic()?;
            for id in &sheet_ids {
                let on = if variant.is_empty() {
                    let p = sch.extras.sheet_props_of(id);
                    pick(p.dnp, p.exclude_from_bom, p.exclude_from_board, p.exclude_from_sim)
                } else {
                    let r = eda_model::variants::sheet_flags_on(sch, id, &sheet_parent, &variant);
                    pick(r.dnp, r.exclude_from_bom, r.exclude_from_board, r.exclude_from_sim)
                };
                new_state |= !on;
            }
            for g in sch.extras.graphics.iter().filter(|g| area_ids.contains(&g.id)) {
                if let SchGraphicKind::RuleArea { dnp, exclude_from_bom, exclude_from_board, exclude_from_sim, .. } = &g.shape {
                    new_state |= !pick(*dnp, *exclude_from_bom, *exclude_from_board, *exclude_from_sim);
                }
            }
            for id in &power_ids {
                let p = sch.extras.power_props.get(id).cloned().unwrap_or_default();
                new_state |= !pick(p.dnp, p.exclude_from_bom, p.exclude_from_board, p.exclude_from_sim);
            }
        }
        let v = Some(new_state);
        if !variant.is_empty() {
            let flags = crate::sch_variants::VariantFlags {
                dnp: (attribute == "dnp").then_some(new_state),
                exclude_from_bom: (attribute == "exclude_from_bom").then_some(new_state),
                exclude_from_board: (attribute == "exclude_from_board").then_some(new_state),
                exclude_from_sim: (attribute == "exclude_from_sim").then_some(new_state),
                exclude_from_pos_files: None,
            };
            if !symbol_ids.is_empty() {
                self.set_symbol_variant(&variant, &symbol_ids, &flags, &BTreeMap::new())?;
            }
            for id in &sheet_ids {
                let r = eda_model::variants::sheet_flags_on(self.schematic()?, id, &sheet_parent, &variant);
                if pick(r.dnp, r.exclude_from_bom, r.exclude_from_board, r.exclude_from_sim) != new_state {
                    self.set_sheet_variant(id, &variant, &flags, &BTreeMap::new())?;
                }
            }
        } else if !symbol_ids.is_empty() {
            match attribute {
                "dnp" => self.set_symbol_attrs(&symbol_ids, v, None, None, None)?,
                "exclude_from_bom" => self.set_symbol_attrs(&symbol_ids, None, v, None, None)?,
                "exclude_from_board" => self.set_symbol_attrs(&symbol_ids, None, None, v, None)?,
                _ => self.set_symbol_attrs(&symbol_ids, None, None, None, v)?,
            }
        }
        let sch = self.schematic_mut()?;
        for id in sheet_ids.iter().filter(|_| variant.is_empty()) {
            let mut p = sch.extras.sheet_props_of(id);
            *match attribute {
                "dnp" => &mut p.dnp,
                "exclude_from_bom" => &mut p.exclude_from_bom,
                "exclude_from_board" => &mut p.exclude_from_board,
                _ => &mut p.exclude_from_sim,
            } = new_state;
            sch.extras.set_sheet_props(id, p);
        }
        for g in sch.extras.graphics.iter_mut().filter(|g| area_ids.contains(&g.id)) {
            if let SchGraphicKind::RuleArea { dnp, exclude_from_bom, exclude_from_board, exclude_from_sim, .. } = &mut g.shape {
                *match attribute {
                    "dnp" => dnp,
                    "exclude_from_bom" => exclude_from_bom,
                    "exclude_from_board" => exclude_from_board,
                    _ => exclude_from_sim,
                } = new_state;
            }
        }
        for id in &power_ids {
            let mut p = sch.extras.power_props.get(id).cloned().unwrap_or_default();
            *match attribute {
                "dnp" => &mut p.dnp,
                "exclude_from_bom" => &mut p.exclude_from_bom,
                "exclude_from_board" => &mut p.exclude_from_board,
                _ => &mut p.exclude_from_sim,
            } = new_state;
            if p.is_default() {
                sch.extras.power_props.remove(id);
            } else {
                sch.extras.power_props.insert(id.clone(), p);
            }
        }
        Ok(())
    }

    /// `SCH_EDIT_TOOL::EditPageNumber`: the placement `sheet` (a `SheetInstance::id`, found on the root sheet or on any sheet
    /// screen) gets `page`. Letters and digits only; empty clears it. With `path` (the sheet path, ending in `sheet`), the page of that path
    /// only (`SCH_SHEET_PATH::SetPageNumber`, the sheet's instance on the path of the sheet in view), as `sheet_list::set_page` keeps it;
    /// without, of every path the sheet is on.
    pub(crate) fn set_sheet_page(&mut self, sheet: &str, page: &str, path: Option<&[String]>) -> Result<(), Vec<CheckResult>> {
        let page = page.trim();
        if !page_number_is_valid(page) {
            return Err(fail("ops_bad_page", sheet, "a page number is letters and digits only (no spaces)"));
        }
        if let Some(path) = path {
            // the root path (`SCH_EDIT_TOOL::EditPageNumber` with the root in view and no sheet selected: "The root sheet and root screen are effectively the
            // same thing"): the root's own page (`SchExtras::root_page`; UI-LEFT S21). The walk gives it 1 when it has none.
            if path.is_empty() {
                let root = self.design.schematic.as_mut().ok_or_else(|| fail("ops_no_schematic", "schematic", "this board has no schematic section yet"))?;
                let page = if page == "1" { String::new() } else { page.to_string() };
                if root.extras.root_page == page {
                    return Err(fail("ops_page_unchanged", "root", "the root sheet already has this page number"));
                }
                root.extras.root_page = page;
                return Ok(());
            }
            if path.last().map(String::as_str) != Some(sheet) || !eda_model::sheet_list::set_page(&mut self.design, path, page) {
                return Err(fail("ops_unknown_sheet", sheet, "no sheet with this id on this sheet path"));
            }
            return Ok(());
        }
        match eda_model::sheet_list::placement_for_every_path(&mut self.design, sheet) {
            Some(s) => {
                s.page = page.to_string();
                Ok(())
            }
            None => Err(fail("ops_unknown_sheet", sheet, "no sheet with this id")),
        }
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
            if !self.hierarchy_screens().iter().filter_map(|f| sheet_list::screen_of_file(&self.design, f)).any(|sch| sch.symbols.iter().any(|s| &s.lib_id == from)) {
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
        // every screen of the hierarchy (`DIALOG_EDIT_SYMBOLS_LIBID::initDlg`: `Hierarchy().GetSymbols( references, SYMBOL_FILTER_ALL, true )`, a row per symbol and
        // sheet path; `TransferDataFromWindow` relinks each candidate's symbol, which a screen shown twice holds once; UI-LEFT S9)
        self.schematic()?;
        for file in self.hierarchy_screens() {
            let Some(sch) = sheet_list::screen_of_file_mut(&mut self.design, &file) else { continue };
            relink(sch, changes, &resolved, update_fields);
        }
        Ok(())
    }

    /// `SCH_EDITOR_CONTROL::IncrementAnnotations` -- see [`increment_plan`]. The references move together: a plan like `R5 -> R6, R6 -> R7`
    /// would trip over itself one rename at a time, so every moved reference first takes a temporary name (never a real one) and then its
    /// final one. Each rename carries what `Cmd::RenameSymbol` carries (the wires', power symbols' and no-connects' `REF.PIN` strings, the
    /// user fields). Refused as a whole when a new reference is already another symbol's.
    pub(crate) fn increment_annotations(&mut self, start: &str, increment: i32, all_sheets: bool) -> Result<(), Vec<CheckResult>> {
        if all_sheets {
            return self.increment_annotations_everywhere(start, increment);
        }
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
            if self.board_part_without_symbol(new) {
                return Err(fail("ops_duplicate_symbol", new, "a footprint with this reference is already on the board"));
            }
        }
        for (i, (old, _)) in plan.iter().enumerate() {
            self.rename_symbol_in_schematic(old, &format!("\u{1}tmp{i}"))?;
        }
        for (i, (_, new)) in plan.iter().enumerate() {
            self.rename_symbol_in_schematic(&format!("\u{1}tmp{i}"), new)?;
        }
        // the parts on the board follow in one go, so a plan like `R5 -> R6, R6 -> R7` never has two parts under one reference
        self.rename_part(&plan.iter().cloned().collect());
        Ok(())
    }

    /// [`Self::increment_annotations`] with "All sheets" (`schematic->Hierarchy().GetSymbols( references, SYMBOL_FILTER_ALL )`, then
    /// `ref.GetSymbol()->SetRef( &sheet, .. )` for each sheet path): the references of every sheet, a shared screen's later paths' own references
    /// (`instance_overrides`) included, move by the one plan (r5 D5-M3). A reference's units on several sheets move together. Refused as a whole when a
    /// new reference is one that does not move.
    fn increment_annotations_everywhere(&mut self, start: &str, increment: i32) -> Result<(), Vec<CheckResult>> {
        let mut screens: Vec<Option<String>> = vec![None];
        screens.extend(self.design.sheet_contents.iter().flat_map(|c| c.keys().cloned()).map(Some));
        let sections = || self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.values()));
        let mut refs: Vec<String> = sections().flat_map(|s| s.symbols.iter().map(|x| x.id.clone()).chain(s.instance_overrides.iter().map(|o| o.reference.clone()))).collect();
        refs.sort();
        refs.dedup();
        let plan = increment_plan(&refs, start, increment).map_err(|m| fail("ops_bad_increment", start, m))?;
        if plan.is_empty() {
            return Ok(());
        }
        let moving: Vec<&String> = plan.iter().map(|(old, _)| old).collect();
        for (_, new) in &plan {
            if refs.iter().any(|r| r == new && !moving.contains(&r)) {
                return Err(fail("ops_duplicate_symbol", new, "a symbol with this reference is already on a sheet"));
            }
            if !moving.contains(&new) && self.board_part_without_symbol(new) {
                return Err(fail("ops_duplicate_symbol", new, "a footprint with this reference is already on the board"));
            }
        }
        let saved_focus = self.focus.clone();
        let mut outcome: Result<(), Vec<CheckResult>> = Ok(());
        'pass: for pass in 0..2 {
            for (i, (old, new)) in plan.iter().enumerate() {
                let temp = format!("\u{1}tmp{i}");
                let (from, to) = if pass == 0 { (old.as_str(), temp.as_str()) } else { (temp.as_str(), new.as_str()) };
                for screen in &screens {
                    self.focus = screen.clone();
                    let has = self.schematic().map(|s| s.symbols.iter().any(|x| x.id == from)).unwrap_or(false);
                    if has {
                        if let Err(e) = self.rename_symbol_here_unchecked(from, to) {
                            outcome = Err(e);
                            break 'pass;
                        }
                    }
                }
            }
        }
        self.focus = saved_focus;
        outcome?;
        let map: BTreeMap<String, String> = plan.iter().cloned().collect();
        for sch in self.design.schematic.iter_mut().chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.values_mut())) {
            for o in sch.instance_overrides.iter_mut() {
                if let Some(new) = map.get(&o.reference) {
                    o.reference = new.clone();
                }
            }
        }
        self.rename_part(&map);
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
