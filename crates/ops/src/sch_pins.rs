//! Swap Pins, Swap Pin Labels and Swap Unit Labels: the three commands `SCH_SELECTION_TOOL` offers on a selection of symbol pins (or of the units of one
//! multi-unit reference), ported from `SCH_EDIT_TOOL::SwapPins`, `SwapPinLabels`, `SwapUnitLabels` and `findSingleNetLabelForPin`
//! (eeschema/tools/sch_edit_tool.cpp), `SwapPinGeometry`, `SymbolHasSheetInstances` and `GetSameSymbolMultiUnitSelection`
//! (eeschema/tools/sch_tool_utils.cpp at 8303b2ad).
//!
//! A pin is an item of the sheet here as it is in KiCad (`SCH_PIN_T`): `pin:<field_key>:<number>` (`eda_model::ir::pin_item_id`), in selection order.
//!
//! - **Swap Pins** trades the places of the pins, one after the other in selection order (pin 1 and 2, then 2 and 3, ...: a cycle). KiCad swaps position,
//!   orientation and length of the pins in the symbol's own copy of its library pins; the IR keeps that as `SchExtras::pin_swaps` (a pin that took
//!   another pin's place), which every pin position reads (`eda_engine::placed::pin_points_in`, the net tracer, the move tools) and the file writer bakes
//!   into the symbol's own cached copy (`lib_name`). The wires stay where they are: pin 1 now sits where pin 2 was, so it is on the net that wire joined.
//!   Afterwards KiCad adds the junctions the moved pins need and cleans the sheet up. Refused for a symbol of a sheet that is placed more than once
//!   ("Pin swaps are disabled for symbols used by multiple sheet instances"), for pins of different symbols, and for fewer than two pins. (KiCad also
//!   hides the command until Preferences > Editing Options > "Allow unconstrained pin swaps" is on: that is the studio's setting, checked before the verb.)
//! - **Swap Pin Labels** trades the text of the net labels on the pins, cycling in selection order. Each pin must have exactly one net label on its
//!   connection and no other pin there.
//! - **Swap Unit Labels** does the same across the units of one multi-unit reference: the labels of each unit are ordered by pin position (x, then y), and
//!   for every pin index the label texts are passed one unit along.
//!
//! The nets are not touched by the verbs themselves: swapping pins moves pins under wires, swapping labels renames nets, and the connection graph is
//! read again after any schematic edit (`reconcile_schematic`).

use crate::sch_scene::{on_segment, Scene};
use crate::Board;
use eda_model::ir::{field_key, parse_pin_item_id, Point};
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn fail(check: &str, subject: &str, why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, why)]
}

/// `findSingleNetLabelForPin`'s message when a pin has no label of its own on its connection.
pub const NO_PIN_LABEL: &str = "Each selected pin must have exactly one attached net label and no other pin connections.";
/// ... and `SwapUnitLabels`'s.
pub const NO_UNIT_LABEL: &str = "Each pin of selected units must have exactly one attached net label and no other pin connections.";

/// One resolved pin: the symbol it is on (index into `sch.symbols`), its number and where it is.
struct PinRef {
    sym: usize,
    key: String,
    number: String,
    at: Point,
}

fn resolve_pin(scene: &Scene, id: &str) -> Result<PinRef, Vec<CheckResult>> {
    let Some((key, number)) = parse_pin_item_id(id) else {
        return Err(fail("ops_unknown_pin", id, "not a pin of a symbol (pin:<reference>:<number>)"));
    };
    let Some(i) = scene.sch.symbols.iter().position(|s| field_key(&s.id, s.unit) == key) else {
        return Err(fail("ops_unknown_symbol", id, format!("no symbol with the reference '{key}' on the sheet")));
    };
    let Some((_, at)) = scene.pins_of(i).into_iter().find(|(n, _)| n == number) else {
        return Err(fail("ops_unknown_pin", id, format!("the symbol '{key}' has no pin '{number}'")));
    };
    Ok(PinRef { sym: i, key: key.to_string(), number: number.to_string(), at })
}

fn dedup_check(pins: &[PinRef], ids: &[String]) -> Result<(), Vec<CheckResult>> {
    let mut seen = BTreeSet::new();
    for (p, id) in pins.iter().zip(ids) {
        if !seen.insert((p.key.clone(), p.number.clone())) {
            return Err(fail("ops_duplicate_pin", id, "a pin is listed twice"));
        }
    }
    Ok(())
}

/// The connected points and wires of the connection that includes `start` (`CONNECTION_SUBGRAPH`, on this sheet): wires join where one ends on the other
/// (a T included), a junction dot joins the wires that pass through it, and whatever sits on a wire or at one of its points is on the connection.
fn connection_of(scene: &Scene, start: Point) -> (BTreeSet<usize>, BTreeSet<Point>) {
    let mut pts: BTreeSet<Point> = BTreeSet::from([start]);
    let mut segs: BTreeSet<usize> = BTreeSet::new();
    loop {
        let before = (segs.len(), pts.len());
        for (i, s) in scene.segs.iter().enumerate() {
            if s.dead || s.bus || segs.contains(&i) {
                continue;
            }
            let touched = pts.iter().any(|&p| on_segment(s.a, s.b, p)) || segs.iter().any(|&j| on_segment(scene.segs[j].a, scene.segs[j].b, s.a) || on_segment(scene.segs[j].a, scene.segs[j].b, s.b));
            if touched {
                segs.insert(i);
                pts.insert(s.a);
                pts.insert(s.b);
            }
        }
        for j in &scene.sch.junctions {
            if segs.iter().any(|&i| on_segment(scene.segs[i].a, scene.segs[i].b, j.at)) {
                pts.insert(j.at);
            }
        }
        if (segs.len(), pts.len()) == before {
            return (segs, pts);
        }
    }
}

/// `findSingleNetLabelForPin`: the index of the one net label on the connection of the pin at `tip`, or `None` when the connection has no label, more than
/// one, or reaches another pin (a power symbol's pin counts: it is a symbol with a pin).
fn single_net_label(scene: &Scene, tip: Point) -> Option<usize> {
    let (segs, pts) = connection_of(scene, tip);
    let on = |p: Point| pts.contains(&p) || segs.iter().any(|&i| on_segment(scene.segs[i].a, scene.segs[i].b, p));
    let mut pin_count = 0;
    for i in 0..scene.sch.symbols.len() {
        pin_count += scene.pins_of(i).into_iter().filter(|(_, at)| on(*at)).count();
    }
    pin_count += scene.sch.power_symbols.iter().filter(|p| on(p.at)).count();
    if pin_count != 1 {
        return None;
    }
    let mut found: Option<usize> = None;
    for (li, l) in scene.sch.labels.iter().enumerate() {
        // `conn->IsNet()`: a label with a bus name is a bus, not a net
        if on(l.at) && !crate::sch_scene::is_bus_label(&l.net) {
            if found.is_some() {
                return None; // more than one label
            }
            found = Some(li);
        }
    }
    found
}

/// The number of sheets that show the screen the verbs act on (`SymbolHasSheetInstances`: a symbol with several instance paths is shared); the root has none.
fn sheet_instances(b: &Board<'_>) -> Vec<String> {
    let Some(file) = &b.focus else { return Vec::new() };
    let screens = b.design.schematic.iter().chain(b.design.sheet_contents.iter().flatten().map(|(_, s)| s));
    screens.flat_map(|s| s.sheets.iter()).filter(|sh| &sh.file == file).map(|sh| sh.name.clone()).collect()
}

impl<'a> Board<'a> {
    /// [`SchCmd::SwapPins`](crate::sch_edit::SchCmd::SwapPins): `SCH_EDIT_TOOL::SwapPins`.
    pub(crate) fn swap_pins(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic()?;
        let scene = Scene::new(sch, self.model);
        // `selection.Size() < 2` returns without doing anything
        if ids.len() < 2 {
            return Err(fail("ops_swap_pins_few", ids.first().map(String::as_str).unwrap_or("pins"), "select at least two pins of one symbol to swap"));
        }
        let pins: Vec<PinRef> = ids.iter().map(|id| resolve_pin(&scene, id)).collect::<Result<_, _>>()?;
        dedup_check(&pins, ids)?;
        // "All pins need to be on the same symbol"
        if let Some(other) = pins.iter().zip(ids).find(|(p, _)| p.sym != pins[0].sym) {
            return Err(fail("ops_swap_pins_symbols", other.1, "all the pins to swap must be on the same symbol"));
        }
        // `SymbolHasSheetInstances`: a symbol of a sheet placed more than once cannot be changed for one placement only
        let shared = sheet_instances(self);
        if shared.len() > 1 {
            return Err(fail(
                "ops_swap_pins_shared",
                &pins[0].key,
                format!("Pin swaps are disabled for symbols used by multiple sheet instances ({}). Duplicate the sheet to edit pins independently.", shared.join(", ")),
            ));
        }
        let key = pins[0].key.clone();
        let sym = pins[0].sym;
        let numbers: Vec<String> = pins.iter().map(|p| p.number.clone()).collect();
        // `for i in 0..n-1: SwapPinGeometry( sorted[i], sorted[i+1] )`
        let model = self.model;
        let sch = self.schematic_mut()?;
        for w in numbers.windows(2) {
            sch.extras.swap_pin_places(&key, &w[0], &w[1]);
        }
        // The pins are somewhere else now: the sheet is read again from where they are, the junctions they need are added (`AddJunctionsIfNeeded`)
        // and the sheet is cleaned up (`SCHEMATIC::CleanUp`). (`TrimOverLappingWires` finds nothing for a single pin: it looks for a line that
        // meets one item at exactly two points, and a pin has one.)
        let mut scene = Scene::new(sch, model);
        let tips: Vec<Point> = scene.pins_of(sym).into_iter().filter(|(n, _)| numbers.contains(n)).map(|(_, at)| at).collect();
        for p in tips {
            if scene.is_explicit_junction_needed(p) {
                scene.add_junction(p);
            }
        }
        scene.clean_up();
        *self.schematic_mut()? = scene.finish();
        Ok(())
    }

    /// [`SchCmd::SwapPinLabels`](crate::sch_edit::SchCmd::SwapPinLabels): `SCH_EDIT_TOOL::SwapPinLabels`.
    pub(crate) fn swap_pin_labels(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic()?;
        let scene = Scene::new(sch, self.model);
        if ids.len() < 2 {
            return Err(fail("ops_swap_labels_few", ids.first().map(String::as_str).unwrap_or("pins"), "select at least two pins to swap the labels of"));
        }
        let pins: Vec<PinRef> = ids.iter().map(|id| resolve_pin(&scene, id)).collect::<Result<_, _>>()?;
        dedup_check(&pins, ids)?;
        let mut labels: Vec<usize> = Vec::new();
        for (p, id) in pins.iter().zip(ids) {
            let Some(l) = single_net_label(&scene, p.at) else {
                return Err(fail("ops_swap_labels_unattached", id, NO_PIN_LABEL));
            };
            labels.push(l);
        }
        // two pins on one connection would have been refused above (the connection holds two pins), so the labels are all different
        let sch = self.schematic_mut()?;
        for w in labels.windows(2) {
            let (a, b) = (sch.labels[w[0]].net.clone(), sch.labels[w[1]].net.clone());
            sch.labels[w[0]].net = b;
            sch.labels[w[1]].net = a;
        }
        Ok(())
    }

    /// [`SchCmd::SwapUnitLabels`](crate::sch_edit::SchCmd::SwapUnitLabels): `SCH_EDIT_TOOL::SwapUnitLabels`.
    pub(crate) fn swap_unit_labels(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic()?;
        // `GetSameSymbolMultiUnitSelection`: symbols of one reference and one library symbol, which has several units, with the same pin count
        let Some(reference) = ids.first() else {
            return Err(fail("ops_swap_units", "units", "select the units of one multi-unit symbol"));
        };
        if ids.iter().any(|id| id != reference) {
            return Err(fail("ops_swap_units", reference, "the units to swap must be units of one reference"));
        }
        let mut units: Vec<usize> = sch.symbols.iter().enumerate().filter(|(_, s)| &s.id == reference).map(|(i, _)| i).collect();
        units.sort_by_key(|&i| sch.symbols[i].unit);
        if units.is_empty() {
            return Err(fail("ops_unknown_symbol", reference, "no symbol with this reference on the sheet"));
        }
        let lib_id = &sch.symbols[units[0]].lib_id;
        let multi = self.model.symbol_of(lib_id).is_some_and(|l| l.unit_count >= 2);
        if !multi || units.iter().any(|&i| &sch.symbols[i].lib_id != lib_id) {
            return Err(fail("ops_swap_units", reference, "the symbol has no units to swap the labels of"));
        }
        if units.len() < 2 {
            return Err(fail("ops_swap_units", reference, "only one unit of this symbol is placed"));
        }
        let scene = Scene::new(sch, self.model);
        let pin_count = scene.shapes[units[0]].pins.len();
        if units.iter().any(|&i| scene.shapes[i].pins.len() != pin_count) {
            return Err(fail("ops_swap_units", reference, "the units have different numbers of pins"));
        }
        // the labels of each unit, ordered by pin position (x, then y)
        let mut vectors: Vec<Vec<usize>> = Vec::new();
        for &i in &units {
            let mut by_pos: Vec<(Point, usize)> = Vec::new();
            for (_, at) in scene.pins_of(i) {
                let Some(l) = single_net_label(&scene, at) else {
                    return Err(fail("ops_swap_labels_unattached", reference, NO_UNIT_LABEL));
                };
                by_pos.push((at, l));
            }
            by_pos.sort_by_key(|(at, _)| (at.x, at.y));
            vectors.push(by_pos.into_iter().map(|(_, l)| l).collect());
        }
        // for every pin index, the texts move one unit along: the last unit's text goes to the first
        let sch = self.schematic_mut()?;
        for pin in 0..pin_count {
            let mut carry = sch.labels[vectors[vectors.len() - 1][pin]].net.clone();
            for v in &vectors {
                let l = &mut sch.labels[v[pin]];
                let next = std::mem::replace(&mut l.net, carry);
                carry = next;
            }
        }
        Ok(())
    }
}
