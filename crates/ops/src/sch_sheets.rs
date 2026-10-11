//! Whole hierarchical sheets coming into the design: Import Sheet / Draw Sheet from File (`Cmd::ImportSheet`) and the sheet part of Paste.
//!
//! Ported from `SCH_EDIT_FRAME::LoadSheetFromFile` and `ChangeSheetFile` (`eeschema/sheet.cpp`), `SCH_DRAWING_TOOLS::ImportSheet` /
//! `DrawSheet` (`eeschema/tools/sch_drawing_tools.cpp`) and the sheet branch of `SCH_EDITOR_CONTROL::Paste`
//! (`eeschema/tools/sch_editor_control.cpp`):
//!
//! * A sheet comes in as a **copy**: its screen goes into the design under a file of its own, and so does the screen of every sheet inside
//!   it. KiCad would let a pasted sheet share the screen of the sheet it was copied from; a design here names a part by its reference
//!   (the model, the PCB and the net list know it by that name), so one screen cannot be placed twice -- the copy is a new sheet with
//!   new references, which is what a user gets from the shared screen after annotating it anyway.
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
use eda_model::ir::{LibrarySymbol, Point, SchematicSection, SheetInstance, Um};
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
        // `checkForNoFullyDefinedLibIds`
        let loose: BTreeSet<&str> = content
            .all_screens()
            .flat_map(|(_, s)| s.symbols.iter().map(|x| x.lib_id.as_str()).chain(s.power_symbols.iter().map(|p| p.lib_id.as_str())))
            .filter(|id| !id.is_empty() && !eda_model::is_synthetic_lib_id(id) && !id.split_once(':').is_some_and(|(l, n)| !l.is_empty() && !n.is_empty()))
            .collect();
        if !loose.is_empty() {
            let shown: Vec<&str> = loose.iter().copied().take(4).collect();
            out.push(SheetFileFinding {
                severity: "error",
                text: format!(
                    "The schematic '{label}' has not had its symbol library links remapped to the symbol library table ({}). The project this schematic belongs to must first be remapped before it can be imported into the current project.",
                    shown.join(", ")
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
    pub(crate) fn add_sheet_tree(&mut self, inc: Incoming<'_>, shift: (Um, Um), name: &str, file_hint: Option<&str>, numbering: &Numbering) -> Result<Vec<String>, Vec<CheckResult>> {
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
        for orig in &order {
            let mut sec = screen_of(orig).unwrap_or_else(crate::empty_schematic_section);
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
            // A screen read from a file places a symbol by its library origin (KiCad's frame); the sheets of this design place one by the corner of its box
            // (what the writer bakes the symbol against): the symbol is moved there, so that its pins stay where the file has them.
            if sec.imported_from_kicad {
                let body_styles = sec.extras.body_styles.clone();
                for s in sec.symbols.iter_mut() {
                    let Some(engine) = engine_of.get(&s.lib_id) else { continue };
                    let style = body_styles.get(&eda_model::ir::field_key(&s.id, s.unit)).copied().unwrap_or(1);
                    let drawn = if style > 1 && engine.has_alternate_body() { engine.in_style(style).into_owned() } else { engine.clone() };
                    let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(&drawn, s.unit);
                    let (ox, oy) = eda_model::sch_clipboard::place_offset_um(s.rot as f64 / 1000.0, s.mirrored, s.mirror_y, (-x0, -y1));
                    s.at = Point { x: s.at.x - ox, y: s.at.y - oy };
                }
                sec.imported_from_kicad = false;
            }
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
                contents.insert(file, sec);
            }
        }
        {
            let sch = self.schematic_mut_or_create();
            sch.sheets.push(placement);
            sch.assign_missing_ids();
        }

        // an import's automatic annotation, now that the sheets are in
        if let Numbering::Annotate { order, options } = numbering {
            self.annotate_new_screens(&files, *order, options);
        }
        Ok(files)
    }

    /// `ReannotateDuplicates` for one screen that is coming in: its symbols (and power symbols, always numbered anew) numbered against `used`, which
    /// then holds them too.
    fn number_screen(&self, sec: &mut SchematicSection, used: &mut Vec<UsedRef>, mode: PasteMode, units_of: &dyn Fn(&Self, &str) -> u32) {
        let mut pasted: Vec<PastedRef> = Vec::new();
        for s in &sec.symbols {
            pasted.push(PastedRef { reference: s.id.clone(), unit: s.unit, unit_count: units_of(self, &s.lib_id), lib_id: s.lib_id.clone(), value: s.value.clone(), at: (s.at.x, s.at.y), power: false });
        }
        for p in &sec.power_symbols {
            pasted.push(PastedRef { reference: p.id.clone(), unit: 1, unit_count: 1, lib_id: p.lib_id.clone(), value: p.net.clone(), at: (p.at.x, p.at.y), power: true });
        }
        let numbered = annotate_paste(used, &pasted, mode);
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
            let numbered = annotate_paste(&used, &pasted, PasteMode::Unique);
            let renames: Vec<SecRename> = power_only.iter().zip(&numbered).filter(|((_, p), new)| p.reference != **new).map(|((i, p), new)| SecRename { item: Item::Power(*i), old: p.reference.clone(), unit: 1, new: new.clone() }).collect();
            rename_in_section(&mut sec, &renames, &BTreeMap::new());
            used.extend(sec.power_symbols.iter().map(|p| UsedRef { reference: p.id.clone(), unit: 1, lib_id: p.lib_id.clone(), value: p.net.clone() }));
            self.design.sheet_contents.get_or_insert_with(BTreeMap::new).insert(file.clone(), sec);
        }
    }

    /// The sheet part of `Cmd::PasteSch`: each sheet symbol of the fragment, named and filed anew, with a copy of the screens it shows and its symbols
    /// numbered like any pasted symbols.
    pub(crate) fn paste_sheets(&mut self, fragment: &SchFragment, dx: Um, dy: Um, mode: PasteMode) -> Result<(), Vec<CheckResult>> {
        let mut sheets: Vec<&SheetInstance> = fragment.section.sheets.iter().collect();
        // `sortedLoadedItems`: by name, naturally
        sheets.sort_by(|a, b| str_num_cmp(&a.name, &b.name));
        let mut names: BTreeSet<String> = self.schematic_mut_or_create().sheets.iter().map(|s| s.name.clone()).collect();
        for sh in sheets {
            let name = unique_sheet_name(&names, &sh.name);
            names.insert(name.clone());
            let inc = Incoming { placement: sh.clone(), screens: &fragment.sheet_screens, lib_symbols: &fragment.lib_symbols };
            self.add_sheet_tree(inc, (dx, dy), &name, None, &Numbering::Paste(mode))?;
        }
        Ok(())
    }

    /// `Cmd::ImportSheet`: see the module doc.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn import_sheet(&mut self, content: &SheetFile, as_sheet: bool, name: &str, file: &str, at: Point, size: (Um, Um), keep_annotations: bool, order: AnnotateOrder, options: &AnnotateOptions) -> Result<(), Vec<CheckResult>> {
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
            if size.0 <= 0 || size.1 <= 0 {
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
            self.add_sheet_tree(Incoming { placement, screens: &screens, lib_symbols: &content.lib_symbols }, (0, 0), name, hint, &numbering)?;
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
        self.paste_sch(&fragment, dx, dy, if keep_annotations { PasteMode::Keep } else { PasteMode::Unique })?;
        if !keep_annotations {
            // `placeSheetContents`: `AnnotateSymbols( ANNOTATE_SELECTION, order, algo, recursive, start, reset = true, ... SYMBOL_FILTER_NON_POWER )` over the items that came in
            let new_refs: Vec<String> = units_here(self).difference(&before).map(|(id, _)| id.clone()).collect::<BTreeSet<_>>().into_iter().collect();
            if !new_refs.is_empty() {
                let opts = AnnotateOptions { scope: Some(AnnotateScope::Selection), ..options.clone() };
                let (_, renames, _) = self.annotate_plan(true, order, Some(&new_refs), &opts);
                self.annotate_apply(&renames, false);
            }
        }
        Ok(())
    }
}
