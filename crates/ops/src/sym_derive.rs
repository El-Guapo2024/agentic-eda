//! Derived symbols in the Symbol Editor: Derive from Existing Symbol, Flatten Symbol, Update Symbol Fields, the parent choice and the fields grid of Symbol Properties.
//!
//! The model is [`eda_model::sym_fields`] (a symbol's fields, `extends`, `LIB_SYMBOL::Flatten`, `CreateSymbol` with a parent, `DIALOG_UPDATE_SYMBOL_FIELDS`); what lives here is
//! what the verbs refuse and what each one leaves behind:
//!
//! * a derived symbol lives in its parent's library and keeps no drawing of its own, so a verb that edits graphics or pins ([`Board::drawing_symbol_mut`]) is refused on it;
//! * a symbol is never its own ancestor (`SetParent`: "would create circular inheritance"; `CheckSavingIntoOwnInheritance`);
//! * `RenameLibrarySymbol` and `DeleteLibrarySymbol` look after the link, which KiCad holds by pointer and this project by name: a rename follows the children and a
//!   Value that was the old name, a delete takes the symbols derived from the one deleted ("Deleting a base symbol will delete all symbols derived from it").

use super::library_editors::check_symbol_lib_id;
use super::Board;
use eda_model::ir::LibrarySymbol;
use eda_model::sym_fields::{self, is_mandatory_field, DeriveOptions, LibrarySymbolField, UpdateFieldsOptions};
use eda_model::CheckResult;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

/// The properties of a symbol that are not its fields (`SCH_IO_KICAD_SEXPR_PARSER::parseProperty` reads them as the symbol's keywords and footprint filters).
const RESERVED_FIELD_NAMES: [&str; 4] = ["ki_keywords", "ki_description", "ki_fp_filters", "ki_locked"];

impl Board<'_> {
    /// See [`Cmd::DeriveSymbol`](crate::Cmd::DeriveSymbol).
    pub(crate) fn derive_symbol_verb(&mut self, parent: &str, lib_id: &str, opts: DeriveOptions, overwrite: bool) -> Result<(), Vec<CheckResult>> {
        check_symbol_lib_id(lib_id)?;
        let section = self
            .design
            .symbol_library
            .as_ref()
            .ok_or_else(|| fail("ops_unknown_symbol", parent, "the project symbol library is empty: open the symbol to derive from in the Symbol Editor first"))?;
        let parent_sym = section.by_lib_id(parent).ok_or_else(|| fail("ops_unknown_symbol", parent, "the symbol to derive from has not been opened in the Symbol Editor yet"))?;
        if sym_fields::library_of(parent) != sym_fields::library_of(lib_id) {
            return Err(fail("ops_bad_symbol", lib_id, format!("a derived symbol is in its parent's library ({:?}): {lib_id:?} is not", sym_fields::library_of(parent))));
        }
        let taken = section.by_lib_id(lib_id).is_some() || self.model.symbol_of(lib_id).is_some();
        if taken && !overwrite {
            return Err(fail("ops_symbol_exists", lib_id, "a symbol with this name already exists; pick another name"));
        }
        // `CheckSavingIntoOwnInheritance`: the new symbol may not replace one of its own ancestors, or the chain would run back into it
        if section.by_lib_id(lib_id).is_some() && (lib_id == parent || section.parent_chain(parent).parents.iter().any(|p| p == lib_id)) {
            return Err(fail("ops_symbol_cycle", lib_id, "a symbol cannot be derived from itself, or replace one of its own parents"));
        }
        let derived = sym_fields::derive_symbol(section, parent_sym, lib_id, opts);
        let lib = self.symbol_library_mut();
        lib.symbols.retain(|s| s.lib_id != lib_id);
        lib.symbols.push(derived);
        Ok(())
    }

    /// See [`Cmd::FlattenSymbol`](crate::Cmd::FlattenSymbol).
    pub(crate) fn flatten_symbol_verb(&mut self, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        let section = self.design.symbol_library.as_ref().ok_or_else(|| fail("ops_unknown_symbol", lib_id, "the project symbol library is empty"))?;
        let sym = section.by_lib_id(lib_id).ok_or_else(|| fail("ops_unknown_symbol", lib_id, "this symbol has not been opened in the Symbol Editor yet"))?;
        if sym.extends.is_none() {
            return Err(fail("ops_not_derived", lib_id, "Symbol is not derived from another symbol."));
        }
        let mut flat = section.flatten(lib_id).map_err(|e| fail("ops_bad_symbol", lib_id, e))?;
        flat.assign_missing_ids();
        let slot = self.library_symbol_mut(lib_id)?;
        *slot = flat;
        Ok(())
    }

    /// See [`Cmd::UpdateSymbolFields`](crate::Cmd::UpdateSymbolFields).
    pub(crate) fn update_symbol_fields_verb(&mut self, lib_id: &str, opts: UpdateFieldsOptions) -> Result<(), Vec<CheckResult>> {
        let section = self.design.symbol_library.as_ref().ok_or_else(|| fail("ops_unknown_symbol", lib_id, "the project symbol library is empty"))?;
        let sym = section.by_lib_id(lib_id).ok_or_else(|| fail("ops_unknown_symbol", lib_id, "this symbol has not been opened in the Symbol Editor yet"))?;
        let Some(parent_id) = sym.extends.clone().filter(|p| section.by_lib_id(p).is_some()) else {
            return Err(fail("ops_not_derived", lib_id, "Symbol is not derived from another symbol."));
        };
        // `m_symbol->GetParent().lock()->Flatten()`: the parent as it is once it has everything it inherits
        let parent_fields = section.flattened_fields(&parent_id).map_err(|e| fail("ops_bad_symbol", lib_id, e))?;
        let updated = sym_fields::update_symbol_fields(&sym.all_fields(), &parent_fields, &opts);
        self.library_symbol_mut(lib_id)?.set_fields(updated);
        Ok(())
    }

    /// See [`Cmd::SetSymbolParent`](crate::Cmd::SetSymbolParent).
    pub(crate) fn set_symbol_parent_verb(&mut self, lib_id: &str, parent: &str) -> Result<(), Vec<CheckResult>> {
        let section = self.design.symbol_library.as_ref().ok_or_else(|| fail("ops_unknown_symbol", lib_id, "the project symbol library is empty"))?;
        let sym = section.by_lib_id(lib_id).ok_or_else(|| fail("ops_unknown_symbol", lib_id, "this symbol has not been opened in the Symbol Editor yet"))?;
        if sym.extends.is_none() {
            return Err(fail("ops_not_derived", lib_id, "Symbol is not derived from another symbol."));
        }
        if parent.is_empty() {
            return Err(fail("ops_bad_symbol", lib_id, "Derived symbol must have a parent selected"));
        }
        if section.by_lib_id(parent).is_none() {
            return Err(fail("ops_unknown_symbol", parent, "the parent is not in the project symbol library"));
        }
        if sym_fields::library_of(parent) != sym_fields::library_of(lib_id) {
            return Err(fail("ops_bad_symbol", lib_id, format!("the parent of a derived symbol is in its own library ({:?})", sym_fields::library_of(lib_id))));
        }
        // "Don't allow a symbol to be derived from itself ... or from any of its descendants (would create circular inheritance)"
        if parent == lib_id || section.derived_names(lib_id).iter().any(|d| d == parent) {
            return Err(fail("ops_symbol_cycle", parent, "a symbol cannot be derived from itself or from a symbol derived from it"));
        }
        self.library_symbol_mut(lib_id)?.extends = Some(parent.to_string());
        Ok(())
    }

    /// See [`Cmd::EditLibrarySymbolFields`](crate::Cmd::EditLibrarySymbolFields).
    pub(crate) fn edit_library_symbol_fields_verb(&mut self, lib_id: &str, fields: &[LibrarySymbolField]) -> Result<(), Vec<CheckResult>> {
        let mut kept: Vec<LibrarySymbolField> = Vec::with_capacity(fields.len());
        for f in fields {
            if !is_mandatory_field(&f.name) {
                // "Fields must have a name."
                if f.name.trim().is_empty() {
                    if f.text.is_empty() {
                        continue; // an empty row of the grid
                    }
                    return Err(fail("ops_bad_field", lib_id, "Fields must have a name."));
                }
                if RESERVED_FIELD_NAMES.contains(&f.name.as_str()) {
                    return Err(fail("ops_bad_field", &f.name, format!("{:?} is the symbol's own keywords or footprint filters, not a field: use the Symbol Properties' Keywords and Footprint Filters", f.name)));
                }
            }
            if kept.iter().any(|k| k.name == f.name) {
                return Err(fail("ops_bad_field", &f.name, format!("Field name {:?} already in use", f.name)));
            }
            if !(f.size_mm > 0.0) {
                return Err(fail("ops_bad_field", &f.name, "a field's text size must be positive"));
            }
            kept.push(f.clone());
        }
        self.library_symbol_mut(lib_id)?.set_fields(kept);
        Ok(())
    }

    /// A rename follows what names the symbol: the children that extend it and a Value that was its name (`SYMBOL_EDITOR_CONTROL::RenameSymbol`:
    /// `if( libSymbol->GetValueField().GetText() == oldName ) SetText( newName )`).
    pub(crate) fn follow_symbol_rename(&mut self, old_lib_id: &str, new_lib_id: &str) {
        let (old_bare, new_bare) = (sym_fields::bare_name(old_lib_id).to_string(), sym_fields::bare_name(new_lib_id).to_string());
        let lib = self.symbol_library_mut();
        for s in lib.symbols.iter_mut() {
            if s.extends.as_deref() == Some(old_lib_id) {
                s.extends = Some(new_lib_id.to_string());
            }
        }
        if let Some(sym) = lib.by_lib_id_mut(new_lib_id) {
            rename_value(sym, &old_bare, &new_bare);
        }
    }

    /// `DeleteLibrarySymbol`: the symbol and every symbol derived from it (`LIB_BUFFER::DeleteBuffer`: "Remove all derived symbols to prevent broken inheritance").
    pub(crate) fn symbols_to_delete(&self, lib_id: &str) -> Vec<String> {
        let mut out = vec![lib_id.to_string()];
        if let Some(section) = &self.design.symbol_library {
            out.extend(section.derived_names(lib_id));
        }
        out
    }
}

/// A Value that is the symbol's old name becomes its new one.
fn rename_value(sym: &mut LibrarySymbol, old_bare: &str, new_bare: &str) {
    match sym.fields.iter_mut().find(|f| f.name == "Value") {
        Some(f) if f.text == old_bare => f.text = new_bare.to_string(),
        _ => {}
    }
}
