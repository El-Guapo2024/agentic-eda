//! A footprint's gates: which pad numbers belong to which unit of a multi-unit symbol (`FOOTPRINT::FP_UNIT_INFO`, the footprint's `(units (unit (name "A") (pins "1" "2"
//! ..)) ..)`), the metadata Swap Gate Nets (`EDIT_TOOL::SwapGateNets`) reads.
//!
//! KiCad gets it from the netlist: the schematic exports, for each symbol, every unit of its library symbol with the pin numbers it has
//! (`NETLIST_EXPORTER_XML::makeSymbols`, `LIB_SYMBOL::GetUnitPinInfo`), and the netlist updater puts it on the footprint (`COMPONENT::SetFootprint`,
//! `BOARD_NETLIST_UPDATER::updateFootprintParameters`). A board file that went through that carries it as `(units ..)`; [`unit_pin_info`] is what the netlist
//! would say for a library symbol, for a board that has not been through KiCad's update.

use crate::annotate::unit_letters;
use crate::symbol::LibSymbol;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `FOOTPRINT::FP_UNIT_INFO`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintUnit {
    /// `m_unitName`: `A`, `B` ...
    pub name: String,
    /// `m_pins`: the pad numbers of the unit.
    pub pins: Vec<String>,
}

/// `(alphanumeric prefix, numeric suffix)` of a pin number, the numeric part `None` when there is no digit at the end (`ParseAlphaNumericPin`).
fn prefix_and_number(pin: &str) -> (String, Option<i64>) {
    let digits = pin.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return (pin.to_string(), None);
    }
    let split = pin.len() - digits;
    (pin[..split].to_string(), pin[split..].parse::<i64>().ok())
}

/// `ExpandStackedPinNotation`: `[1,2,5-7]` is the pins 1, 2, 5, 6 and 7 (a range keeps its alphabetic prefix: `[A1-A3]`); a number without brackets is itself, and
/// so is one the notation cannot be read from. The second value is whether the notation was valid.
pub fn expand_stacked_pin_notation(pin: &str) -> (Vec<String>, bool) {
    let has_brackets = pin.contains('[') || pin.contains(']');
    if has_brackets && !(pin.starts_with('[') && pin.ends_with(']')) {
        return (vec![pin.to_string()], false);
    }
    if !(pin.starts_with('[') && pin.ends_with(']')) {
        return (vec![pin.to_string()], true);
    }
    let inner = &pin[1..pin.len() - 1];
    let mut expanded = Vec::new();
    for part in inner.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.find('-') {
            Some(dash) => {
                let (start, end) = (part[..dash].trim(), part[dash + 1..].trim());
                let ((start_prefix, start_value), (end_prefix, end_value)) = (prefix_and_number(start), prefix_and_number(end));
                match (start_value, end_value) {
                    (Some(a), Some(b)) if start_prefix == end_prefix && a <= b => {
                        for n in a..=b {
                            expanded.push(format!("{start_prefix}{n}"));
                        }
                    }
                    _ => return (vec![pin.to_string()], false),
                }
            }
            None => expanded.push(part.to_string()),
        }
    }
    if expanded.is_empty() {
        return (vec![pin.to_string()], false);
    }
    (expanded, true)
}

/// `LIB_SYMBOL::GetUnitPinInfo`: for each unit of the symbol (at least one), its display name and the numbers of its pins -- the pins of the unit and the ones common to
/// every unit, whichever body style they are in, sorted by position (x, then y), a stacked number given as the pins it stands for, no number twice.
pub fn unit_pin_info(symbol: &LibSymbol) -> Vec<FootprintUnit> {
    let unit_count = symbol.unit_count.max(1);
    let mut all: Vec<&crate::symbol::LibPin> = symbol.pins.iter().collect();
    if let Some(alternate) = &symbol.alternate {
        all.extend(alternate.pins.iter());
    }
    (1..=unit_count)
        .map(|unit| {
            // `GetGraphicalPins( unit, 0 )`: a pin of unit 0 is common to all.
            let mut pins: Vec<&crate::symbol::LibPin> = all.iter().copied().filter(|p| p.unit == 0 || p.unit == unit).collect();
            pins.sort_by(|a, b| a.at.x.partial_cmp(&b.at.x).unwrap_or(std::cmp::Ordering::Equal).then(a.at.y.partial_cmp(&b.at.y).unwrap_or(std::cmp::Ordering::Equal)));
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut numbers = Vec::new();
            for pin in pins {
                let (expanded, valid) = expand_stacked_pin_notation(&pin.number);
                if valid && !expanded.is_empty() {
                    for n in expanded {
                        if seen.insert(n.clone()) {
                            numbers.push(n);
                        }
                    }
                } else if !pin.number.is_empty() && seen.insert(pin.number.clone()) {
                    numbers.push(pin.number.clone());
                }
            }
            FootprintUnit { name: unit_letters(unit), pins: numbers }
        })
        .collect()
}

/// The units of the footprint of `reference` (`FOOTPRINT::GetUnitInfo`): the ones its board file gave it, else the ones of the library symbol the schematic draws for
/// it, else of the one the part resolves to. Fewer than two units mean a footprint with no gates to swap.
pub fn unit_info(design: &crate::ir::Design, model: &crate::ConstraintModel, reference: &str) -> Vec<FootprintUnit> {
    if let Some(extra) = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)) {
        if !extra.units.is_empty() {
            return extra.units.clone();
        }
    }
    let Some(part) = model.part(reference) else { return Vec::new() };
    // The symbol the schematic placed for the part, on whichever sheet; else the one a derivation draws it with.
    let placed = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).flat_map(|s| s.symbols.iter()).find(|s| s.id == reference && !s.lib_id.is_empty()).map(|s| s.lib_id.clone());
    let lib_id = placed.unwrap_or_else(|| model.lib_id_of(part));
    // A symbol drawn for the part (the generated box, `eda:U1`) has one unit.
    if crate::symbol::is_synthetic_lib_id(&lib_id) {
        return Vec::new();
    }
    match model.symbol_of(&lib_id) {
        Some(symbol) => unit_pin_info(&symbol),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{LibPin, SPoint};

    fn pin(number: &str, unit: u32, x: f64, y: f64) -> LibPin {
        LibPin { number: number.into(), name: String::new(), electrical_type: "passive".into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: 0.0, length_mm: 2.54, unit, hidden: false }
    }

    fn symbol(unit_count: u32, pins: Vec<LibPin>) -> LibSymbol {
        let mut s = crate::symbol::builtin("Device:R").expect("a builtin symbol");
        s.unit_count = unit_count;
        s.pins = pins;
        s.alternate = None;
        s
    }

    #[test]
    fn stacked_numbers_expand_as_kicad_expands_them() {
        assert_eq!(expand_stacked_pin_notation("7"), (vec!["7".to_string()], true));
        assert_eq!(expand_stacked_pin_notation("[1,2,5-7]"), (["1", "2", "5", "6", "7"].map(String::from).to_vec(), true));
        assert_eq!(expand_stacked_pin_notation("[A1-A3]"), (["A1", "A2", "A3"].map(String::from).to_vec(), true));
        // A prefix that differs, an inverted range, a bracket left open: not valid, and the number is itself.
        assert_eq!(expand_stacked_pin_notation("[A1-B3]"), (vec!["[A1-B3]".to_string()], false));
        assert_eq!(expand_stacked_pin_notation("[3-1]"), (vec!["[3-1]".to_string()], false));
        assert_eq!(expand_stacked_pin_notation("[1,2"), (vec!["[1,2".to_string()], false));
    }

    #[test]
    fn a_quad_gate_has_four_units_each_with_its_own_pins_and_the_power_pins_of_unit_zero_in_all() {
        // A 74HC00-like symbol: four gates of three pins, the supply pins common to all four.
        let mut pins = Vec::new();
        for (u, (a, b, y)) in [("1", "2", "3"), ("4", "5", "6"), ("10", "9", "8"), ("13", "12", "11")].iter().enumerate() {
            let unit = u as u32 + 1;
            pins.push(pin(y, unit, 10.0, 0.0));
            pins.push(pin(a, unit, -10.0, 2.54));
            pins.push(pin(b, unit, -10.0, -2.54));
        }
        pins.push(pin("14", 0, 0.0, 10.0));
        pins.push(pin("7", 0, 0.0, -10.0));
        let units = unit_pin_info(&symbol(4, pins));
        assert_eq!(units.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["A", "B", "C", "D"]);
        // Sorted by position, x then y: the two inputs on the left (the one lower on the sheet, y smaller, first), the supply pins on the axis, the output on the right.
        assert_eq!(units[0].pins, ["2", "1", "7", "14", "3"]);
        assert_eq!(units[3].pins, ["12", "13", "7", "14", "11"]);
        assert!(units.iter().all(|u| u.pins.len() == 5));
    }

    #[test]
    fn a_single_unit_symbol_is_one_unit_and_a_number_stacked_or_repeated_is_listed_once() {
        let units = unit_pin_info(&symbol(1, vec![pin("1", 1, 0.0, 0.0), pin("[2,3]", 1, 2.0, 0.0), pin("3", 1, 3.0, 0.0)]));
        assert_eq!(units, vec![FootprintUnit { name: "A".into(), pins: vec!["1".into(), "2".into(), "3".into()] }]);
    }
}
