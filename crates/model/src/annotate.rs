//! Annotation: the numbering `Annotate Schematic` does, as a pure function of a list of references.
//!
//! A port of `SCH_REFERENCE` / `SCH_REFERENCE_LIST` (`eeschema/sch_reference_list.cpp`, `.h`) and of the `REFDES_TRACKER` it asks for
//! the next free number (`eeschema/refdes_tracker.cpp`), the way `SCH_EDIT_FRAME::AnnotateSymbols` (`eeschema/annotate.cpp`) drives
//! them for `DIALOG_ANNOTATE` (`eeschema/dialogs/dialog_annotate.cpp`):
//!
//! * [`Entry::split`] is `SCH_REFERENCE::Split`: `R12` is the prefix `R` and the number 12, `R?` and anything that does not end in a
//!   digit is *new* (not annotated).
//! * [`annotate_by_options`] is `AnnotateByOptions` + `Annotate`: the references are sorted by prefix, then (for the sheet number
//!   algorithms) sheet, then X then Y or Y then X, and each new one takes the first number from its starting number up that no
//!   other reference of the same prefix holds -- or that holds only symbols of the same library symbol and value, none with that
//!   unit, for a multi-unit part (`FindFirstUnusedReference`, `REFDES_TRACKER::GetNextRefDesForUnits`, `areUnitsAvailable`). The
//!   units of a multi-unit symbol that shared a reference stay together (`SCH_MULTI_UNIT_REFERENCE_MAP`).
//! * [`check_annotation`] is `SCH_REFERENCE_LIST::CheckAnnotation`, the reference checks KiCad runs at the end of an annotation and
//!   that ERC's `unannotated`, `duplicate_reference` and `different_unit_value` come from.
//!
//! Nothing here knows a schematic. The caller (`eda_ops`) builds the entries from the sheets in scope, hands the sort key of each
//! one in (`key`, a handle of its own, must be unique) and writes the numbers back with [`Entry::reference`].
//!
//! Not ported: the reference designator tracker's memory of numbers used before ("reuse designators" is on, KiCad's default, so a
//! number a deleted symbol gave up is free again), and the duplicate time-stamp repair (`ReplaceDuplicateTimeStamps`): the symbols
//! here have no UUIDs.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use crate::units::Nm;

/// `ANNOTATE_ORDER_T` without `UNSORTED`: which axis comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// `SORT_BY_X_POSITION`: left to right, then top to bottom.
    ByX,
    /// `SORT_BY_Y_POSITION` (the default): top to bottom, then left to right.
    #[default]
    ByY,
}

/// `ANNOTATE_ALGO_T`: how the starting number of a reference is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Algo {
    /// `INCREMENTAL_BY_REF` ("Use first free number after"): from the start number up, the first number nothing holds.
    #[default]
    FirstFree,
    /// `SHEET_NUMBER_X_100`: from the sheet number times 100 up (sheet 2 starts at 201).
    SheetX100,
    /// `SHEET_NUMBER_X_1000`: from the sheet number times 1000 up.
    SheetX1000,
}

impl Algo {
    fn uses_sheet_number(self) -> bool {
        self != Algo::FirstFree
    }

    fn step(self) -> i64 {
        match self {
            Algo::FirstFree => 1,
            Algo::SheetX100 => 100,
            Algo::SheetX1000 => 1000,
        }
    }
}

/// The position of the `?` of a placeholder reference the sheet importer gives symbols it cannot tell apart (`R?1`, `R?2`): a `?` followed by
/// digits to the end. (A reference KiCad writes never has one: its unannotated form is `R?`.)
fn placeholder_mark(reference: &str) -> Option<usize> {
    let q = reference.rfind('?')?;
    let tail = &reference[q + 1..];
    (!tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())).then_some(q)
}

/// Is this reference not annotated: empty, ending in `?` (`R?`), or a placeholder (`R?1`)?
pub fn is_unannotated(reference: &str) -> bool {
    reference.is_empty() || reference.ends_with('?') || placeholder_mark(reference).is_some()
}

/// `LIB_SYMBOL::LetterSubReference( unit, 'A' )`: 1 is `A`, 26 is `Z`, 27 is `AA`.
pub fn unit_letters(unit: u32) -> String {
    letter_sub_reference(unit, 'A' as u32)
}

/// `LIB_SYMBOL::LetterSubReference( aUnit, aInitialLetter )`: the letters of `unit` counted from `initial` (`'A'` gives `A` .. `Z`, `AA`; `'a'` gives `a` ..),
/// a character past the 26 as KiCad's `wxChar( aInitialLetter + u )` makes it.
pub fn letter_sub_reference(unit: u32, initial: u32) -> String {
    let mut unit = unit as i64;
    let mut suffix = String::new();
    loop {
        let u = (unit - 1).rem_euclid(26);
        suffix.insert(0, char::from_u32(initial + u as u32).unwrap_or('?'));
        unit = (unit - u) / 26;
        if unit <= 0 {
            break;
        }
    }
    suffix
}

/// Schematic Setup's "Symbol unit notation" (`SCHEMATIC_SETTINGS::m_SubpartIdSeparator`, `m_SubpartFirstId`): how a unit is named after its reference. KiCad's
/// default is no separator and letters from `A` (`U1A`); a project may say `U1.1`, `U1-A`, `U1_a` and so on (`SchExtras::subpart_id_separator`,
/// `SchExtras::subpart_first_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubpartIds {
    /// The character code between the reference and the unit, 0 for none.
    pub separator: u32,
    /// The character code of unit 1: a digit names the units by number.
    pub first_id: u32,
}

impl Default for SubpartIds {
    fn default() -> Self {
        SubpartIds { separator: 0, first_id: 'A' as u32 }
    }
}

impl SubpartIds {
    /// The settings a schematic's root extras hold (KiCad's defaults where they say nothing).
    pub fn of(extras: &crate::sch_extras::SchExtras) -> SubpartIds {
        let d = SubpartIds::default();
        SubpartIds { separator: extras.subpart_id_separator.unwrap_or(d.separator), first_id: extras.subpart_first_id.unwrap_or(d.first_id) }
    }

    /// The settings of a design (its root sheet's extras).
    pub fn of_design(design: &crate::ir::Design) -> SubpartIds {
        design.schematic.as_ref().map_or_else(SubpartIds::default, |s| SubpartIds::of(&s.extras))
    }

    /// `SCHEMATIC_SETTINGS::SubReference( aUnit, aAddSeparator )` (KiCad 25ec2335, `schematic_settings.cpp`): nothing for a unit below 1; the separator when
    /// there is one and `add_separator`; then the unit's number when the first id is a digit, else its letters from the first id.
    pub fn sub_reference(&self, unit: u32, add_separator: bool) -> String {
        let mut sub = String::new();
        if unit < 1 {
            return sub;
        }
        if self.separator != 0 && add_separator {
            sub.extend(char::from_u32(self.separator));
        }
        if ('0' as u32..='9' as u32).contains(&self.first_id) {
            sub.push_str(&unit.to_string());
        } else {
            sub.push_str(&letter_sub_reference(unit, self.first_id));
        }
        sub
    }

    /// The separator alone, as `SCH_SYMBOL::SubReference` puts it before a unit's display name (`if( sep != 0 && aAddSeparator ) subRef << wxChar( sep )`).
    pub fn separator_text(&self, add_separator: bool) -> String {
        if self.separator != 0 && add_separator { char::from_u32(self.separator).map(String::from).unwrap_or_default() } else { String::new() }
    }

    /// The two keys as KiCad's loader takes them from a project's `schematic` object (`PARAM<int>`: a value out of `0..=126`, or of `'1'..='z'`, is the default),
    /// `None` for one the project does not have or that is the default.
    pub fn from_project(schematic: &serde_json::Value) -> (Option<u32>, Option<u32>) {
        let read = |key: &str, default: u32, min: u32, max: u32| schematic.get(key).and_then(|v| v.as_i64()).map(|v| if v < min as i64 || v > max as i64 { default } else { v as u32 }).filter(|v| *v != default);
        (read("subpart_id_separator", 0, 0, 126), read("subpart_first_id", 'A' as u32, '1' as u32, 'z' as u32))
    }
}

/// One placed symbol as the numbering sees it: `SCH_REFERENCE`.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The caller's handle for the instance (`IsSameInstance`: a symbol on a sheet). Unique within one numbering.
    pub key: usize,
    /// The unit this instance draws (`GetUnitSelection`).
    pub unit: u32,
    /// How many units its library symbol has (1 for a single-unit one).
    pub unit_count: u32,
    /// The library symbol's name (`GetLibId().GetLibItemName()`): units only share a number when this is the same.
    pub lib_name: String,
    /// The Value field (`m_value`; `~` when empty): units only share a number when this is the same.
    pub value: String,
    /// The number of the sheet it is on (`SetSheetNumber( GetPageNumberAsInt() )`).
    pub sheet_num: i64,
    /// Its position on the sheet (`m_symbolPos`).
    pub pos: (Nm, Nm),
    /// A power symbol (its number is written with a leading zero, `#PWR012`).
    pub power: bool,
    // ---- `SCH_REFERENCE`'s working state
    original: String,
    text: String,
    num: i64,
    num_str: String,
    is_new: bool,
    flag: bool,
    split_done: bool,
}

impl Entry {
    /// `SCH_REFERENCE::SCH_REFERENCE`: `reference` is the symbol's reference text as it stands (`R12`, `R?`, `#PWR01`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(key: usize, reference: &str, unit: u32, unit_count: u32, lib_name: &str, value: &str, sheet_num: i64, pos: (Nm, Nm), power: bool) -> Entry {
        Entry {
            key,
            unit,
            unit_count: unit_count.max(1),
            lib_name: lib_name.to_string(),
            value: if value.is_empty() { "~".to_string() } else { value.to_string() },
            sheet_num,
            pos,
            power,
            original: reference.to_string(),
            text: reference.to_string(),
            num: -1,
            num_str: String::new(),
            is_new: false,
            flag: false,
            split_done: false,
        }
    }

    /// `SCH_REFERENCE::Split`: the prefix and the number. A reference ending in `?`, or in anything but a digit, is new. Done once: the
    /// text is the prefix afterwards (the C++ guards a second split with `IsSplitNeeded`).
    pub fn split(&mut self) {
        if self.split_done {
            return;
        }
        self.split_done = true;
        self.num = -1;
        self.num_str.clear();
        if let Some(q) = placeholder_mark(&self.text) {
            self.is_new = true;
            self.text.truncate(q);
            return;
        }
        let mut chars: Vec<char> = self.text.chars().collect();
        let Some(&last) = chars.last() else {
            self.is_new = true;
            return;
        };
        if last == '?' {
            self.is_new = true;
            chars.pop();
            self.text = chars.into_iter().collect();
        } else if !last.is_ascii_digit() {
            self.is_new = true;
        } else {
            // walk back over blanks and digits; the number is what follows the first other character
            let mut ll = chars.len() as isize - 1;
            while ll >= 0 {
                let c = chars[ll as usize];
                if c <= ' ' || c.is_ascii_digit() {
                    ll -= 1;
                } else {
                    let from = (ll + 1) as usize;
                    if chars.get(from).is_some_and(|c| c.is_ascii_digit()) {
                        let digits: String = chars[from..].iter().take_while(|c| c.is_ascii_digit()).collect();
                        self.num = digits.parse().unwrap_or(-1);
                    }
                    self.num_str = chars[from..].iter().collect();
                    chars.truncate(from);
                    break;
                }
            }
            self.text = chars.into_iter().collect();
        }
    }

    /// The reference text the entry was made with (`R12`, `R?`), before any numbering.
    pub fn original(&self) -> &str {
        &self.original
    }

    /// The reference it was made with, with the unit's letters after it for a multi-unit symbol (`GetRef( sheet, true )`: `U1A`).
    pub fn original_with_unit(&self) -> String {
        if self.unit_count > 1 {
            format!("{}{}", self.original, unit_letters(self.unit))
        } else {
            self.original.clone()
        }
    }

    /// `GetRef()`: the prefix, once split.
    pub fn prefix(&self) -> &str {
        &self.text
    }

    /// `GetRefNumber()`: the number as written (`?` when there is none).
    pub fn number_text(&self) -> String {
        if self.num < 0 {
            "?".to_string()
        } else {
            self.num_str.clone()
        }
    }

    /// The number, when the reference has one.
    pub fn number(&self) -> Option<i64> {
        (self.num >= 0).then_some(self.num)
    }

    /// True for a reference that is not annotated (`m_isNew`): it ended in `?` or in no digit, or [`remove_annotation`] marked it.
    pub fn is_new(&self) -> bool {
        self.is_new
    }

    /// What `SCH_REFERENCE::Annotate` writes into the symbol: the prefix and the number, `R?` when there is none.
    pub fn reference(&self) -> String {
        if self.num < 0 {
            format!("{}?", self.text)
        } else {
            format!("{}{}", self.text, self.num_str)
        }
    }

    /// `CompareRef`: the prefix, ignoring case.
    fn compare_prefix(&self, o: &Entry) -> Ordering {
        let fold = |s: &str| s.chars().map(|c| c.to_ascii_uppercase()).collect::<Vec<_>>();
        fold(&self.text).cmp(&fold(&o.text))
    }

    /// `SCH_REFERENCE::formatRefStr`: a power symbol's number carries a leading zero, so it cannot collide with a part's.
    fn format_number(&self, n: i64) -> String {
        if self.power {
            format!("0{n}")
        } else {
            n.to_string()
        }
    }

    /// `SCH_REFERENCE::SubReference`-less text of the reference with its unit letters: `U1A`.
    pub fn with_unit(&self) -> String {
        let r = self.reference();
        if self.unit_count > 1 {
            format!("{r}{}", unit_letters(self.unit))
        } else {
            r
        }
    }
}

/// `SCH_REFERENCE_LIST::SplitReferences`.
pub fn split_all(list: &mut [Entry]) {
    list.iter_mut().for_each(Entry::split);
}

/// `SCH_REFERENCE_LIST::RemoveAnnotation`: treat every reference as not annotated (the numbers they hold stay, to tell which units went together).
pub fn remove_annotation(list: &mut [Entry]) {
    list.iter_mut().for_each(|e| e.is_new = true);
}

/// `sortByXPosition` / `sortByYPosition`.
fn sort_cmp(order: SortOrder, a: &Entry, b: &Entry) -> Ordering {
    let by_pos = match order {
        SortOrder::ByX => a.pos.0.cmp(&b.pos.0).then(a.pos.1.cmp(&b.pos.1)),
        SortOrder::ByY => a.pos.1.cmp(&b.pos.1).then(a.pos.0.cmp(&b.pos.0)),
    };
    // the last key stands for the symbols' time stamps: it makes the order total
    a.compare_prefix(b).then(a.sheet_num.cmp(&b.sheet_num)).then(by_pos).then(a.key.cmp(&b.key))
}

/// `REFDES_TRACKER::areUnitsAvailable`: every unit in `required` is free on a number that others hold -- none of them is another library
/// symbol or another value, and none already has that unit.
fn units_available(r: &Entry, holders: &[&Entry], required: &[u32]) -> bool {
    required.iter().all(|unit| holders.iter().all(|h| h.lib_name == r.lib_name && h.value == r.value && h.unit != *unit))
}

/// `SCH_REFERENCE_LIST::FindFirstUnusedReference` + `REFDES_TRACKER::GetNextRefDesForUnits`: the first number from `min` up that no
/// annotated reference of `r`'s prefix holds -- or that holds only references that leave every one of `required` units free.
fn first_unused(list: &[Entry], r: &Entry, min: i64, required: &[u32]) -> i64 {
    let mut candidate = min;
    loop {
        let holders: Vec<&Entry> = list.iter().filter(|e| !e.is_new && e.compare_prefix(r) == Ordering::Equal && e.num == candidate).collect();
        if holders.is_empty() || (!required.is_empty() && units_available(r, &holders, required)) {
            return candidate;
        }
        candidate += 1;
    }
}

/// `buildFullReference`: reference, number and unit, the key of the "already allocated" set.
fn full_reference(e: &Entry, unit: u32) -> String {
    format!("{}{}..{unit}", e.text, e.number_text())
}

/// `SCH_REFERENCE_LIST::GetUnitsMatchingRef`: the units of `group` that are the same part as `r` (same value, library symbol, prefix and
/// number), `r`'s own first, sorted and without repeats.
fn units_matching(group: &[Entry], r: &Entry) -> Vec<u32> {
    let mut units = vec![r.unit];
    for g in group {
        if g.value != r.value || g.lib_name != r.lib_name || g.compare_prefix(r) != Ordering::Equal || g.num != r.num {
            continue;
        }
        units.push(g.unit);
    }
    units.sort_unstable();
    units.dedup();
    units
}

/// `SCH_REFERENCE_LIST::AnnotateByOptions`: sort `list` and number its new references.
///
/// * `locked` are the multi-unit symbols whose units stay together (`SCH_MULTI_UNIT_REFERENCE_MAP`): one group of entries per
///   reference, as they were *before* any reset. A group renumbers as one: the first of its units that comes up takes a number free for
///   all of the group's units and the others copy it.
/// * `additional` are the references outside the list that must not be given again (`aAdditionalRefs`).
/// * `start` is the number to count after (the dialog's "Use first free number after"); the sheet number algorithms count from the
///   sheet's number times 100 or 1000 instead.
pub fn annotate_by_options(list: &mut Vec<Entry>, sort: SortOrder, algo: Algo, start: i64, locked: &[Vec<Entry>], additional: &[Entry]) {
    list.sort_by(|a, b| sort_cmp(sort, a, b));
    annotate(list, algo.uses_sheet_number(), algo.step(), start, locked, additional);
}

/// `SCH_REFERENCE_LIST::Annotate`.
fn annotate(list: &mut Vec<Entry>, use_sheet_num: bool, sheet_step: i64, start: i64, locked: &[Vec<Entry>], additional: &[Entry]) {
    if list.is_empty() {
        return;
    }
    let original = list.len();
    // the references of the other symbols: taken, and never numbered here
    let mut in_use: BTreeSet<String> = BTreeSet::new();
    for a in additional {
        let mut a = a.clone();
        a.split();
        if !a.is_new {
            in_use.insert(full_reference(&a, a.unit));
        }
        a.is_new = false;
        list.push(a);
    }
    // the groups, split once
    let groups: Vec<Vec<Entry>> = locked
        .iter()
        .map(|g| {
            g.iter()
                .map(|e| {
                    let mut e = e.clone();
                    e.split();
                    e
                })
                .collect()
        })
        .collect();

    let mut first = 0usize;
    let mut min_ref_id = if use_sheet_num { list[first].sheet_num * sheet_step + 1 } else { start + 1 };

    for ii in 0..list.len() {
        if list[ii].flag {
            continue;
        }
        let unit_ref = list[ii].clone();
        let locked_group = groups.iter().find(|g| g.iter().any(|l| l.key == unit_ref.key));

        if list[first].compare_prefix(&unit_ref) != Ordering::Equal || (use_sheet_num && list[first].sheet_num != unit_ref.sheet_num) {
            // a new reference found: it needs a new number
            first = ii;
            min_ref_id = if use_sheet_num { unit_ref.sheet_num * sheet_step + 1 } else { start + 1 };
        }

        // one part per package: the trivial case
        if unit_ref.unit_count <= 1 {
            if unit_ref.is_new {
                let n = first_unused(list, &unit_ref, min_ref_id, &[]);
                let e = &mut list[ii];
                e.num = n;
                e.num_str = e.format_number(n);
            }
            let e = &mut list[ii];
            e.flag = true;
            e.is_new = false;
            continue;
        }

        if let Some(group) = locked_group {
            let units = units_matching(group, &unit_ref);
            if unit_ref.is_new {
                let n = first_unused(list, &unit_ref, min_ref_id, &units);
                let e = &mut list[ii];
                e.num = n;
                e.num_str = e.format_number(n);
                e.is_new = false;
                e.flag = true;
            }
            for locked_ref in group {
                if locked_ref.key == unit_ref.key {
                    // this is the symbol being annotated: it holds its unit
                    list[ii].unit = locked_ref.unit;
                    let held = full_reference(&list[ii], list[ii].unit);
                    in_use.insert(held);
                }
                if locked_ref.value != unit_ref.value || locked_ref.lib_name != unit_ref.lib_name {
                    continue;
                }
                // the matching symbol later in the list gets the new number, unless that full reference is already allocated (it can
                // be, when the old annotation had duplicates)
                for jj in ii + 1..list.len() {
                    if list[jj].key != locked_ref.key {
                        continue;
                    }
                    let candidate = full_reference(&list[ii], locked_ref.unit);
                    if !in_use.contains(&candidate) {
                        let (n, s) = (list[ii].num, list[ii].num_str.clone());
                        let e = &mut list[jj];
                        e.num = n;
                        e.num_str = s;
                        e.is_new = false;
                        e.flag = true;
                        in_use.insert(candidate);
                        break;
                    }
                }
            }
        } else if unit_ref.is_new {
            // a unit of a multi-unit symbol that was not annotated: nothing says which part it belongs to, so it takes the first number
            // that has its unit free; the other units are numbered by the passes that follow
            let n = first_unused(list, &unit_ref, min_ref_id, &[unit_ref.unit]);
            let e = &mut list[ii];
            e.num = n;
            e.num_str = e.format_number(n);
            e.is_new = false;
            e.flag = true;
        }
    }
    list.truncate(original);
}

/// What `SCH_REFERENCE_LIST::CheckAnnotation` found wrong, as the `ERCE_*` code it reports under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// `ERCE_UNANNOTATED`
    Unannotated,
    /// `ERCE_EXTRA_UNITS`: a unit past the library symbol's last one.
    ExtraUnits,
    /// `ERCE_DUPLICATE_REFERENCE`: one reference and unit twice, or one reference over symbols with different unit counts.
    DuplicateReference,
    /// `ERCE_DIFFERENT_UNIT_VALUE`
    DifferentUnitValue,
}

/// One finding of [`check_annotation`]: its message (KiCad's words) and the entries it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
    /// `Entry::key` of the first symbol involved.
    pub first: usize,
    /// `Entry::key` of the second, for the problems about two.
    pub second: Option<usize>,
}

/// `SCH_REFERENCE_LIST::SortByRefAndValue` + `CheckAnnotation`: the findings, in KiCad's order, and KiCad's wording.
///
/// Like KiCad it stops the first pass at the first reference that is not annotated or has an extra unit (the `break`), so a schematic
/// full of `R?` reports one `Item not annotated`; the pass over duplicates always runs.
pub fn check_annotation(mut list: Vec<Entry>) -> Vec<Problem> {
    list.sort_by(|a, b| {
        a.compare_prefix(b)
            .then(a.value.cmp(&b.value))
            .then(a.unit.cmp(&b.unit))
            .then(a.sheet_num.cmp(&b.sheet_num))
            .then(a.pos.0.cmp(&b.pos.0))
            .then(a.pos.1.cmp(&b.pos.1))
            .then(a.key.cmp(&b.key))
    });
    split_all(&mut list);
    let mut out: Vec<Problem> = Vec::new();

    for e in &list {
        if e.is_new {
            let tmp = if e.num >= 0 { e.num_str.clone() } else { "?".to_string() };
            let message = if e.unit > 0 && e.unit < 0x7FFF_FFFF && e.unit_count > 1 {
                format!("Item not annotated: {}{} (unit {})", e.text, tmp, e.unit)
            } else {
                format!("Item not annotated: {}{}", e.text, tmp)
            };
            out.push(Problem { kind: ProblemKind::Unannotated, message, first: e.key, second: None });
            break;
        }
        // a unit past the library symbol's last one (the library changed after the annotation)
        if e.unit_count.max(1) < e.unit {
            let tmp = if e.num >= 0 { e.num_str.clone() } else { "?".to_string() };
            let message = format!("Error: symbol {}{}{} (unit {}) exceeds units defined ({})", e.text, tmp, unit_letters(e.unit), e.unit, e.unit_count);
            out.push(Problem { kind: ProblemKind::ExtraUnits, message, first: e.key, second: None });
            break;
        }
    }

    for pair in list.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        if first.compare_prefix(second) != Ordering::Equal || first.num != second.num {
            continue;
        }
        let num = |e: &Entry| if e.num >= 0 { e.num_str.clone() } else { "?".to_string() };
        // the same reference and unit twice
        if first.unit == second.unit {
            let unit = if first.unit_count > 1 { unit_letters(first.unit) } else { String::new() };
            let message = format!("Duplicate items {}{}{}\n", first.text, num(first), unit);
            out.push(Problem { kind: ProblemKind::DuplicateReference, message, first: first.key, second: Some(second.key) });
            continue;
        }
        // different units, but the library symbols do not agree how many there are (U3 of one part and U3B)
        if first.unit_count != second.unit_count {
            let message = format!("Differing unit counts for item {}{}{} and {}{}{}\n", first.text, num(first), unit_letters(first.unit), second.text, num(second), unit_letters(second.unit));
            out.push(Problem { kind: ProblemKind::DuplicateReference, message, first: first.key, second: Some(second.key) });
            continue;
        }
        // different values on the units of one reference
        if first.value != second.value {
            let message = format!(
                "Different values for {}{}{} ({}) and {}{}{} ({})",
                first.text,
                first.num,
                unit_letters(first.unit),
                first.value,
                second.text,
                second.num,
                unit_letters(second.unit),
                second.value
            );
            out.push(Problem { kind: ProblemKind::DifferentUnitValue, message, first: first.key, second: Some(second.key) });
        }
    }
    out
}

#[cfg(test)]
mod tests {

    use super::{SubpartIds, letter_sub_reference};

    /// 25ec2335 `SCHEMATIC_SETTINGS::SubReference`: the separator when asked for, the number for a digit first id, else letters from the first id (r7 H5).
    #[test]
    fn a_projects_unit_notation_names_the_units() {
        assert_eq!(SubpartIds::default().sub_reference(2, true), "B");
        let dotted = SubpartIds { separator: '.' as u32, first_id: '1' as u32 };
        assert_eq!((dotted.sub_reference(1, true), dotted.sub_reference(12, true), dotted.sub_reference(3, false), dotted.sub_reference(0, true)), (".1".into(), ".12".into(), "3".into(), String::new()));
        let lower = SubpartIds { separator: '-' as u32, first_id: 'a' as u32 };
        assert_eq!((lower.sub_reference(1, true), lower.sub_reference(27, true)), ("-a".into(), "-aa".into()));
        assert_eq!(letter_sub_reference(28, 'A' as u32), "AB");
    }
    use super::*;

    fn e(key: usize, reference: &str, pos: (Nm, Nm)) -> Entry {
        Entry::new(key, reference, 1, 1, "R", "10k", 1, pos, false)
    }

    fn unit_entry(key: usize, reference: &str, unit: u32, units: u32, value: &str, pos: (Nm, Nm)) -> Entry {
        Entry::new(key, reference, unit, units, "LM358", value, 1, pos, false)
    }

    fn run(list: Vec<Entry>, sort: SortOrder, algo: Algo, start: i64, locked: &[Vec<Entry>], extra: &[Entry]) -> Vec<(usize, String)> {
        let mut list = list;
        split_all(&mut list);
        annotate_by_options(&mut list, sort, algo, start, locked, extra);
        let mut out: Vec<(usize, String)> = list.iter().map(|e| (e.key, e.reference())).collect();
        out.sort();
        out
    }

    #[test]
    fn split_follows_sch_reference_split() {
        let mut r = e(0, "R12", (Nm::ZERO, Nm::ZERO));
        r.split();
        assert_eq!((r.prefix(), r.number(), r.number_text().as_str(), r.is_new()), ("R", Some(12), "12", false));
        let mut r = e(0, "R?", (Nm::ZERO, Nm::ZERO));
        r.split();
        assert_eq!((r.prefix(), r.number(), r.is_new(), r.reference().as_str()), ("R", None, true, "R?"));
        let mut r = e(0, "R?12", (Nm::ZERO, Nm::ZERO));
        r.split();
        assert_eq!((r.prefix(), r.number(), r.is_new()), ("R", None, true), "the importer's placeholder is not a number");
        assert!(is_unannotated("R?") && is_unannotated("R?3") && is_unannotated("") && !is_unannotated("R3") && !is_unannotated("U1A"));
        let mut r = e(0, "U1A", (Nm::ZERO, Nm::ZERO));
        r.split();
        assert!(r.is_new(), "a reference that does not end in a digit is not annotated");
        let mut p = Entry::new(0, "#PWR012", 1, 1, "GND", "GND", 1, (Nm::ZERO, Nm::ZERO), true);
        p.split();
        assert_eq!((p.prefix(), p.number(), p.number_text().as_str()), ("#PWR", Some(12), "012"));
        assert_eq!(unit_letters(1), "A");
        assert_eq!(unit_letters(26), "Z");
        assert_eq!(unit_letters(27), "AA");
    }

    #[test]
    fn a_new_reference_takes_the_first_free_number_not_the_one_after_the_highest() {
        let list = vec![e(0, "R1", (Nm::ZERO, Nm::ZERO)), e(1, "R2", (Nm::new(10), Nm::ZERO)), e(2, "R4", (Nm::new(20), Nm::ZERO)), e(3, "R?", (Nm::new(30), Nm::ZERO))];
        let out = run(list, SortOrder::ByY, Algo::FirstFree, 0, &[], &[]);
        assert_eq!(out[3], (3, "R3".to_string()), "R3 is the gap");
        assert_eq!(out[2], (2, "R4".to_string()), "an annotated reference is not touched");
    }

    #[test]
    fn the_order_is_by_prefix_then_x_then_y_or_y_then_x() {
        // A is left of and below B: by Y, B comes first; by X, A does
        let list = || vec![e(0, "R?", (Nm::new(5_000), Nm::new(50_000))), e(1, "R?", (Nm::new(60_000), Nm::new(40_000))), Entry::new(2, "C?", 1, 1, "C", "1u", 1, (Nm::new(90_000), Nm::new(90_000)), false)];
        let by_y = run(list(), SortOrder::ByY, Algo::FirstFree, 0, &[], &[]);
        assert_eq!(by_y, vec![(0, "R2".to_string()), (1, "R1".to_string()), (2, "C1".to_string())]);
        let by_x = run(list(), SortOrder::ByX, Algo::FirstFree, 0, &[], &[]);
        assert_eq!(by_x, vec![(0, "R1".to_string()), (1, "R2".to_string()), (2, "C1".to_string())]);
    }

    #[test]
    fn the_start_number_and_the_sheet_algorithms_set_where_counting_starts() {
        let sheet = |key: usize, reference: &str, sheet_num: i64, x: i64| Entry::new(key, reference, 1, 1, "R", "1k", sheet_num, (Nm::new(x), Nm::ZERO), false);
        let list = || vec![sheet(0, "R?", 1, 0), sheet(1, "R?", 1, 10), sheet(2, "R?", 2, 0), sheet(3, "R?", 3, 0)];
        assert_eq!(run(list(), SortOrder::ByY, Algo::FirstFree, 10, &[], &[]).iter().map(|x| x.1.clone()).collect::<Vec<_>>(), vec!["R11", "R12", "R13", "R14"]);
        assert_eq!(run(list(), SortOrder::ByY, Algo::SheetX100, 0, &[], &[]).iter().map(|x| x.1.clone()).collect::<Vec<_>>(), vec!["R101", "R102", "R201", "R301"]);
        assert_eq!(run(list(), SortOrder::ByY, Algo::SheetX1000, 0, &[], &[]).iter().map(|x| x.1.clone()).collect::<Vec<_>>(), vec!["R1001", "R1002", "R2001", "R3001"]);
    }

    #[test]
    fn the_other_symbols_of_the_schematic_keep_their_numbers() {
        let mut other = e(100, "R1", (Nm::ZERO, Nm::ZERO));
        other.split();
        let mut other2 = e(101, "R2", (Nm::ZERO, Nm::ZERO));
        other2.split();
        let out = run(vec![e(0, "R?", (Nm::ZERO, Nm::ZERO))], SortOrder::ByY, Algo::FirstFree, 0, &[], &[other, other2]);
        assert_eq!(out, vec![(0, "R3".to_string())]);
    }

    #[test]
    fn reset_renumbers_by_position_and_lets_go_of_the_old_numbers() {
        let mut list = vec![e(0, "R7", (Nm::new(30), Nm::ZERO)), e(1, "R3", (Nm::new(10), Nm::ZERO)), e(2, "R9", (Nm::new(20), Nm::ZERO))];
        split_all(&mut list);
        remove_annotation(&mut list);
        annotate_by_options(&mut list, SortOrder::ByX, Algo::FirstFree, 0, &[], &[]);
        let mut out: Vec<(usize, String)> = list.iter().map(|x| (x.key, x.reference())).collect();
        out.sort();
        assert_eq!(out, vec![(0, "R3".to_string()), (1, "R1".to_string()), (2, "R2".to_string())]);
    }

    #[test]
    fn the_units_of_one_reference_keep_one_number_and_each_unit_is_held_once() {
        // U1 has units A and B on the sheet; a second chip's unit A is new. Resetting numbers both as parts.
        let list = vec![unit_entry(0, "U1", 1, 2, "LM358", (Nm::ZERO, Nm::ZERO)), unit_entry(1, "U1", 2, 2, "LM358", (Nm::ZERO, Nm::new(50))), unit_entry(2, "U5", 1, 2, "LM358", (Nm::new(100), Nm::ZERO)), unit_entry(3, "U5", 2, 2, "LM358", (Nm::new(100), Nm::new(50)))];
        let mut locked_group_u1 = vec![list[0].clone(), list[1].clone()];
        let mut locked_group_u5 = vec![list[2].clone(), list[3].clone()];
        locked_group_u1.iter_mut().chain(locked_group_u5.iter_mut()).for_each(Entry::split);
        let mut work = list.clone();
        split_all(&mut work);
        remove_annotation(&mut work);
        annotate_by_options(&mut work, SortOrder::ByY, Algo::FirstFree, 0, &[locked_group_u1, locked_group_u5], &[]);
        let mut out: Vec<(usize, String)> = work.iter().map(|x| (x.key, x.reference())).collect();
        out.sort();
        // the first group (by position) takes U1, the second U2; both units of a group share the number
        assert_eq!(out, vec![(0, "U1".to_string()), (1, "U1".to_string()), (2, "U2".to_string()), (3, "U2".to_string())]);
    }

    #[test]
    fn an_unannotated_unit_takes_a_number_where_its_unit_is_free() {
        // U1 unit 1 is placed; a new unit-1 of the same part cannot share U1, a new unit 2 can
        let mut held = unit_entry(10, "U1", 1, 2, "LM358", (Nm::ZERO, Nm::ZERO));
        held.split();
        let out = run(vec![unit_entry(0, "U?", 1, 2, "LM358", (Nm::new(50), Nm::ZERO)), unit_entry(1, "U?", 2, 2, "LM358", (Nm::new(60), Nm::ZERO))], SortOrder::ByY, Algo::FirstFree, 0, &[], &[held]);
        assert_eq!(out, vec![(0, "U2".to_string()), (1, "U1".to_string())], "the unit that is free on U1 joins it");
        // a different value keeps the numbers apart
        let mut held = unit_entry(10, "U1", 1, 2, "TL072", (Nm::ZERO, Nm::ZERO));
        held.split();
        let out = run(vec![unit_entry(1, "U?", 2, 2, "LM358", (Nm::new(60), Nm::ZERO))], SortOrder::ByY, Algo::FirstFree, 0, &[], &[held]);
        assert_eq!(out, vec![(1, "U2".to_string())]);
    }

    #[test]
    fn a_power_symbol_is_numbered_with_a_leading_zero() {
        let pwr = |key: usize, reference: &str, x: i64| Entry::new(key, reference, 1, 1, "GND", "GND", 1, (Nm::new(x), Nm::ZERO), true);
        let mut taken = pwr(9, "#PWR01", 0);
        taken.split();
        let out = run(vec![pwr(0, "#PWR?", 10)], SortOrder::ByY, Algo::FirstFree, 0, &[], &[taken]);
        assert_eq!(out, vec![(0, "#PWR02".to_string())]);
    }

    #[test]
    fn the_checks_say_what_kicad_says() {
        let msgs = |list: Vec<Entry>| check_annotation(list).into_iter().map(|p| (p.kind, p.message)).collect::<Vec<_>>();
        assert!(msgs(vec![e(0, "R1", (Nm::ZERO, Nm::ZERO)), e(1, "R2", (Nm::ZERO, Nm::ZERO))]).is_empty());
        assert_eq!(msgs(vec![e(0, "R?", (Nm::ZERO, Nm::ZERO)), e(1, "R1", (Nm::ZERO, Nm::ZERO))]), vec![(ProblemKind::Unannotated, "Item not annotated: R?".to_string())]);
        assert_eq!(msgs(vec![e(0, "R1", (Nm::ZERO, Nm::ZERO)), e(1, "R1", (Nm::new(5), Nm::new(5)))]), vec![(ProblemKind::DuplicateReference, "Duplicate items R1\n".to_string())]);
        assert_eq!(
            msgs(vec![unit_entry(0, "U1", 3, 2, "LM358", (Nm::ZERO, Nm::ZERO))]),
            vec![(ProblemKind::ExtraUnits, "Error: symbol U1C (unit 3) exceeds units defined (2)".to_string())]
        );
        assert_eq!(
            msgs(vec![unit_entry(0, "U1", 1, 2, "LM358", (Nm::ZERO, Nm::ZERO)), unit_entry(1, "U1", 2, 2, "TL072", (Nm::ZERO, Nm::new(9)))]),
            vec![(ProblemKind::DifferentUnitValue, "Different values for U1A (LM358) and U1B (TL072)".to_string())]
        );
        assert_eq!(msgs(vec![unit_entry(0, "U1", 1, 2, "LM358", (Nm::ZERO, Nm::ZERO)), unit_entry(1, "U1", 2, 2, "LM358", (Nm::ZERO, Nm::new(9)))]), vec![]);
    }
}
