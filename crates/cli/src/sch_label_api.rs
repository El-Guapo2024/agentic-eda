//! `GET /api/sch/existing_labels?kind=local|global`: the names Label Properties offers in its Label combo box for a new or edited local or global label.
//!
//! Ported from KiCad 8303b2ad `eeschema/dialogs/dialog_label_properties.cpp::DIALOG_LABEL_PROPERTIES::TransferDataToWindow` (the same at 25ec2335):
//! "Load the combobox with the existing labels of the same type" -- for a global or a local label (`SCH_GLOBAL_LABEL_T` / `SCH_LABEL_T`; a hierarchical label
//! and a directive label get no list) the dialog walks every screen of the schematic (`SCH_SCREENS allScreens( m_Parent->Schematic().Root() )`) and takes
//!
//! * the text of every label of the same type on it (`screen->Items().OfType( m_currentLabel->Type() )`),
//! * for a global label, the Value of every power symbol that is like a global label (`IsSymbolLikePowerGlobalLabel`: a global power symbol whose library
//!   symbol has one pin, a power input), and for a local label the Value of every power symbol that is like a local label (`IsSymbolLikePowerLocalLabel`:
//!   the same with a local power symbol),
//! * then every bus alias of the project, written `{<alias>}` (`Schematic().GetAllBusAliases()`),
//!
//! into a `std::set<wxString>`: each name once, in `wxString`'s order (a code-point comparison, which is Rust's `str` order).
//!
//! **Differs**: KiCad shows each text through `UnescapeString`; the studio shows a label's text as it keeps it everywhere else (Label Properties'
//! own text, the canvas), so a name picked here is the stored one. The kind of a power symbol is the one connectivity gives it (`SchExtras::power_is_local`:
//! the screen's own cache where it disagrees with the design's library symbol, else the library symbol, `SCH_PIN::IsLocalPower`).

use crate::board;
use eda_model::ir::{Design, LabelKind};
use eda_model::ConstraintModel;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// Which label the dialog is for: the combo box is filled only for these two (`m_currentLabel->Type() == SCH_GLOBAL_LABEL_T || .. == SCH_LABEL_T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabelType {
    Local,
    Global,
}

/// `IsSymbolLikePowerGlobalLabel` / `IsSymbolLikePowerLocalLabel` without the kind: a power symbol whose library symbol has exactly one pin, a power input
/// (`GetAllLibPins().size() == 1 && pin_list[0]->GetType() == PT_POWER_IN`). `None` when no library symbol is known for the id (`!GetLibSymbolRef()`).
fn like_a_label(model: &ConstraintModel, lib_id: &str) -> Option<bool> {
    if lib_id.is_empty() || eda_model::is_synthetic_lib_id(lib_id) {
        return None;
    }
    let lib = model.symbol_of(lib_id)?;
    Some(lib.power && lib.pins.len() == 1 && lib.pins[0].electrical_type == "power_in")
}

/// The names `TransferDataToWindow` puts in the combo box of a label of type `ty`, in its order.
pub(crate) fn existing_labels(design: &Design, model: &ConstraintModel, ty: LabelType) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    // `SCH_SCREENS( Root() )`: every screen once, however many sheets show it
    let screens = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()));
    for sch in screens {
        for l in &sch.labels {
            let same = match ty {
                LabelType::Local => matches!(l.kind, LabelKind::Local),
                LabelType::Global => matches!(l.kind, LabelKind::Global { .. }),
            };
            if same {
                names.insert(l.net.clone());
            }
        }
        for ps in &sch.power_symbols {
            if like_a_label(model, &ps.lib_id) != Some(true) {
                continue;
            }
            let lib_local = model.symbol_of(&ps.lib_id).map(|lib| lib.power && lib.connectivity.power_local);
            let local = sch.extras.power_is_local(&ps.lib_id, lib_local);
            if local == (ty == LabelType::Local) {
                names.insert(ps.net.clone());
            }
        }
    }
    // "Add bus aliases to label list"
    for alias in &design.bus_aliases {
        names.insert(format!("{{{}}}", alias.name));
    }
    names.into_iter().collect()
}

/// The route: `{ "ok": true, "labels": [..] }`, empty for any other kind (a hierarchical label's dialog has no list).
pub(crate) fn existing_labels_route(dir: &Path, kind: &str) -> Value {
    let ty = match kind {
        "local" => LabelType::Local,
        "global" => LabelType::Global,
        _ => return json!({ "ok": true, "labels": [] }),
    };
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    json!({ "ok": true, "labels": existing_labels(&design, &model, ty) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{BusAlias, NetLabel, Point, PowerSymbol, SchematicSection};
    use eda_model::units::Nm;

    fn label(net: &str, kind: LabelKind) -> NetLabel {
        NetLabel { id: String::new(), net: net.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, kind }
    }

    fn power(lib_id: &str, net: &str) -> PowerSymbol {
        PowerSymbol { id: format!("#PWR{net}"), lib_id: lib_id.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, net: net.into(), pin: String::new() }
    }

    /// A one-pin power-input power symbol, global or local.
    fn rail(lib_id: &str, local: bool) -> eda_model::LibSymbol {
        let mut lib = eda_model::symbol::builtin("power:GND").expect("the built-in GND");
        lib.lib_id = lib_id.into();
        lib.connectivity.power_local = local;
        lib
    }

    #[test]
    fn every_screen_s_labels_the_power_nets_of_the_kind_and_the_bus_aliases() {
        let shape = eda_model::ir::LabelShape::Input;
        let mut root = SchematicSection::default();
        root.labels = vec![label("SDA", LabelKind::Local), label("CLK", LabelKind::Global { shape }), label("H", LabelKind::Hierarchical { shape })];
        root.power_symbols = vec![power("power:GND", "GND"), power("lp:+48V", "+48V"), power("power:PWR_FLAG", "PWR_FLAG")];
        let mut sheet = SchematicSection::default();
        sheet.labels = vec![label("MISO", LabelKind::Local), label("SDA", LabelKind::Local), label("RST", LabelKind::Global { shape })];
        // this screen's cache has the library's global +5V as a local one
        sheet.power_symbols = vec![power("lp:+5V", "+5V")];
        sheet.extras.local_power.insert("lp:+5V".into());
        let mut design: Design = serde_json::from_value(json!({ "provenance": { "engine_version": "t", "intent_hash": "x", "seed": 0, "stage_hashes": [] } })).expect("an empty design");
        design.schematic = Some(root);
        design.sheet_contents = Some([("sub.kicad_sch".to_string(), sheet)].into_iter().collect());
        design.bus_aliases = vec![BusAlias { name: "I2C".into(), members: vec!["SDA".into(), "SCL".into()] }];
        let model = ConstraintModel { symbols: vec![rail("lp:+48V", true), rail("lp:+5V", false)], ..Default::default() };

        assert_eq!(existing_labels(&design, &model, LabelType::Local), ["+48V", "+5V", "MISO", "SDA", "{I2C}"]);
        // PWR_FLAG's one pin is a power output: not like a label
        assert_eq!(existing_labels(&design, &model, LabelType::Global), ["CLK", "GND", "RST", "{I2C}"]);
    }
}
