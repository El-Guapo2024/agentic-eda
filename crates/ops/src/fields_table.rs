//! Symbol Fields Table: the data model behind eeschema's "Symbol Fields
//! Table" dialog (`eeschema/dialogs/dialog_symbol_fields_table.cpp`) and its
//! `FIELDS_EDITOR_GRID_DATA_MODEL` (`eeschema/fields_data_model.cpp`).
//!
//! What is ported:
//!  * `SCH_REFERENCE::Split` / `SCH_REFERENCE_LIST::Shorthand`
//!    (`sch_reference_list.cpp`): reference splitting and the collapsed
//!    "R1-R3, R5" reference text ([`shorthand`]).
//!  * `FIELDS_EDITOR_GRID_DATA_MODEL::RebuildRows` / `groupMatch` /
//!    `unitMatch` / `Sort` / `cmp` / `GetValue(group, col, ...)`
//!    ([`build_table`]): rows of symbols, optionally grouped by the columns
//!    whose "Group By" box is checked (KiCad's "Grouped By Value and
//!    Footprint" preset = group on Value + Footprint), the `${QUANTITY}` /
//!    `${ITEM_NUMBER}` generated columns, and the "-- mixed values --"
//!    placeholder for a group whose members disagree.
//!  * `FIELDS_EDITOR_GRID_DATA_MODEL::Export` + `BOM_FMT_PRESET`
//!    (`common/settings/bom_settings.cpp`): the BOM text writer with its
//!    field/string/reference/range delimiters and keep-tabs/line-breaks
//!    options ([`export_bom`]), including the CSV/TSV/Semicolons presets.
//!  * `FIELDS_EDITOR_GRID_DATA_MODEL::ApplyData` (the part that matters for
//!    our IR): [`apply_field_changes`], the one function both
//!    `Cmd::SetSymbolFields` and the preview/export endpoints use, so what
//!    the table shows before "Apply" is exactly what Apply then commits.
//!
//! Scope limits (also listed in `web/studio/PARITY-sch.md`): our IR has no
//! DNP / exclude-from-BOM / exclude-from-board attributes, no field
//! visibility and no hierarchy-wide scope selector, so the attribute
//! columns (`${DNP}`, ...) and the DNP / "include excluded" filters do not
//! exist, and the table covers one sheet's symbols.

use std::collections::BTreeMap;

use eda_model::ir::{SchematicSection, SymbolInstance};
use eda_model::ConstraintModel;
use serde::{Deserialize, Serialize};

pub const REFERENCE: &str = "Reference";
pub const VALUE: &str = "Value";
pub const FOOTPRINT: &str = "Footprint";
pub const DATASHEET: &str = "Datasheet";
/// `FIELDS_EDITOR_GRID_DATA_MODEL::QUANTITY_VARIABLE`.
pub const QUANTITY: &str = "${QUANTITY}";
/// `FIELDS_EDITOR_GRID_DATA_MODEL::ITEM_NUMBER_VARIABLE`.
pub const ITEM_NUMBER: &str = "${ITEM_NUMBER}";
/// `INDETERMINATE_STATE` (`include/widgets/ui_common.h`).
pub const INDETERMINATE: &str = "-- mixed values --";

/// `IsGeneratedField`: a field whose name is a `${VAR}` expression.
pub fn is_generated_field(name: &str) -> bool {
    name.starts_with("${") && name.ends_with('}')
}

fn is_mandatory(name: &str) -> bool {
    matches!(name, REFERENCE | VALUE | FOOTPRINT | DATASHEET)
}

// ---------------------------------------------------------------------
// References
// ---------------------------------------------------------------------

/// One reference as `SCH_REFERENCE` splits it: `R12` -> prefix `R`, number
/// 12. An unannotated `R?` (or a reference not ending in a digit) has
/// `num == None` (`m_numRef = -1`, `GetRefNumber() == "?"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub prefix: String,
    pub num: Option<u64>,
}

impl Ref {
    /// `SCH_REFERENCE::Split`.
    pub fn parse(text: &str) -> Ref {
        let t = text.trim_end_matches('?');
        let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || t.len() != text.len() {
            return Ref { prefix: t.to_string(), num: None };
        }
        let split = t.len() - digits;
        Ref { prefix: t[..split].trim_end().to_string(), num: t[split..].parse().ok() }
    }
    /// `GetRefNumber()`.
    pub fn number_text(&self) -> String {
        self.num.map_or_else(|| "?".to_string(), |n| n.to_string())
    }
    /// `GetRef() << GetRefNumber()`.
    pub fn full(&self) -> String {
        format!("{}{}", self.prefix, self.number_text())
    }
}

/// `StrNumCmp` (`common/string_utils.cpp`): natural compare, digit runs by
/// value, everything else by character (upper-cased when `ignore_case`).
pub fn str_num_cmp(a: &str, b: &str, ignore_case: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        let (mut c1, mut c2) = (a[i], b[j]);
        if c1.is_ascii_digit() && c2.is_ascii_digit() {
            let (mut n1, mut n2) = (0u128, 0u128);
            while i < a.len() && a[i].is_ascii_digit() {
                n1 = n1.saturating_mul(10).saturating_add(a[i] as u128 - '0' as u128);
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                n2 = n2.saturating_mul(10).saturating_add(b[j] as u128 - '0' as u128);
                j += 1;
            }
            match n1.cmp(&n2) {
                Equal => {}
                o => return o,
            }
            c1 = a.get(i).copied().unwrap_or('\0');
            c2 = b.get(j).copied().unwrap_or('\0');
        }
        let (k1, k2) = if ignore_case { (c1.to_uppercase().next().unwrap_or(c1), c2.to_uppercase().next().unwrap_or(c2)) } else { (c1, c2) };
        match k1.cmp(&k2) {
            Equal => {}
            o => return o,
        }
        if i < a.len() {
            i += 1;
        }
        if j < b.len() {
            j += 1;
        }
    }
    match (i >= a.len(), j >= b.len()) {
        (true, false) => Less,
        (false, true) => Greater,
        _ => Equal,
    }
}

/// `SCH_REFERENCE_LIST::Shorthand`: `aList` must already be sorted
/// (`StrNumCmp` on prefix+number) and de-duplicated. Three or more
/// consecutive numbers collapse to `R1-R3` (joined by `range_delimiter`);
/// exactly two stay listed (`R1, R2`, joined by `ref_delimiter`) -- and an
/// empty `range_delimiter` disables ranges altogether, as in KiCad.
pub fn shorthand(list: &[Ref], ref_delimiter: &str, range_delimiter: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < list.len() {
        let r = &list[i];
        let mut range = 1usize;
        while i + range < list.len() && list[i + range].prefix == r.prefix && matches!((r.num, list[i + range].num), (Some(a), Some(b)) if b as i128 == a as i128 + range as i128) {
            range += 1;
            if range == 2 && range_delimiter.is_empty() {
                break;
            }
        }
        if !out.is_empty() {
            out.push_str(ref_delimiter);
        }
        if range == 1 {
            out.push_str(&r.full());
        } else if range == 2 || range_delimiter.is_empty() {
            out.push_str(&r.full());
            out.push_str(ref_delimiter);
            out.push_str(&list[i + 1].full());
        } else {
            out.push_str(&r.full());
            out.push_str(range_delimiter);
            out.push_str(&list[i + range - 1].full());
        }
        i += range;
    }
    out
}

// ---------------------------------------------------------------------
// Table model
// ---------------------------------------------------------------------

/// One column (`FIELDS_EDITOR_GRID_DATA_MODEL::COL_ATTRS` / `BOM_FIELD`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// Canonical field name: `Reference`, `Value`, `Footprint`,
    /// `Datasheet`, `${QUANTITY}`, or a user field's name.
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "d_true")]
    pub show: bool,
    #[serde(default)]
    pub group_by: bool,
}

fn d_true() -> bool {
    true
}

/// The dialog's view state (`BOM_PRESET`): columns in order, grouping
/// switch, sort column, filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSpec {
    pub columns: Vec<Column>,
    #[serde(default)]
    pub group_symbols: bool,
    #[serde(default)]
    pub sort_field: String,
    #[serde(default = "d_true")]
    pub sort_asc: bool,
    #[serde(default)]
    pub filter: String,
}

impl TableSpec {
    /// `BOM_PRESET::GroupedByValueFootprint` (labels as KiCad shows them),
    /// minus the `${DNP}` column our IR has no attribute for.
    pub fn grouped_by_value_footprint() -> TableSpec {
        let col = |name: &str, label: &str, group_by: bool| Column { name: name.into(), label: label.into(), show: true, group_by };
        TableSpec {
            columns: vec![col(REFERENCE, "Reference", false), col(VALUE, "Value", true), col(DATASHEET, "Datasheet", false), col(FOOTPRINT, "Footprint", true), col(QUANTITY, "Qty", false)],
            group_symbols: true,
            sort_field: REFERENCE.into(),
            sort_asc: true,
            filter: String::new(),
        }
    }
    /// `BOM_PRESET::DefaultEditing`: the dialog's own initial view -- every
    /// symbol its own row (grouping is on, but nothing is a "Group By"
    /// column except Value/Footprint, as shipped).
    pub fn default_editing() -> TableSpec {
        let col = |name: &str, label: &str, group_by: bool| Column { name: name.into(), label: label.into(), show: true, group_by };
        TableSpec {
            columns: vec![col(REFERENCE, "Reference", false), col(QUANTITY, "Qty", false), col(VALUE, "Value", true), col(FOOTPRINT, "Footprint", true), col(DATASHEET, "Datasheet", false)],
            group_symbols: true,
            sort_field: REFERENCE.into(),
            sort_asc: true,
            filter: String::new(),
        }
    }
}

/// `DATA_MODEL_ROW::m_Flag` (`GROUP_SINGLETON` / `GROUP_COLLAPSED`; the
/// expanded `CHILD_ITEM`s are carried in [`TableRow::children`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowFlag {
    Singleton,
    Group,
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableRow {
    /// Distinct reference designators in this row (ids to edit by).
    pub refs: Vec<String>,
    pub flag: RowFlag,
    pub item_number: usize,
    /// One display string per column in `columns` order
    /// (`GetValue(row, col, ", ", "-", resolve=true)`).
    pub cells: Vec<String>,
    /// True where `cells[i]` is the "-- mixed values --" placeholder.
    pub mixed: Vec<bool>,
    /// Per-symbol rows of a group (what "expand" reveals).
    pub children: Vec<TableRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Table {
    pub rows: Vec<TableRow>,
}

/// One placed symbol as the table sees it (`SCH_REFERENCE`).
#[derive(Debug, Clone)]
struct SymRef {
    id: String,
    r: Ref,
}

type Store = BTreeMap<String, BTreeMap<String, String>>;

/// The effective value of a field for a symbol id -- the table's
/// `m_dataStore` entry (`updateDataStoreSymbolField`): instance copy for
/// `Value` falls back to the part's own, like `schematic_json` does.
fn field_value(sch: &SchematicSection, model: &ConstraintModel, sym: &SymbolInstance, field: &str) -> String {
    match field {
        REFERENCE => sym.id.clone(),
        VALUE => {
            if sym.value.is_empty() {
                model.part(&sym.id).and_then(|p| p.value.clone()).unwrap_or_default()
            } else {
                sym.value.clone()
            }
        }
        FOOTPRINT => sym.footprint.clone(),
        DATASHEET => sym.datasheet.clone(),
        other => sch.user_fields.get(&sym.id).and_then(|m| m.get(other)).cloned().unwrap_or_default(),
    }
}

/// Every user-field name any symbol carries (the dialog adds one column
/// per such name on open: `DIALOG_SYMBOL_FIELDS_TABLE::LoadFieldNames`).
pub fn user_field_names(sch: &SchematicSection) -> Vec<String> {
    let mut names: Vec<String> = sch.user_fields.values().flat_map(|m| m.keys().cloned()).collect();
    names.sort();
    names.dedup();
    names
}

/// `EDA_COMBINED_MATCHER`'s role for the filter box, reduced to its
/// common cases: case-insensitive substring, or a `*`/`?` wildcard
/// containment match when the filter contains one.
fn filter_matches(filter: &str, full_ref: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let hay = full_ref.to_lowercase();
    let pat = filter.to_lowercase();
    if pat.contains('*') || pat.contains('?') {
        let wrapped = format!("*{pat}*");
        crate::search::wild_match(&wrapped, &hay)
    } else {
        hay.contains(&pat)
    }
}

/// `ValueStringCompare` (`string_utils.cpp`), simplified: compares the
/// leading text case-insensitively, then the number with its SI suffix
/// applied (`4k7` == `4.7k`, `100n` < `1u`), then the remaining text.
pub fn value_string_compare(a: &str, b: &str) -> std::cmp::Ordering {
    fn split(s: &str) -> (String, f64, String) {
        let chars: Vec<char> = s.chars().collect();
        let first_digit = chars.iter().position(|c| c.is_ascii_digit());
        let Some(start) = first_digit else { return (s.to_lowercase(), 0.0, String::new()) };
        let beg: String = chars[..start].iter().collect();
        let mut end = start;
        while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.' || chars[end] == ',') {
            end += 1;
        }
        let mut num: String = chars[start..end].iter().collect::<String>().replace(',', ".");
        let mut rest: String = chars[end..].iter().collect();
        // "4k7" -> 4.7k: a modifier letter between digits is the decimal point.
        let mut mult = 1.0;
        if let Some(first) = rest.chars().next() {
            let m = match first {
                'p' => Some(1e-12),
                'n' => Some(1e-9),
                'u' | 'µ' | 'μ' => Some(1e-6),
                'm' => Some(1e-3),
                'k' | 'K' => Some(1e3),
                'M' => Some(1e6),
                'G' => Some(1e9),
                _ => None,
            };
            if let Some(m) = m {
                mult = m;
                let after: String = rest.chars().skip(1).collect();
                let frac: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !frac.is_empty() && !num.contains('.') {
                    num = format!("{num}.{frac}");
                    rest = after.chars().skip(frac.len()).collect();
                } else {
                    rest = after;
                }
            }
        }
        (beg.to_lowercase(), num.parse::<f64>().unwrap_or(0.0) * mult, rest.to_lowercase())
    }
    let (b1, n1, e1) = split(a);
    let (b2, n2, e2) = split(b);
    b1.cmp(&b2).then(n1.partial_cmp(&n2).unwrap_or(std::cmp::Ordering::Equal)).then(e1.cmp(&e2))
}

struct Model<'a> {
    spec: &'a TableSpec,
    store: Store,
}

impl Model<'_> {
    fn col_of(&self, name: &str) -> Option<usize> {
        self.spec.columns.iter().position(|c| c.name == name)
    }

    /// `groupMatch`.
    fn group_match(&self, lh: &SymRef, rh: &SymRef) -> bool {
        let Some(ref_col) = self.col_of(REFERENCE) else { return false };
        let mut matched = false;
        if self.spec.columns[ref_col].group_by {
            if lh.r.prefix != rh.r.prefix {
                return false;
            }
            matched = true;
        }
        for (i, c) in self.spec.columns.iter().enumerate() {
            if i == ref_col || !c.group_by {
                continue;
            }
            let get = |s: &SymRef| self.store.get(&s.id).and_then(|m| m.get(&c.name)).cloned().unwrap_or_default();
            if get(lh) != get(rh) {
                return false;
            }
            matched = true;
        }
        matched
    }
}

/// `unitMatch`: same reference, annotated -> another unit of one symbol.
fn unit_match(l: &SymRef, r: &SymRef) -> bool {
    l.r.num.is_some() && l.r.prefix == r.r.prefix && l.r.num == r.r.num
}

struct RawRow {
    refs: Vec<SymRef>,
    group: bool,
}

/// `GetValue(group, col, refDelimiter, refRangeDelimiter, resolveVars,
/// listMixedValues)`; returns `(text, is_mixed)`.
fn row_value(m: &Model<'_>, refs: &[SymRef], col: usize, ref_delim: &str, range_delim: &str, list_mixed: bool, item_number: usize, child: bool) -> (String, bool) {
    let name = m.spec.columns[col].name.as_str();
    if name == REFERENCE || name == QUANTITY || name == ITEM_NUMBER {
        // Sort + remove duplicates (other units of multi-unit parts).
        let mut v: Vec<Ref> = refs.iter().map(|s| s.r.clone()).collect();
        v.sort_by(|a, b| str_num_cmp(&a.full(), &b.full(), true));
        v.dedup_by(|a, b| a.num.is_some() && a.full() == b.full());
        return match name {
            REFERENCE => (shorthand(&v, ref_delim, range_delim), false),
            QUANTITY => (v.len().to_string(), false),
            _ if !child => (item_number.to_string(), false),
            _ => (String::new(), false),
        };
    }
    let mut values: Vec<String> = Vec::new();
    let mut first: Option<String> = None;
    for s in refs {
        let v = m.store.get(&s.id).and_then(|mm| mm.get(name)).cloned().unwrap_or_default();
        if list_mixed {
            if !values.contains(&v) {
                values.push(v);
            }
        } else if let Some(f) = &first {
            if *f != v {
                return (INDETERMINATE.to_string(), true);
            }
        } else {
            first = Some(v);
        }
    }
    if list_mixed {
        values.sort();
        return (values.into_iter().filter(|v| !v.is_empty()).collect::<Vec<_>>().join(","), false);
    }
    (first.unwrap_or_default(), false)
}

fn sym_refs(sch: &SchematicSection) -> Vec<SymRef> {
    sch.symbols.iter().map(|s| SymRef { id: s.id.clone(), r: Ref::parse(&s.id) }).collect()
}

fn build_store(sch: &SchematicSection, model: &ConstraintModel, spec: &TableSpec) -> Store {
    let mut store: Store = BTreeMap::new();
    for s in &sch.symbols {
        let entry = store.entry(s.id.clone()).or_default();
        for c in &spec.columns {
            if c.name != REFERENCE && c.name != QUANTITY && c.name != ITEM_NUMBER {
                entry.insert(c.name.clone(), field_value(sch, model, s, &c.name));
            }
        }
    }
    store
}

/// `RebuildRows` + `Sort`, then every row's resolved cell text.
pub fn build_table(sch: &SchematicSection, model: &ConstraintModel, spec: &TableSpec) -> Table {
    let m = Model { spec, store: build_store(sch, model, spec) };
    let mut rows: Vec<RawRow> = Vec::new();
    'sym: for sr in sym_refs(sch) {
        if !filter_matches(&spec.filter, &sr.r.full()) {
            continue;
        }
        let multi_unit = sch.symbols.iter().filter(|s| s.id == sr.id).count() > 1;
        if !spec.group_symbols && !multi_unit {
            rows.push(RawRow { refs: vec![sr], group: false });
            continue;
        }
        for row in rows.iter_mut() {
            let row_ref = row.refs[0].clone();
            if unit_match(&sr, &row_ref) {
                row.refs.push(sr);
                continue 'sym;
            } else if spec.group_symbols && m.group_match(&sr, &row_ref) {
                row.refs.push(sr);
                row.group = true;
                continue 'sym;
            }
        }
        rows.push(RawRow { refs: vec![sr], group: false });
    }

    // Sort(): refs inside each row first, then rows by the sort column.
    for row in rows.iter_mut() {
        row.refs.sort_by(|a, b| str_num_cmp(&a.r.full(), &b.r.full(), true));
    }
    let sort_col = m.col_of(&spec.sort_field).unwrap_or(0);
    let key = |row: &RawRow| row_value(&m, &row.refs, sort_col, ", ", "-", false, 0, false).0.trim().to_string();
    let is_ref_col = spec.columns.get(sort_col).is_some_and(|c| c.name == REFERENCE);
    rows.sort_by(|a, b| {
        let by_ref = || str_num_cmp(&a.refs[0].r.full(), &b.refs[0].r.full(), true);
        let ord = if is_ref_col {
            by_ref()
        } else {
            let (l, r) = (key(a), key(b));
            if l == r {
                by_ref()
            } else {
                value_string_compare(&l, &r)
            }
        };
        if spec.sort_asc {
            ord
        } else {
            ord.reverse()
        }
    });

    let cells_of = |refs: &[SymRef], item: usize, child: bool| -> (Vec<String>, Vec<bool>) {
        let mut cells = Vec::new();
        let mut mixed = Vec::new();
        for col in 0..spec.columns.len() {
            let (t, mx) = row_value(&m, refs, col, ", ", "-", false, item, child);
            cells.push(t);
            mixed.push(mx);
        }
        (cells, mixed)
    };
    let distinct = |refs: &[SymRef]| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for s in refs {
            if !v.contains(&s.id) {
                v.push(s.id.clone());
            }
        }
        v
    };
    let rows = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let item = i + 1;
            let (cells, mixed) = cells_of(&row.refs, item, false);
            let children = if row.group {
                let mut seen: Vec<&str> = Vec::new();
                row.refs
                    .iter()
                    .filter(|s| {
                        let new = !seen.contains(&s.id.as_str());
                        seen.push(&s.id);
                        new
                    })
                    .map(|s| {
                        let one: Vec<SymRef> = row.refs.iter().filter(|x| x.id == s.id).cloned().collect();
                        let (cells, mixed) = cells_of(&one, item, true);
                        TableRow { refs: vec![s.id.clone()], flag: RowFlag::Child, item_number: item, cells, mixed, children: vec![] }
                    })
                    .collect()
            } else {
                vec![]
            };
            TableRow { refs: distinct(&row.refs), flag: if row.group { RowFlag::Group } else { RowFlag::Singleton }, item_number: item, cells, mixed, children }
        })
        .collect();
    Table { rows }
}

// ---------------------------------------------------------------------
// BOM export
// ---------------------------------------------------------------------

/// `BOM_FMT_PRESET`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BomFmt {
    #[serde(default)]
    pub name: String,
    pub field_delimiter: String,
    pub string_delimiter: String,
    pub ref_delimiter: String,
    pub ref_range_delimiter: String,
    #[serde(default)]
    pub keep_tabs: bool,
    #[serde(default)]
    pub keep_line_breaks: bool,
}

impl BomFmt {
    /// `BOM_FMT_PRESET::CSV`.
    pub fn csv() -> BomFmt {
        BomFmt {
            name: "CSV".into(),
            field_delimiter: ",".into(),
            string_delimiter: "\"".into(),
            ref_delimiter: ",".into(),
            ref_range_delimiter: String::new(),
            keep_tabs: false,
            keep_line_breaks: false,
        }
    }
    /// `BOM_FMT_PRESET::TSV`.
    pub fn tsv() -> BomFmt {
        BomFmt {
            name: "TSV".into(),
            field_delimiter: "\t".into(),
            string_delimiter: String::new(),
            ref_delimiter: ",".into(),
            ref_range_delimiter: String::new(),
            keep_tabs: false,
            keep_line_breaks: false,
        }
    }
    /// `BOM_FMT_PRESET::Semicolons`.
    pub fn semicolons() -> BomFmt {
        BomFmt {
            name: "Semicolons".into(),
            field_delimiter: ";".into(),
            string_delimiter: "'".into(),
            ref_delimiter: ",".into(),
            ref_range_delimiter: String::new(),
            keep_tabs: false,
            keep_line_breaks: false,
        }
    }
    /// `BOM_FMT_PRESET::BuiltInPresets`.
    pub fn built_in() -> Vec<BomFmt> {
        vec![BomFmt::csv(), BomFmt::tsv(), BomFmt::semicolons()]
    }
    pub fn preset(name: &str) -> Option<BomFmt> {
        BomFmt::built_in().into_iter().find(|p| p.name == name)
    }
}

/// `FIELDS_EDITOR_GRID_DATA_MODEL::Export`: header row of shown column
/// labels, then one line per non-child row, each field wrapped in
/// `string_delimiter` (embedded delimiters doubled) and the line ended
/// with `\n` after the last shown column. References use
/// `ref_delimiter`/`ref_range_delimiter` and mixed values are listed
/// comma-separated (`GetExportValue`, `listMixedValues = true`).
pub fn export_bom(sch: &SchematicSection, model: &ConstraintModel, spec: &TableSpec, fmt: &BomFmt) -> String {
    let Some(last_col) = spec.columns.iter().rposition(|c| c.show) else { return String::new() };
    let format_field = |mut field: String, last: bool| -> String {
        if !fmt.keep_line_breaks {
            field = field.replace(['\r', '\n'], "");
        }
        if !fmt.keep_tabs {
            field = field.replace('\t', "");
        }
        if !fmt.string_delimiter.is_empty() {
            field = field.replace(&fmt.string_delimiter, &format!("{0}{0}", fmt.string_delimiter));
        }
        format!("{0}{1}{0}{2}", fmt.string_delimiter, field, if last { "\n" } else { fmt.field_delimiter.as_str() })
    };

    let mut out = String::new();
    for (i, c) in spec.columns.iter().enumerate() {
        if c.show {
            out.push_str(&format_field(if c.label.is_empty() { c.name.clone() } else { c.label.clone() }, i == last_col));
        }
    }

    // Rebuild the rows without the dialog's expand state (children are
    // never exported), but with export-style reference/mixed formatting.
    let table = build_table(sch, model, spec);
    let mut spec_rows = Vec::new();
    for row in &table.rows {
        spec_rows.push(row.refs.clone());
    }
    let m = Model { spec, store: build_store(sch, model, spec) };
    let all = sym_refs(sch);
    for (idx, ids) in spec_rows.iter().enumerate() {
        let refs: Vec<SymRef> = all.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
        for (i, c) in spec.columns.iter().enumerate() {
            if !c.show {
                continue;
            }
            let (text, _) = row_value(&m, &refs, i, &fmt.ref_delimiter, &fmt.ref_range_delimiter, true, idx + 1, false);
            out.push_str(&format_field(text, i == last_col));
        }
    }
    out
}

// ---------------------------------------------------------------------
// Applying edits (ApplyData)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEdit {
    /// Reference designator of the symbol to edit (all its units).
    pub id: String,
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldRename {
    pub from: String,
    pub to: String,
}

/// The whole "Apply" of the dialog as one value: column removals
/// (`RemoveColumn`), renames (`RenameColumn`), additions (`AddColumn`,
/// `userAdded`) and cell edits (`SetValue`), applied in that order, with
/// edits addressed by the *final* field names -- the same "data store ->
/// symbol" order `ApplyData` effectively has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChanges {
    #[serde(default)]
    pub edits: Vec<FieldEdit>,
    #[serde(default)]
    pub add_fields: Vec<String>,
    #[serde(default)]
    pub rename_fields: Vec<FieldRename>,
    #[serde(default)]
    pub remove_fields: Vec<String>,
}

/// Apply `changes` to `sch`. All validation happens before any mutation, so
/// an `Err` leaves `sch` untouched (the batched verb is atomic).
/// `ApplyData`'s own rules carried over: Reference is not editable,
/// generated (`${...}`) fields are skipped as read-only, mandatory
/// fields (Value/Footprint/Datasheet) are set on the instance, user fields
/// live in `SchematicSection::user_fields`, and an empty user value is kept
/// (an `userAdded` column creates the field even when empty) rather than
/// dropped.
pub fn apply_field_changes(sch: &mut SchematicSection, changes: &FieldChanges) -> Result<(), String> {
    let known_ids: Vec<&str> = sch.symbols.iter().map(|s| s.id.as_str()).collect();
    for e in &changes.edits {
        if e.field == REFERENCE {
            return Err("the Reference field is not editable from the fields table (use Annotate / the symbol's Rename)".into());
        }
        if is_generated_field(&e.field) {
            return Err(format!("{} is a generated field and is read-only", e.field));
        }
        if e.field.trim().is_empty() {
            return Err("a field needs a name".into());
        }
        if !known_ids.contains(&e.id.as_str()) {
            return Err(format!("no symbol with reference '{}'", e.id));
        }
    }
    let mut names: Vec<String> = user_field_names(sch);
    for r in &changes.remove_fields {
        if is_mandatory(r) || is_generated_field(r) {
            return Err(format!("the {r} field cannot be removed"));
        }
        names.retain(|n| n != r);
    }
    for r in &changes.rename_fields {
        if is_mandatory(&r.from) || is_mandatory(&r.to) || is_generated_field(&r.from) || is_generated_field(&r.to) {
            return Err("mandatory and generated fields cannot be renamed or used as a new name".into());
        }
        if r.to.trim().is_empty() {
            return Err("a field needs a name".into());
        }
        if r.from != r.to && names.iter().any(|n| n.eq_ignore_ascii_case(&r.to)) {
            return Err(format!("a field named '{}' already exists", r.to));
        }
        if let Some(p) = names.iter().position(|n| *n == r.from) {
            names[p] = r.to.clone();
        }
    }
    for a in &changes.add_fields {
        if a.trim().is_empty() || is_mandatory(a) || is_generated_field(a) {
            return Err(format!("'{a}' is not a valid new field name"));
        }
        if names.iter().any(|n| n == a) {
            continue;
        }
        names.push(a.clone());
    }

    // Validated: mutate.
    for r in &changes.remove_fields {
        for m in sch.user_fields.values_mut() {
            m.remove(r);
        }
    }
    for r in &changes.rename_fields {
        for m in sch.user_fields.values_mut() {
            if let Some(v) = m.remove(&r.from) {
                m.insert(r.to.clone(), v);
            }
        }
    }
    let ids: Vec<String> = {
        let mut v: Vec<String> = sch.symbols.iter().map(|s| s.id.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    for a in &changes.add_fields {
        for id in &ids {
            sch.user_fields.entry(id.clone()).or_default().entry(a.clone()).or_default();
        }
    }
    for e in &changes.edits {
        match e.field.as_str() {
            VALUE | FOOTPRINT | DATASHEET => {
                for s in sch.symbols.iter_mut().filter(|s| s.id == e.id) {
                    match e.field.as_str() {
                        VALUE => s.value = e.value.clone(),
                        FOOTPRINT => s.footprint = e.value.clone(),
                        _ => s.datasheet = e.value.clone(),
                    }
                }
            }
            other => {
                sch.user_fields.entry(e.id.clone()).or_default().insert(other.to_string(), e.value.clone());
            }
        }
    }
    sch.user_fields.retain(|_, m| !m.is_empty());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Point;

    fn r(s: &str) -> Ref {
        Ref::parse(s)
    }
    fn list(v: &[&str]) -> Vec<Ref> {
        v.iter().map(|s| r(s)).collect()
    }

    #[test]
    fn shorthand_collapses_runs_of_three_or_more() {
        // SCH_REFERENCE_LIST::Shorthand with the grid's own ", " / "-".
        assert_eq!(shorthand(&list(&["R1", "R2", "R3", "R5"]), ", ", "-"), "R1-R3, R5");
        // The BOM preset's "," delimiter.
        assert_eq!(shorthand(&list(&["R1", "R2", "R3", "R5"]), ",", "-"), "R1-R3,R5");
    }

    #[test]
    fn shorthand_lists_a_pair_and_a_single_without_a_range() {
        assert_eq!(shorthand(&list(&["R1", "R2"]), ", ", "-"), "R1, R2");
        assert_eq!(shorthand(&list(&["C7"]), ", ", "-"), "C7");
        assert_eq!(shorthand(&list(&["R1", "R3", "R4", "R5", "C1", "C2"]), ", ", "-"), "R1, R3-R5, C1, C2");
    }

    #[test]
    fn shorthand_with_empty_range_delimiter_never_ranges() {
        // `refRangeDelimiter.IsEmpty()` -> every ref listed (CSV preset).
        assert_eq!(shorthand(&list(&["R1", "R2", "R3", "R4"]), ",", ""), "R1,R2,R3,R4");
    }

    #[test]
    fn shorthand_needs_the_same_prefix_and_consecutive_numbers() {
        assert_eq!(shorthand(&list(&["C1", "R2", "R3", "R4"]), ", ", "-"), "C1, R2-R4");
        assert_eq!(shorthand(&list(&["R9", "R10", "R11"]), ", ", "-"), "R9-R11");
    }

    #[test]
    fn ref_parse_matches_sch_reference_split() {
        assert_eq!(r("R12"), Ref { prefix: "R".into(), num: Some(12) });
        assert_eq!(r("R?"), Ref { prefix: "R".into(), num: None });
        assert_eq!(r("U1").full(), "U1");
        assert_eq!(r("J").number_text(), "?");
    }

    #[test]
    fn str_num_cmp_is_natural() {
        use std::cmp::Ordering::*;
        assert_eq!(str_num_cmp("R2", "R10", true), Less);
        assert_eq!(str_num_cmp("r2", "R2", true), Equal);
        assert_eq!(str_num_cmp("C1", "R1", true), Less);
    }

    #[test]
    fn value_compare_understands_si_suffixes() {
        use std::cmp::Ordering::*;
        assert_eq!(value_string_compare("100n", "1u"), Less);
        assert_eq!(value_string_compare("4k7", "4.7k"), Equal);
        assert_eq!(value_string_compare("10k", "2k2"), Greater);
    }

    fn sym(id: &str, value: &str, fp: &str, unit: u32) -> SymbolInstance {
        SymbolInstance {
            id: id.into(),
            at: Point { x: 0, y: 0 },
            rot: 0,
            mirrored: false,
            mirror_y: false,
            lib_id: String::new(),
            unit,
            value: value.into(),
            footprint: fp.into(),
            datasheet: String::new(),
        }
    }

    fn sch(symbols: Vec<SymbolInstance>) -> SchematicSection {
        SchematicSection {
            symbols,
            wires: vec![],
            labels: vec![],
            texts: vec![],
            power_symbols: vec![],
            no_connects: vec![],
            bus_entries: vec![],
            erc_exclusions: vec![],
            erc_pin_map: None,
            user_fields: Default::default(),
            title_block: None,
            sheets: vec![],
            instance_overrides: vec![], junctions: vec![], lines: vec![], extras: Default::default(),
            imported_from_kicad: false,
        }
    }

    fn demo() -> SchematicSection {
        sch(vec![
            sym("R1", "10k", "R_0603", 1),
            sym("R2", "10k", "R_0603", 1),
            sym("R3", "10k", "R_0603", 1),
            sym("R5", "10k", "R_0603", 1),
            sym("R4", "1k", "R_0603", 1),
            sym("C1", "100n", "C_0603", 1),
        ])
    }

    #[test]
    fn grouping_by_value_and_footprint_collapses_refs() {
        let model = ConstraintModel::default();
        let t = build_table(&demo(), &model, &TableSpec::grouped_by_value_footprint());
        // C1 | R4 | R1-R3,R5 sorted by first reference: C1, R1.., R4.
        let refs: Vec<&str> = t.rows.iter().map(|r| r.cells[0].as_str()).collect();
        assert_eq!(refs, vec!["C1", "R1-R3, R5", "R4"]);
        let qty: Vec<&str> = t.rows.iter().map(|r| r.cells[4].as_str()).collect();
        assert_eq!(qty, vec!["1", "4", "1"]);
        assert_eq!(t.rows[1].flag, RowFlag::Group);
        assert_eq!(t.rows[1].children.len(), 4);
        assert_eq!(t.rows[0].flag, RowFlag::Singleton);
    }

    #[test]
    fn ungrouped_table_is_one_row_per_symbol_and_multi_unit_merges() {
        let model = ConstraintModel::default();
        let mut spec = TableSpec::grouped_by_value_footprint();
        spec.group_symbols = false;
        let mut s = demo();
        s.symbols.push(sym("U1", "OPA", "SOIC", 1));
        s.symbols.push(sym("U1", "OPA", "SOIC", 2));
        let t = build_table(&s, &model, &spec);
        assert_eq!(t.rows.len(), 7, "6 single symbols + U1's two units as one row");
        let u1 = t.rows.iter().find(|r| r.cells[0] == "U1").unwrap();
        assert_eq!(u1.cells[4], "1", "two units of one symbol count once");
    }

    #[test]
    fn a_group_with_differing_non_grouped_cells_shows_mixed_values() {
        let model = ConstraintModel::default();
        let mut s = demo();
        s.symbols[0].datasheet = "a.pdf".into();
        let t = build_table(&s, &model, &TableSpec::grouped_by_value_footprint());
        let row = t.rows.iter().find(|r| r.cells[0].starts_with("R1")).unwrap();
        assert_eq!(row.cells[2], INDETERMINATE);
        assert!(row.mixed[2]);
    }

    #[test]
    fn export_csv_matches_kicad_format() {
        let model = ConstraintModel::default();
        let out = export_bom(&demo(), &model, &TableSpec::grouped_by_value_footprint(), &BomFmt::csv());
        let expected = "\"Reference\",\"Value\",\"Datasheet\",\"Footprint\",\"Qty\"\n\
                        \"C1\",\"100n\",\"\",\"C_0603\",\"1\"\n\
                        \"R1,R2,R3,R5\",\"10k\",\"\",\"R_0603\",\"4\"\n\
                        \"R4\",\"1k\",\"\",\"R_0603\",\"1\"\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn export_tsv_has_no_string_delimiter_and_ref_ranges_when_asked() {
        let model = ConstraintModel::default();
        let mut fmt = BomFmt::tsv();
        fmt.ref_range_delimiter = "-".into();
        let out = export_bom(&demo(), &model, &TableSpec::grouped_by_value_footprint(), &fmt);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "Reference\tValue\tDatasheet\tFootprint\tQty");
        assert_eq!(lines[2], "R1-R3,R5\t10k\t\tR_0603\t4");
    }

    #[test]
    fn export_doubles_string_delimiters_and_strips_tabs_and_breaks() {
        let model = ConstraintModel::default();
        let mut s = sch(vec![sym("R1", "a\"b\tc\nd", "", 1)]);
        s.user_fields.clear();
        let mut spec = TableSpec::grouped_by_value_footprint();
        spec.columns.retain(|c| c.name == REFERENCE || c.name == VALUE);
        let out = export_bom(&s, &model, &spec, &BomFmt::csv());
        assert_eq!(out, "\"Reference\",\"Value\"\n\"R1\",\"a\"\"bcd\"\n");
        let mut keep = BomFmt::csv();
        keep.keep_tabs = true;
        keep.keep_line_breaks = true;
        let out = export_bom(&s, &model, &spec, &keep);
        assert!(out.contains("\"a\"\"b\tc\nd\""), "{out:?}");
    }

    #[test]
    fn hidden_columns_are_not_exported() {
        let model = ConstraintModel::default();
        let mut spec = TableSpec::grouped_by_value_footprint();
        for c in spec.columns.iter_mut() {
            c.show = c.name == REFERENCE || c.name == QUANTITY;
        }
        let out = export_bom(&demo(), &model, &spec, &BomFmt::csv());
        assert!(out.starts_with("\"Reference\",\"Qty\"\n"));
        assert_eq!(out.lines().count(), 4);
    }

    #[test]
    fn apply_changes_sets_fields_adds_renames_and_removes() {
        let mut s = demo();
        apply_field_changes(
            &mut s,
            &FieldChanges {
                add_fields: vec!["MPN".into(), "Tmp".into()],
                edits: vec![
                    FieldEdit { id: "R1".into(), field: "MPN".into(), value: "ERJ-3".into() },
                    FieldEdit { id: "R1".into(), field: "Value".into(), value: "22k".into() },
                    FieldEdit { id: "C1".into(), field: "Datasheet".into(), value: "c.pdf".into() },
                ],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s.symbols[0].value, "22k");
        assert_eq!(s.symbols.iter().find(|x| x.id == "C1").unwrap().datasheet, "c.pdf");
        assert_eq!(s.user_fields["R1"]["MPN"], "ERJ-3");
        assert_eq!(s.user_fields["R2"]["MPN"], "", "an added column creates the (empty) field on every symbol");

        apply_field_changes(&mut s, &FieldChanges { rename_fields: vec![FieldRename { from: "MPN".into(), to: "Part Number".into() }], remove_fields: vec!["Tmp".into()], ..Default::default() })
            .unwrap();
        assert_eq!(s.user_fields["R1"]["Part Number"], "ERJ-3");
        assert!(!s.user_fields["R1"].contains_key("MPN") && !s.user_fields["R1"].contains_key("Tmp"));
    }

    #[test]
    fn apply_changes_is_atomic_on_error() {
        let mut s = demo();
        let before = s.clone();
        let bad = FieldChanges {
            edits: vec![FieldEdit { id: "R1".into(), field: "Value".into(), value: "1".into() }, FieldEdit { id: "R404".into(), field: "Value".into(), value: "2".into() }],
            ..Default::default()
        };
        assert!(apply_field_changes(&mut s, &bad).is_err());
        assert_eq!(s.symbols[0].value, before.symbols[0].value, "nothing applied");
        assert!(apply_field_changes(&mut s, &FieldChanges { edits: vec![FieldEdit { id: "R1".into(), field: "Reference".into(), value: "X".into() }], ..Default::default() }).is_err());
        assert!(apply_field_changes(&mut s, &FieldChanges { remove_fields: vec!["Value".into()], ..Default::default() }).is_err());
    }
}
