//! The fields grid of the Footprint Editor's Footprint Properties (`DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR`, pcbnew/dialogs/dialog_footprint_properties_fp_editor.cpp at
//! KiCad 8303b2ad): the Reference's and the Value's text and layout and the user fields of a footprint of the project library, each with its name, text and the layout of
//! its text ([`eda_model::fp_edit::FieldLayout`]: shown, size, thickness, italic, layer, orientation, keep upright, place, knockout, mirrored).
//!
//! They arrive as part of [`Cmd::EditFootprintProperties`](crate::Cmd::EditFootprintProperties), which is the whole General tab committing together; what it leaves out of
//! them stays. The rules are the board dialog's ([`Board::check_layout`]: a text size between 1 um and 250 mm either way, a pen the size allows, a layer the board has) and
//! `Validate`'s for the names: every user field has one, none is a mandatory field's, none repeats another's.

use super::Board;
use eda_model::fp_edit::{FieldLayout, REFERENCE, VALUE};
use eda_model::ir::FootprintField;
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn fail(name: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_bad_field", name, msg.into())]
}

impl Board<'_> {
    /// Judge the grid before anything of the panel is taken.
    pub(crate) fn check_library_fields(&self, footprint: &str, reference: Option<&FieldLayout>, value: Option<&FieldLayout>, fields: Option<&[FootprintField]>) -> Result<(), Vec<CheckResult>> {
        if let Some(l) = reference {
            self.check_layout(&format!("{footprint} Reference"), l)?;
        }
        if let Some(l) = value {
            self.check_layout(&format!("{footprint} Value"), l)?;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for f in fields.unwrap_or(&[]) {
            let name = f.name.trim();
            // `Validate`: "Fields must have a name."
            if name.is_empty() {
                return Err(fail(footprint, "Fields must have a name."));
            }
            if name != f.name {
                return Err(fail(footprint, format!("The field name {:?} starts or ends with a space.", f.name)));
            }
            if name == REFERENCE || name == VALUE || name == "Datasheet" || name == "Description" || name == "Footprint" {
                return Err(fail(footprint, format!("{name:?} is the name of a mandatory field.")));
            }
            if name.starts_with("ki_") {
                return Err(fail(footprint, format!("{name:?} is the footprint's own keywords or footprint filters, not a field.")));
            }
            if !seen.insert(name.to_lowercase()) {
                return Err(fail(footprint, format!("There are two fields called {name:?}.")));
            }
            if let Some(l) = &f.layout {
                self.check_layout(&format!("{footprint} {name}"), l)?;
            }
        }
        Ok(())
    }

    /// The Datasheet and Description rows of the grid: a layout the board's rules take (the rows' names are fixed).
    pub(crate) fn check_library_info_fields(&self, footprint: &str, datasheet: Option<&FieldLayout>, description: Option<&FieldLayout>) -> Result<(), Vec<CheckResult>> {
        if let Some(l) = datasheet {
            self.check_layout(&format!("{footprint} Datasheet"), l)?;
        }
        if let Some(l) = description {
            self.check_layout(&format!("{footprint} Description"), l)?;
        }
        Ok(())
    }

    /// Take the Datasheet and Description rows into the footprint (judged by [`Board::check_library_info_fields`]); a row left out stays as it is.
    pub(crate) fn apply_library_info_fields(&mut self, name: &str, datasheet: Option<&FieldLayout>, description: Option<&FieldLayout>) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(name)?;
        if let Some(l) = datasheet {
            fp.datasheet = Some(l.clone());
        }
        if let Some(l) = description {
            fp.description_field = Some(l.clone());
        }
        Ok(())
    }

    /// Take the grid into the footprint (judged by [`Board::check_library_fields`]). The Reference's and the Value's visibility is the footprint's own
    /// (`reference_visible`, `value_visible`: the Show check boxes), a user field's its `visible`: the layouts say the same.
    pub(crate) fn apply_library_fields(&mut self, name: &str, reference: Option<&FieldLayout>, value: Option<&FieldLayout>, fields: Option<&[FootprintField]>) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(name)?;
        if let Some(l) = reference {
            let mut l = l.clone();
            l.visible = fp.reference_visible;
            fp.reference = Some(l);
        } else if let Some(l) = fp.reference.as_mut() {
            l.visible = fp.reference_visible;
        }
        if let Some(l) = value {
            let mut l = l.clone();
            l.visible = fp.value_visible;
            fp.value = Some(l);
        } else if let Some(l) = fp.value.as_mut() {
            l.visible = fp.value_visible;
        }
        if let Some(list) = fields {
            fp.fields = list
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    if let Some(l) = f.layout.as_mut() {
                        l.visible = f.visible;
                    }
                    f
                })
                .collect();
        }
        Ok(())
    }
}
