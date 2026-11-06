//! A symbol cross-reference follows its symbol through a rename: the sheet's half of [`Board::rename_part`].
//!
//! KiCad's eeschema dialogs store a `${R1:VALUE}` typed into a text, a text box or a table cell by the symbol's path (`SCHEMATIC::ConvertRefsToKIIDs` in
//! `DIALOG_TEXT_PROPERTIES`, `DIALOG_TABLE_PROPERTIES` and `DIALOG_TABLECELL_PROPERTIES::TransferDataFromWindow`) and show it by the symbol's reference again
//! (`ConvertKIIDsToRefs` when they open), so a symbol annotated again is still the one the text names. This studio keeps the designator in the text (see
//! [`eda_model::cross_refs`]); a rename rewrites the cross-references of the sheets' texts, text boxes and table cells to the new designators, which is what
//! KiCad's text shows and edits afterwards. **Differs**: a field's or a label's text is not rewritten (KiCad's field and label dialogs convert theirs too), and a
//! cross-reference the file had by designator (one that never went through a KiCad dialog, so KiCad does not follow it) follows the rename here.

use super::Board;
use eda_model::cross_refs::rename_cross_references;
use eda_model::ir::SchematicSection;
use eda_model::sch_extras::SchGraphicKind;
use std::collections::BTreeMap;

/// Every text, text box and table cell of one sheet's screen, rewritten.
fn follow_on(sch: &mut SchematicSection, renames: &BTreeMap<String, String>) {
    let fix = |text: &mut String| {
        if let Some(n) = rename_cross_references(text, renames) {
            *text = n;
        }
    };
    for t in sch.extras.tables.iter_mut() {
        for c in t.cells.iter_mut() {
            fix(&mut c.text);
        }
    }
    for t in sch.texts.iter_mut() {
        fix(&mut t.content);
    }
    for g in sch.extras.graphics.iter_mut() {
        if let SchGraphicKind::TextBox { text, .. } = &mut g.shape {
            fix(text);
        }
    }
}

impl Board<'_> {
    /// The cross-references of every sheet (the root's screen and each sheet's) to a symbol of `renames` (old reference -> new) name the new reference.
    pub(crate) fn follow_cross_references(&mut self, renames: &BTreeMap<String, String>) {
        if renames.is_empty() {
            return;
        }
        if let Some(s) = self.design.schematic.as_mut() {
            follow_on(s, renames);
        }
        if let Some(c) = self.design.sheet_contents.as_mut() {
            for s in c.values_mut() {
                follow_on(s, renames);
            }
        }
    }
}
