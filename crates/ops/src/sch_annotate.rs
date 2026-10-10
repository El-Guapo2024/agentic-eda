//! Annotate Schematic (`Cmd::Annotate`): `DIALOG_ANNOTATE` and `SCH_EDIT_FRAME::AnnotateSymbols` over the whole hierarchy.
//!
//! Ported from `eeschema/dialogs/dialog_annotate.cpp` and `eeschema/annotate.cpp` (scope, order, numbering, reset or keep, the reports
//! and the final `CheckAnnotate`), with the numbering itself in [`eda_model::annotate`] (`sch_reference_list.cpp`).
//!
//! * Scope: the entire schematic (`ANNOTATE_ALL`), the current sheet (`ANNOTATE_CURRENT_SHEET`, with the sheets under it when
//!   `recursive`) or the selection (`ANNOTATE_SELECTION`: the selected symbols, and the sheets under the selected sheet symbols when
//!   `recursive`). The current sheet is the screen the verbs act on (`Cmd::OnSheet`).
//! * A reference is one part for the whole design, so what is not in the scope is taken into account (`additionalRefs`) and keeps
//!   its number. The sheet number the "sheet number x 100" algorithms count by is the sheet's page (`GetPageNumberAsInt`).
//! * Symbols of the sheets in scope are renamed together with everything that is keyed by the reference: the pins of its wires, power
//!   symbols and no-connect flags, its user fields, where its fields are drawn, its body style, its lock, and the net list's pins.
//!
//! Not done here: a stand-alone "Clear Annotation" -- a design cannot hold two symbols named `R?` (a part is its reference), so
//! "Clear existing annotations" numbers again at once -- and the PCB: a footprint on the board keeps the reference it has (the same
//! limit as `Cmd::RenameSymbol`).

use eda_model::annotate::{self, Algo, Entry, Problem, SortOrder};
use eda_model::ir::{field_key, SchematicSection};
use eda_model::sheet_list::{self, SheetEntry};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::{AnnotateOrder, Board};

/// `ANNOTATE_SCOPE_T`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotateScope {
    /// "Entire schematic" (`ANNOTATE_ALL`).
    All,
    /// "Current sheet only" (`ANNOTATE_CURRENT_SHEET`); the sheet in view -- and, with `recursive`, the sheets under it.
    #[default]
    CurrentSheet,
    /// "Selection only" (`ANNOTATE_SELECTION`).
    Selection,
}

/// The rest of the dialog: the options `AnnotateSymbols` takes besides the scope's selection, the order and the reset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotateOptions {
    /// Absent: the selection when `ids` are given, else the current sheet -- what `Cmd::Annotate` meant before it had a scope.
    #[serde(default)]
    pub scope: Option<AnnotateScope>,
    /// "Use first free number after" (the default) or "First free number after sheet number x 100 / x 1000".
    #[serde(default)]
    pub algo: Algo,
    /// The number the first-free search starts after (`m_AnnotateStartNum`).
    #[serde(default)]
    pub start_number: i64,
    /// "Recursive": a scope of one sheet takes the sheets under it too.
    #[serde(default)]
    pub recursive: bool,
    /// "Regroup units": with a reset, the units of multi-unit symbols are numbered by where they are placed, not kept with the reference they had.
    #[serde(default)]
    pub regroup_units: bool,
    /// The sheet symbols selected on the current sheet (`Selection` scope with `recursive`): ids of `SheetInstance`s of that screen.
    #[serde(default)]
    pub sheets: Vec<String>,
}

/// One line of the dialog's message window (`RPT_SEVERITY`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnotateMessage {
    /// `action` (what was done), `error` or `warning`.
    pub severity: &'static str,
    pub text: String,
}

/// What an annotation did, or would do: the messages the dialog shows and the findings of the final check.
#[derive(Debug, Clone, Serialize)]
pub struct AnnotateReport {
    pub messages: Vec<AnnotateMessage>,
    /// How many references changed.
    pub changed: usize,
    /// The findings of `CheckAnnotate` on the result: not annotated, duplicate, different values, units past the library symbol's last.
    pub problems: Vec<Problem>,
}

/// Where an entry lives: its screen (`""` the root) and which item of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Symbol(usize),
    Power(usize),
}

#[derive(Debug, Clone)]
struct Slot {
    file: String,
    item: Item,
}

/// Which items of a screen the scope takes.
#[derive(Debug, Clone)]
enum Take {
    All,
    Ids(BTreeSet<String>),
}

#[derive(Debug, Clone)]
struct ScreenScope {
    file: String,
    symbols: Take,
    power: bool,
}

/// One reference to change.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rename {
    file: String,
    item: Item,
    old: String,
    unit: u32,
    new: String,
}

fn lib_item_name(lib_id: &str) -> &str {
    lib_id.rsplit(':').next().unwrap_or(lib_id)
}

impl<'a> Board<'a> {
    fn screen_of_file(&self, file: &str) -> Option<&SchematicSection> {
        if file.is_empty() {
            self.design.schematic.as_ref()
        } else {
            self.design.sheet_contents.as_ref().and_then(|c| c.get(file))
        }
    }

    fn screen_of_file_mut(&mut self, file: &str) -> Option<&mut SchematicSection> {
        if file.is_empty() {
            self.design.schematic.as_mut()
        } else {
            self.design.sheet_contents.as_mut().and_then(|c| c.get_mut(file))
        }
    }

    /// The screens the scope takes, and what of each.
    fn annotate_scopes(&self, scope: AnnotateScope, ids: Option<&[String]>, opts: &AnnotateOptions) -> Vec<ScreenScope> {
        let walk = sheet_list::walk(&self.design);
        let current = self.focus.clone().unwrap_or_default();
        let mut out: Vec<ScreenScope> = Vec::new();
        let add = |out: &mut Vec<ScreenScope>, file: &str, symbols: Take, power: bool| {
            if let Some(have) = out.iter_mut().find(|s| s.file == file) {
                have.power |= power;
                if matches!(symbols, Take::All) {
                    have.symbols = Take::All;
                }
            } else {
                out.push(ScreenScope { file: file.to_string(), symbols, power });
            }
        };
        let current_path: Option<Vec<String>> = walk.iter().find(|e| e.file == current).map(|e| e.path.clone());
        match scope {
            AnnotateScope::All => {
                for e in &walk {
                    add(&mut out, &e.file, Take::All, true);
                }
            }
            AnnotateScope::CurrentSheet => {
                add(&mut out, &current, Take::All, true);
                if let (true, Some(path)) = (opts.recursive, &current_path) {
                    for e in sheet_list::within(&walk, path, false) {
                        add(&mut out, &e.file, Take::All, false);
                    }
                }
            }
            AnnotateScope::Selection => {
                add(&mut out, &current, Take::Ids(ids.unwrap_or_default().iter().cloned().collect()), false);
                if let (true, Some(path)) = (opts.recursive, &current_path) {
                    for sheet in &opts.sheets {
                        let mut under = path.clone();
                        under.push(sheet.clone());
                        for e in sheet_list::within(&walk, &under, true) {
                            add(&mut out, &e.file, Take::All, false);
                        }
                    }
                }
            }
        }
        out
    }

    /// Every symbol and power symbol of the design as an annotation entry, with the entries the scope takes split off from the rest
    /// (`SCH_REFERENCE_LIST`: `references` and `additionalRefs`).
    fn annotation_entries(&self, scopes: &[ScreenScope]) -> (Vec<Slot>, Vec<Entry>, Vec<Entry>) {
        let walk = sheet_list::walk(&self.design);
        let sheet_num = |file: &str| walk.iter().find(|e| e.file == file).map_or(0, SheetEntry::page_number_as_int);
        let mut files: Vec<String> = vec![String::new()];
        files.extend(self.design.sheet_contents.iter().flat_map(|c| c.keys().cloned()));
        let mut slots: Vec<Slot> = Vec::new();
        let (mut mine, mut others): (Vec<Entry>, Vec<Entry>) = (Vec::new(), Vec::new());
        for file in files {
            let Some(sch) = self.screen_of_file(&file) else { continue };
            let scope = scopes.iter().find(|s| s.file == file);
            let num = sheet_num(&file);
            for (i, s) in sch.symbols.iter().enumerate() {
                let value = if s.value.is_empty() { self.model.part(&s.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() };
                let units = self
                    .model
                    .part(&s.id)
                    .and_then(|p| self.model.real_symbol_of(&s.lib_id, p))
                    .or_else(|| self.design_symbol(&s.lib_id))
                    .map_or(1, |l| l.unit_count.max(1));
                let key = slots.len();
                slots.push(Slot { file: file.clone(), item: Item::Symbol(i) });
                let entry = Entry::new(key, &s.id, s.unit, units, lib_item_name(&s.lib_id), &value, num, (s.at.x, s.at.y), false);
                let taken = scope.is_some_and(|sc| match &sc.symbols {
                    Take::All => true,
                    Take::Ids(ids) => ids.contains(&s.id),
                });
                (if taken { &mut mine } else { &mut others }).push(entry);
            }
            for (i, p) in sch.power_symbols.iter().enumerate() {
                let key = slots.len();
                slots.push(Slot { file: file.clone(), item: Item::Power(i) });
                let entry = Entry::new(key, &p.id, 1, 1, lib_item_name(&p.lib_id), &p.net, num, (p.at.x, p.at.y), true);
                (if scope.is_some_and(|sc| sc.power) { &mut mine } else { &mut others }).push(entry);
            }
        }
        (slots, mine, others)
    }

    /// `AnnotateSymbols` up to the point where the references are written: the references that change and the messages for them.
    fn annotate_plan(&self, reset: bool, order: AnnotateOrder, ids: Option<&[String]>, opts: &AnnotateOptions) -> (Vec<Slot>, Vec<Rename>, Vec<AnnotateMessage>) {
        let scope = opts.scope.unwrap_or(if ids.is_some() { AnnotateScope::Selection } else { AnnotateScope::CurrentSheet });
        let scopes = self.annotate_scopes(scope, ids, opts);
        let (slots, mut list, others) = self.annotation_entries(&scopes);
        let sort = match order {
            AnnotateOrder::YThenX => SortOrder::ByY,
            AnnotateOrder::XThenY => SortOrder::ByX,
        };

        // the units that stay together: the multi-unit symbols of the scope, by the reference they have (`GetMultiUnitSymbols`); never an
        // unassigned one. A reset that finds one reference holding a unit twice starts that part afresh.
        let mut locked: Vec<Vec<Entry>> = Vec::new();
        if !(opts.regroup_units && reset) {
            let mut by_ref: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
            for e in list.iter().filter(|e| e.unit_count > 1 && !e.original().is_empty() && !e.original().ends_with('?') && !e.power) {
                by_ref.entry(e.original().to_string()).or_default().push(e.clone());
            }
            locked = by_ref.into_values().filter(|g| !reset || g.iter().map(|e| e.unit).collect::<BTreeSet<_>>().len() == g.len()).collect();
        }

        // what each one was, for the messages (`mapExistingAnnotation`)
        let previous: BTreeMap<usize, String> = list.iter().filter(|e| !e.original().is_empty() && !e.original().ends_with('?')).map(|e| (e.key, e.original_with_unit())).collect();

        annotate::split_all(&mut list);
        if reset {
            annotate::remove_annotation(&mut list);
        }
        annotate::annotate_by_options(&mut list, sort, opts.algo, opts.start_number, &locked, &others);

        let mut renames: Vec<Rename> = Vec::new();
        let mut messages: Vec<AnnotateMessage> = Vec::new();
        for e in &list {
            let slot = &slots[e.key];
            let new_ref = e.reference();
            if new_ref != e.original() {
                let (id, unit) = (e.original().to_string(), e.unit);
                renames.push(Rename { file: slot.file.clone(), item: slot.item, old: id, unit, new: new_ref });
            }
            // the words of the report: the value stands for the symbol (`symbol->GetValue( true, sheet, false )`)
            let shown = self.annotated_value(slot);
            let multi = e.unit_count > 1;
            let new_full = e.with_unit();
            let text = match previous.get(&e.key) {
                Some(prev) => {
                    if *prev == new_full {
                        continue;
                    }
                    if multi {
                        format!("Updated {shown} (unit {}) from {prev} to {new_full}.", annotate::unit_letters(e.unit))
                    } else {
                        format!("Updated {shown} from {prev} to {new_full}.")
                    }
                }
                None => {
                    if multi {
                        format!("Annotated {shown} (unit {}) as {new_full}.", annotate::unit_letters(e.unit))
                    } else {
                        format!("Annotated {shown} as {new_full}.")
                    }
                }
            };
            messages.push(AnnotateMessage { severity: "action", text });
        }
        (slots, renames, messages)
    }

    /// `symbol->GetValue( true, sheet, false )`: the Value field the report names a symbol by.
    fn annotated_value(&self, slot: &Slot) -> String {
        let Some(sch) = self.screen_of_file(&slot.file) else { return String::new() };
        match slot.item {
            Item::Symbol(i) => sch.symbols.get(i).map(|s| if s.value.is_empty() { self.model.part(&s.id).and_then(|p| p.value.clone()).unwrap_or_default() } else { s.value.clone() }).unwrap_or_default(),
            Item::Power(i) => sch.power_symbols.get(i).map(|p| p.net.clone()).unwrap_or_default(),
        }
    }

    /// Write the new references: the symbols, and everything keyed by the old ones, on every screen at once (a reset can swap two
    /// references, so the names are taken from the mapping, never one rename after another).
    fn annotate_apply(&mut self, renames: &[Rename]) {
        if renames.is_empty() {
            return;
        }
        // the net list follows: its pins are "REF.PIN"
        let uniform = |file: &str, old: &str, new_of: &BTreeMap<(String, String, u32), String>| -> Option<String> {
            // one new reference for every placed unit of `old` on the screen, when they all move together
            let units: Vec<u32> = self.screen_of_file(file).map(|s| s.symbols.iter().filter(|x| x.id == old).map(|x| x.unit).collect()).unwrap_or_default();
            let news: BTreeSet<Option<&String>> = units.iter().map(|u| new_of.get(&(file.to_string(), old.to_string(), *u))).collect();
            match news.into_iter().collect::<Vec<_>>().as_slice() {
                [Some(n)] => Some((*n).clone()),
                _ => None,
            }
        };
        let mut new_of: BTreeMap<(String, String, u32), String> = BTreeMap::new();
        for r in renames {
            new_of.insert((r.file.clone(), r.old.clone(), r.unit), r.new.clone());
        }
        // references renamed as a whole (every unit of it, on its screen), and the ones whose units went separate ways
        let mut whole: BTreeMap<String, String> = BTreeMap::new();
        for r in renames.iter().filter(|r| matches!(r.item, Item::Symbol(_))) {
            if let Some(n) = uniform(&r.file, &r.old, &new_of) {
                whole.insert(r.old.clone(), n);
            }
        }
        let base = self.design.nets.clone().unwrap_or_else(|| self.model.nets.clone());

        let files: BTreeSet<String> = renames.iter().map(|r| r.file.clone()).collect();
        for file in files {
            let mine: Vec<&Rename> = renames.iter().filter(|r| r.file == file).collect();
            // pins of a unit that moved alone: which pin numbers belong to which unit
            let pin_units: BTreeMap<String, BTreeMap<String, u32>> = {
                let mut m: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
                if let Some(sch) = self.screen_of_file(&file) {
                    for r in mine.iter().filter(|r| matches!(r.item, Item::Symbol(_))) {
                        if whole.get(&r.old).is_some_and(|n| *n == r.new) || m.contains_key(&r.old) {
                            continue;
                        }
                        let units_of: BTreeMap<String, u32> = sch
                            .symbols
                            .iter()
                            .find(|s| s.id == r.old)
                            .and_then(|s| self.model.part(&s.id).and_then(|p| self.model.real_symbol_of(&s.lib_id, p)).or_else(|| self.design_symbol(&s.lib_id)))
                            .map(|l| l.pins.iter().map(|p| (p.number.clone(), p.unit)).collect())
                            .unwrap_or_default();
                        m.insert(r.old.clone(), units_of);
                    }
                }
                m
            };
            let Some(sch) = self.screen_of_file_mut(&file) else { continue };
            // the pins named in the sheet's own wires, power symbols and no-connect flags
            let new_pin = |pin: &str| -> Option<String> {
                let (r, n) = pin.split_once('.')?;
                if let Some(new) = whole.get(r).filter(|_| mine.iter().any(|m| m.old == r)) {
                    return Some(format!("{new}.{n}"));
                }
                let units = pin_units.get(r)?;
                let unit = units.get(n).copied().unwrap_or(0);
                let target = if unit == 0 { None } else { new_of.get(&(file.clone(), r.to_string(), unit)) };
                target.map(|new| format!("{new}.{n}"))
            };
            for w in sch.wires.iter_mut() {
                for p in w.pins.iter_mut() {
                    if let Some(np) = new_pin(p) {
                        *p = np;
                    }
                }
            }
            for ps in sch.power_symbols.iter_mut() {
                if let Some(np) = new_pin(&ps.pin) {
                    ps.pin = np;
                }
            }
            for nc in sch.no_connects.iter_mut() {
                if let Some(np) = new_pin(&nc.pin) {
                    nc.pin = np;
                }
            }
            // what is kept per placed unit under its reference
            let mut layout = Vec::new();
            let mut autoplaced = Vec::new();
            let mut styles = Vec::new();
            for r in &mine {
                let (old_key, new_key) = (field_key(&r.old, r.unit), field_key(&r.new, r.unit));
                if let Some(v) = sch.field_layout.remove(&old_key) {
                    layout.push((new_key.clone(), v));
                }
                if let Some(v) = sch.extras.fields_autoplaced.remove(&old_key) {
                    autoplaced.push((new_key.clone(), v));
                }
                if let Some(v) = sch.extras.body_styles.remove(&old_key) {
                    styles.push((new_key, v));
                }
            }
            sch.field_layout.extend(layout);
            sch.extras.fields_autoplaced.extend(autoplaced);
            sch.extras.body_styles.extend(styles);
            // the user fields and the lock belong to the part: they move with a reference renamed as a whole
            let moved: Vec<(String, String)> = whole.iter().filter(|(o, _)| mine.iter().any(|m| &m.old == *o)).map(|(o, n)| (o.clone(), n.clone())).collect();
            let mut fields = Vec::new();
            for (old, new) in &moved {
                if let Some(f) = sch.user_fields.remove(old) {
                    fields.push((new.clone(), f));
                }
            }
            sch.user_fields.extend(fields);
            for l in sch.extras.locked.iter_mut() {
                if let Some((_, new)) = moved.iter().find(|(o, _)| o == l) {
                    *l = new.clone();
                }
            }
            sch.extras.locked.sort();
            sch.extras.locked.dedup();
            // and the symbols themselves
            for r in &mine {
                match r.item {
                    Item::Symbol(i) => {
                        if let Some(s) = sch.symbols.get_mut(i) {
                            s.id = r.new.clone();
                        }
                    }
                    Item::Power(i) => {
                        if let Some(p) = sch.power_symbols.get_mut(i) {
                            p.id = r.new.clone();
                        }
                    }
                }
            }
            sch.symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
            sch.power_symbols.sort_by(|a, b| a.id.cmp(&b.id));
        }

        // the net list's pins, so a net keeps its name through the renumbering
        let remap = |pin: &str| -> String {
            let Some((r, n)) = pin.split_once('.') else { return pin.to_string() };
            // a pin of a part renamed as a whole; a part split between references keeps the pin where it was
            match whole.get(r) {
                Some(new) => format!("{new}.{n}"),
                None => pin.to_string(),
            }
        };
        if !whole.is_empty() {
            let nets: Vec<eda_model::Net> = base
                .into_iter()
                .map(|n| {
                    let mut pins: Vec<String> = n.pins.iter().map(|p| remap(p)).collect();
                    pins.sort();
                    pins.dedup();
                    eda_model::Net { name: n.name, pins }
                })
                .collect();
            self.design.nets = Some(nets);
        }
    }

    /// `SCH_EDIT_FRAME::CheckAnnotate`: the findings on the references the scope takes, as they stand now.
    fn annotation_problems(&self, scope: AnnotateScope, ids: Option<&[String]>, opts: &AnnotateOptions) -> Vec<Problem> {
        let mut scopes = self.annotate_scopes(scope, ids, opts);
        // the check takes the power symbols of the whole schematic only for the entire schematic (`SYMBOL_FILTER_NON_POWER` otherwise)
        if scope != AnnotateScope::All {
            scopes.iter_mut().for_each(|s| s.power = false);
        }
        let (_, mine, _) = self.annotation_entries(&scopes);
        if mine.is_empty() {
            return Vec::new();
        }
        annotate::check_annotation(mine)
    }

    /// `Cmd::Annotate`: see the module doc.
    pub(crate) fn annotate(&mut self, reset: bool, order: AnnotateOrder, ids: Option<&[String]>, opts: &AnnotateOptions) -> Result<(), Vec<CheckResult>> {
        self.schematic()?;
        let (_, renames, _) = self.annotate_plan(reset, order, ids, opts);
        self.annotate_apply(&renames);
        Ok(())
    }

    /// What an annotation would do to the sheet `sheet` is in view of (a `/`-joined placement path, the root when empty), without doing it: the
    /// dialog's message window. The board is left as it was.
    pub fn annotate_report(&self, sheet: &str, reset: bool, order: AnnotateOrder, ids: Option<&[String]>, opts: &AnnotateOptions) -> Result<AnnotateReport, Vec<CheckResult>> {
        let mut board = self.fork();
        let target = board.screen_at(sheet)?;
        board.focus = target;
        board.schematic()?;
        let scope = opts.scope.unwrap_or(if ids.is_some() { AnnotateScope::Selection } else { AnnotateScope::CurrentSheet });
        let (_, renames, mut messages) = board.annotate_plan(reset, order, ids, opts);
        let changed = renames.len();
        board.annotate_apply(&renames);
        let problems = board.annotation_problems(scope, ids, opts);
        for p in &problems {
            messages.push(AnnotateMessage { severity: "error", text: p.message.trim_end().to_string() });
        }
        if problems.is_empty() {
            messages.push(AnnotateMessage { severity: "action", text: "Annotation complete.".to_string() });
        }
        Ok(AnnotateReport { messages, changed, problems })
    }
}
