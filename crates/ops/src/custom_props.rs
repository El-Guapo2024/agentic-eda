//! The board items' "Custom Properties" (KiCad 25ec2335 `EDA_ITEM::m_customProperties`, `common/eda_item.cpp`): a name and a text, as many as an item has,
//! shown by the Properties panel in a group of their own ("Custom Properties", `EDA_CUSTOM_PROPERTY`) and written in the board file as
//! `(custom_property "name" "text")` (`KICAD_FORMAT::FormatCustomProperties`). The panel adds one (`PCB_PROPERTIES_PANEL::addBlankCustomProperty`), edits its text
//! (`EDA_CUSTOM_PROPERTY::setter`), renames it (`onPropertyLabelEdited`) and removes it (`removeCustomProperty`); here each is [`Cmd::SetCustomProperty`], one undo step.
//! The design keeps them by item id (`DrawingsSection::custom_properties`).
//!
//! [`Cmd::SetCustomProperty`]: crate::Cmd::SetCustomProperty

use std::collections::BTreeSet;

use eda_model::CheckResult;

use crate::Board;

impl Board<'_> {
    /// Every id a custom property can be kept under: the board's items ([`crate::pcb_groups::universe`]) and the pads of its footprints (`REF.NUM`, `REF.NUM#k`:
    /// `PAD` is an `EDA_ITEM` with properties of its own, `format( const PAD* )` writes them).
    fn custom_property_holders(&self) -> (BTreeSet<String>, BTreeSet<String>) {
        let items = crate::pcb_groups::universe(&self.design);
        let footprints: BTreeSet<String> = self.design.placement.iter().flat_map(|p| p.footprints.iter().map(|f| f.id.clone())).collect();
        (items, footprints)
    }

    fn holds_custom_properties(items: &BTreeSet<String>, footprints: &BTreeSet<String>, id: &str) -> bool {
        items.contains(id) || id.split_once('.').is_some_and(|(reference, pad)| !pad.is_empty() && footprints.contains(reference))
    }

    /// `Cmd::SetCustomProperty` -- see that variant's own doc. `SetCustomProperty( aKey, aValue )` replaces the text of a name the item has (the map's own order
    /// is the names', `std::map`).
    pub(crate) fn set_custom_property(&mut self, ids: &[String], name: &str, value: Option<&str>, rename_to: Option<&str>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_custom_property", "set_custom_property", "no items given")]);
        }
        if name.is_empty() || rename_to.is_some_and(str::is_empty) {
            return Err(vec![CheckResult::fail("ops_bad_custom_property", name, "a custom property needs a name")]);
        }
        let (items, footprints) = self.custom_property_holders();
        for id in ids {
            if !Self::holds_custom_properties(&items, &footprints, id) {
                return Err(vec![CheckResult::fail("ops_unknown_item", id, "no board item with this id")]);
            }
        }
        let dr = self.drawings_mut();
        for id in ids {
            match (rename_to, value) {
                // `onPropertyLabelEdited`: `if( GetCustomProperty( aOldName, value ) ) { RemoveCustomProperty( aOldName ); SetCustomProperty( aNewName, value ); }`
                (Some(new_name), _) => {
                    if let Some(props) = dr.custom_properties.get_mut(id) {
                        if let Some(text) = props.remove(name) {
                            props.insert(new_name.to_string(), text);
                        }
                    }
                }
                (None, Some(text)) => {
                    dr.custom_properties.entry(id.clone()).or_default().insert(name.to_string(), text.to_string());
                }
                (None, None) => {
                    if let Some(props) = dr.custom_properties.get_mut(id) {
                        props.remove(name);
                    }
                }
            }
        }
        dr.custom_properties.retain(|_, props| !props.is_empty());
        Ok(())
    }

    /// An item that is gone takes its custom properties with it (the `EDA_ITEM` is deleted), so a later item that gets its id starts with none.
    pub(crate) fn tidy_custom_properties(&mut self) {
        if self.design.drawings.as_ref().is_none_or(|d| d.custom_properties.is_empty()) {
            return;
        }
        let (items, footprints) = self.custom_property_holders();
        if let Some(dr) = self.design.drawings.as_mut() {
            dr.custom_properties.retain(|id, props| !props.is_empty() && Self::holds_custom_properties(&items, &footprints, id));
        }
    }
}
