//! Whole hierarchical sheets coming into the design: Import Sheet / Draw Sheet from File (`Cmd::ImportSheet`) and the sheet part of Paste.
//!
//! Ported from `SCH_EDIT_FRAME::LoadSheetFromFile` and `ChangeSheetFile` (`eeschema/sheet.cpp`), `SCH_DRAWING_TOOLS::ImportSheet` /
//! `DrawSheet` (`eeschema/tools/sch_drawing_tools.cpp`) and the sheet branch of `SCH_EDITOR_CONTROL::Paste`
//! (`eeschema/tools/sch_editor_control.cpp`):
//!
//! * A pasted sheet whose file the design already shows **shares that screen** (`Paste`: `SearchHierarchy` / `loadedScreens` find the
//!   screen, `sheet->SetScreen( existingScreen )`): the new sheet symbol names the same file, and its symbols get references of their own
//!   for that placement (`updatePastedSheet` -> `updatePastedSymbol` adds a `SCH_SYMBOL_INSTANCE` for the new path, numbered by
//!   `ReannotateDuplicates`), kept in `SchematicSection::instance_overrides` under each new sheet path. A screen that has sheets of its
//!   own is shared whole, as KiCad shares it (`updatePastedSheet` recurses into the subsheets): every screen below the new sheet gets
//!   references of its own for the new paths, each starting from the reference it has under the copied sheet. The screen in the
//!   clipboard is not used, as KiCad does not use it.
//! * Any other sheet comes in as a **copy** (Import Sheet, and a paste of a sheet whose file the design does not show: KiCad's
//!   `LoadSheetFromFile`): its screen goes into the design under a file of its own, and so does the screen of every sheet inside it.
//! * The new sheet has a name no sheet on the page has: the pasted name, its trailing number counted up until it is free
//!   (`Paste`: `wxIsdigit( baseName.Last() )` ... `existingSheetNames`).
//! * The library symbols its symbols name are merged with the design's: the design's own win, the sheet's come in under their own names
//!   (`Cmd::PasteSch` does the same for a fragment).
//! * The symbols inside are numbered: a pasted sheet like a pasted fragment (`ReannotateDuplicates`: a reference nothing else holds is
//!   kept, so a Cut and Paste keeps the numbers); an imported file as `DrawSheet` does when annotating is automatic
//!   (`AnnotateSymbols( ANNOTATE_SELECTION ... reset )`), unless "Keep annotations" is on, which numbers only what is taken.
//! * A file that places itself, or a parent of itself, is refused (`CheckSheetForRecursion`); symbols that name no library at all are
//!   refused (`checkForNoFullyDefinedLibIds`). Missing libraries and duplicate references are findings ([`Board::check_sheet_file`]).

use crate::sch_annotate::{rename_in_section, AnnotateOptions, AnnotateScope, Item, ScreenScope, SecRename, Take};
use crate::sch_clipboard::LibMerge;
use crate::{AnnotateOrder, Board};
use eda_model::ir::{LibrarySymbol, Point, SchematicSection, SheetInstance, Nm};
use eda_model::sch_clipboard::{annotate_paste, PasteMode, PastedRef, SchFragment, UsedRef};
use eda_model::sheet_file::SheetFile;
use eda_model::sheet_list::str_num_cmp;
use eda_model::CheckResult;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// One thing the checks of a sheet file found: `error` stops the import, `warning` is KiCad's "Continue Load?", `info` says what will happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SheetFileFinding {
    pub severity: &'static str,
    pub text: String,
}

/// The key a file's own screen has among its sheets' screens, where one list holds them all.
const OWN: &str = "\u{0}file";

/// How the symbols of a sheet that comes in are numbered.
#[derive(Debug, Clone)]
pub(crate) enum Numbering {
    /// Paste's `ReannotateDuplicates` (and Paste Special's choices).
    Paste(PasteMode),
    /// Draw Sheet from File: the automatic annotation of the symbols of the new sheet, all of them (a reset) in the order and with the
    /// numbering the schematic's settings say.
    Annotate { order: AnnotateOrder, options: AnnotateOptions },
}

/// A sheet that comes in: the sheet symbol, the screens it shows, the library symbols its symbols name.
pub(crate) struct Incoming<'a> {
    pub placement: SheetInstance,
    /// Every screen a sheet of this tree shows, by the file its placement names (the placement's own screen too).
    pub screens: &'a BTreeMap<String, SchematicSection>,
    pub lib_symbols: &'a [LibrarySymbol],
    /// The sheet's user fields (`SchExtras::sheet_fields` of the screen it came from): `SCH_SHEET::m_fields` travel with any copy of the sheet.
    pub fields: Option<Vec<eda_model::sch_extras::LabelField>>,
}

/// `wxIsdigit( baseName.Last() )`: the name counted up until no sheet on the page has it.
pub fn unique_sheet_name(existing: &BTreeSet<String>, name: &str) -> String {
    let mut base = name.to_string();
    let mut number = String::new();
    while base.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        number.insert(0, base.pop().unwrap_or_default());
    }
    let mut uniquifier = number.parse::<i64>().unwrap_or(0).max(0) + 1;
    let mut candidate = name.to_string();
    while existing.contains(&candidate) {
        candidate = format!("{base}{uniquifier}");
        uniquifier += 1;
    }
    candidate
}

/// The files `file` shows, depth first, parents before children (`file` itself first); `Err(file)` for a file that is its own ancestor.
fn tree_of(file: &str, screen_of: &dyn Fn(&str) -> Option<SchematicSection>, order: &mut Vec<String>, stack: &mut Vec<String>) -> Result<(), String> {
    if stack.iter().any(|f| f == file) {
        return Err(file.to_string());
    }
    if order.iter().any(|f| f == file) {
        return Ok(());
    }
    order.push(file.to_string());
    stack.push(file.to_string());
    if let Some(sec) = screen_of(file) {
        for sh in &sec.sheets {
            tree_of(&sh.file, screen_of, order, stack)?;
        }
    }
    stack.pop();
    Ok(())
}

/// The symbols of the shared screens before a command ([`Board::shared_screens_before`]): file -> ((reference, unit), place).
#[derive(Default)]
pub(crate) struct SharedScreens(BTreeMap<String, Vec<((String, u32), Point)>>);

impl<'a> Board<'a> {
    /// `LoadSheetFromFile`'s checks on a file that is to be a sheet: what stops it, what KiCad asks "Continue Load?" about, and what will happen to the
    /// references. (The libraries the project does not have installed are the server's to find: the verb reads no library.)
    pub fn check_sheet_file(&self, content: &SheetFile, name: &str, keep_annotations: bool) -> Vec<SheetFileFinding> {
        let mut out = Vec::new();
        let label = if name.is_empty() { "the file" } else { name };
        // `CheckSheetForRecursion`: a sheet of the file that shows a parent of itself
        let mut order = Vec::new();
        if let Some(file) = content.root.sheets.iter().find_map(|sh| tree_of(&sh.file, &|f| content.screens.get(f).cloned(), &mut order, &mut Vec::new()).err()) {
            out.push(SheetFileFinding { severity: "error", text: format!("The sheet changes cannot be made because the schematic '{label}' places '{file}', which has itself as a parent somewhere in its hierarchy.") });
        }
        // `checkForNoFullyDefinedLibIds`: `SCH_SCREENS::HasNoFullyDefinedLibIds` refuses a file whose symbols, every one of them, name no library ("return true
        // (i.e. has no fully defined symbol) only if at least one symbol is found"); one symbol with a library nickname lets the file in
        let ids: Vec<&str> = content.all_screens().flat_map(|(_, s)| s.symbols.iter().map(|x| x.lib_id.as_str()).chain(s.power_symbols.iter().map(|p| p.lib_id.as_str()))).collect();
        let nickname = |id: &str| id.split_once(':').is_some_and(|(l, n)| !l.is_empty() && !n.is_empty());
        if !ids.is_empty() && !ids.iter().any(|id| nickname(id) || eda_model::is_synthetic_lib_id(id)) {
            out.push(SheetFileFinding {
                severity: "error",
                text: format!(
                    "The schematic '{label}' has not had its symbol library links remapped to the symbol library table.  The project this schematic belongs to must first be remapped before it can be imported into the current project."
                ),
            });
        }
        for note in &content.notes {
            out.push(SheetFileFinding { severity: "warning", text: note.clone() });
        }
        // the references the design has already
        let have: BTreeSet<String> = self.used_references().into_iter().map(|u| u.reference).collect();
        let clash: Vec<String> = content.references().into_iter().filter(|r| have.contains(r) && !r.starts_with('#')).collect();
        if !clash.is_empty() {
            let shown: Vec<&str> = clash.iter().map(String::as_str).take(8).collect();
            let more = if clash.len() > shown.len() { format!(" and {} more", clash.len() - shown.len()) } else { String::new() };
            let what = if keep_annotations { "are numbered anew even though the annotations are kept: a design here names a part by its reference" } else { "are numbered anew" };
            out.push(SheetFileFinding { severity: "info", text: format!("{} reference(s) of the sheet are used in this schematic already ({}{more}) and {what}.", clash.len(), shown.join(", ")) });
        }
        out
    }

    /// Put a copy of the sheet `inc` on the screen the verbs act on: the sheet symbol (named `name`, moved by `shift`), a copy of each screen it shows under a
    /// file of its own, the library symbols its symbols need, its symbols numbered. Returns the files the copies got (the sheet's first).
    pub(crate) fn add_sheet_tree(&mut self, inc: Incoming<'_>, shift: (Nm, Nm), name: &str, file_hint: Option<&str>, numbering: &Numbering) -> Result<Vec<String>, Vec<CheckResult>> {
        let pages_before = eda_model::sheet_list::walk(&self.design);
        let own = self.design.sheet_contents.clone().unwrap_or_default();
        let screen_of = |f: &str| -> Option<SchematicSection> { inc.screens.get(f).or_else(|| own.get(f)).cloned() };
        // the files the sheet shows, parents first
        let mut order = Vec::new();
        if let Err(file) = tree_of(&inc.placement.file, &screen_of, &mut order, &mut Vec::new()) {
            return Err(vec![CheckResult::fail("ops_sheet_recursion", &file, format!("the sheet '{name}' shows '{file}', which has itself as a parent somewhere in the hierarchy"))]);
        }
        // a file of its own for each
        // (a screen no sheet shows any more is free to be written over: the sheet that came back from a Cut is the one it was)
        let reachable = eda_model::sheet_list::reachable_files(&self.design);
        let mut taken: BTreeSet<String> = own.keys().filter(|f| reachable.contains(*f)).cloned().collect();
        let mut new_of: BTreeMap<String, String> = BTreeMap::new();
        for (i, orig) in order.iter().enumerate() {
            let stem = if i == 0 { file_hint.map(|f| f.trim_end_matches(".kicad_sch").to_string()).unwrap_or_else(|| name.to_string()) } else { orig.trim_end_matches(".kicad_sch").rsplit('/').next().unwrap_or(orig).to_string() };
            let fresh = eda_engine::hier::sheet_file_name(&stem, &taken);
            taken.insert(fresh.clone());
            new_of.insert(orig.clone(), fresh);
        }
        // the copies
        let mut copies: Vec<(String, SchematicSection)> = Vec::new();
        // each copy's sheets' user fields, by the sheet's place on its screen: the sheets get new ids, and their fields go with them (r3 B3-M2)
        let mut inner_fields: BTreeMap<String, Vec<Option<Vec<eda_model::sch_extras::LabelField>>>> = BTreeMap::new();
        for orig in &order {
            let mut sec = screen_of(orig).unwrap_or_else(crate::empty_schematic_section);
            let fields = std::mem::take(&mut sec.extras.sheet_fields);
            inner_fields.insert(new_of[orig].clone(), sec.sheets.iter().map(|sh| fields.get(&sh.id).cloned()).collect());
            // (the pages kept per sheet path are of the original's paths)
            sec.extras.sheet_pages.clear();
            for sh in sec.sheets.iter_mut() {
                if let Some(f) = new_of.get(&sh.file) {
                    sh.file = f.clone();
                }
                sh.id.clear();
                sh.pins.iter_mut().for_each(|p| p.id.clear());
            }
            copies.push((new_of[orig].clone(), sec));
        }
        // the library symbols
        let mut wanted: Vec<String> = copies.iter().flat_map(|(_, s)| s.symbols.iter().map(|x| x.lib_id.clone()).chain(s.power_symbols.iter().map(|p| p.lib_id.clone()))).filter(|l| !l.is_empty()).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let LibMerge { lib_name, engine_of, publish } = self.merge_lib_symbols(&wanted, inc.lib_symbols);
        for (_, sec) in copies.iter_mut() {
            place_by_box_corner(sec, &engine_of);
            sec.symbols.iter_mut().for_each(|s| {
                if let Some(n) = lib_name.get(&s.lib_id) {
                    s.lib_id = n.clone();
                }
            });
            sec.power_symbols.iter_mut().for_each(|p| {
                if let Some(n) = lib_name.get(&p.lib_id) {
                    p.lib_id = n.clone();
                }
            });
        }
        let units_of = |board: &Self, lib_id: &str| -> u32 { engine_of.get(lib_id).cloned().or_else(|| board.design_symbol(lib_id)).map_or(1, |l| l.unit_count.max(1)) };

        // the numbers (before the copies are in the design, so a copy does not hold its own references against itself)
        match numbering {
            Numbering::Paste(mode) => {
                let mut used = self.used_references();
                for (_, sec) in copies.iter_mut() {
                    self.number_screen(sec, &mut used, *mode, &units_of);
                }
            }
            Numbering::Annotate { .. } => {}
        }

        // the sheet symbol
        let mut placement = inc.placement.clone();
        placement.id.clear();
        placement.name = name.to_string();
        placement.file = new_of[&inc.placement.file].clone();
        placement.page.clear();
        placement.at = Point { x: placement.at.x + shift.0, y: placement.at.y + shift.1 };
        for p in placement.pins.iter_mut() {
            p.id.clear();
            p.at = Point { x: p.at.x + shift.0, y: p.at.y + shift.1 };
        }
        let files: Vec<String> = copies.iter().map(|(f, _)| f.clone()).collect();

        // into the design
        if !publish.is_empty() {
            let lib = self.design.symbol_library.get_or_insert_with(Default::default);
            lib.symbols.extend(publish);
            lib.symbols.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
        }
        {
            let contents = self.design.sheet_contents.get_or_insert_with(BTreeMap::new);
            for (file, mut sec) in copies {
                sec.assign_missing_ids();
                for (sh, fields) in sec.sheets.iter().zip(inner_fields.remove(&file).unwrap_or_default()) {
                    if let Some(fields) = fields {
                        sec.extras.sheet_fields.insert(sh.id.clone(), fields);
                    }
                }
                contents.insert(file, sec);
            }
        }
        {
            let sch = self.schematic_mut_or_create();
            sch.sheets.push(placement);
            sch.assign_missing_ids();
            if let (Some(fields), Some(id)) = (inc.fields.clone(), sch.sheets.last().map(|s| s.id.clone())) {
                sch.extras.sheet_fields.insert(id, fields);
            }
        }

        // the new sheet paths' page numbers before the annotation, which may number by page (`DrawSheet` gives the sheet `GetNextPageNumber`
        // and `LoadSheetFromFile` the sheets inside it theirs before `AnnotateSymbols` runs; `Board::apply` would number them after)
        eda_model::sheet_list::keep_pages(&mut self.design, &pages_before);
        // an import's automatic annotation, now that the sheets are in
        if let Numbering::Annotate { order, options } = numbering {
            self.annotate_new_screens(&files, *order, options);
        }
        // a screen the incoming tree shows more than once (KiCad's `complex_hierarchy` places `ampli_ht` twice) is one screen here too: each placement past
        // the first gets references of its own
        let mode = match numbering {
            Numbering::Paste(mode) => *mode,
            Numbering::Annotate { .. } => PasteMode::Unique,
        };
        self.number_later_placements(&files, mode, &units_of);
        Ok(files)
    }

    /// The references a screen of `files` that is shown more than once has on its later placements (`SCH_SYMBOL_INSTANCE` per sheet path: `updatePastedSheet` ->
    /// `updatePastedSymbol` for each path, `ReannotateDuplicates`): its symbols numbered against every reference of the design, the placements before it
    /// included, kept in `SchematicSection::instance_overrides` under the sheet path. The first sheet path keeps the screen's own. A path that already
    /// has references of its own is left as it is; so are power symbols.
    fn number_later_placements(&mut self, files: &[String], mode: PasteMode, units_of: &dyn Fn(&Self, &str) -> u32) {
        // the copies' screens came with the overrides of the placements they were copied from, under ids no placement here has
        eda_model::sheet_list::prune_orphaned_overrides(&mut self.design);
        let entries = eda_model::sheet_list::walk(&self.design);
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for e in entries.iter().skip(1) {
            if !files.contains(&e.file) || seen.insert(e.file.clone()) {
                continue;
            }
            let Some(screen) = self.design.sheet_contents.as_ref().and_then(|c| c.get(&e.file)).cloned() else { continue };
            if screen.instance_overrides.iter().any(|o| o.sheet_path == e.path) {
                continue;
            }
            let pasted: Vec<PastedRef> = screen
                .symbols
                .iter()
                .map(|s| PastedRef { reference: s.id.clone(), unit: s.unit, unit_count: units_of(self, &s.lib_id), lib_id: s.lib_id.clone(), value: s.value.clone(), at: (s.at.x, s.at.y), power: false })
                .collect();
            let numbered = annotate_paste(&self.used_references(), &pasted, mode, Default::default(), 1);
            let Some(sec) = self.design.sheet_contents.as_mut().and_then(|c| c.get_mut(&e.file)) else { continue };
            for (s, reference) in screen.symbols.iter().zip(numbered) {
                if reference != s.id {
                    sec.instance_overrides.push(eda_model::ir::SymbolPathOverride { at: s.at, sheet_path: e.path.clone(), reference, unit: s.unit });
                }
            }
        }
    }

    /// The symbols of every screen that is shown more than once or keeps references for a placement, before a command: (reference, unit) and where each is
    /// (see [`Board::tidy_shared_screens`]).
    pub(crate) fn shared_screens_before(&self) -> SharedScreens {
        let counts = eda_model::sheet_list::placement_counts(&self.design);
        let mut out = SharedScreens::default();
        for (file, sec) in self.design.sheet_contents.iter().flatten() {
            if counts.get(file).copied().unwrap_or(0) > 1 || !sec.instance_overrides.is_empty() {
                out.0.insert(file.clone(), sec.symbols.iter().map(|s| ((s.id.clone(), s.unit), s.at)).collect());
            }
        }
        out
    }

    /// After a command, the references each placement gives a shared screen's symbols (`SchematicSection::instance_overrides`, which name a symbol by where
    /// it is and its unit) kept in step with what the command did to the symbols, as KiCad keeps them on the symbol itself (`SCH_SYMBOL::m_instances`):
    ///
    /// * a symbol that moved (`Move`, `Drag`, `Rotate`, ...) takes its references with it;
    /// * a symbol that was deleted takes them away, and so does a placement that is gone (`PruneOrphanedSymbolInstances`);
    /// * a symbol new on a screen shown more than once gets a reference of its own on each sheet path past the first (KiCad gives it an instance for every
    ///   path, `SCH_SCREEN::Append` / `updatePastedSymbol`, and annotates each one: `ReannotateDuplicates`), numbered against every reference of the design.
    ///
    /// A symbol the command did not add keeps whatever references it had, so an imported design is not renumbered by an unrelated edit.
    pub(crate) fn tidy_shared_screens(&mut self, before: SharedScreens, numbering: eda_model::sch_clipboard::PasteNumbering) {
        if before.0.is_empty() && !self.design.sheet_contents.iter().flatten().any(|(_, s)| !s.instance_overrides.is_empty()) {
            let counts = eda_model::sheet_list::placement_counts(&self.design);
            if counts.values().all(|&n| n <= 1) {
                return;
            }
        }
        let mut new_symbols: BTreeMap<String, BTreeSet<(String, u32)>> = BTreeMap::new();
        for (file, sec) in self.design.sheet_contents.iter_mut().flatten() {
            let was: &[((String, u32), Point)] = before.0.get(file).map(Vec::as_slice).unwrap_or(&[]);
            let now: BTreeMap<(String, u32), Point> = sec.symbols.iter().map(|s| ((s.id.clone(), s.unit), s.at)).collect();
            let here: BTreeSet<(Point, u32)> = sec.symbols.iter().map(|s| (s.at, s.unit)).collect();
            if before.0.contains_key(file) {
                let had: BTreeSet<&(String, u32)> = was.iter().map(|(k, _)| k).collect();
                new_symbols.insert(file.clone(), now.keys().filter(|k| !had.contains(k)).cloned().collect());
            }
            sec.instance_overrides.retain_mut(|o| {
                if here.contains(&(o.at, o.unit)) {
                    return true;
                }
                // the symbol that was there: where it is now, or gone
                let Some(((id, unit), _)) = was.iter().find(|(k, at)| *at == o.at && k.1 == o.unit) else { return false };
                match now.get(&(id.clone(), *unit)) {
                    Some(at) => {
                        o.at = *at;
                        true
                    }
                    None => false,
                }
            });
        }
        eda_model::sheet_list::prune_orphaned_overrides(&mut self.design);

        // the new symbols of a screen shown more than once, on each sheet path past the first
        let entries = eda_model::sheet_list::walk(&self.design);
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for e in entries.iter().skip(1) {
            let Some(fresh) = new_symbols.get(&e.file).filter(|n| !n.is_empty()) else { continue };
            if seen.insert(e.file.clone()) {
                continue; // the first placement: the symbol's own reference
            }
            let Some(screen) = self.design.sheet_contents.as_ref().and_then(|c| c.get(&e.file)).cloned() else { continue };
            let symbols: Vec<&eda_model::ir::SymbolInstance> = screen
                .symbols
                .iter()
                .filter(|s| fresh.contains(&(s.id.clone(), s.unit)) && eda_model::sheet_list::override_under(&screen, s, &e.path).is_none())
                .collect();
            if symbols.is_empty() {
                continue;
            }
            let pasted: Vec<PastedRef> = symbols
                .iter()
                .map(|s| PastedRef { reference: s.id.clone(), unit: s.unit, unit_count: self.design_symbol(&s.lib_id).map_or(1, |l| l.unit_count.max(1)), lib_id: s.lib_id.clone(), value: s.value.clone(), at: (s.at.x, s.at.y), power: false })
                .collect();
            // by the project's method and start number and this path's page (`PlaceSymbol`'s `annotate` lambda and `Paste` loop over the screen's paths,
            // `ReannotateByOptions` gives each `SetSheetNumber( path->GetPageNumberAsInt() )`, r4 D4-M1)
            let numbered = annotate_paste(&self.used_references(), &pasted, PasteMode::Unique, numbering, e.page_number_as_int());
            let Some(sec) = self.design.sheet_contents.as_mut().and_then(|c| c.get_mut(&e.file)) else { continue };
            for (s, reference) in symbols.iter().zip(numbered) {
                if reference != s.id {
                    sec.instance_overrides.push(eda_model::ir::SymbolPathOverride { at: s.at, sheet_path: e.path.clone(), reference, unit: s.unit });
                }
            }
        }
    }

    /// `ReannotateDuplicates` for one screen that is coming in: its symbols (and power symbols, always numbered anew) numbered against `used`, which
    /// then holds them too.
    /// (**Differs**: the contents of a pasted sheet are numbered with KiCad's default annotation settings, not Schematic Setup's -- r2 D-C3 threads them
    /// through the paste of the sheet in view only.)
    fn number_screen(&self, sec: &mut SchematicSection, used: &mut Vec<UsedRef>, mode: PasteMode, units_of: &dyn Fn(&Self, &str) -> u32) {
        let mut pasted: Vec<PastedRef> = Vec::new();
        for s in &sec.symbols {
            pasted.push(PastedRef { reference: s.id.clone(), unit: s.unit, unit_count: units_of(self, &s.lib_id), lib_id: s.lib_id.clone(), value: s.value.clone(), at: (s.at.x, s.at.y), power: false });
        }
        for p in &sec.power_symbols {
            pasted.push(PastedRef { reference: p.id.clone(), unit: 1, unit_count: 1, lib_id: p.lib_id.clone(), value: p.net.clone(), at: (p.at.x, p.at.y), power: true });
        }
        let numbered = annotate_paste(used, &pasted, mode, Default::default(), 1);
        let n_symbols = sec.symbols.len();
        let mut renames: Vec<SecRename> = Vec::new();
        for (i, new) in numbered.iter().enumerate() {
            if i < n_symbols {
                let s = &sec.symbols[i];
                if s.id != *new {
                    renames.push(SecRename { item: Item::Symbol(i), old: s.id.clone(), unit: s.unit, new: new.clone() });
                }
            } else if let Some(p) = sec.power_symbols.get(i - n_symbols) {
                if p.id != *new {
                    renames.push(SecRename { item: Item::Power(i - n_symbols), old: p.id.clone(), unit: 1, new: new.clone() });
                }
            }
        }
        let pin_units = self.partial_pin_units(sec, &renames);
        rename_in_section(sec, &renames, &pin_units);
        for s in &sec.symbols {
            used.push(UsedRef { reference: s.id.clone(), unit: s.unit, lib_id: s.lib_id.clone(), value: s.value.clone() });
        }
        for p in &sec.power_symbols {
            used.push(UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() });
        }
    }

    /// `AnnotateSymbols( ANNOTATE_SELECTION, order, algo, recursive, start, reset = true )` over the screens a sheet has just brought in: every
    /// symbol of them is numbered afresh, against everything else in the design. Power symbols are numbered apart (a sheet from a file keeps
    /// its power symbols, but two of one id cannot stay).
    fn annotate_new_screens(&mut self, files: &[String], order: AnnotateOrder, options: &AnnotateOptions) {
        let scopes: Vec<ScreenScope> = files.iter().map(|f| ScreenScope { file: f.clone(), symbols: Take::All, power: false }).collect();
        let opts = AnnotateOptions { scope: Some(AnnotateScope::Selection), ..options.clone() };
        let (_, renames, _) = self.annotate_plan_in(&scopes, true, order, &opts);
        self.annotate_apply(&renames, false);
        // the power symbols: against every power symbol that is not on the new screens
        let mut used: Vec<UsedRef> = Vec::new();
        {
            let new: BTreeSet<&String> = files.iter().collect();
            let contents = self.design.sheet_contents.clone().unwrap_or_default();
            for (file, sec) in &contents {
                if new.contains(file) {
                    continue;
                }
                used.extend(sec.power_symbols.iter().map(|p| UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() }));
            }
            if let Some(root) = &self.design.schematic {
                used.extend(root.power_symbols.iter().map(|p| UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() }));
            }
        }
        for file in files {
            let Some(mut sec) = self.design.sheet_contents.as_mut().and_then(|c| c.remove(file)) else { continue };
            let power_only: Vec<(usize, PastedRef)> = sec.power_symbols.iter().enumerate().map(|(i, p)| (i, PastedRef { reference: p.id.clone(), unit: 1, unit_count: 1, lib_id: p.lib_id.clone(), value: p.net.clone(), at: (p.at.x, p.at.y), power: true })).collect();
            let pasted: Vec<PastedRef> = power_only.iter().map(|(_, p)| p.clone()).collect();
            let numbered = annotate_paste(&used, &pasted, PasteMode::Unique, Default::default(), 1);
            let renames: Vec<SecRename> = power_only.iter().zip(&numbered).filter(|((_, p), new)| p.reference != **new).map(|((i, p), new)| SecRename { item: Item::Power(*i), old: p.reference.clone(), unit: 1, new: new.clone() }).collect();
            rename_in_section(&mut sec, &renames, &BTreeMap::new());
            used.extend(sec.power_symbols.iter().map(|p| UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() }));
            self.design.sheet_contents.get_or_insert_with(BTreeMap::new).insert(file.clone(), sec);
        }
    }

    /// The sheet part of `Cmd::PasteSch`: each sheet symbol of the fragment, named and filed anew, with a copy of the screens it shows and its symbols
    /// numbered like any pasted symbols.
    pub(crate) fn paste_sheets(&mut self, fragment: &SchFragment, dx: Nm, dy: Nm, mode: PasteMode) -> Result<(), Vec<CheckResult>> {
        let mut sheets: Vec<&SheetInstance> = fragment.section.sheets.iter().collect();
        // `sortedLoadedItems`: by name, naturally
        sheets.sort_by(|a, b| str_num_cmp(&a.name, &b.name));
        let mut names: BTreeSet<String> = self.schematic_mut_or_create().sheets.iter().map(|s| s.name.clone()).collect();
        for sh in sheets {
            // `TestForRecursion`: the sheet's file, or that of a sheet inside it, is the screen the paste lands on (or one above it): KiCad drops the sheet
            // and says so. A verb cannot say and go on, so the paste is refused with KiCad's words.
            let mut tree: BTreeSet<String> = BTreeSet::new();
            let mut todo = vec![sh.file.clone()];
            while let Some(f) = todo.pop() {
                if tree.insert(f.clone()) {
                    if let Some(sc) = fragment.sheet_screens.get(&f).or_else(|| self.design.sheet_contents.as_ref().and_then(|c| c.get(&f))) {
                        todo.extend(sc.sheets.iter().map(|x| x.file.clone()));
                    }
                }
            }
            let above: BTreeSet<String> = match self.focus.as_deref() {
                Some(here) => eda_model::sheet_list::walk(&self.design).iter().filter(|e| e.file == here).flat_map(|e| (1..=e.path.len()).filter_map(|k| eda_model::sheet_list::placement_at(&self.design, &e.path[..k]).map(|p| p.file.clone()))).collect(),
                None => BTreeSet::new(),
            };
            if !tree.is_disjoint(&above) {
                return Err(vec![CheckResult::fail("ops_sheet_recursion", &sh.name, format!("The pasted sheet '{}'\nwas dropped because the destination already has the sheet or one of its subsheets as a parent.", sh.file))]);
            }
            let name = unique_sheet_name(&names, &sh.name);
            names.insert(name.clone());
            let fields = fragment.section.extras.sheet_fields.get(&sh.id).cloned();
            if !self.share_pasted_sheet(sh, (dx, dy), &name, mode, fields.clone())? {
                let inc = Incoming { placement: sh.clone(), screens: &fragment.sheet_screens, lib_symbols: &fragment.lib_symbols, fields };
                self.add_sheet_tree(inc, (dx, dy), &name, None, &Numbering::Paste(mode))?;
            }
            // the pins come with the sheet whole (`SCH_SHEET_PIN` is an `EDA_TEXT`): their text's size, bold and italic too
            let looks: Vec<Option<eda_model::sch_extras::LabelLook>> = sh.pins.iter().map(|p| fragment.section.extras.sheet_pin_looks.get(&p.id).copied()).collect();
            if looks.iter().any(Option::is_some) {
                let sch = self.schematic_mut()?;
                sch.assign_missing_ids();
                let new_pins: Vec<String> = sch.sheets.iter().rev().find(|s| s.name == name).map(|s| s.pins.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
                for (id, look) in new_pins.into_iter().zip(looks) {
                    if let Some(look) = look {
                        sch.extras.sheet_pin_looks.insert(id, look);
                    }
                }
            }
        }
        Ok(())
    }

    /// The pasted sheet `sh` as a second placement of the screen the design already shows under its file (see the module doc): `false`, and nothing done,
    /// when the design shows no such screen.
    ///
    /// `SCH_EDITOR_CONTROL::Paste`: the sheet gets a new KIID and its pins new ones, and is added on every sheet path that shows the screen pasted onto
    /// (`FindAllSheetsForScreen( pasteRoot.LastScreen() )`, in page order). `updatePastedSheet` walks the screen and every screen below it, and
    /// `updatePastedSymbol` gives each of their symbols an instance for each new sheet path, its reference the one the clipboard carried (the instance
    /// whose path ends in the clipboard's path: the copied sheet, then the sheets below it) -- kept by Paste Special's "Keep existing annotations",
    /// numbered by `ReannotateDuplicates` against every reference of the schematic otherwise, path by path, each path's references then counting as
    /// used. The power symbols keep theirs.
    fn share_pasted_sheet(&mut self, sh: &SheetInstance, shift: (Nm, Nm), name: &str, mode: PasteMode, fields: Option<Vec<eda_model::sch_extras::LabelField>>) -> Result<bool, Vec<CheckResult>> {
        // a screen pasted into itself would place itself (`TestForRecursion`): the copy does not
        if !eda_model::sheet_list::reachable_files(&self.design).contains(&sh.file) || self.focus.as_deref() == Some(sh.file.as_str()) {
            return Ok(false);
        }
        if !self.design.sheet_contents.as_ref().is_some_and(|c| c.contains_key(&sh.file)) {
            return Ok(false);
        }
        // what an earlier placement under the id this one will get left behind is not this one's (`PruneOrphanedSymbolInstances`)
        eda_model::sheet_list::prune_orphaned_overrides(&mut self.design);
        let before = eda_model::sheet_list::walk(&self.design);
        // the sheet paths that show the screen pasted onto (`sheetPathsForScreen`, `SortByPageNumbers`)
        let destinations: Vec<Vec<String>> = match self.focus.as_deref() {
            None => vec![Vec::new()],
            Some(here) => eda_model::sheet_list::sort_by_page_numbers(&before).into_iter().filter(|e| e.file == here).map(|e| e.path).collect(),
        };
        // the copied sheet's path, whose references the clipboard carried (`setPastedSymbolInstances`; the clipboard names the copied sheet by its
        // uuid, which is the placement's id or made from it: `eda_kicad::sheet_uuid`)
        let copied_from: Option<Vec<String>> = before
            .iter()
            .skip(1)
            .find(|e| e.file == sh.file && !sh.id.is_empty() && eda_model::sheet_list::placement_at(&self.design, &e.path).is_some_and(|p| p.id == sh.id || eda_kicad::sheet_uuid(p) == sh.id))
            .map(|e| e.path.clone());
        let units_of = |board: &Self, lib_id: &str| -> u32 { board.design_symbol(lib_id).map_or(1, |l| l.unit_count.max(1)) };
        let mut used = self.used_references();

        let mut placement = sh.clone();
        placement.id.clear();
        placement.name = name.to_string();
        placement.page.clear();
        placement.at = Point { x: placement.at.x + shift.0, y: placement.at.y + shift.1 };
        for p in placement.pins.iter_mut() {
            p.id.clear();
            p.at = Point { x: p.at.x + shift.0, y: p.at.y + shift.1 };
        }
        let id = {
            let sch = self.schematic_mut_or_create();
            sch.sheets.push(placement);
            sch.assign_missing_ids();
            let id = sch.sheets.last().map(|s| s.id.clone()).unwrap_or_default();
            // the sheet's user fields come with it (r3 B3-M2)
            if let Some(fields) = fields {
                sch.extras.sheet_fields.insert(id.clone(), fields);
            }
            id
        };
        // every sheet path the new sheet adds: the sheet on each destination, and every sheet below it there (`updatePastedSheet`'s recursion)
        let after = eda_model::sheet_list::walk(&self.design);
        for dest in &destinations {
            let mut top = dest.clone();
            top.push(id.clone());
            for e in after.iter().filter(|e| e.path.starts_with(&top)) {
                let Some(screen) = self.design.sheet_contents.as_ref().and_then(|c| c.get(&e.file)).cloned() else { continue };
                // `updatePastedSymbol`: the instance whose path ends in the clipboard path, the copied sheet and the sheets below it
                let source: Option<Vec<String>> = copied_from.as_ref().map(|from| from.iter().chain(&e.path[top.len()..]).cloned().collect());
                let pasted: Vec<PastedRef> = screen
                    .symbols
                    .iter()
                    .map(|s| PastedRef { reference: source.as_ref().map_or_else(|| s.id.clone(), |p| eda_model::sheet_list::reference_under(&screen, s, p)), unit: s.unit, unit_count: units_of(self, &s.lib_id), lib_id: s.lib_id.clone(), value: s.value.clone(), at: (s.at.x, s.at.y), power: false })
                    .collect();
                let numbered = annotate_paste(&used, &pasted, mode, Default::default(), 1);
                let Some(sec) = self.design.sheet_contents.as_mut().and_then(|c| c.get_mut(&e.file)) else { continue };
                for (s, reference) in screen.symbols.iter().zip(numbered) {
                    used.push(UsedRef { reference: reference.clone(), unit: s.unit, lib_id: s.lib_id.clone(), value: s.value.clone() });
                    if reference != s.id {
                        sec.instance_overrides.push(eda_model::ir::SymbolPathOverride { at: s.at, sheet_path: e.path.clone(), reference, unit: s.unit });
                    }
                }
            }
        }
        Ok(true)
    }

    /// `Cmd::ImportSheet`: see the module doc.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn import_sheet(&mut self, content: &SheetFile, as_sheet: bool, name: &str, file: &str, at: Point, size: (Nm, Nm), keep_annotations: bool, order: AnnotateOrder, options: &AnnotateOptions, how: &crate::design_blocks::Placement<'_>) -> Result<(), Vec<CheckResult>> {
        if !how.block.is_empty() {
            if let Err(why) = eda_model::groups::check_design_block_id(how.block) {
                return Err(vec![CheckResult::fail("ops_bad_design_block", how.block, why)]);
            }
        }
        let name = name.trim();
        if let Some(e) = self.check_sheet_file(content, name, keep_annotations).into_iter().find(|f| f.severity == "error") {
            return Err(vec![CheckResult::fail("ops_sheet_file", name, e.text)]);
        }
        if as_sheet {
            if name.is_empty() {
                return Err(vec![CheckResult::fail("ops_bad_sheet", "sheet", "a sheet needs a name")]);
            }
            let file = file.trim();
            if file.contains('/') || file.contains('\\') {
                return Err(vec![CheckResult::fail("ops_bad_sheet", file, "a sheet file is a bare file name (no folders)")]);
            }
            if size.0 <= Nm::ZERO || size.1 <= Nm::ZERO {
                return Err(vec![CheckResult::fail("ops_bad_sheet", name, "a sheet needs a positive size")]);
            }
            if self.schematic_mut_or_create().sheets.iter().any(|s| s.name == name) {
                return Err(vec![CheckResult::fail("ops_sheet_name_taken", name, format!("a sheet named '{name}' already exists on this sheet"))]);
            }
            let mut screens = content.screens.clone();
            screens.insert(OWN.to_string(), content.root.clone());
            let placement = SheetInstance { id: String::new(), name: name.to_string(), file: OWN.to_string(), at, size, pins: vec![], page: String::new() };
            let numbering = if keep_annotations { Numbering::Paste(PasteMode::Keep) } else { Numbering::Annotate { order, options: options.clone() } };
            let hint = (!file.is_empty()).then_some(file);
            self.add_sheet_tree(Incoming { placement, screens: &screens, lib_symbols: &content.lib_symbols, fields: None }, (Nm::ZERO, Nm::ZERO), name, hint, &numbering)?;
            // `DrawSheet`: `if( isDrawSheetFromDesignBlock && place_as_group )` the sheet alone goes in a group named and linked as the block
            if !how.block.is_empty() {
                let new_sheet = self.schematic_mut()?.sheets.last().map(|s| s.id.clone());
                if let Some(id) = new_sheet {
                    self.group_placed(&[id], how)?;
                }
            }
            self.take_bus_aliases(content);
            return Ok(());
        }
        // the contents of the file go on the sheet in view, as a paste of the file's screen carried to `at`
        let fragment = SchFragment { section: content.root.clone(), lib_symbols: content.lib_symbols.clone(), notes: content.notes.clone(), sheet_screens: content.screens.clone() };
        if fragment.is_empty() {
            return Err(vec![CheckResult::fail("ops_empty_paste", name, "the file holds nothing to put on the sheet")]);
        }
        let anchor = eda_model::sch_clipboard::top_left_anchor(&fragment.section).unwrap_or_default();
        let (dx, dy) = (at.x - anchor.x, at.y - anchor.y);
        let units_here = |b: &Self| -> BTreeSet<(String, u32)> { b.schematic().map(|s| s.symbols.iter().map(|x| (x.id.clone(), x.unit)).collect()).unwrap_or_default() };
        let before = units_here(self);
        let top = self.paste_sch(&fragment, dx, dy, if keep_annotations { PasteMode::Keep } else { PasteMode::Unique }, Default::default())?;
        // `placeSheetContents`: `if( placeAsGroup )` a group of what came in (before the numbering, which renames the members it lists)
        self.group_placed(&top, how)?;
        if !keep_annotations {
            // `placeSheetContents`: `AnnotateSymbols( ANNOTATE_SELECTION, order, algo, recursive, start, reset = true, ... SYMBOL_FILTER_NON_POWER )` over the items that came in
            let new_refs: Vec<String> = units_here(self).difference(&before).map(|(id, _)| id.clone()).collect::<BTreeSet<_>>().into_iter().collect();
            if !new_refs.is_empty() {
                let opts = AnnotateOptions { scope: Some(AnnotateScope::Selection), ..options.clone() };
                let (_, renames, _) = self.annotate_plan(true, order, Some(&new_refs), &opts);
                self.annotate_apply(&renames, false);
            }
        }
        self.take_bus_aliases(content);
        Ok(())
    }

    /// The bus aliases of a sheet file taken in: into the design's list (`parseBusAlias` adds a file's to the schematic it loads into), one of a name the
    /// design has not got yet. **Differs**: so do the aliases of the file's own project (`.kicad_pro`), which `LoadSheetFromFile` does not read, so that
    /// the sheet's buses keep their members.
    fn take_bus_aliases(&mut self, content: &SheetFile) {
        for alias in &content.bus_aliases {
            if !self.design.bus_aliases.iter().any(|a| a.name == alias.name) {
                self.design.bus_aliases.push(alias.clone());
            }
        }
    }
}

/// A KiCad file keeps a symbol's reference on every sheet path (`SCH_SYMBOL::m_instances`, one `(path .. (reference ..))` each, the first placement's too); this
/// design keeps the first placement's as the symbol's own id (`SymbolInstance::id`, which Annotate and Edit Reference rename) and only the later paths' as
/// `SchematicSection::instance_overrides`. So a screen read whole (`Cmd::ReplaceSchematic`) gives each symbol the reference of the first sheet path that shows
/// its screen (in walk order) and drops that path's override, the way a screen this design drew has it. A symbol whose units the first path names
/// differently, or whose new reference another symbol of the design already holds, keeps its id and its overrides.
pub(crate) fn fold_first_path_references(design: &mut eda_model::ir::Design) {
    let walk = eda_model::sheet_list::walk(design);
    let mut first: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in walk.iter().skip(1) {
        first.entry(e.file.clone()).or_insert_with(|| e.path.clone());
    }
    let mut held: BTreeSet<String> = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).flat_map(|s| s.symbols.iter().map(|x| x.id.clone())).collect();
    for (file, path) in first {
        let Some(sec) = design.sheet_contents.as_mut().and_then(|c| c.get_mut(&file)) else { continue };
        let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for s in &sec.symbols {
            let reference = eda_model::sheet_list::reference_under(sec, s, &path);
            names.entry(s.id.clone()).or_default().insert(reference);
        }
        let mut renames: Vec<SecRename> = Vec::new();
        for (old, new) in names {
            let [new] = new.into_iter().collect::<Vec<_>>().try_into().unwrap_or_else(|_| [old.clone()]);
            // (an unannotated reference, `R?`, is no name of its own: several symbols have it, each keeps its placeholder)
            if new == old || new.is_empty() || new.contains('?') || held.contains(&new) {
                continue;
            }
            held.insert(new.clone());
            for (i, s) in sec.symbols.iter().enumerate().filter(|(_, s)| s.id == old) {
                renames.push(SecRename { item: Item::Symbol(i), old: old.clone(), unit: s.unit, new: new.clone() });
            }
        }
        if !renames.is_empty() {
            rename_in_section(sec, &renames, &BTreeMap::new());
        }
        let own: BTreeSet<(Point, u32, String)> = sec.symbols.iter().map(|s| (s.at, s.unit, s.id.clone())).collect();
        sec.instance_overrides.retain(|o| !(o.sheet_path == path && own.contains(&(o.at, o.unit, o.reference.clone()))));
    }
}

/// A screen read from a file places a symbol by its library origin (KiCad's frame, `imported_from_kicad`); the sheets of this design place one by the corner
/// of its box (what the `.kicad_sch` writer bakes the symbol against): each symbol is moved there, so that its pins stay where the file has them, and the
/// screen is in the design's frame after. `engine_of` is the engine's symbol of each `lib_id` the screen names (`LibMerge::engine_of`), by the name the
/// screen gives it. A screen already in the design's frame is left alone.
pub(crate) fn place_by_box_corner(sec: &mut SchematicSection, engine_of: &BTreeMap<String, eda_model::LibSymbol>) {
    if !sec.imported_from_kicad {
        return;
    }
    let body_styles = sec.extras.body_styles.clone();
    let mut moved: Vec<((Point, u32), Point)> = Vec::new();
    for s in sec.symbols.iter_mut() {
        let Some(engine) = engine_of.get(&s.lib_id) else { continue };
        let style = body_styles.get(&eda_model::ir::field_key(&s.id, s.unit)).copied().unwrap_or(1);
        let drawn = if style > 1 && engine.has_alternate_body() { engine.in_style(style).into_owned() } else { engine.clone() };
        let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(&drawn, s.unit);
        let (ox, oy) = eda_model::sch_clipboard::place_offset_nm(s.rot as f64 / 1000.0, s.mirrored, s.mirror_y, (-x0, -y1));
        let was = s.at;
        s.at = Point { x: s.at.x - ox, y: s.at.y - oy };
        moved.push(((was, s.unit), s.at));
    }
    // The references each sheet path gives the symbols (`SymbolPathOverride`, which names its symbol by where it is and its unit) go with them: KiCad keeps
    // them on the symbol itself (`SCH_SYMBOL::m_instances`, `GetRef( &aSheet )`).
    for o in sec.instance_overrides.iter_mut() {
        if let Some((_, now)) = moved.iter().find(|((at, unit), _)| *at == o.at && *unit == o.unit) {
            o.at = *now;
        }
    }
    sec.imported_from_kicad = false;
}
