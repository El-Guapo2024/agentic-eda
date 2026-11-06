//! What File > Import > Netlist... (`DIALOG_IMPORT_NETLIST`, `BOARD_NETLIST_UPDATER`; `eda_ops::netlist_update`) leaves on the board that the board's own
//! derivation would take back on the next load: [`ImportedNetlist`], kept in `DrawingsSection::imported_netlist` and laid over the model last
//! (`crates/cli/src/board.rs::build_model`).
//!
//! KiCad's Import Netlist changes the board and nothing else: the schematic keeps its symbols, its fields and its nets, and the two disagree until the next
//! Update PCB from Schematic. Here the board's nets are the design's one net list (`Design::nets`), which the import writes as Swap Pad Nets does, but
//! every load names them after the schematic the board is drawn from (`net_names::kicad_names`), and every footprint takes its path, sheet, fields and
//! pin names from its symbol there (`SymbolLink`, `eda_kicad::symbol_links`). So what the netlist said of those is kept here and laid over that:
//!
//! * [`ImportedNetlist::nets`]: the names of the nets the netlist put pads on, which the load leaves as they are instead of naming them after the drawing;
//! * [`ImportedFootprint::link`]: each footprint's `FOOTPRINT::GetPath`, `GetSheetname`, `GetSheetfile`, its fields and its pads' pin functions and types
//!   (`updateFootprintParameters`, `updateComponentPadConnections`), in place of its symbol's;
//! * [`ImportedFootprint::value`]: the footprint's Value where the netlist gave another one than the part's (`aFootprint->SetValue`).
//!
//! An import of a later netlist replaces what an earlier one said of the footprints it matches and keeps the rest; Undo takes the whole step back.

use serde::{Deserialize, Serialize};

use crate::SymbolLink;

/// What one footprint took from its netlist component.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedFootprint {
    /// The footprint's reference (`FootprintInstance::id`).
    pub reference: String,
    /// Its link to the component (path, sheet name and file, fields, pins' functions and types).
    pub link: SymbolLink,
    /// `FOOTPRINT::GetValue` when it is not the part's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// `FOOTPRINT::GetFilters`: the component's `ki_fp_filters` (kept so that a second import of the same netlist has nothing to say of them).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub filters: String,
}

/// The board's record of the netlists imported onto it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedNetlist {
    /// The file the last import read (its name), for messages.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// The nets the netlist put pads on, by name, sorted: named as the netlist names them, not after the schematic the board is drawn from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nets: Vec<String>,
    /// One entry per footprint a netlist matched or added, sorted by reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprints: Vec<ImportedFootprint>,
}

impl ImportedNetlist {
    /// The entry of footprint `reference`.
    pub fn footprint(&self, reference: &str) -> Option<&ImportedFootprint> {
        self.footprints.iter().find(|f| f.reference == reference)
    }

    /// Lays the record over a model: the footprints' links, and the Value of each footprint it holds one for. A footprint the model has no part for is passed
    /// over (a reference renamed since: the record follows the rename where the rename verb knows of it).
    pub fn apply(&self, model: &mut crate::ConstraintModel) {
        for f in &self.footprints {
            let Some(part) = model.parts.iter_mut().find(|p| p.reference == f.reference) else { continue };
            if let Some(v) = &f.value {
                part.value = Some(v.clone());
            }
            model.symbol_links.insert(f.reference.clone(), f.link.clone());
        }
    }

    /// Whether `net` is a name the netlist gave.
    pub fn names(&self, net: &str) -> bool {
        self.nets.binary_search_by(|n| n.as_str().cmp(net)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record lays each footprint's link and Value over the model's part of that reference, and only those.
    #[test]
    fn the_record_is_laid_over_the_parts_it_names() {
        let mut model = crate::ConstraintModel::default();
        model.parts.push(crate::Part { reference: "R1".into(), mpn: None, lcsc: None, value: Some("1k".into()), package: None, footprint: None, symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None });
        let link = SymbolLink { path: "/abc".into(), sheetname: "/".into(), ..Default::default() };
        let rec = ImportedNetlist {
            source: "a.net".into(),
            nets: vec!["/A".into(), "GND".into()],
            footprints: vec![
                ImportedFootprint { reference: "R1".into(), link: link.clone(), value: Some("2k".into()), filters: String::new() },
                ImportedFootprint { reference: "GONE".into(), link: link.clone(), value: None, filters: String::new() },
            ],
        };
        rec.apply(&mut model);
        assert_eq!(model.parts[0].value.as_deref(), Some("2k"));
        assert_eq!(model.symbol_links.get("R1"), Some(&link));
        assert!(!model.symbol_links.contains_key("GONE"));
        assert!(rec.names("GND") && !rec.names("VCC"));
    }
}
