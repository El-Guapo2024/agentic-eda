//! Update Schematic from PCB's nets (`common.Control.updateSchematicFromPCB`, `DIALOG_UPDATE_FROM_PCB`'s "Net names", "Prefer symbol unit swaps over label swaps" and
//! "Prefer symbol pin swaps over label swaps"): what the schematic must change so that each pin is on the net its pad has on the board. Ported from KiCad 25ec2335
//! `eeschema/tools/backannotate.cpp` -- `BACK_ANNOTATE::applyChangelist` (its unit-swap pass and its net-name loop), `CollectBackannotatePcbNets`,
//! `PlanBackannotateUnitSwaps`, `netsInUnitOrder`, `applyPinSwaps`, `processNetNameChange`, `orientLabel` and `addConnections` -- which 8303b2ad has too, a little
//! older (no stacked pins in the unit planner, no `PUBLICATION_HOLD`); `eeschema/tools/sch_tool_utils.cpp` (`SwapPinGeometry`, `SymbolHasSheetInstances`).
//!
//! The board's net of a pad is the net list in force (`design.nets`, else the intent's), which Swap Pad Nets and Swap Gate Nets change; the schematic's net of a
//! pin is what the drawing traces to (`eda_engine::nets`, KiCad's names). KiCad compares the two by their short names (`NETINFO_ITEM::GetShortNetname`, the
//! text after the last `/`, against `SCH_ITEM::GetConnectionName( &sheet, false, true )`), and so does [`plan_net_back_annotation`]. Every net is read before
//! anything changes, as KiCad's `PUBLICATION_HOLD` reads them.
//!
//! * **Unit swaps** (`m_preferUnitSwaps`): the placed units of one footprint whose pads' nets, unit by unit in the library's pin order, are the nets another of its
//!   units is drawn on trade units (`SCH_SYMBOL::SetUnit`): "Swap %s unit %s with unit %s." -- what a Swap Gate Nets on the board did, carried back. A symbol so
//!   swapped is passed over by the rest (`SKIP_STRUCT`).
//! * **Pin swaps** (`m_preferPinSwaps`): two pins of one symbol each on the net the other's pad has trade places (`SwapPinGeometry`, the studio's Swap Pins):
//!   "Swap %s pins %s and %s to match PCB." -- only with Preferences > Editing Options > "Allow unconstrained pin swaps" on and a symbol on a sheet placed once;
//!   otherwise KiCad says why not ("Would swap %s pins %s and %s to match PCB, but ...") and the pins go on to the labels.
//! * **Net names** (`m_processNetNames`): for every other pin on another net than its pad, the driver of what the pin is wired to on its sheet is renamed
//!   ("Change %s pin %s net label from '%s' to '%s'."), or, where nothing but pins is there, a label is put on the pin ("Add label '%s' to %s pin %s net."); a net
//!   a power symbol drives is not changed ("Net %s cannot be changed to %s because it is driven by a power pin."). A pad with no pin is "Cannot find %s pin '%s'.".
//!
//! What comes out is the report and the studio's own verbs that do it (`SetSymbolUnit` with `swap`, `SwapPins`, `EditLabel`, `EditSheetPin`, `PlaceLabel`, each on
//! its sheet), which the dialog sends with its footprint changes as one undo step of both editors; the net list then follows the drawing
//! (`reconcile_schematic`), which now draws the board's nets.
//!
//! Units of one footprint placed on different sheets, or on a sheet shown by more than one placement, are swapped too (UI-LEFT-3 P3-34): KiCad sets each
//! symbol's unit on its own sheet path (`SetUnit`, `SetUnitSelection( sheet, .. )`), the studio sends a `SetSymbolUnit` on each sheet (one with `swap` where
//! the two are on one sheet path). The driver walk takes the symbols' pins as `GetDriverPriority` does: a hidden power input of an ordinary symbol is a
//! global power pin (`SCH_PIN::IsGlobalPower`'s legacy rule), so a net it drives is not renamed. `processNetNameChange`'s fallback to the connection graph's
//! resolved driver, for when "the physical connection walk could not find a label driver" (a label in the middle of an unsplit wire, which `ConnectedItems`
//! does not reach), is the walk itself here: [`driver_at`] finds a label anywhere on a wire of the pin's connection, which is the subgraph the fallback reads,
//! so what the fallback would rename (the label, a power symbol the subgraph has) the walk already found.
//!
//! **Differs** (the studio's one design, or what it does not hold): the order of the report is the sheets' walk and the symbols' order on each, where KiCad
//! sorts by the symbols' uuids; the label a pin gets reads away from the pin as the symbol draws it (`orientLabel` reads the library pin, which a pin swap
//! does not move).

use std::collections::{BTreeMap, BTreeSet};

use eda_engine::nets::{base_pin, TracedNet};
use eda_model::fp_units::{expand_stacked_pin_notation, unit_pin_info};
use eda_model::ir::{field_key, pin_item_id, Design, LabelKind, Point, SchematicSection};
use eda_model::sch_extras::LabelSpin;
use eda_model::sheet_list::{reference_under, screen_of, unit_under, walk};
use eda_model::{ConstraintModel, Net};
use serde::{Deserialize, Serialize};

use crate::sch_edit::SchCmd;
use crate::sch_scene::{on_segment, placed_lib_symbol, Matrix, Scene};
use crate::Cmd;

/// The dialog's check boxes that the nets read (`DIALOG_UPDATE_FROM_PCB`: "Net names", "Prefer symbol unit swaps over label swaps", "Prefer symbol pin swaps over
/// label swaps", all on by default), and the schematic preference "Allow unconstrained pin swaps" (`EESCHEMA_SETTINGS::m_Input.allow_unconstrained_pin_swaps`,
/// off by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackAnnotateOptions {
    pub net_names: bool,
    pub prefer_unit_swaps: bool,
    pub prefer_pin_swaps: bool,
    pub allow_pin_swaps: bool,
}

impl Default for BackAnnotateOptions {
    fn default() -> Self {
        BackAnnotateOptions { net_names: true, prefer_unit_swaps: true, prefer_pin_swaps: true, allow_pin_swaps: false }
    }
}

/// One line of the dialog's report (`REPORTER::ReportHead`): `severity` is `action`, `info`, `warning` or `error` (`RPT_SEVERITY_*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackAnnotateLine {
    pub severity: &'static str,
    pub text: String,
}

/// What Update Schematic from PCB does to the nets: its report and the verbs that do it, in order.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NetBackAnnotation {
    pub lines: Vec<BackAnnotateLine>,
    pub cmds: Vec<Cmd>,
}

/// `NETINFO_ITEM::SetNetname`'s short name: the text after the last `/` (`/Sheet/A` is `A`).
fn short_name(net: &str) -> &str {
    net.rsplit('/').next().unwrap_or(net)
}

/// One placed symbol of the changelist (`CHANGELIST_ITEM`: a `SCH_REFERENCE` and its footprint's data).
#[derive(Clone)]
struct Item {
    /// The placement path of its sheet, and the screen's index of the symbol.
    path: Vec<String>,
    sym: usize,
    /// `SCH_REFERENCE::GetRef`: the reference this placement gives it, without the unit.
    reference: String,
    /// The unit it is drawn as there.
    unit: u32,
    /// The screen is shown by more than one placement (`SymbolHasSheetInstances`): the sheet names.
    shared_sheets: Vec<String>,
}

/// A schematic driver's place in `CONNECTION_SUBGRAPH::PRIORITY`, and what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Driver {
    /// `PIN`: an ordinary pin (the pin itself).
    Pin,
    SheetPin(usize, usize),
    HierLabel(usize),
    LocalLabel(usize),
    LocalPower,
    GlobalPower,
    GlobalLabel(usize),
}

impl Driver {
    fn priority(self) -> u8 {
        match self {
            Driver::Pin => 1,
            Driver::SheetPin(..) => 2,
            Driver::HierLabel(_) => 3,
            Driver::LocalLabel(_) => 4,
            Driver::LocalPower => 5,
            Driver::GlobalPower => 6,
            Driver::GlobalLabel(_) => 7,
        }
    }
}

fn on_sheet(path: &[String], cmd: SchCmd) -> Cmd {
    Cmd::OnSheet { sheet: path.join("/"), cmd: Box::new(Cmd::SchEdit(cmd)) }
}

/// The points and wire segments of the connection the point `start` is on, on this sheet (`addConnections`: what is physically connected).
fn connection_at<'s>(scene: &'s Scene<'_>, start: Point) -> impl Fn(Point) -> bool + 's {
    let (segs, pts) = crate::sch_pins::connection_of(scene, start);
    move |p: Point| pts.contains(&p) || segs.iter().any(|&i| on_segment(scene.segs[i].a, scene.segs[i].b, p))
}

/// `processNetNameChange`'s driver walk: the best driver of what the pin at `tip` is connected to on its sheet (`GetDriverPriority`, the first of the best).
fn driver_at(scene: &Scene, model: &ConstraintModel, tip: Point) -> Driver {
    let on = connection_at(scene, tip);
    let mut best = Driver::Pin;
    let mut consider = |d: Driver| {
        if d.priority() > best.priority() {
            best = d;
        }
    };
    for (i, l) in scene.sch.labels.iter().enumerate() {
        if !on(l.at) || crate::sch_scene::is_bus_label(&l.net) {
            continue;
        }
        consider(match l.kind {
            LabelKind::Local => Driver::LocalLabel(i),
            LabelKind::Global { .. } => Driver::GlobalLabel(i),
            LabelKind::Hierarchical { .. } => Driver::HierLabel(i),
        });
    }
    for (si, sheet) in scene.sch.sheets.iter().enumerate() {
        for (pi, pin) in sheet.pins.iter().enumerate() {
            if on(pin.at) {
                consider(Driver::SheetPin(si, pi));
            }
        }
    }
    // `GetDriverPriority( SCH_PIN )` of the symbols' pins: a hidden power input of an ordinary symbol is a global power pin (`SCH_PIN::IsGlobalPower`: "Legacy
    // support: invisible power-in pins on non-power symbols act as global power"), a local power symbol's power input a local one; the pin itself counts too
    for (si, sym) in scene.sch.symbols.iter().enumerate() {
        let pins: Vec<(String, Point)> = scene.pins_of(si).into_iter().filter(|(_, at)| on(*at)).collect();
        if pins.is_empty() {
            continue;
        }
        let Some(lib) = placed_lib_symbol(model, &scene.sch, sym) else { continue };
        for (number, _) in pins {
            let Some(pin) = lib.pins.iter().chain(lib.other_bodies().flat_map(|a| a.pins.iter())).find(|p| p.number == number) else { continue };
            if pin.electrical_type != "power_in" {
                continue;
            }
            if lib.power && lib.connectivity.power_local {
                consider(Driver::LocalPower);
            } else if lib.power || pin.hidden {
                consider(Driver::GlobalPower);
            }
        }
    }
    for ps in &scene.sch.power_symbols {
        if on(ps.at) {
            // a PWR_FLAG's pin is no power input and names nothing (`SCH_PIN::IsGlobalPower`)
            let lib = model.symbol_of(&ps.lib_id);
            let local = lib.as_ref().is_some_and(|l| l.power && l.connectivity.power_local);
            let names = lib.as_ref().map_or(true, |l| l.pins.iter().any(|p| p.electrical_type == "power_in"));
            if local {
                consider(Driver::LocalPower);
            } else if names {
                consider(Driver::GlobalPower);
            }
        }
    }
    best
}

/// `orientLabel`: the spin of a label put on a pin -- away from the pin (`PIN_RIGHT` gives `LEFT`, ...), turned and mirrored with the symbol.
fn label_spin(scene: &Scene, sym: usize, number: &str) -> LabelSpin {
    let s = &scene.sch.symbols[sym];
    let shape = &scene.shapes[sym];
    let body = shape.pins.iter().position(|(n, _)| n == number).and_then(|k| shape.dirs.get(k)).map(|&(dx, dy)| Matrix::of(s.rot, s.mirrored, s.mirror_y).apply_dir(dx, dy)).unwrap_or((1, 0));
    match body {
        (1, 0) => LabelSpin::Left,
        (-1, 0) => LabelSpin::Right,
        (0, -1) => LabelSpin::Bottom,
        (0, 1) => LabelSpin::Up,
        _ => LabelSpin::Left,
    }
}

/// Builds the plan of [`plan_net_back_annotation`].
struct Planner<'a> {
    design: &'a Design,
    model: &'a ConstraintModel,
    /// `REF.PIN` -> the short name of the net the drawing puts it on.
    drawn: BTreeMap<String, String>,
    /// footprint reference -> pad number -> the short name of its board net (`m_pinMap`: pads with a net only).
    pads: BTreeMap<String, BTreeMap<String, String>>,
    scenes: BTreeMap<Vec<String>, Scene<'a>>,
    out: NetBackAnnotation,
    /// The label texts the plan sets, by sheet path and label index (a second change of one label replaces the first, as KiCad's second `SetText` does).
    label_texts: BTreeMap<(Vec<String>, usize), String>,
    sheet_pin_names: BTreeMap<(Vec<String>, usize, usize), String>,
}

impl<'a> Planner<'a> {
    fn line(&mut self, severity: &'static str, text: String) {
        self.out.lines.push(BackAnnotateLine { severity, text });
    }

    fn screen(&self, path: &[String]) -> &'a SchematicSection {
        screen_of(self.design, path).expect("a walked sheet has a screen")
    }

    fn scene(&self, path: &[String]) -> &Scene<'a> {
        &self.scenes[path]
    }

    /// The pins `item`'s symbol draws (`SCH_SYMBOL::GetPins( &sheet )`), with where each is.
    fn drawn_pins(&self, item: &Item) -> Vec<(String, Point)> {
        self.scene(&item.path).pins_of(item.sym)
    }

    /// The schematic net of a pin `number` of `item` there (`GetConnectionName( &sheet, false, true )`), `None` when the symbol does not draw it (or the tracer
    /// has no net for it).
    fn pin_net(&self, item: &Item, number: &str) -> Option<String> {
        if !self.drawn_pins(item).iter().any(|(n, _)| n == number) {
            return None;
        }
        let key = format!("{}.{number}", item.reference);
        if let Some(n) = self.drawn.get(&key) {
            return Some(n.clone());
        }
        // a stacked number is each of its pads in the net list
        let (pads, valid) = expand_stacked_pin_notation(number);
        if valid {
            if let Some(n) = pads.iter().find_map(|p| self.drawn.get(&format!("{}.{p}", item.reference))) {
                return Some(n.clone());
            }
        }
        None
    }

    /// The library symbol of `item`, with every unit's pins (`SCH_SYMBOL::m_pins`).
    fn lib_of(&self, item: &Item) -> Option<eda_model::symbol::LibSymbol> {
        let screen = self.screen(&item.path);
        placed_lib_symbol(self.model, screen, &screen.symbols[item.sym])
    }

    /// `SCH_SYMBOL::GetPin( number )`: a pin of that number in any unit of the symbol (its electrical type), `None` for none.
    fn lib_pin_type(&self, item: &Item, number: &str) -> Option<String> {
        let lib = self.lib_of(item)?;
        let found = lib.pins.iter().chain(lib.other_bodies().flat_map(|a| a.pins.iter())).find(|p| p.number == number).map(|p| if p.hidden && p.electrical_type == "power_in" { "power_in_hidden".to_string() } else { p.electrical_type.clone() });
        found
    }

    /// The unit-swap pass of `applyChangelist` for the units `items` of one footprint; answers the items it swapped.
    fn unit_swaps(&mut self, reference: &str, items: &mut [Item]) -> BTreeSet<usize> {
        let mut swapped = BTreeSet::new();
        if items.len() < 2 {
            return swapped;
        }
        let pcb = self.pads.get(reference).cloned().unwrap_or_default();
        // `CollectBackannotatePcbNets`: the board's net of each logical pin, through the pads each drawn pin stands for; one net per stacked pin
        let mut by_logical: BTreeMap<String, String> = BTreeMap::new();
        let mut unit_pins: Vec<Vec<String>> = Vec::new();
        let mut sch_nets: Vec<BTreeMap<String, String>> = Vec::new();
        for item in items.iter() {
            let Some(lib) = self.lib_of(item) else { return swapped };
            let infos = unit_pin_info(&lib);
            unit_pins.push(infos.get(item.unit.saturating_sub(1) as usize).filter(|_| item.unit > 0).map(|u| u.pins.clone()).unwrap_or_default());
            let mut sch: BTreeMap<String, String> = BTreeMap::new();
            for (number, _) in self.drawn_pins(item) {
                let (logical, valid) = expand_stacked_pin_notation(&number);
                if !valid || logical.is_empty() {
                    return swapped;
                }
                let nets: BTreeSet<String> = logical.iter().map(|p| pcb.get(p).cloned().unwrap_or_default()).collect();
                if nets.len() != 1 {
                    return swapped;
                }
                let net = nets.into_iter().next().unwrap_or_default();
                let schematic = self.pin_net(item, &number).unwrap_or_default();
                for p in logical {
                    match by_logical.get(&p) {
                        Some(n) if *n != net => return swapped,
                        _ => {
                            by_logical.insert(p.clone(), net.clone());
                        }
                    }
                    sch.insert(p, schematic.clone());
                }
            }
            sch_nets.push(sch);
        }
        // `netsInUnitOrder`
        let in_order = |pins: &[String], nets: &BTreeMap<String, String>| -> Vec<String> {
            if pins.is_empty() {
                nets.values().cloned().collect()
            } else {
                pins.iter().map(|p| nets.get(p).cloned().unwrap_or_default()).collect()
            }
        };
        // `PlanBackannotateUnitSwaps`
        let mut target: BTreeMap<usize, usize> = BTreeMap::new();
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for i in 0..items.len() {
            let pcb_by_pin: BTreeMap<String, String> = unit_pins[i].iter().map(|p| (p.clone(), by_logical.get(p).cloned().unwrap_or_default())).collect();
            let pcb_nets = in_order(&unit_pins[i], &pcb_by_pin);
            let Some(j) = (0..items.len()).find(|j| !used.contains(j) && in_order(&unit_pins[*j], &sch_nets[*j]) == pcb_nets) else { return swapped };
            target.insert(i, j);
            used.insert(j);
        }
        if target.iter().all(|(i, j)| i == j) {
            return swapped;
        }
        let mut steps: Vec<(usize, usize)> = Vec::new();
        let mut visited: BTreeSet<usize> = BTreeSet::new();
        for start in 0..items.len() {
            if visited.contains(&start) {
                continue;
            }
            let mut cycle = Vec::new();
            let mut cur = start;
            while visited.insert(cur) {
                cycle.push(cur);
                match target.get(&cur) {
                    Some(&next) if next != cur => cur = next,
                    _ => break,
                }
            }
            if cycle.len() < 2 {
                continue;
            }
            for k in (1..cycle.len()).rev() {
                steps.push((cycle[k - 1], cycle[k]));
            }
        }
        for (a, b) in steps {
            let (au, bu) = (items[a].unit, items[b].unit);
            // `a.sym->SetUnit( bUnit ); b.sym->SetUnit( aUnit )` with `SetUnitSelection( sheet, .. )` on each one's own sheet path: one swap where the two are on one
            // sheet path (`SetSymbolUnit` with `swap`), else each set on its own sheet (a screen shown twice: that placement's unit, `SchExtras::path_units`)
            let (sym_a, sym_b) = (&self.screen(&items[a].path).symbols[items[a].sym], &self.screen(&items[b].path).symbols[items[b].sym]);
            let (id_a, own_a, id_b, own_b) = (sym_a.id.clone(), sym_a.unit, sym_b.id.clone(), sym_b.unit);
            if items[a].path == items[b].path {
                self.out.cmds.push(on_sheet(&items[a].path, SchCmd::SetSymbolUnit { id: id_a, unit: Some(own_a), new_unit: bu, swap: true }));
            } else {
                self.out.cmds.push(on_sheet(&items[a].path, SchCmd::SetSymbolUnit { id: id_a, unit: Some(own_a), new_unit: bu, swap: false }));
                self.out.cmds.push(on_sheet(&items[b].path, SchCmd::SetSymbolUnit { id: id_b, unit: Some(own_b), new_unit: au, swap: false }));
            }
            items[a].unit = bu;
            items[b].unit = au;
            swapped.insert(a);
            swapped.insert(b);
            // `SubReference( unit, false )`
            let letters = |u: u32| eda_model::annotate::unit_letters(u);
            self.line("action", format!("Swap {} unit {} with unit {}.", items[a].reference, letters(au), letters(bu)));
        }
        swapped
    }

    /// `applyPinSwaps`: the pin numbers it swapped.
    fn pin_swaps(&mut self, item: &Item, opts: &BackAnnotateOptions) -> BTreeSet<String> {
        let mut swapped = BTreeSet::new();
        let pcb = self.pads.get(&item.reference).cloned().unwrap_or_default();
        struct Change {
            number: String,
            current: String,
            target: String,
        }
        let mut mismatches: Vec<Change> = Vec::new();
        let mut by_current: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (number, desired) in &pcb {
            // power pins are passed over, and a pin the symbol does not draw has no net
            let Some(kind) = self.lib_pin_type(item, number) else { continue };
            if kind == "power_in_hidden" {
                continue;
            }
            let current = self.pin_net(item, number).unwrap_or_default();
            if desired.is_empty() || current.is_empty() || *desired == current {
                continue;
            }
            by_current.entry(current.clone()).or_default().push(mismatches.len());
            mismatches.push(Change { number: number.clone(), current, target: desired.clone() });
        }
        if mismatches.len() < 2 {
            return swapped;
        }
        let shared = !item.shared_sheets.is_empty();
        let mut handled = vec![false; mismatches.len()];
        let key = {
            let s = &self.screen(&item.path).symbols[item.sym];
            field_key(&s.id, s.unit)
        };
        for i in 0..mismatches.len() {
            if handled[i] {
                continue;
            }
            let Some(candidates) = by_current.get(&mismatches[i].target) else { continue };
            let Some(partner) = candidates.iter().copied().find(|&c| c != i && !handled[c] && mismatches[c].target == mismatches[i].current) else { continue };
            let (a, b) = (mismatches[i].number.clone(), mismatches[partner].number.clone());
            handled[i] = true;
            handled[partner] = true;
            if !opts.allow_pin_swaps || shared {
                let why = if shared {
                    format!("but the symbol is used by multiple sheet instances ({}).", item.shared_sheets.join(", "))
                } else {
                    "but unconstrained pin swaps are disabled in the schematic preferences.".to_string()
                };
                self.line("info", format!("Would swap {} pins {a} and {b} to match PCB, {why}", item.reference));
                continue;
            }
            self.out.cmds.push(on_sheet(&item.path, SchCmd::SwapPins { pins: vec![pin_item_id(&key, &a), pin_item_id(&key, &b)] }));
            self.line("action", format!("Swap {} pins {a} and {b} to match PCB.", item.reference));
            swapped.insert(a);
            swapped.insert(b);
        }
        swapped
    }

    /// `processNetNameChange` for the pin `number` of `item`, from `old` to `new`.
    fn net_name_change(&mut self, item: &Item, number: &str, old: &str, new: &str) {
        let Some(tip) = self.drawn_pins(item).into_iter().find(|(n, _)| n == number).map(|(_, at)| at) else { return };
        let scene = self.scene(&item.path);
        let driver = driver_at(scene, self.model, tip);
        let spin = label_spin(scene, item.sym, number);
        let label_line = format!("Change {} pin {number} net label from '{old}' to '{new}'.", item.reference);
        match driver {
            Driver::LocalLabel(i) | Driver::GlobalLabel(i) | Driver::HierLabel(i) => {
                self.label_texts.insert((item.path.clone(), i), new.to_string());
                self.line("action", label_line);
            }
            Driver::SheetPin(s, p) => {
                self.sheet_pin_names.insert((item.path.clone(), s, p), new.to_string());
                self.line("action", label_line);
            }
            Driver::LocalPower | Driver::GlobalPower => {
                self.line("error", format!("Net {old} cannot be changed to {new} because it is driven by a power pin."));
            }
            Driver::Pin => {
                self.out.cmds.push(on_sheet(&item.path, SchCmd::PlaceLabel { net: new.to_string(), at: tip, kind: LabelKind::Local, spin: Some(spin), size_nm: None, bold: None, italic: None, auto_rotate: None }));
                self.line("action", format!("Add label '{new}' to {} pin {number} net.", item.reference));
            }
        }
    }
}

/// What Update Schematic from PCB does to the nets of `design` (which has its schematic: the stored one, or the one the studio shows for a board with none),
/// whose drawing traces to `drawn` (`board::trace_kicad_names`) and whose board has the net list `board_nets`. See the module doc.
pub fn plan_net_back_annotation(design: &Design, model: &ConstraintModel, board_nets: &[Net], drawn: &[TracedNet], opts: &BackAnnotateOptions) -> NetBackAnnotation {
    let mut out = NetBackAnnotation::default();
    if !opts.net_names || design.schematic.is_none() {
        return out;
    }
    let mut pads: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for net in board_nets {
        if net.name.is_empty() {
            continue;
        }
        for pin in &net.pins {
            if let Some((reference, pad)) = pin.split_once('.') {
                pads.entry(reference.to_string()).or_default().entry(pad.to_string()).or_insert_with(|| short_name(&net.name).to_string());
            }
        }
    }
    let mut drawn_of: BTreeMap<String, String> = BTreeMap::new();
    for t in drawn {
        for p in &t.pins {
            drawn_of.entry(base_pin(p).to_string()).or_insert_with(|| short_name(&t.name).to_string());
        }
    }

    // `getChangeList`: every placed symbol that is a footprint's, by sheet; its units together (`m_multiUnitsRefs`)
    let entries = walk(design);
    let mut placements_of_file: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for e in &entries {
        placements_of_file.entry(e.file.as_str()).or_default().push(e.name.clone());
    }
    let mut scenes = BTreeMap::new();
    let mut items: Vec<Item> = Vec::new();
    for e in &entries {
        let Some(screen) = screen_of(design, &e.path) else { continue };
        scenes.insert(e.path.clone(), Scene::new(screen, model));
        let shared: Vec<String> = placements_of_file.get(e.file.as_str()).filter(|p| !e.path.is_empty() && p.len() > 1).cloned().unwrap_or_default();
        for (i, sym) in screen.symbols.iter().enumerate() {
            let reference = reference_under(screen, sym, &e.path);
            if sym.exclude_from_board || reference.starts_with('#') || !pads.contains_key(&reference) {
                continue;
            }
            items.push(Item { path: e.path.clone(), sym: i, reference, unit: unit_under(screen, sym, &e.path), shared_sheets: shared.clone() });
        }
    }
    let mut planner = Planner { design, model, drawn: drawn_of, pads, scenes, out: std::mem::take(&mut out), label_texts: BTreeMap::new(), sheet_pin_names: BTreeMap::new() };

    // The unit swaps first: they change which unit each symbol is.
    let mut skip: BTreeSet<usize> = BTreeSet::new();
    if opts.prefer_unit_swaps {
        let mut by_reference: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (k, item) in items.iter().enumerate() {
            by_reference.entry(item.reference.clone()).or_default().push(k);
        }
        for (reference, ks) in by_reference {
            let mut group: Vec<Item> = ks.iter().map(|&k| items[k].clone()).collect();
            for g in planner.unit_swaps(&reference, &mut group) {
                skip.insert(ks[g]);
            }
        }
    }

    for (k, item) in items.iter().enumerate() {
        if skip.contains(&k) {
            continue;
        }
        let swapped = if opts.prefer_pin_swaps { planner.pin_swaps(item, opts) } else { BTreeSet::new() };
        let pcb = planner.pads.get(&item.reference).cloned().unwrap_or_default();
        for (number, net) in &pcb {
            if swapped.contains(number) {
                continue;
            }
            if planner.lib_pin_type(item, number).is_none() {
                planner.line("error", format!("Cannot find {} pin '{number}'.", item.reference));
                continue;
            }
            if let Some(current) = planner.pin_net(item, number) {
                if current != *net {
                    planner.net_name_change(item, number, &current, net);
                }
            }
        }
    }

    // The labels and sheet pins renamed, each once with the text it ends with.
    let labels = std::mem::take(&mut planner.label_texts);
    for ((path, i), text) in labels {
        let label = &screen_of(design, &path).expect("a walked sheet").labels[i];
        if label.net != text {
            let cmd = SchCmd::EditLabel { id: label.id.clone(), text: Some(text), shape: None, spin: None, size_nm: None, bold: None, italic: None, auto_rotate: None, fields: None };
            planner.out.cmds.push(on_sheet(&path, cmd));
        }
    }
    let pins = std::mem::take(&mut planner.sheet_pin_names);
    for ((path, s, p), name) in pins {
        let pin = &screen_of(design, &path).expect("a walked sheet").sheets[s].pins[p];
        if pin.name != name {
            let cmd = SchCmd::EditSheetPin { id: pin.id.clone(), name: Some(name), shape: None, at: None, size_nm: None, bold: None, italic: None };
            planner.out.cmds.push(on_sheet(&path, cmd));
        }
    }
    planner.out
}

impl crate::Board<'_> {
    /// [`Cmd::CarrySymbolFields`]: the fields Footprint Properties changed, carried to the symbol `id` as `BACK_ANNOTATE::applyChangelist` with "Other fields"
    /// (`m_processOtherFields`, KiCad 8303b2ad `eeschema/tools/backannotate.cpp`) carries a field, on every sheet the symbol is on (its units share them):
    ///
    /// 1. "Existing fields has changed value": a field of a name in `fields` that names no variable (`HasTextVars`) and shows another text takes the footprint's
    ///    (`symField->SetText( fpFieldValue )`); Reference and Value are passed over ("Skip fields that are individually controlled");
    /// 2. "New field has been added to footprint": a name in `fields` the symbol has no field of is added with the text, hidden at the symbol
    ///    (`newField.SetVisible( false )`, `SetTextPos( symbol->GetPosition() )`: the studio's user field with no placement, which is hidden at its origin);
    /// 3. "Existing field has been deleted from footprint": a field named in `remove` that is not mandatory is removed, with its places and the variants' texts
    ///    of it (`symbol->RemoveField( fieldName )`).
    ///
    /// **Differs**: KiCad compares every field of every footprint when Update Schematic from PCB runs; the studio carries, at Footprint Properties' OK, only the
    /// fields that dialog changed (the coordinator's decision, 2026-11-14), so a footprint whose own fields differ from its symbol's does not rewrite the symbol
    /// unasked.
    pub(crate) fn carry_symbol_fields(&mut self, id: &str, fields: &BTreeMap<String, String>, remove: &[String]) -> Result<(), Vec<eda_model::CheckResult>> {
        use eda_model::sym_fields::is_mandatory_field;
        let fail = |m: &str| vec![eda_model::CheckResult::fail("ops_unknown_item", id, m.to_string())];
        if self.design.schematic.is_none() {
            return Err(vec![eda_model::CheckResult::fail("ops_no_schematic", "schematic", "this board has no schematic section yet")]);
        }
        let model = self.model;
        let mut found = false;
        let screens = self.design.schematic.iter_mut().chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.values_mut()));
        for sch in screens {
            let Some(first) = sch.symbols.iter().position(|s| s.id == id) else { continue };
            found = true;
            for (name, text) in fields {
                if name == "Reference" || name == "Value" {
                    continue;
                }
                let sym = sch.symbols[first].clone();
                match name.as_str() {
                    "Datasheet" => {
                        if !eda_model::text_vars::has_text_vars(&sym.datasheet) && sym.datasheet != *text {
                            for s in sch.symbols.iter_mut().filter(|s| s.id == id) {
                                s.datasheet = text.clone();
                            }
                        }
                    }
                    "Footprint" => {
                        let now = eda_model::footprint::symbol_footprint_field(&sym, model.part(id));
                        if !eda_model::text_vars::has_text_vars(&now) && now != *text {
                            for s in sch.symbols.iter_mut().filter(|s| s.id == id) {
                                s.set_footprint(text);
                            }
                        }
                    }
                    other => {
                        let own = sch.user_fields.get(id).and_then(|m| m.get(other)).cloned();
                        // the Description a symbol shows without one of its own is its library symbol's (`UpdateFields`): a field it has
                        let shown = own.clone().or_else(|| (other == "Description").then(|| model.part(id).and_then(|p| model.real_symbol_of(&sym.lib_id, p)).map(|l| l.description).unwrap_or_default()));
                        match shown {
                            Some(now) if eda_model::text_vars::has_text_vars(&now) || now == *text => {}
                            // 1. and 2.: the text set, or the field added (hidden: a user field with no placement is)
                            _ => {
                                sch.user_fields.entry(id.to_string()).or_default().insert(other.to_string(), text.clone());
                            }
                        }
                    }
                }
            }
            // 3.
            let gone: Vec<String> = sch.user_fields.get(id).map(|m| m.keys().filter(|n| !is_mandatory_field(n) && remove.contains(*n)).cloned().collect()).unwrap_or_default();
            for name in gone {
                if let Some(m) = sch.user_fields.get_mut(id) {
                    m.remove(&name);
                }
                crate::sch_fields::drop_field_placements(sch, &[id.to_string()], &name);
                eda_model::variants::rename_field(sch, Some(id), &name, None);
            }
            if sch.user_fields.get(id).is_some_and(|m| m.is_empty()) {
                sch.user_fields.remove(id);
            }
        }
        if !found {
            return Err(fail("no symbol with this reference"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "back_annotate_tests.rs"]
mod tests;
