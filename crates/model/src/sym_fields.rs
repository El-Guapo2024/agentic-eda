//! The properties of a library symbol and the derivation between library symbols: KiCad's `SCH_FIELD`s of a `LIB_SYMBOL` (`Reference`, `Value`, `Footprint`, `Datasheet` and
//! `Description` -- `MANDATORY_FIELDS` -- and the user fields after them), `(extends "Parent")`, `LIB_SYMBOL::Flatten`, `GetDerivedSymbolNames` (`LIB_BUFFER`), the
//! creation of a derived symbol (`LIB_SYMBOL_LIBRARY_MANAGER::CreateSymbol`) and what Update Symbol Fields does (`DIALOG_UPDATE_SYMBOL_FIELDS::onOkButtonClicked`), at KiCad
//! 8303b2ad.
//!
//! A derived symbol keeps no drawing of its own: its graphics, pins, units, body styles, power flag and pin text options are its root's (`Flatten` starts from a copy of the
//! root), and it overrides fields -- a mandatory field it gives a text replaces the parent's whole field, text and layout; the user fields it has replace or add to the
//! parent's -- plus its keywords and footprint filters when it has some. Here a library symbol names its parent by `lib_id` ([`LibrarySymbol::extends`]), a link that resolves
//! through the project library ([`SymbolLibrarySection`]); KiCad links by pointer, so a rename or a delete of a parent is the verb's business here (`crates/ops/src/sym_derive.rs`).
//!
//! The fields' storage: [`LibrarySymbol::fields`] holds the layout of every field (where it is, how big, hidden or shown) and the user fields, and the texts of `Value`
//! and `Footprint`; the text of `Reference`, `Datasheet` and `Description` stays in the members that were always the symbol's (`reference_prefix`, `datasheet`, `description`),
//! and wins over a copy kept in the list, so a verb that has always written those three (Symbol Properties) is still the whole story for them. A symbol with no list has the
//! five fields at their default places.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::ir::{LibrarySymbol, SymbolLibrarySection};
use crate::symbol::SPoint;

/// `MANDATORY_FIELDS`, by the names a `.kicad_sym` file gives them (`GetCanonicalFieldName`).
pub const MANDATORY_FIELDS: [&str; 5] = ["Reference", "Value", "Footprint", "Datasheet", "Description"];

/// Whether `name` is one of the five mandatory fields.
pub fn is_mandatory_field(name: &str) -> bool {
    MANDATORY_FIELDS.contains(&name)
}

/// `GR_TEXT_H_ALIGN_T` of a field's `(justify ...)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldHAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// `GR_TEXT_V_ALIGN_T` of a field's `(justify ...)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldVAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

fn d_size() -> f64 {
    1.27
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default_size(v: &f64) -> bool {
    (*v - 1.27).abs() < 1e-9
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn is_center_h(a: &FieldHAlign) -> bool {
    *a == FieldHAlign::Center
}

fn is_center_v(a: &FieldVAlign) -> bool {
    *a == FieldVAlign::Center
}

fn origin() -> SPoint {
    SPoint { x: 0.0, y: 0.0 }
}

/// One property of a library symbol (`SCH_FIELD` of a `LIB_SYMBOL`): its text and the layout `(property "Name" "text" (at x y angle) ... (effects ...))` keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySymbolField {
    /// `Reference`, `Value`, `Footprint`, `Datasheet`, `Description` or a user field's name.
    pub name: String,
    #[serde(default)]
    pub text: String,
    /// `(at x y angle)`: where the field's text is anchored, in the symbol's frame (mm, +y up).
    #[serde(default = "origin")]
    pub at: SPoint,
    /// The text's angle in degrees: 0 (horizontal) or 90 (vertical), as `(at x y angle)` has it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle_deg: f64,
    /// `(effects (font (size h w)))`: the text height, mm (a field's text is drawn square).
    #[serde(default = "d_size", skip_serializing_if = "is_default_size")]
    pub size_mm: f64,
    /// `(hide yes)`: the field is not drawn unless the editor shows hidden fields.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// `(show_name yes)`: the text is drawn as `Name: text`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_name: bool,
    /// `(do_not_autoplace yes)`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub do_not_autoplace: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// `(justify left|right ...)`.
    #[serde(default, skip_serializing_if = "is_center_h")]
    pub h_align: FieldHAlign,
    /// `(justify ... top|bottom)`.
    #[serde(default, skip_serializing_if = "is_center_v")]
    pub v_align: FieldVAlign,
    /// `(property private "Name" ..)` (`SCH_FIELD::IsPrivate`): a field of the library symbol only, which a placed symbol does not get.
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
}

impl Default for LibrarySymbolField {
    fn default() -> Self {
        LibrarySymbolField {
            name: String::new(),
            text: String::new(),
            at: origin(),
            angle_deg: 0.0,
            size_mm: 1.27,
            hidden: false,
            show_name: false,
            do_not_autoplace: false,
            bold: false,
            italic: false,
            h_align: FieldHAlign::Center,
            v_align: FieldVAlign::Center,
            private: false,
        }
    }
}

impl LibrarySymbolField {
    /// A field with this name and text, everything else at its default.
    pub fn named(name: &str, text: &str) -> Self {
        LibrarySymbolField { name: name.to_string(), text: text.to_string(), ..Default::default() }
    }

    /// `EDA_TEXT::SetAttributes` of the parts Update Symbol Fields' "sizes and styles" resets: the size, the angle, bold, italic and the alignment.
    pub fn take_effects_of(&mut self, other: &LibrarySymbolField) {
        self.size_mm = other.size_mm;
        self.angle_deg = other.angle_deg;
        self.bold = other.bold;
        self.italic = other.italic;
        self.h_align = other.h_align;
        self.v_align = other.v_align;
    }
}

/// The part of a `lib_id` after the library nickname.
pub fn bare_name(lib_id: &str) -> &str {
    lib_id.split_once(':').map_or(lib_id, |(_, name)| name)
}

/// The library nickname of a `lib_id` (`""` for a bare name).
pub fn library_of(lib_id: &str) -> &str {
    lib_id.split_once(':').map_or("", |(lib, _)| lib)
}

impl LibrarySymbol {
    /// The default place and visibility of a mandatory field of a symbol that has none stored (`LIB_SYMBOL::LIB_SYMBOL`: the reference and the value are shown, the rest hidden;
    /// the places are the ones a new symbol's file has always been written with).
    pub fn default_field(&self, name: &str) -> LibrarySymbolField {
        let mut f = LibrarySymbolField::named(name, "");
        match name {
            "Reference" => f.at = SPoint { x: 0.0, y: 5.08 },
            "Value" => f.at = SPoint { x: 0.0, y: 2.54 },
            _ => f.hidden = true,
        }
        f
    }

    /// The symbol's fields as the editor lists them: the five mandatory ones in order, then the user fields in the order they were added. A mandatory field the symbol
    /// stores none of is at its default; the text of `Reference`, `Datasheet` and `Description` is the symbol's own member, and `Value` with no stored text is the symbol's
    /// name (`The symbol name in library and also the default symbol value when loaded in the schematic`).
    pub fn all_fields(&self) -> Vec<LibrarySymbolField> {
        let mut out = Vec::with_capacity(MANDATORY_FIELDS.len() + self.fields.len());
        for name in MANDATORY_FIELDS {
            let stored = self.fields.iter().find(|f| f.name == name);
            let mut f = stored.cloned().unwrap_or_else(|| self.default_field(name));
            f.name = name.to_string();
            f.text = match name {
                "Reference" => self.reference_prefix.clone(),
                "Datasheet" => self.datasheet.clone(),
                "Description" => self.description.clone(),
                "Value" if stored.is_none() => bare_name(&self.lib_id).to_string(),
                _ => f.text,
            };
            out.push(f);
        }
        out.extend(self.fields.iter().filter(|f| !is_mandatory_field(&f.name)).cloned());
        out
    }

    /// One field by name, as [`all_fields`](Self::all_fields) lists it.
    pub fn field(&self, name: &str) -> Option<LibrarySymbolField> {
        self.all_fields().into_iter().find(|f| f.name == name)
    }

    /// Replaces the symbol's fields by `fields` (the five mandatory ones and the user fields, in any order: the mandatory ones are put first, in KiCad's order, and one the list
    /// lacks is at its default). The members that hold the texts of `Reference`, `Datasheet` and `Description` follow the list.
    pub fn set_fields(&mut self, fields: Vec<LibrarySymbolField>) {
        let mut stored: Vec<LibrarySymbolField> = Vec::with_capacity(fields.len());
        for name in MANDATORY_FIELDS {
            if let Some(f) = fields.iter().find(|f| f.name == name) {
                stored.push(f.clone());
            }
        }
        stored.extend(fields.iter().filter(|f| !is_mandatory_field(&f.name)).cloned());
        if let Some(f) = stored.iter().find(|f| f.name == "Reference") {
            self.reference_prefix = f.text.clone();
        }
        if let Some(f) = stored.iter().find(|f| f.name == "Datasheet") {
            self.datasheet = f.text.clone();
        }
        if let Some(f) = stored.iter().find(|f| f.name == "Description") {
            self.description = f.text.clone();
        }
        self.fields = stored;
    }

    /// The user fields (`FIELD_T::USER`), in order.
    pub fn user_fields(&self) -> Vec<LibrarySymbolField> {
        self.fields.iter().filter(|f| !is_mandatory_field(&f.name)).cloned().collect()
    }
}

/// What walking up from a symbol through `extends` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentChain {
    /// The `lib_id`s of the parents, nearest first, the root last. Empty for a symbol that is not derived (or whose parent is not in the library).
    pub parents: Vec<String>,
    /// The chain ran back into a symbol it had already passed (`GetInheritanceDepth`'s "circular inheritance detected"): the loop is cut there.
    pub cycle: bool,
    /// A parent named by `extends` that the library does not have: the chain stops before it.
    pub missing: Option<String>,
}

impl SymbolLibrarySection {
    /// The parents of `lib_id`, nearest first (`LIB_SYMBOL::GetParent` all the way up), stopping at a cycle or at a parent the library lacks.
    pub fn parent_chain(&self, lib_id: &str) -> ParentChain {
        let mut parents: Vec<String> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        seen.insert(lib_id);
        let mut cycle = false;
        let mut missing = None;
        let mut next = self.by_lib_id(lib_id).and_then(|s| s.extends.as_deref());
        while let Some(parent_id) = next {
            if !seen.insert(parent_id) {
                cycle = true;
                break;
            }
            match self.by_lib_id(parent_id) {
                Some(p) => {
                    parents.push(p.lib_id.clone());
                    next = p.extends.as_deref();
                }
                None => {
                    missing = Some(parent_id.to_string());
                    break;
                }
            }
        }
        ParentChain { parents, cycle, missing }
    }

    /// `LIB_SYMBOL::IsDerived`: the symbol names a parent and the library has it.
    pub fn is_derived(&self, lib_id: &str) -> bool {
        self.by_lib_id(lib_id).and_then(|s| s.extends.as_deref()).is_some_and(|p| self.by_lib_id(p).is_some())
    }

    /// `LIB_SYMBOL::GetRootSymbol`: the symbol at the end of the chain (the symbol itself when it is not derived).
    pub fn root_of(&self, lib_id: &str) -> Option<&LibrarySymbol> {
        let chain = self.parent_chain(lib_id);
        match chain.parents.last() {
            Some(root) => self.by_lib_id(root),
            None => self.by_lib_id(lib_id),
        }
    }

    /// `LIB_BUFFER::GetDerivedSymbolNames`: every symbol that derives from `lib_id` directly or through others, each parent before the symbols derived from it, in library
    /// order among symbols of one parent.
    pub fn derived_names(&self, lib_id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        self.collect_derived(lib_id, lib_id, &mut out);
        out
    }

    fn collect_derived(&self, start: &str, parent: &str, out: &mut Vec<String>) {
        for s in self.symbols.iter().filter(|s| s.extends.as_deref() == Some(parent)) {
            if out.contains(&s.lib_id) || s.lib_id == start {
                continue; // a cycle in the data: never walk a symbol twice
            }
            out.push(s.lib_id.clone());
            self.collect_derived(start, &s.lib_id, out);
        }
    }

    /// Every symbol related to `lib_id` -- its root and everything derived from the root (`setScope( SCOPE_RELATED_SYMBOLS )` of the fields table: the root, then
    /// `GetDerivedSymbolNames( root )`). Just the symbol itself when it is not derived and has no children; empty for a symbol the library lacks.
    pub fn related_names(&self, lib_id: &str) -> Vec<String> {
        let Some(root) = self.root_of(lib_id) else { return Vec::new() };
        let root_id = root.lib_id.clone();
        let mut out = vec![root_id.clone()];
        out.extend(self.derived_names(&root_id));
        out
    }

    /// `LIB_SYMBOL::Flatten`: the symbol with everything it inherits put into it. A symbol that is not derived is copied as it is. A derived one starts as a copy of its
    /// root, takes each derived symbol's overrides from the root down -- a mandatory field with a text of its own replaces the one it had, text and layout; a user field
    /// replaces the one of its name or is added; keywords and footprint filters when it has some -- and last its own; it keeps its own name, and the excluded-from-simulation,
    /// -BOM, -board and -position-files flags of its nearest parent. The result is nobody's child.
    ///
    /// Errors: the symbol is not in the library, or a parent it names is not.
    pub fn flatten(&self, lib_id: &str) -> Result<LibrarySymbol, String> {
        let sym = self.by_lib_id(lib_id).ok_or_else(|| format!("no symbol {lib_id:?} in the library"))?;
        if sym.extends.is_none() {
            return Ok(sym.clone());
        }
        let chain = self.parent_chain(lib_id);
        if let Some(missing) = &chain.missing {
            return Err(format!("the parent {missing:?} of {lib_id:?} is not in the library"));
        }
        if chain.parents.is_empty() {
            // the parent chain ran back into the symbol at once: "just copy this symbol"
            let mut copy = sym.clone();
            copy.extends = None;
            return Ok(copy);
        }
        let ancestors: Vec<&LibrarySymbol> = chain.parents.iter().filter_map(|p| self.by_lib_id(p)).collect();
        let root = *ancestors.last().expect("a chain with parents has a root");
        let mut out = root.clone();
        let mut fields = root.all_fields();
        let mut keywords = root.keywords.clone();
        let mut filters = root.footprint_filters.clone();
        // every derived symbol's overrides, from the one next to the root down to this symbol's own
        for derived in ancestors.iter().rev().skip(1).chain(std::iter::once(&sym)) {
            overlay_fields(&mut fields, &derived.all_fields());
            if !derived.keywords.is_empty() {
                keywords = derived.keywords.clone();
            }
            if !derived.footprint_filters.is_empty() {
                filters = derived.footprint_filters.clone();
            }
        }
        out.lib_id = sym.lib_id.clone();
        out.extends = None;
        out.published = sym.published;
        out.keywords = keywords;
        out.footprint_filters = filters;
        // `parentChain.front()`: the flags come from the nearest parent
        let nearest = ancestors[0];
        out.in_bom = nearest.in_bom;
        out.on_board = nearest.on_board;
        out.exclude_from_sim = nearest.exclude_from_sim;
        out.exclude_from_pos_files = nearest.exclude_from_pos_files;
        out.set_fields(fields);
        Ok(out)
    }

    /// The fields of the flattened symbol: what Update Symbol Fields compares a derived symbol with.
    pub fn flattened_fields(&self, lib_id: &str) -> Result<Vec<LibrarySymbolField>, String> {
        self.flatten(lib_id).map(|s| s.all_fields())
    }
}

/// `Flatten`'s "overwrite the parent's mandatory fields for the fields defined in derived, then grab all the rest": into `fields` (a root's, then each flattened step) the
/// mandatory fields of `derived` that have a text, whole; its user fields, in place of the one of their name or after the others.
fn overlay_fields(fields: &mut Vec<LibrarySymbolField>, derived: &[LibrarySymbolField]) {
    for f in derived {
        if is_mandatory_field(&f.name) {
            if f.text.is_empty() {
                continue;
            }
        }
        match fields.iter_mut().find(|x| x.name == f.name) {
            Some(slot) => *slot = f.clone(),
            None => fields.push(f.clone()),
        }
    }
}

/// The creation of a derived symbol (`LIB_SYMBOL_LIBRARY_MANAGER::CreateSymbol` with a parent), as `DIALOG_LIB_NEW_SYMBOL` offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeriveOptions {
    /// "Keep linked footprint": the Footprint field keeps the parent's text (else it is emptied).
    pub keep_footprint: bool,
    /// "Keep linked datasheet".
    pub keep_datasheet: bool,
    /// "Transfer user-defined fields".
    pub transfer_user_fields: bool,
    /// "Keep content of user-defined fields": a transferred field keeps its text (else it is emptied).
    pub keep_content_user_fields: bool,
}

impl Default for DeriveOptions {
    fn default() -> Self {
        DeriveOptions { keep_footprint: false, keep_datasheet: false, transfer_user_fields: false, keep_content_user_fields: false }
    }
}

/// The new symbol `CreateSymbol` makes for `parent`: derived from it, with the five mandatory fields copied from the parent (`*field = *parentField`) -- the Value is the
/// new name where the parent is a power symbol, the Footprint and the Datasheet are emptied unless kept -- and, if asked, the parent's user fields.
pub fn derive_symbol(section: &SymbolLibrarySection, parent: &LibrarySymbol, new_lib_id: &str, opts: DeriveOptions) -> LibrarySymbol {
    let mut sym = LibrarySymbol::new_empty(new_lib_id);
    sym.extends = Some(parent.lib_id.clone());
    let parent_is_power = section.root_of(&parent.lib_id).unwrap_or(parent).power;
    let mut fields: Vec<LibrarySymbolField> = Vec::new();
    for mut f in parent.all_fields().into_iter().filter(|f| is_mandatory_field(&f.name)) {
        match f.name.as_str() {
            "Value" if parent_is_power => f.text = bare_name(new_lib_id).to_string(),
            "Footprint" if !opts.keep_footprint => f.text.clear(),
            "Datasheet" if !opts.keep_datasheet => f.text.clear(),
            _ => {}
        }
        fields.push(f);
    }
    if opts.transfer_user_fields {
        for mut f in parent.user_fields() {
            if !opts.keep_content_user_fields {
                f.text.clear();
            }
            fields.push(f);
        }
    }
    sym.set_fields(fields);
    sym
}

/// `DIALOG_UPDATE_SYMBOL_FIELDS`'s checkboxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFieldsOptions {
    /// The fields to update (the checked rows of the list, by name).
    pub fields: BTreeSet<String>,
    /// "Remove fields if not in parent symbol".
    pub remove_extras: bool,
    /// "Reset fields if empty in parent symbol": a field takes the parent's text even when that is empty.
    pub reset_empty: bool,
    /// "Update/reset field text".
    pub reset_text: bool,
    /// "Update/reset field visibilities" (hidden and show name).
    pub reset_visibility: bool,
    /// "Update/reset field sizes and styles".
    pub reset_effects: bool,
    /// "Update/reset field positions".
    pub reset_positions: bool,
}

/// `DIALOG_UPDATE_SYMBOL_FIELDS::onOkButtonClicked`: the symbol's own fields made to follow `parent` (the fields of the parent flattened), for the fields in `opts.fields`.
/// Each of those the symbol has takes what is asked of the parent's field of the name (the text, unless the parent's is empty and "reset if empty" is off; hidden and
/// "show name"; the size and styles; the position); one the parent lacks is removed when asked. A field of `opts.fields` that the parent has and the symbol lacks is
/// added, with the parent's text, effects, visibility and place.
pub fn update_symbol_fields(own: &[LibrarySymbolField], parent: &[LibrarySymbolField], opts: &UpdateFieldsOptions) -> Vec<LibrarySymbolField> {
    let mut result: Vec<LibrarySymbolField> = Vec::new();
    for field in own {
        let mut field = field.clone();
        let mut copy = true;
        if opts.fields.contains(&field.name) {
            if let Some(pf) = parent.iter().find(|p| p.name == field.name) {
                let reset_text = if pf.text.is_empty() { opts.reset_empty } else { opts.reset_text };
                if reset_text {
                    field.text = pf.text.clone();
                }
                if opts.reset_visibility {
                    field.hidden = pf.hidden;
                    field.show_name = pf.show_name;
                }
                if opts.reset_effects {
                    // the visible bit and the position are part of what `SetAttributes()` copies; the dialog keeps the field's own
                    field.take_effects_of(pf);
                }
                if opts.reset_positions {
                    field.at = pf.at;
                }
            } else if opts.remove_extras {
                copy = false;
            }
        }
        if copy {
            result.push(field);
        }
    }
    for pf in parent {
        if opts.fields.contains(&pf.name) && !own.iter().any(|f| f.name == pf.name) {
            // `newField->SetAttributes( *parentField )`: the visible bit and the position come with it
            result.push(pf.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::LibraryFill;
    use crate::ir::{LibrarySymbolGraphic, LibrarySymbolPin};

    fn pin(number: &str, y: f64) -> LibrarySymbolPin {
        LibrarySymbolPin {
            id: String::new(),
            number: number.into(),
            name: format!("P{number}"),
            electrical_type: "passive".into(),
            shape: "line".into(),
            at: SPoint { x: -5.08, y },
            angle_deg: 0.0,
            length_mm: 2.54,
            unit: 1,
            body_style: 1,
            hidden: false,
            name_size_mm: None,
            number_size_mm: None,
            alternates: Vec::new(),
        }
    }

    /// `Lib:Base`: a rectangle, two pins, a Footprint, a Datasheet and a user field `MPN`.
    fn base() -> LibrarySymbol {
        let mut s = LibrarySymbol::new_empty("Lib:Base");
        s.reference_prefix = "U".into();
        s.datasheet = "http://base/ds.pdf".into();
        s.description = "the base".into();
        s.keywords = "base chip".into();
        s.footprint_filters = vec!["SOIC*".into()];
        s.graphics.push(LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: 1, start: SPoint { x: -2.54, y: -2.54 }, end: SPoint { x: 2.54, y: 2.54 }, stroke_mm: 0.254, fill: LibraryFill::None });
        s.pins = vec![pin("1", 1.27), pin("2", -1.27)];
        s.unit_count = 1;
        let mut fields = s.all_fields();
        fields[1].text = "BASE".into();
        fields[2].text = "Package_SO:SOIC-8".into();
        fields.push(LibrarySymbolField { name: "MPN".into(), text: "X-1".into(), at: SPoint { x: 0.0, y: -5.0 }, hidden: true, ..Default::default() });
        s.set_fields(fields);
        s.assign_missing_ids();
        s
    }

    fn section_with(extra: Vec<LibrarySymbol>) -> SymbolLibrarySection {
        let mut section = SymbolLibrarySection { symbols: vec![base()], ..Default::default() };
        section.symbols.extend(extra);
        section
    }

    #[test]
    fn a_symbol_with_no_stored_fields_has_the_five_mandatory_ones_at_their_defaults() {
        let s = LibrarySymbol::new_empty("Lib:R");
        let f = s.all_fields();
        assert_eq!(f.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), MANDATORY_FIELDS);
        assert_eq!((f[0].text.as_str(), f[1].text.as_str()), ("U", "R"), "the reference prefix is the symbol's, the value is its name");
        assert!(!f[0].hidden && !f[1].hidden && f[2].hidden && f[3].hidden && f[4].hidden, "the reference and the value are shown, the rest hidden");
        assert_eq!((f[0].at.y, f[1].at.y), (5.08, 2.54));
    }

    #[test]
    fn the_members_that_were_always_the_symbols_win_over_a_stored_copy_of_the_text() {
        let mut s = base();
        s.datasheet = "changed".into();
        s.reference_prefix = "IC".into();
        let f = s.all_fields();
        assert_eq!((f[0].text.as_str(), f[3].text.as_str()), ("IC", "changed"));
        assert_eq!((f[1].text.as_str(), f[2].text.as_str()), ("BASE", "Package_SO:SOIC-8"), "Value and Footprint are the list's own");
        // and set_fields writes them back
        let mut g = f.clone();
        g[4].text = "new description".into();
        s.set_fields(g);
        assert_eq!(s.description, "new description");
        assert_eq!(s.user_fields().len(), 1);
    }

    #[test]
    fn set_fields_puts_the_mandatory_fields_first_in_kicads_order() {
        let mut s = LibrarySymbol::new_empty("Lib:X");
        s.set_fields(vec![LibrarySymbolField::named("Zed", "z"), LibrarySymbolField::named("Datasheet", "d"), LibrarySymbolField::named("Reference", "R"), LibrarySymbolField::named("Alpha", "a")]);
        assert_eq!(s.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Reference", "Datasheet", "Zed", "Alpha"]);
        assert_eq!((s.reference_prefix.as_str(), s.datasheet.as_str()), ("R", "d"));
    }

    #[test]
    fn a_symbol_derived_from_another_has_the_parents_drawing_and_its_own_properties_once_flattened() {
        let mut child = derive_symbol(&section_with(vec![]), &base(), "Lib:Child", DeriveOptions::default());
        let mut f = child.all_fields();
        f[1].text = "CHILD".into(); // its own Value
        child.set_fields(f);
        assert_eq!(child.extends.as_deref(), Some("Lib:Base"));
        assert!(child.graphics.is_empty() && child.pins.is_empty(), "a derived symbol draws nothing of its own");
        let section = section_with(vec![child]);
        assert!(section.is_derived("Lib:Child") && !section.is_derived("Lib:Base"));
        let flat = section.flatten("Lib:Child").unwrap();
        assert_eq!((flat.lib_id.as_str(), flat.extends.as_deref()), ("Lib:Child", None));
        assert_eq!((flat.graphics.len(), flat.pins.len()), (1, 2), "the root's rectangle and pins");
        let ff = flat.all_fields();
        assert_eq!(ff[1].text, "CHILD", "the child's Value replaces the parent's");
        assert_eq!(ff[0].text, "U", "the Reference was copied from the parent when the symbol was derived");
        // the Footprint and the Datasheet are not kept in the new symbol unless asked, so the symbol has an empty text of its own -- and an empty field of a derived symbol is not an
        // override: the flattened symbol has the parent's
        let own = section.by_lib_id("Lib:Child").unwrap();
        assert_eq!((own.field("Footprint").unwrap().text.as_str(), own.field("Datasheet").unwrap().text.as_str()), ("", ""));
        assert_eq!((ff[2].text.as_str(), ff[3].text.as_str()), ("Package_SO:SOIC-8", "http://base/ds.pdf"));
        assert_eq!(ff[4].text, "the base", "the Description is the parent's");
        assert_eq!(flat.field("MPN").unwrap().text, "X-1", "a user field the child does not have is the root's (transferring them copies them into the child, where an emptied one overrides)");
        assert!(section.by_lib_id("Lib:Child").unwrap().user_fields().is_empty(), "user fields are transferred only when asked");
        assert_eq!(flat.keywords, "base chip", "keywords the child has none of come from the parent");
        assert_eq!(flat.footprint_filters, ["SOIC*"]);
    }

    #[test]
    fn deriving_keeps_what_the_dialog_is_asked_to_keep() {
        let section = section_with(vec![]);
        let kept = derive_symbol(&section, &base(), "Lib:Kept", DeriveOptions { keep_footprint: true, keep_datasheet: true, transfer_user_fields: true, keep_content_user_fields: true });
        let f = kept.all_fields();
        assert_eq!((f[2].text.as_str(), f[3].text.as_str()), ("Package_SO:SOIC-8", "http://base/ds.pdf"));
        assert_eq!(kept.user_fields().len(), 1);
        assert_eq!(kept.user_fields()[0].text, "X-1");
        let emptied = derive_symbol(&section, &base(), "Lib:Emptied", DeriveOptions { transfer_user_fields: true, ..Default::default() });
        assert_eq!(emptied.user_fields()[0].text, "", "a transferred field without its content");
        assert_eq!(emptied.user_fields()[0].at.y, -5.0, "and with its place");
        // a derived power symbol shows its own name as the net
        let mut gnd = LibrarySymbol::new_empty("Lib:GND");
        gnd.power = true;
        let power = derive_symbol(&SymbolLibrarySection { symbols: vec![gnd.clone()], ..Default::default() }, &gnd, "Lib:GNDA", DeriveOptions::default());
        assert_eq!(power.field("Value").unwrap().text, "GNDA");
    }

    #[test]
    fn flatten_takes_the_overrides_of_every_step_from_the_root_down() {
        let section = section_with(vec![]);
        let mut mid = derive_symbol(&section, &base(), "Lib:Mid", DeriveOptions { transfer_user_fields: true, keep_content_user_fields: true, ..Default::default() });
        let mut f = mid.all_fields();
        f[1].text = "MID".into();
        f[3].text = "http://mid".into();
        f.iter_mut().find(|x| x.name == "MPN").unwrap().text = "MID-1".into();
        f.push(LibrarySymbolField::named("Colour", "red"));
        mid.set_fields(f);
        mid.keywords = "mid".into();
        let mut leaf = LibrarySymbol::new_empty("Lib:Leaf");
        leaf.extends = Some("Lib:Mid".into());
        let mut lf = vec![LibrarySymbolField::named("Value", "LEAF"), LibrarySymbolField::named("Colour", "blue")];
        lf[0].at = SPoint { x: 7.0, y: 7.0 };
        leaf.set_fields(lf);
        leaf.reference_prefix.clear(); // inherits the Reference
        let mut section = section_with(vec![mid, leaf]);
        // the leaf's empty fields inherit: the Reference text comes from the step above
        let flat = section.flatten("Lib:Leaf").unwrap();
        let ff = flat.all_fields();
        assert_eq!(ff[0].text, "U", "an empty field takes the parent's, text and place");
        assert_eq!((ff[1].text.as_str(), ff[1].at.x), ("LEAF", 7.0), "the leaf's Value with its place");
        assert_eq!(ff[3].text, "http://mid", "the Datasheet of the step between");
        assert_eq!(flat.field("MPN").unwrap().text, "MID-1");
        assert_eq!(flat.field("Colour").unwrap().text, "blue", "the leaf's user field replaces the one it overrides");
        assert_eq!(flat.keywords, "mid", "keywords: the nearest step that has some");
        assert_eq!(flat.pins.len(), 2);
        assert_eq!(section.parent_chain("Lib:Leaf").parents, ["Lib:Mid", "Lib:Base"]);
        assert_eq!(section.root_of("Lib:Leaf").unwrap().lib_id, "Lib:Base");
        assert_eq!(section.derived_names("Lib:Base"), ["Lib:Mid", "Lib:Leaf"]);
        assert_eq!(section.related_names("Lib:Leaf"), ["Lib:Base", "Lib:Mid", "Lib:Leaf"]);
        // flags come from the nearest parent
        section.by_lib_id_mut("Lib:Mid").unwrap().in_bom = false;
        assert!(!section.flatten("Lib:Leaf").unwrap().in_bom);
        assert!(section.flatten("Lib:Mid").unwrap().in_bom, "Mid's nearest parent is Base");
    }

    #[test]
    fn flatten_of_a_root_is_a_copy_and_of_a_symbol_whose_parent_is_missing_an_error() {
        let section = section_with(vec![]);
        assert_eq!(section.flatten("Lib:Base").unwrap(), base());
        assert!(section.flatten("Lib:Nope").is_err());
        let mut orphan = LibrarySymbol::new_empty("Lib:Orphan");
        orphan.extends = Some("Lib:Gone".into());
        let section = section_with(vec![orphan]);
        assert!(!section.is_derived("Lib:Orphan"), "a parent the library lacks is no parent");
        assert!(section.flatten("Lib:Orphan").unwrap_err().contains("Lib:Gone"));
        assert_eq!(section.parent_chain("Lib:Orphan").missing.as_deref(), Some("Lib:Gone"));
    }

    #[test]
    fn a_cycle_in_the_data_is_cut_instead_of_followed_for_ever() {
        let mut a = LibrarySymbol::new_empty("Lib:A");
        a.extends = Some("Lib:B".into());
        let mut b = LibrarySymbol::new_empty("Lib:B");
        b.extends = Some("Lib:A".into());
        let section = SymbolLibrarySection { symbols: vec![a, b], ..Default::default() };
        let chain = section.parent_chain("Lib:A");
        assert!(chain.cycle);
        assert_eq!(chain.parents, ["Lib:B"]);
        assert!(section.flatten("Lib:A").is_ok(), "no loop");
        assert_eq!(section.derived_names("Lib:A"), ["Lib:B"]);
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn update_symbol_fields_makes_the_checked_fields_follow_the_parent() {
        let parent = base().all_fields();
        let mut own = parent.clone();
        own[1].text = "CHILD".into();
        own[2].text = "Mine:FP".into();
        own[3].text = "http://mine".into();
        own[3].hidden = false;
        own[4].at = SPoint { x: 9.0, y: 9.0 };
        own[4].size_mm = 2.0;
        own.retain(|f| f.name != "MPN");
        own.push(LibrarySymbolField::named("Extra", "mine"));
        let opts = |fields: &[&str]| UpdateFieldsOptions { fields: names(fields), remove_extras: false, reset_empty: false, reset_text: true, reset_visibility: true, reset_effects: true, reset_positions: true };

        // text, visibility, sizes and positions of the Datasheet and the Description
        let r = update_symbol_fields(&own, &parent, &opts(&["Datasheet", "Description"]));
        assert_eq!(r[3].text, "http://base/ds.pdf");
        assert!(r[3].hidden, "visibility follows the parent");
        assert_eq!((r[4].at.x, r[4].size_mm), (parent[4].at.x, parent[4].size_mm), "position and size follow the parent");
        assert_eq!(r[1].text, "CHILD", "a field that is not checked is left alone");
        assert_eq!(r[2].text, "Mine:FP");
        // the parent's user field the symbol lacks is added, with its place and visibility
        let r = update_symbol_fields(&own, &parent, &opts(&["MPN"]));
        let added = r.iter().find(|f| f.name == "MPN").expect("added");
        assert_eq!((added.text.as_str(), added.at.y, added.hidden), ("X-1", -5.0, true));
        assert!(r.iter().any(|f| f.name == "Extra"), "its own extra field stays unless asked");
    }

    #[test]
    fn update_symbol_fields_respects_each_option() {
        let parent = base().all_fields();
        let mut own = parent.clone();
        own[2].text = "Mine:FP".into(); // the parent's Footprint is "Package_SO:SOIC-8"
        own[3].text = "http://mine".into();
        let mut empty_parent = parent.clone();
        empty_parent[3].text.clear(); // the parent's Datasheet is empty
        let base_opts = UpdateFieldsOptions { fields: names(&["Footprint", "Datasheet", "Extra"]), remove_extras: false, reset_empty: false, reset_text: false, reset_visibility: false, reset_effects: false, reset_positions: false };
        // nothing asked: nothing changes
        assert_eq!(update_symbol_fields(&own, &empty_parent, &base_opts), own);
        // text only: the Footprint follows, the Datasheet (empty in the parent) does not without "reset if empty"
        let r = update_symbol_fields(&own, &empty_parent, &UpdateFieldsOptions { reset_text: true, ..base_opts.clone() });
        assert_eq!((r[2].text.as_str(), r[3].text.as_str()), ("Package_SO:SOIC-8", "http://mine"));
        let r = update_symbol_fields(&own, &empty_parent, &UpdateFieldsOptions { reset_text: true, reset_empty: true, ..base_opts.clone() });
        assert_eq!(r[3].text, "", "reset if empty: the empty text follows too");
        // `reset_empty` alone concerns only the parent's empty fields
        let r = update_symbol_fields(&own, &empty_parent, &UpdateFieldsOptions { reset_empty: true, ..base_opts.clone() });
        assert_eq!((r[2].text.as_str(), r[3].text.as_str()), ("Mine:FP", ""));
        // remove_extras: a checked field the parent lacks goes
        own.push(LibrarySymbolField::named("Extra", "x"));
        let r = update_symbol_fields(&own, &parent, &UpdateFieldsOptions { remove_extras: true, ..base_opts.clone() });
        assert!(!r.iter().any(|f| f.name == "Extra"));
        let r = update_symbol_fields(&own, &parent, &base_opts);
        assert!(r.iter().any(|f| f.name == "Extra"));
        // effects keep the field's own place and visibility
        let mut moved = own.clone();
        moved[3].at = SPoint { x: 4.0, y: 4.0 };
        moved[3].hidden = false;
        let mut styled_parent = parent.clone();
        styled_parent[3].bold = true;
        styled_parent[3].size_mm = 0.8;
        styled_parent[3].hidden = true;
        let r = update_symbol_fields(&moved, &styled_parent, &UpdateFieldsOptions { reset_effects: true, ..base_opts });
        assert!(r[3].bold && r[3].size_mm == 0.8);
        assert!(!r[3].hidden && r[3].at.x == 4.0, "its visibility and its place stay");
    }

    #[test]
    fn a_field_is_json_with_defaults_left_out() {
        let f = LibrarySymbolField::named("MPN", "X");
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v, serde_json::json!({ "name": "MPN", "text": "X", "at": { "x": 0.0, "y": 0.0 } }));
        let back: LibrarySymbolField = serde_json::from_value(serde_json::json!({ "name": "Value", "text": "R", "at": { "x": 1.0, "y": 2.0 }, "hidden": true, "h_align": "left" })).unwrap();
        assert!(back.hidden && back.h_align == FieldHAlign::Left && back.size_mm == 1.27);
        // a symbol of an older design.json has none of this
        let s: LibrarySymbol = serde_json::from_str(r#"{"lib_id":"Lib:Old"}"#).unwrap();
        assert!(s.extends.is_none() && s.fields.is_empty());
    }
}
