//! A symbol cross-reference follows its symbol through a rename: the sheet's half of [`Board::rename_part`].
//!
//! KiCad's eeschema dialogs store a `${R1:VALUE}` typed into a text, a text box, a table cell, a label or a field by the symbol's path
//! (`SCHEMATIC::ConvertRefsToKIIDs` in `DIALOG_TEXT_PROPERTIES`, `DIALOG_TABLE_PROPERTIES`, `DIALOG_TABLECELL_PROPERTIES`, `DIALOG_LABEL_PROPERTIES`,
//! `DIALOG_FIELD_PROPERTIES::UpdateField`, `DIALOG_SHEET_PROPERTIES` and `DIALOG_SYMBOL_PROPERTIES::TransferDataFromWindow`) and show it by the symbol's reference
//! again (`ConvertKIIDsToRefs` when they open), so a symbol annotated again is still the one the text names. This studio keeps the designator in the text (see
//! [`eda_model::cross_refs`]); a rename rewrites the cross-references of every text [`eda_model::cross_refs::for_each_cross_reference_text`] lists to the new
//! designators, which is what KiCad's text shows and edits afterwards: a placed symbol's Value, Footprint and Datasheet and its variants' field texts too
//! (`DIALOG_SYMBOL_PROPERTIES::TransferDataFromWindow` converts every field, a variant's text as well; UI-LEFT-3 S3-19). A symbol that shows its part's Value
//! (no text of its own) gets the part's text rewritten as its own, the part being the intent's and every placement of it showing the one text. **Differs**: a
//! cross-reference the file had by designator (one that never went through a KiCad dialog, so KiCad does not follow it) follows the rename here.

use super::Board;
use eda_model::cross_refs::{for_each_cross_reference_text, rename_cross_references};
use eda_model::ir::SchematicSection;
use std::collections::BTreeMap;

/// Every text of one sheet's screen that KiCad stores by path, rewritten; a symbol whose Value is its part's (`model`) takes the part's text, rewritten, when it
/// names a renamed symbol.
fn follow_on(sch: &mut SchematicSection, model: &eda_model::ConstraintModel, renames: &BTreeMap<String, String>) {
    // the part under the symbol's id, or under its old reference when the screen has renamed it already
    let old_of = |id: &str| renames.iter().find(|(_, new)| new.as_str() == id).map(|(old, _)| old.clone());
    for sym in sch.symbols.iter_mut().filter(|s| s.value.is_empty()) {
        let part = model.part(&sym.id).or_else(|| old_of(&sym.id).and_then(|o| model.part(&o)));
        if let Some(n) = part.and_then(|p| p.value.as_deref()).and_then(|v| rename_cross_references(v, renames)) {
            sym.value = n;
        }
    }
    for_each_cross_reference_text(sch, |text| {
        if let Some(n) = rename_cross_references(text, renames) {
            *text = n;
        }
    });
}

impl Board<'_> {
    /// The cross-references of every sheet (the root's screen and each sheet's) to a symbol of `renames` (old reference -> new) name the new reference.
    pub(crate) fn follow_cross_references(&mut self, renames: &BTreeMap<String, String>) {
        if renames.is_empty() {
            return;
        }
        let model = self.model;
        if let Some(s) = self.design.schematic.as_mut() {
            follow_on(s, model, renames);
        }
        if let Some(c) = self.design.sheet_contents.as_mut() {
            for s in c.values_mut() {
                follow_on(s, model, renames);
            }
        }
    }
}
