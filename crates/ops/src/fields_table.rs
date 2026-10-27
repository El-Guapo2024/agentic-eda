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
//!  * The attribute columns (`isAttribute`, `getAttributeValue`,
//!    `setAttributeValue`): `${DNP}`, `${EXCLUDE_FROM_BOM}`,
//!    `${EXCLUDE_FROM_BOARD}` and `${EXCLUDE_FROM_SIM}` read and write
//!    `SymbolInstance::{dnp, exclude_from_bom, exclude_from_board,
//!    exclude_from_sim}` as "1" / "0", and the two filters of the dialog's
//!    options menu (`SetExcludeDNP`, `SetIncludeExcludedFromBOM`,
//!    `RebuildRows`): [`TableSpec::exclude_dnp`] and
//!    [`TableSpec::include_excluded_from_bom`]. The built-in presets carry
//!    the columns as `BOM_PRESET::BuiltInPresets` does.
//!
//! Scope limits (also listed in `web/studio/PARITY-sch.md`): our IR has no
//! field visibility, no design variants and no scope selector. The table
//! covers the whole design -- every sheet ([`project_view`]).

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
/// The attribute columns, as `FIELDS_EDITOR_GRID_DATA_MODEL::isAttribute` names them: a symbol's flags as checkbox cells.
pub const DNP: &str = "${DNP}";
pub const EXCLUDE_FROM_BOARD: &str = "${EXCLUDE_FROM_BOARD}";
pub const EXCLUDE_FROM_BOM: &str = "${EXCLUDE_FROM_BOM}";
pub const EXCLUDE_FROM_SIM: &str = "${EXCLUDE_FROM_SIM}";

/// `FIELDS_EDITOR_GRID_DATA_MODEL::isAttribute`.
pub fn is_attribute(name: &str) -> bool {
    matches!(name, DNP | EXCLUDE_FROM_BOARD | EXCLUDE_FROM_BOM | EXCLUDE_FROM_SIM)
}

/// `getAttributeValue`: "1" when the symbol has the attribute, "0" when it has not.
fn attribute_value(sym: &SymbolInstance, name: &str) -> &'static str {
    let on = match name {
        DNP => sym.dnp,
        EXCLUDE_FROM_BOARD => sym.exclude_from_board,
        EXCLUDE_FROM_BOM => sym.exclude_from_bom,
        EXCLUDE_FROM_SIM => sym.exclude_from_sim,
        _ => false,
    };
    if on {
        "1"
    } else {
        "0"
    }
}

/// `setAttributeValue`: "1" turns the attribute on, any other value off (`newValue = aValue == wxS( "1" )`).
fn set_attribute(sym: &mut SymbolInstance, name: &str, value: &str) {
    let on = value == "1";
    match name {
        DNP => sym.dnp = on,
        EXCLUDE_FROM_BOARD => sym.exclude_from_board = on,
        EXCLUDE_FROM_BOM => sym.exclude_from_bom = on,
        EXCLUDE_FROM_SIM => sym.exclude_from_sim = on,
        _ => {}
    }
}

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
    /// `m_numRefStr`: the number as the reference writes it (`R01` keeps `01`), which `GetRefNumber()` returns and `unitMatch` compares (r5 C5-L8).
    pub num_text: String,
}

impl Ref {
    /// `SCH_REFERENCE::Split`.
    pub fn parse(text: &str) -> Ref {
        let t = text.trim_end_matches('?');
        let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || t.len() != text.len() {
            return Ref { prefix: t.to_string(), num: None, num_text: String::new() };
        }
        let split = t.len() - digits;
        Ref { prefix: t[..split].trim_end().to_string(), num: t[split..].parse().ok(), num_text: t[split..].to_string() }
    }
    /// `GetRefNumber()`: `m_numRefStr`, or `?` for an unannotated reference.
    pub fn number_text(&self) -> String {
        if self.num.is_none() {
            "?".to_string()
        } else {
            self.num_text.clone()
        }
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
    /// `BOM_PRESET::excludeDNP`: symbols marked Do not populate are left out of the table, and of an export ("Include 'DNP' Symbols" off).
    #[serde(default)]
    pub exclude_dnp: bool,
    /// `BOM_PRESET::includeExcludedFromBOM`: symbols marked Exclude from BOM are listed ("Include 'Exclude from BOM' Symbols" on). They are never in an export.
    #[serde(default)]
    pub include_excluded_from_bom: bool,
}

impl TableSpec {
    /// `BOM_PRESET::GroupedByValueFootprint` (labels as KiCad shows them): the dialog's "Grouped By Value and Footprint" view, `${DNP}` last and grouped on, so a part
    /// marked Do not populate is its own row; the "Exclude from BOM" symbols are not listed.
    pub fn grouped_by_value_footprint() -> TableSpec {
        let col = |name: &str, label: &str, group_by: bool| Column { name: name.into(), label: label.into(), show: true, group_by };
        TableSpec {
            columns: vec![col(REFERENCE, "Reference", false), col(VALUE, "Value", true), col(DATASHEET, "Datasheet", false), col(FOOTPRINT, "Footprint", true), col(QUANTITY, "Qty", false), col(DNP, "DNP", true)],
            group_symbols: true,
            sort_field: REFERENCE.into(),
            sort_asc: true,
            filter: String::new(),
            exclude_dnp: false,
            include_excluded_from_bom: false,
        }
    }

    /// `BOM_PRESET::GroupedByValue`.
    pub fn grouped_by_value() -> TableSpec {
        let col = |name: &str, label: &str, group_by: bool| Column { name: name.into(), label: label.into(), show: true, group_by };
        TableSpec {
            columns: vec![col(REFERENCE, "Reference", false), col(VALUE, "Value", true), col(DATASHEET, "Datasheet", false), col(FOOTPRINT, "Footprint", false), col(QUANTITY, "Qty", false), col(DNP, "DNP", true)],
            group_symbols: true,
            sort_field: REFERENCE.into(),
            sort_asc: true,
            filter: String::new(),
            exclude_dnp: false,
            include_excluded_from_bom: false,
        }
    }

    /// `BOM_PRESET::DefaultEditing`: the dialog's own initial view -- every attribute that decides whether a part is on the BOM and the board is a column and a "Group By",
    /// and the symbols excluded from the BOM are listed (they can be edited here). `${EXCLUDE_FROM_SIM}` is a column that is not shown until it is asked for (the
    /// "Attributes" preset shows it).
    pub fn default_editing() -> TableSpec {
        let col = |name: &str, label: &str, group_by: bool| Column { name: name.into(), label: label.into(), show: true, group_by };
        let mut sim = col(EXCLUDE_FROM_SIM, "Exclude from Simulation", false);
        sim.show = false;
        TableSpec {
            columns: vec![
                col(REFERENCE, "Reference", false),
                col(QUANTITY, "Qty", false),
                col(VALUE, "Value", true),
                col(DNP, "DNP", true),
                col(EXCLUDE_FROM_BOM, "Exclude from BOM", true),
                col(EXCLUDE_FROM_BOARD, "Exclude from Board", true),
                col(FOOTPRINT, "Footprint", true),
                col(DATASHEET, "Datasheet", false),
                sim,
            ],
            group_symbols: true,
            sort_field: REFERENCE.into(),
            sort_asc: true,
            filter: String::new(),
            exclude_dnp: false,
            include_excluded_from_bom: true,
        }
    }

    /// `BOM_PRESET::Attributes`: the reference, the value and the four attributes, each a column of its own and none a "Group By" but the value (a column the preset does not
    /// show stays in the list, hidden).
    pub fn attributes() -> TableSpec {
        let col = |name: &str, label: &str, show: bool, group_by: bool| Column { name: name.into(), label: label.into(), show, group_by };
        TableSpec {
            columns: vec![
                col(REFERENCE, "Reference", true, false),
                col(VALUE, "Value", true, true),
                col(DATASHEET, "Datasheet", false, false),
                col(FOOTPRINT, "Footprint", false, true),
                col(DNP, "Do Not Place", true, false),
                col(EXCLUDE_FROM_BOM, "Exclude from BOM", true, false),
                col(EXCLUDE_FROM_BOARD, "Exclude from Board", true, false),
                col(EXCLUDE_FROM_SIM, "Exclude from Simulation", true, false),
            ],
            group_symbols: true,
            sort_field: REFERENCE.into(),
            sort_asc: true,
            filter: String::new(),
            exclude_dnp: false,
            include_excluded_from_bom: true,
        }
    }

    /// `BOM_PRESET::BuiltInPresets`, by KiCad's names.
    pub fn built_in() -> Vec<(&'static str, TableSpec)> {
        vec![("Default Editing", TableSpec::default_editing()), ("Grouped By Value", TableSpec::grouped_by_value()), ("Grouped By Value and Footprint", TableSpec::grouped_by_value_footprint()), ("Attributes", TableSpec::attributes())]
    }

    /// The columns of the built-in preset `name` applied to this view the way `FIELDS_EDITOR_GRID_DATA_MODEL::ApplyBomPreset` does: the preset's columns come first, in its
    /// order (a column this view has that the preset has not, a user field, stays after them hidden: "BOM Presets can add, but not remove, columns"), and its grouping,
    /// sort, filter and the two filters replace this view's.
    pub fn with_preset(&self, name: &str) -> Option<TableSpec> {
        let (_, preset) = TableSpec::built_in().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
        let mut out = preset;
        for c in &self.columns {
            if !out.columns.iter().any(|o| o.name == c.name) {
                out.columns.push(Column { show: false, group_by: false, ..c.clone() });
            }
        }
        Some(out)
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
    /// Where the symbol is in the section's list: the filters read the attributes the sheet has (`ref.GetSymbol()->ResolveDNP`), not the staged ones.
    index: usize,
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
        attr if is_attribute(attr) => attribute_value(sym, attr).to_string(),
        other => sch.user_fields.get(&sym.id).and_then(|m| m.get(other)).cloned().unwrap_or_default(),
    }
}

/// The symbols of the whole design as one section, for the table to read: the root sheet's, then every other sheet screen's, then one more row for
/// each symbol of a screen shown on a later sheet path under a reference of its own. KiCad's dialog lists a reference per sheet path
/// (`Hierarchy().GetSymbols( m_symbolsList, .. )`, where `SCH_SHEET_PATH::AppendSymbol` makes one `SCH_REFERENCE( aSymbol, *this )` for each path,
/// `GetRef( this )`), so a screen placed twice counts twice in the grid, in `${QUANTITY}` and in the export, as in kicad-cli's BOM (r5 C5-M3). The
/// extra row is the screen's symbol under the path's reference ([`eda_model::sheet_list::reference_under`]) and unit, with the symbol's fields; an
/// edit to it is an edit to that symbol ([`path_aliases`]). A path whose reference is the symbol's own id adds nothing (the screen's row stands
/// for it, and two units of one reference are one row anyway).
pub fn project_view(design: &eda_model::ir::Design) -> Option<SchematicSection> {
    let mut sch = design.schematic.clone()?;
    for screen in design.sheet_contents.iter().flat_map(|c| c.values()) {
        sch.symbols.extend(screen.symbols.iter().cloned());
        for (reference, fields) in &screen.user_fields {
            sch.user_fields.entry(reference.clone()).or_default().extend(fields.clone());
        }
    }
    for (alias, screen, sym, unit) in path_rows(design) {
        if sch.symbols.iter().any(|s| s.id == alias && s.unit == unit) {
            continue;
        }
        let mut copy = sym.clone();
        copy.id = alias.clone();
        copy.unit = unit;
        sch.symbols.push(copy);
        if let Some(fields) = screen.user_fields.get(&sym.id) {
            sch.user_fields.entry(alias).or_default().extend(fields.clone());
        }
    }
    Some(sch)
}

/// The placed symbols a later sheet path shows under a reference other than the symbol's id: `(reference, screen, symbol, unit)`, in the hierarchy's order.
fn path_rows(design: &eda_model::ir::Design) -> Vec<(String, &SchematicSection, &SymbolInstance, u32)> {
    use eda_model::sheet_list;
    let mut out = Vec::new();
    for entry in sheet_list::walk(design).into_iter().skip(1) {
        let Some(screen) = sheet_list::screen_of(design, &entry.path) else { continue };
        for sym in &screen.symbols {
            let reference = sheet_list::reference_under(screen, sym, &entry.path);
            if reference != sym.id {
                out.push((reference, screen, sym, sheet_list::unit_under(screen, sym, &entry.path)));
            }
        }
    }
    out
}

/// The references [`project_view`] lists for a later sheet path, each with the id of the screen's symbol it shows: a field is the symbol's, whichever
/// path's row it is edited on.
pub fn path_aliases(design: &eda_model::ir::Design) -> BTreeMap<String, String> {
    let ids: std::collections::BTreeSet<&str> = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).flat_map(|s| s.symbols.iter().map(|x| x.id.as_str())).collect();
    path_rows(design).into_iter().filter(|(alias, ..)| !ids.contains(alias.as_str())).map(|(alias, _, sym, _)| (alias, sym.id.clone())).collect()
}

/// `changes` for the table over [`project_view`]: an edit to a symbol goes to every row that shows it (the symbol's own and its later paths'), as a
/// field is the symbol's on every path.
pub fn changes_for_view(design: &eda_model::ir::Design, changes: &FieldChanges) -> FieldChanges {
    let aliases = path_aliases(design);
    let mut out = changes.clone();
    out.edits.clear();
    for e in &changes.edits {
        let own = aliases.get(&e.id).cloned().unwrap_or_else(|| e.id.clone());
        let mut push = |id: &str| {
            if !out.edits.iter().any(|x| x.id == id && x.field == e.field) {
                out.edits.push(FieldEdit { id: id.to_string(), ..e.clone() });
            }
        };
        push(&own);
        for (alias, of) in &aliases {
            if *of == own {
                push(alias);
            }
        }
    }
    out
}

/// `changes` for the sheets: an edit made on a later path's row is an edit to the screen's symbol.
pub fn changes_for_sheets(design: &eda_model::ir::Design, edits: &[FieldEdit]) -> Vec<FieldEdit> {
    let aliases = path_aliases(design);
    let mut out: Vec<FieldEdit> = Vec::new();
    for e in edits {
        let id = aliases.get(&e.id).cloned().unwrap_or_else(|| e.id.clone());
        out.retain(|x| !(x.id == id && x.field == e.field));
        out.push(FieldEdit { id, ..e.clone() });
    }
    out
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

/// `unitMatch`: same reference, annotated -> another unit of one symbol. The numbers compare as text (`GetRefNumber()`), so `R01` and `R1` are two symbols.
fn unit_match(l: &SymRef, r: &SymRef) -> bool {
    l.r.num.is_some() && l.r.prefix == r.r.prefix && l.r.num_text == r.r.num_text
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
    sch.symbols.iter().enumerate().map(|(index, s)| SymRef { id: s.id.clone(), r: Ref::parse(&s.id), index }).collect()
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
    build_table_over(sch, sch, model, spec)
}

/// [`build_table`] for a table whose cells show `sch` (the schematic with the dialog's staged edits laid over it) while the two attribute filters read `base`, the
/// schematic as it stands: KiCad's filters ask the symbol itself (`ResolveDNP`, `ResolveExcludedFromBOM`), and an edit sits in the data store until Apply, so a row
/// someone has just ticked "Exclude from BOM" on does not vanish from under the cursor.
pub fn build_table_over(sch: &SchematicSection, base: &SchematicSection, model: &ConstraintModel, spec: &TableSpec) -> Table {
    let m = Model { spec, store: build_store(sch, model, spec) };
    let mut rows: Vec<RawRow> = Vec::new();
    'sym: for sr in sym_refs(sch) {
        if !filter_matches(&spec.filter, &sr.r.full()) {
            continue;
        }
        // "if( m_excludeDNP ) ... if( isDNP ) continue;" and "if( !m_includeExcluded ) ... if( isExcluded ) continue;"
        if let Some(sym) = base.symbols.get(sr.index) {
            if (spec.exclude_dnp && sym.dnp) || (!spec.include_excluded_from_bom && sym.exclude_from_bom) {
                continue;
            }
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
    export_bom_over(sch, sch, model, spec, fmt)
}

/// [`export_bom`] with the attribute filters read off `base` (see [`build_table_over`]). A symbol marked Exclude from BOM is never exported, whatever the table shows
/// (`PreviewRefresh`: `SetIncludeExcludedFromBOM( false )` before `Export`); one marked Do not populate is left out when the table leaves it out.
pub fn export_bom_over(sch: &SchematicSection, base: &SchematicSection, model: &ConstraintModel, spec: &TableSpec, fmt: &BomFmt) -> String {
    let mut spec = spec.clone();
    spec.include_excluded_from_bom = false;
    let spec = &spec;
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
    let table = build_table_over(sch, base, model, spec);
    let mut spec_rows = Vec::new();
    for row in &table.rows {
        spec_rows.push(row.refs.clone());
    }
    let m = Model { spec, store: resolved_store(sch, model, build_store(sch, model, spec)) };
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

/// `GetExportValue` -> `GetValue( .., resolveVars = true, .. )`: each symbol's cells with their `${..}` expanded through `ref.GetSymbol()->ResolveTextVar`
/// (r5 C5-L9), so the preview says what kicad-cli's BOM file says. Ported for what a symbol answers itself (`SCH_SYMBOL::ResolveTextVar`): a field by its
/// name (case-insensitive), `FOOTPRINT_LIBRARY`, `FOOTPRINT_NAME`, `DNP` and the `EXCLUDE_FROM_*` flags; a sheet, schematic or project variable is left
/// as written (the file from kicad-cli has them).
fn resolved_store(sch: &SchematicSection, model: &ConstraintModel, mut store: Store) -> Store {
    use eda_model::text_vars::{has_text_vars, shown_text};
    for (id, entry) in store.iter_mut() {
        if !entry.values().any(|v| has_text_vars(v)) {
            continue;
        }
        let Some(sym) = sch.symbols.iter().find(|s| &s.id == id) else { continue };
        let mut fields: Vec<(String, String)> = vec![(REFERENCE.into(), sym.id.clone()), (VALUE.into(), field_value(sch, model, sym, VALUE)), (FOOTPRINT.into(), sym.footprint.clone()), (DATASHEET.into(), sym.datasheet.clone())];
        if let Some(user) = sch.user_fields.get(id) {
            fields.extend(user.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        let resolver = |token: &str| -> Option<String> {
            if let Some((_, text)) = fields.iter().find(|(n, _)| n.eq_ignore_ascii_case(token)) {
                return Some(text.clone());
            }
            let flag = |on: bool, text: &str| Some(if on { text.to_string() } else { String::new() });
            match token {
                "FOOTPRINT_LIBRARY" => Some(sym.footprint.split_once(':').map_or("", |(l, _)| l).to_string()),
                "FOOTPRINT_NAME" => Some(sym.footprint.split_once(':').map_or("", |(_, n)| n).to_string()),
                "DNP" => flag(sym.dnp, "DNP"),
                "EXCLUDE_FROM_BOM" => flag(sym.exclude_from_bom, "Excluded from BOM"),
                "EXCLUDE_FROM_BOARD" => flag(sym.exclude_from_board, "Excluded from board"),
                "EXCLUDE_FROM_SIM" => flag(sym.exclude_from_sim, "Excluded from simulation"),
                _ => None,
            }
        };
        for v in entry.values_mut() {
            if has_text_vars(v) {
                *v = shown_text(v, &resolver, 0);
            }
        }
    }
    store
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
        // the attribute columns are generated names too, but a checkbox in them is a cell to edit (`SetValue`: "!IsGeneratedField( ... ) && !ColIsAttribute( aCol )")
        if is_generated_field(&e.field) && !is_attribute(&e.field) {
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
    let ids: Vec<String> = {
        let mut v: Vec<String> = sch.symbols.iter().map(|s| s.id.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    for r in &changes.remove_fields {
        for m in sch.user_fields.values_mut() {
            m.remove(r);
        }
        // the places the field had on the sheet go with it
        crate::sch_fields::drop_field_placements(sch, &ids, r);
    }
    for r in &changes.rename_fields {
        for m in sch.user_fields.values_mut() {
            if let Some(v) = m.remove(&r.from) {
                m.insert(r.to.clone(), v);
            }
        }
        // and a renamed field keeps its places
        crate::sch_fields::rename_field_placements(sch, &ids, &r.from, &r.to);
    }
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
            // `setAttributeValue` on every unit of the symbol ("Attributes bypass the field logic")
            attr if is_attribute(attr) => {
                for s in sch.symbols.iter_mut().filter(|s| s.id == e.id) {
                    set_attribute(s, attr, &e.value);
                }
            }
            // `ApplyData`: "Add a not existing field if it has a value for this symbol" -- `createField = !destField && ( !srcValue.IsEmpty() ||
            // userAdded )`; an empty edit of a field the symbol has not, in a column not added in this dialog, creates nothing (r5 C5-L9)
            other => {
                let exists = sch.user_fields.get(&e.id).is_some_and(|m| m.contains_key(other));
                if exists || !e.value.is_empty() || changes.add_fields.iter().any(|a| a == other) {
                    sch.user_fields.entry(e.id.clone()).or_default().insert(other.to_string(), e.value.clone());
                }
            }
        }
    }
    sch.user_fields.retain(|_, m| !m.is_empty());
    Ok(())
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
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
        assert_eq!(r("R12"), Ref { prefix: "R".into(), num: Some(12), num_text: "12".into() });
        assert_eq!(r("R?"), Ref { prefix: "R".into(), num: None, num_text: String::new() });
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
            at: Point { x: Nm::ZERO, y: Nm::ZERO },
            rot: 0,
            mirrored: false,
            mirror_y: false,
            lib_id: String::new(),
            unit,
            value: value.into(),
            footprint: fp.into(),
            datasheet: String::new(),
            dnp: false,
            exclude_from_bom: false,
            exclude_from_board: false,
            exclude_from_sim: false,
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
            field_layout: Default::default(),
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
        // KiCad's "Grouped By Value and Footprint" ends with the ${DNP} column, "1" for a part marked Do not populate
        let expected = "\"Reference\",\"Value\",\"Datasheet\",\"Footprint\",\"Qty\",\"DNP\"\n\
                        \"C1\",\"100n\",\"\",\"C_0603\",\"1\",\"0\"\n\
                        \"R1,R2,R3,R5\",\"10k\",\"\",\"R_0603\",\"4\",\"0\"\n\
                        \"R4\",\"1k\",\"\",\"R_0603\",\"1\",\"0\"\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn export_tsv_has_no_string_delimiter_and_ref_ranges_when_asked() {
        let model = ConstraintModel::default();
        let mut fmt = BomFmt::tsv();
        fmt.ref_range_delimiter = "-".into();
        let out = export_bom(&demo(), &model, &TableSpec::grouped_by_value_footprint(), &fmt);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "Reference\tValue\tDatasheet\tFootprint\tQty\tDNP");
        assert_eq!(lines[2], "R1-R3,R5\t10k\t\tR_0603\t4\t0");
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

    // ---------------------------------------------------------------------------------------------------------------------- attributes

    /// The demo parts with R2 marked Do not populate, R3 excluded from the BOM and C1 excluded from the board.
    fn flagged() -> SchematicSection {
        let mut s = demo();
        s.symbols[1].dnp = true; // R2
        s.symbols[2].exclude_from_bom = true; // R3
        s.symbols[5].exclude_from_board = true; // C1
        s
    }

    fn col(spec: &TableSpec, name: &str) -> usize {
        spec.columns.iter().position(|c| c.name == name).unwrap_or_else(|| panic!("no column {name}"))
    }

    #[test]
    fn the_attribute_columns_show_a_symbols_flags_as_1_and_0() {
        let model = ConstraintModel::default();
        let mut spec = TableSpec::default_editing();
        spec.group_symbols = false;
        let t = build_table(&flagged(), &model, &spec);
        let row = |r: &str| t.rows.iter().find(|row| row.cells[0] == r).unwrap_or_else(|| panic!("no row {r}"));
        let cell = |r: &str, name: &str| row(r).cells[col(&spec, name)].clone();
        assert_eq!((cell("R1", DNP), cell("R2", DNP)), ("0".to_string(), "1".to_string()));
        assert_eq!((cell("R3", EXCLUDE_FROM_BOM), cell("R1", EXCLUDE_FROM_BOM)), ("1".to_string(), "0".to_string()));
        assert_eq!((cell("C1", EXCLUDE_FROM_BOARD), cell("R1", EXCLUDE_FROM_BOARD)), ("1".to_string(), "0".to_string()));
        assert_eq!(cell("R1", EXCLUDE_FROM_SIM), "0");
        // the default view lists the symbols excluded from the BOM (it is where they are edited)
        assert!(t.rows.iter().any(|r| r.cells[0] == "R3"));
    }

    #[test]
    fn a_group_by_attribute_column_splits_a_group_and_a_plain_one_shows_mixed_values() {
        let model = ConstraintModel::default();
        // "Grouped By Value and Footprint" groups on ${DNP} too: R2 is a row of its own (R3 is excluded from the BOM, which that view does not list)
        let t = build_table(&flagged(), &model, &TableSpec::grouped_by_value_footprint());
        let refs: Vec<&str> = t.rows.iter().map(|r| r.cells[0].as_str()).collect();
        assert_eq!(refs, vec!["C1", "R1, R5", "R2", "R4"]);
        // with ${DNP} not a Group By column the 10k parts are one row, and the column says they differ
        let mut spec = TableSpec::grouped_by_value_footprint();
        spec.include_excluded_from_bom = true;
        let dnp = col(&spec, DNP);
        spec.columns[dnp].group_by = false;
        let t = build_table(&flagged(), &model, &spec);
        let row = t.rows.iter().find(|r| r.cells[0].starts_with("R1")).unwrap();
        assert_eq!(row.cells[dnp], INDETERMINATE);
        assert!(row.mixed[dnp]);
        // a row whose parts agree shows the one value
        let r4 = t.rows.iter().find(|r| r.cells[0] == "R4").unwrap();
        assert_eq!((r4.cells[dnp].as_str(), r4.mixed[dnp]), ("0", false));
    }

    #[test]
    fn exclude_dnp_leaves_the_do_not_populate_symbols_out_and_reads_the_sheet_not_the_staged_edit() {
        let model = ConstraintModel::default();
        let mut spec = TableSpec::default_editing();
        spec.group_symbols = false;
        let listed = |t: &Table| t.rows.iter().map(|r| r.cells[0].clone()).collect::<Vec<_>>();
        assert!(listed(&build_table(&flagged(), &model, &spec)).contains(&"R2".to_string()), "DNP symbols are listed by default");
        spec.exclude_dnp = true;
        let t = build_table(&flagged(), &model, &spec);
        assert!(!listed(&t).contains(&"R2".to_string()), "{:?}", listed(&t));
        assert_eq!(t.rows.len(), 5);
        // an edit that has not been applied does not take a row away: the table shows the staged cell, the filter reads the sheet
        let mut staged = flagged();
        apply_field_changes(&mut staged, &FieldChanges { edits: vec![FieldEdit { id: "R1".into(), field: DNP.into(), value: "1".into() }], ..Default::default() }).unwrap();
        let t = build_table_over(&staged, &flagged(), &model, &spec);
        let r1 = t.rows.iter().find(|r| r.cells[0] == "R1").expect("R1 is still listed: it is not marked on the sheet yet");
        assert_eq!(r1.cells[col(&spec, DNP)], "1", "and its cell shows what was ticked");
    }

    #[test]
    fn symbols_excluded_from_the_bom_are_listed_only_when_asked_and_never_exported() {
        let model = ConstraintModel::default();
        let mut spec = TableSpec::grouped_by_value_footprint();
        spec.group_symbols = false;
        let refs = |t: &Table| t.rows.iter().map(|r| r.cells[0].clone()).collect::<Vec<_>>();
        // KiCad's own default for a view: the excluded ones are hidden
        assert!(!spec.include_excluded_from_bom);
        assert!(!refs(&build_table(&flagged(), &model, &spec)).contains(&"R3".to_string()));
        spec.include_excluded_from_bom = true;
        assert!(refs(&build_table(&flagged(), &model, &spec)).contains(&"R3".to_string()));
        // an export leaves them out whatever the table shows ("PreviewRefresh": SetIncludeExcludedFromBOM( false ))
        let out = export_bom(&flagged(), &model, &spec, &BomFmt::csv());
        assert!(!out.contains("R3"), "{out}");
        assert!(out.contains("R2"), "a DNP symbol is exported unless the table leaves it out: {out}");
        spec.exclude_dnp = true;
        let out = export_bom(&flagged(), &model, &spec, &BomFmt::csv());
        assert!(!out.contains("R2") && !out.contains("R3"), "{out}");
    }

    #[test]
    fn apply_changes_sets_an_attribute_on_every_unit_and_any_other_value_clears_it() {
        let mut s = demo();
        s.symbols.push(sym("U1", "OPA", "SOIC", 1));
        s.symbols.push(sym("U1", "OPA", "SOIC", 2));
        let edit = |id: &str, field: &str, value: &str| FieldChanges { edits: vec![FieldEdit { id: id.into(), field: field.into(), value: value.into() }], ..Default::default() };
        apply_field_changes(&mut s, &edit("U1", DNP, "1")).unwrap();
        apply_field_changes(&mut s, &edit("U1", EXCLUDE_FROM_SIM, "1")).unwrap();
        apply_field_changes(&mut s, &edit("R1", EXCLUDE_FROM_BOARD, "1")).unwrap();
        apply_field_changes(&mut s, &edit("R1", EXCLUDE_FROM_BOM, "1")).unwrap();
        let units: Vec<(bool, bool)> = s.symbols.iter().filter(|x| x.id == "U1").map(|x| (x.dnp, x.exclude_from_sim)).collect();
        assert_eq!(units, vec![(true, true), (true, true)], "both units of U1");
        let r1 = s.symbols.iter().find(|x| x.id == "R1").unwrap();
        assert_eq!((r1.exclude_from_board, r1.exclude_from_bom, r1.dnp), (true, true, false));
        // "newValue = aValue == 1": anything else turns it off
        apply_field_changes(&mut s, &edit("U1", DNP, "0")).unwrap();
        apply_field_changes(&mut s, &edit("R1", EXCLUDE_FROM_BOM, "no")).unwrap();
        assert!(s.symbols.iter().filter(|x| x.id == "U1").all(|x| !x.dnp));
        assert!(!s.symbols.iter().find(|x| x.id == "R1").unwrap().exclude_from_bom);
        // the other generated fields stay read-only, and an attribute of an unknown symbol is refused as any edit is
        assert!(apply_field_changes(&mut s, &edit("R1", QUANTITY, "9")).is_err());
        assert!(apply_field_changes(&mut s, &edit("R404", DNP, "1")).is_err());
    }

    #[test]
    fn the_built_in_presets_have_kicads_columns_and_a_preset_keeps_the_view_s_other_columns_hidden() {
        let names: Vec<&str> = TableSpec::built_in().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["Default Editing", "Grouped By Value", "Grouped By Value and Footprint", "Attributes"]);
        fn shown(t: &TableSpec) -> Vec<&str> {
            t.columns.iter().filter(|c| c.show).map(|c| c.name.as_str()).collect()
        }
        assert_eq!(shown(&TableSpec::default_editing()), vec![REFERENCE, QUANTITY, VALUE, DNP, EXCLUDE_FROM_BOM, EXCLUDE_FROM_BOARD, FOOTPRINT, DATASHEET]);
        assert_eq!(shown(&TableSpec::attributes()), vec![REFERENCE, VALUE, DNP, EXCLUDE_FROM_BOM, EXCLUDE_FROM_BOARD, EXCLUDE_FROM_SIM]);
        // the filters of each: Default Editing and Attributes list the excluded symbols, the two grouped views do not
        assert_eq!(TableSpec::built_in().into_iter().map(|(_, t)| (t.exclude_dnp, t.include_excluded_from_bom)).collect::<Vec<_>>(), vec![(false, true), (false, false), (false, false), (false, true)]);
        // a view with a user field and an unrelated filter, with the Attributes preset applied: the preset's columns lead, the user field stays but is hidden, the filter is the preset's
        let mut view = TableSpec::default_editing();
        view.columns.push(Column { name: "MPN".into(), label: "MPN".into(), show: true, group_by: true });
        view.filter = "R*".into();
        view.exclude_dnp = true;
        let applied = view.with_preset("attributes").expect("a built-in preset");
        assert_eq!(shown(&applied), shown(&TableSpec::attributes()));
        let mpn = applied.columns.iter().find(|c| c.name == "MPN").unwrap();
        assert!(!mpn.show && !mpn.group_by);
        assert_eq!((applied.filter.as_str(), applied.exclude_dnp, applied.include_excluded_from_bom), ("", false, true));
        assert!(view.with_preset("no such preset").is_none());
        // the names a view is sent with are read back
        let json = serde_json::to_string(&TableSpec::default_editing()).unwrap();
        assert_eq!(serde_json::from_str::<TableSpec>(&json).unwrap(), TableSpec::default_editing());
        // a view from before the filters existed has neither
        let old: TableSpec = serde_json::from_str(r#"{"columns": [{"name": "Reference"}], "group_symbols": true}"#).unwrap();
        assert!(!old.exclude_dnp && !old.include_excluded_from_bom);
    }

    #[test]
    fn a_leading_zero_reference_is_its_own_symbol() {
        // r5 C5-L8: `SCH_REFERENCE::Split` keeps `m_numRefStr` ("01"); `unitMatch` compares `GetRefNumber()`.
        assert_eq!(r("R01").full(), "R01");
        let s = sch(vec![sym("R01", "10k", "", 1), sym("R1", "10k", "", 1)]);
        let mut spec = TableSpec::grouped_by_value();
        spec.group_symbols = false;
        let t = build_table(&s, &ConstraintModel::default(), &spec);
        let mut refs: Vec<String> = t.rows.iter().flat_map(|r| r.refs.clone()).collect();
        refs.sort();
        assert_eq!(t.rows.len(), 2, "two rows, not two units of one symbol");
        assert_eq!(refs, vec!["R01".to_string(), "R1".to_string()]);
    }

    #[test]
    fn an_empty_edit_creates_no_field() {
        // r5 C5-L9: `ApplyData`'s `createField = !destField && ( !srcValue.IsEmpty() || userAdded )`
        let mut s = sch(vec![sym("R1", "10k", "", 1), sym("R2", "10k", "", 1)]);
        s.user_fields.insert("R1".into(), [("MPN".to_string(), "X".to_string())].into_iter().collect());
        let edit = |id: &str, v: &str| FieldEdit { id: id.into(), field: "MPN".into(), value: v.into() };
        apply_field_changes(&mut s, &FieldChanges { edits: vec![edit("R1", ""), edit("R2", "")], ..Default::default() }).unwrap();
        assert_eq!(s.user_fields.get("R1").and_then(|m| m.get("MPN")).map(String::as_str), Some(""), "a field the symbol has is emptied");
        assert!(s.user_fields.get("R2").is_none_or(|m| !m.contains_key("MPN")), "R2 had no MPN and gets none");
        // a column added in the dialog is created empty
        apply_field_changes(&mut s, &FieldChanges { edits: vec![FieldEdit { id: "R2".into(), field: "Note".into(), value: String::new() }], add_fields: vec!["Note".into()], ..Default::default() }).unwrap();
        assert!(s.user_fields.get("R2").is_some_and(|m| m.contains_key("Note")));
    }

    /// A screen with R1 shown on two sheet paths: the second path's R1 is R5.
    fn shared_design() -> eda_model::ir::Design {
        use crate::sch_annotate_tests::{placement, section};
        let mut d = crate::sch_annotate_tests::design();
        let mut screen = section(vec![sym("R1", "10k", "", 1)], vec![]);
        screen.user_fields.insert("R1".into(), [("MPN".to_string(), "M1".to_string())].into_iter().collect());
        let at = screen.symbols[0].at;
        screen.instance_overrides.push(eda_model::ir::SymbolPathOverride { at, sheet_path: vec!["s2".into()], reference: "R5".into(), unit: 1 });
        let mut second = placement("s2", "Two", "shared.kicad_sch", "");
        second.at = Point { x: Nm::new(100_000_000), y: Nm::ZERO };
        d.schematic = Some(section(vec![], vec![placement("s1", "One", "shared.kicad_sch", ""), second]));
        d.sheet_contents = Some([("shared.kicad_sch".to_string(), screen)].into_iter().collect());
        d
    }

    #[test]
    fn a_shared_screen_lists_a_reference_per_placement() {
        // r5 C5-M3: `Hierarchy().GetSymbols(..)` -> one `SCH_REFERENCE` per sheet path.
        let d = shared_design();
        let view = project_view(&d).unwrap();
        let spec = TableSpec::grouped_by_value();
        let t = build_table(&view, &ConstraintModel::default(), &spec);
        assert_eq!(t.rows.len(), 1);
        let col = |n: &str| spec.columns.iter().position(|c| c.name == n).unwrap();
        assert_eq!(t.rows[0].cells[col(REFERENCE)], "R1, R5");
        assert_eq!(t.rows[0].cells[col(QUANTITY)], "2");
        // an edit on R5's row is an edit to the screen's R1, and it shows on both rows
        let changes = FieldChanges { edits: vec![FieldEdit { id: "R5".into(), field: "MPN".into(), value: "M2".into() }], ..Default::default() };
        let mut staged = view.clone();
        apply_field_changes(&mut staged, &changes_for_view(&d, &changes)).unwrap();
        assert_eq!(staged.user_fields["R1"]["MPN"], "M2");
        assert_eq!(staged.user_fields["R5"]["MPN"], "M2");
        assert_eq!(changes_for_sheets(&d, &changes.edits), vec![FieldEdit { id: "R1".into(), field: "MPN".into(), value: "M2".into() }]);
    }

    #[test]
    fn the_export_resolves_a_fields_variables() {
        // r5 C5-L9: `Export` -> `GetExportValue` -> `GetValue( .., resolveVars = true, .. )`
        let mut s = sch(vec![sym("R1", "10k", "", 1)]);
        s.user_fields.insert("R1".into(), [("Note".to_string(), "${VALUE}-X".to_string())].into_iter().collect());
        let mut spec = TableSpec::grouped_by_value();
        spec.columns.push(Column { name: "Note".into(), label: "Note".into(), show: true, group_by: false });
        let text = export_bom(&s, &ConstraintModel::default(), &spec, &BomFmt::csv());
        assert!(text.contains("\"10k-X\""), "{text}");
    }
}
