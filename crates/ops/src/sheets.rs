//! Tools > Reorganize into Module Sheets: the verb that turns a flat schematic into one sheet per functional module.
//!
//! The derivation is `eda_engine::derive_hierarchy` over the modules `eda_model::modules::infer_modules` finds; this file is what
//! makes it an edit of an existing design: what is kept (every symbol's reference, value, footprint, datasheet and library symbol,
//! the user fields, the ERC exclusions and pin map, the title block), and where the netlist the new sheets must draw comes from.

use eda_engine::nets::{trace_nets, ScreenIn};
use eda_engine::placed::pin_points;
use eda_engine::{derive_hierarchy, EngineOptions, Keep};
use eda_model::modules::infer_modules;
use eda_model::{CheckResult, Net};
use std::collections::{BTreeMap, BTreeSet};

use crate::Board;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// How many pins a netlist actually connects: pins on a net with at least one other pin.
fn connected_pins(nets: &[Net]) -> usize {
    nets.iter().filter(|n| n.pins.len() >= 2).map(|n| n.pins.len()).sum()
}

fn pin_set(n: &Net) -> Vec<String> {
    let mut p = n.pins.clone();
    p.sort();
    p
}

/// The share of the intent's connections (nets of two or more pins) that `nets` has exactly: 1.0 for the intent itself, near 0.0 for
/// a list that left every pin on a net of its own.
fn agreement(nets: &[Net], intent: &[Net]) -> f64 {
    let have: BTreeSet<Vec<String>> = nets.iter().filter(|n| n.pins.len() >= 2).map(pin_set).collect();
    let want: Vec<Vec<String>> = intent.iter().filter(|n| n.pins.len() >= 2).map(pin_set).collect();
    if want.is_empty() {
        return 1.0;
    }
    want.iter().filter(|p| have.contains(*p)).count() as f64 / want.len() as f64
}

impl<'a> Board<'a> {
    /// The intent's own nets, when the caller has them: used to give a net its declared name back (see
    /// [`Cmd::ReorganizeSheets`](crate::Cmd::ReorganizeSheets)).
    pub fn with_intent_nets(mut self, nets: Vec<Net>) -> Self {
        self.intent_nets = Some(nets);
        self
    }

    /// The netlist the new sheets must draw.
    ///
    /// The live one (`Board::model`'s nets: the schematic's own retrace once it has been edited, else the intent's) is the answer
    /// while it is believable. It is not when a retrace that read the drawing wrongly (an older build placed every pin where no wire
    /// ended) left most pins on a net of their own: then the intent's connections are mostly missing from it. So the candidates
    /// are judged against the intent, when the caller has it -- the live list (believable from 70% of the intent's connections), then
    /// a fresh retrace of the drawing (from 90%), then the intent itself. Without the intent, whichever of the live list and the
    /// retrace connects more pins. A net the retrace named `NET_3` takes the intent's name when the intent has a net of exactly
    /// those pins.
    fn netlist_for_reorganize(&self, sch: &eda_model::ir::SchematicSection) -> Vec<Net> {
        let live = self.model.nets.clone();
        let retrace = || -> Vec<Net> {
            let mut drawn = sch.clone();
            let mut pins = BTreeMap::new();
            for sym in &drawn.symbols {
                let Some(part) = self.model.part(&sym.id) else { continue };
                let resolved = self.model.real_symbol_of(&sym.lib_id, part);
                for (number, at) in pin_points(sym, part, resolved.as_ref()) {
                    pins.insert(format!("{}.{number}", sym.id), at);
                }
            }
            let mut screens = vec![ScreenIn { sch: &mut drawn, file: String::new(), pins }];
            trace_nets(&mut screens)
        };
        let Some(intent) = &self.intent_nets else {
            // Nothing to judge by: the better connected of the two (a drawing read from a KiCad file keeps the stored list).
            if sch.imported_from_kicad {
                return live;
            }
            let traced = retrace();
            return if connected_pins(&traced) > connected_pins(&live) { traced } else { live };
        };
        // A list that was edited by hand differs from the intent in a few nets; a broken one in nearly all. A fresh retrace of the
        // drawing has to be closer still, because a drawing can short nets together that the intent keeps apart.
        const LIVE_BELIEVABLE: f64 = 0.7;
        const TRACED_BELIEVABLE: f64 = 0.9;
        if agreement(&live, intent) >= LIVE_BELIEVABLE {
            return live;
        }
        if !sch.imported_from_kicad {
            let mut traced = retrace();
            if agreement(&traced, intent) >= TRACED_BELIEVABLE {
                let by_pins: BTreeMap<Vec<String>, &str> = intent.iter().map(|n| (pin_set(n), n.name.as_str())).collect();
                let mut used: BTreeSet<String> = traced.iter().filter(|n| !n.name.starts_with("NET_")).map(|n| n.name.clone()).collect();
                for n in traced.iter_mut().filter(|n| n.name.starts_with("NET_")) {
                    if let Some(name) = by_pins.get(&pin_set(n)) {
                        if used.insert((*name).to_string()) {
                            n.name = (*name).to_string();
                        }
                    }
                }
                traced.sort_by(|a, b| a.name.cmp(&b.name));
                return traced;
            }
        }
        intent.clone()
    }

    pub(crate) fn reorganize_sheets(&mut self) -> Result<(), Vec<CheckResult>> {
        if self.focus.is_some() {
            return Err(fail("ops_not_on_root", "sheets", "Reorganize into Module Sheets runs on the root sheet"));
        }
        let root = self.design.schematic.clone().ok_or_else(|| fail("ops_no_schematic", "schematic", "this board has no schematic section yet"))?;
        if !root.sheets.is_empty() || self.design.sheet_contents.as_ref().is_some_and(|c| !c.is_empty()) {
            return Err(fail("ops_already_hierarchical", "sheets", "the schematic already has sheets; there is nothing to reorganize"));
        }
        let mut symbols = BTreeMap::new();
        for s in &root.symbols {
            if symbols.insert(s.id.clone(), s.clone()).is_some() {
                return Err(fail("ops_multi_unit", &s.id, format!("{} is a multi-unit symbol; module sheets do not split units yet", s.id)));
            }
        }
        for r in symbols.keys() {
            if self.model.part(r).is_none() {
                return Err(fail("ops_unknown_part", r, format!("{r} is on the sheet but is not a part of the design, so it cannot be placed in a module")));
            }
        }

        let mut nets = self.netlist_for_reorganize(&root);
        // The canonical form a retrace writes: pins sorted within a net, nets by name.
        for n in nets.iter_mut() {
            n.pins.sort();
        }
        nets.sort_by(|a, b| a.name.cmp(&b.name));
        let mut model = self.model.clone();
        model.nets = nets.clone();
        let recorded: Vec<eda_model::ir::ModuleRegion> = self.design.placement.as_ref().map(|p| p.modules.clone()).unwrap_or_default();
        let modules = infer_modules(&model, &recorded);
        if modules.len() < 2 {
            return Err(fail("ops_one_module", "sheets", "the design is a single functional module; one sheet already holds it"));
        }
        let keep = Keep {
            locked: root.extras.locked.iter().filter(|id| symbols.contains_key(*id)).cloned().collect(),
            symbols,
            user_fields: root.user_fields.clone(),
            erc_exclusions: root.erc_exclusions.clone(),
            erc_pin_map: root.erc_pin_map.clone(),
            title_block: root.title_block.clone(),
        };
        let p = &self.design.provenance;
        let opts = EngineOptions { seed: p.seed, engine_version: p.engine_version.clone(), intent_hash: p.intent_hash.clone() };
        let derived = derive_hierarchy(&model, &opts, &modules, &keep)?;
        self.design.schematic = derived.schematic;
        self.design.sheet_contents = derived.sheet_contents;
        self.design.nets = Some(nets);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Board, Cmd, Domain};
    use eda_model::ir::{Design, PlacementSection, Point};
    use eda_model::ConstraintModel;

    fn mcu30() -> ConstraintModel {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml");
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A board holding mcu30's flat schematic, as `derive_schematic` draws it.
    fn flat_board(model: &ConstraintModel) -> Board<'_> {
        let mut design: Design = eda_engine::derive_schematic(model, &EngineOptions::new(1, "t")).unwrap();
        design.placement = Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
        Board::new(design, model, 100, 300)
    }

    fn drawn_nets(model: &ConstraintModel) -> BTreeMap<String, Vec<String>> {
        model
            .nets
            .iter()
            .map(|n| {
                let mut p = n.pins.clone();
                p.sort();
                (n.name.clone(), p)
            })
            .filter(|(_, p)| p.len() >= 2)
            .collect()
    }

    /// The nets a design's sheets draw, by tracing them.
    fn traced(model: &ConstraintModel, d: &Design) -> BTreeMap<String, Vec<String>> {
        let mut all: Vec<(String, eda_model::ir::SchematicSection)> = vec![(String::new(), d.schematic.clone().unwrap())];
        all.extend(d.sheet_contents.iter().flatten().map(|(f, s)| (f.clone(), s.clone())));
        let pins: Vec<BTreeMap<String, Point>> = all
            .iter()
            .map(|(_, s)| {
                let mut m = BTreeMap::new();
                for sym in &s.symbols {
                    let part = model.part(&sym.id).unwrap();
                    for (n, at) in pin_points(sym, part, model.real_symbol_of(&sym.lib_id, part).as_ref()) {
                        m.insert(format!("{}.{n}", sym.id), at);
                    }
                }
                m
            })
            .collect();
        let mut screens: Vec<ScreenIn> = Vec::new();
        for ((file, sch), p) in all.iter_mut().zip(pins) {
            screens.push(ScreenIn { sch, file: file.clone(), pins: p });
        }
        trace_nets(&mut screens).into_iter().filter(|n| n.pins.len() >= 2).map(|n| (n.name, n.pins)).collect()
    }

    #[test]
    fn reorganize_splits_the_flat_mcu30_into_module_sheets_and_the_nets_stay_the_nets() {
        let model = mcu30();
        let mut b = flat_board(&model);
        assert!(b.design().sheet_contents.is_none());
        let sch_before = b.design().schematic.clone().unwrap();
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let d = b.design();
        let root = d.schematic.as_ref().unwrap();
        assert_eq!(root.sheets.len(), 3);
        assert!(root.symbols.is_empty());
        assert_eq!(d.sheet_contents.as_ref().unwrap().len(), 3);
        // The nets after are the nets before: drawn back from the new sheets, and stored.
        assert_eq!(traced(&model, d), drawn_nets(&model));
        let stored: BTreeMap<String, Vec<String>> = d.nets.clone().unwrap().into_iter().filter(|n| n.pins.len() >= 2).map(|n| (n.name, n.pins)).collect();
        assert_eq!(stored, drawn_nets(&model));
        // Every symbol keeps its identity: same references, same library symbol, same value.
        let mut after: Vec<&eda_model::ir::SymbolInstance> = d.sheet_contents.as_ref().unwrap().values().flat_map(|s| s.symbols.iter()).collect();
        after.sort_by(|a, b| a.id.cmp(&b.id));
        let mut before: Vec<&eda_model::ir::SymbolInstance> = sch_before.symbols.iter().collect();
        before.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(after.len(), before.len());
        for (a, b) in after.iter().zip(&before) {
            assert_eq!((&a.id, &a.lib_id, &a.value, &a.footprint), (&b.id, &b.lib_id, &b.value, &b.footprint));
        }
        // A second time is refused: there are sheets now.
        assert_eq!(b.apply(&Cmd::ReorganizeSheets).unwrap_err()[0].check, "ops_already_hierarchical");
    }

    /// A board whose stored net list lost every connection (an older retrace read the drawing wrongly) is reorganized by the
    /// intent's nets, not by the broken list.
    #[test]
    fn a_stale_net_list_does_not_decide_what_the_new_sheets_draw() {
        let model = mcu30();
        let mut stale = model.clone();
        stale.nets = model.parts.iter().flat_map(|p| p.pins.iter().map(move |pin| (p.reference.clone(), pin.number.clone()))).enumerate().map(|(i, (r, n))| eda_model::Net { name: format!("NET_{}", i + 1), pins: vec![format!("{r}.{n}")] }).collect();
        let mut design: Design = eda_engine::derive_schematic(&model, &EngineOptions::new(1, "t")).unwrap();
        design.placement = Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
        design.nets = Some(stale.nets.clone());
        let mut b = Board::new(design, &stale, 100, 300).with_intent_nets(model.nets.clone());
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        assert_eq!(traced(&model, b.design()), drawn_nets(&model));
        assert_eq!(b.design().schematic.as_ref().unwrap().sheets.len(), 3);
    }

    #[test]
    fn reorganize_is_a_schematic_edit() {
        assert_eq!(Cmd::ReorganizeSheets.domain(), Domain::Schematic);
    }

    #[test]
    fn a_command_on_a_sheet_edits_that_sheet_alone() {
        let model = mcu30();
        let mut b = flat_board(&model);
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let root_before = b.design().schematic.clone().unwrap();
        let mcu = root_before.sheets.iter().find(|s| s.name.starts_with("MCU")).unwrap().clone();
        let file = mcu.file.clone();
        let at0 = b.design().sheet_contents.as_ref().unwrap()[&file].symbols.iter().find(|s| s.id == "U1").unwrap().at;
        let to = Point { x: at0.x + 2_540, y: at0.y + 1_270 };
        let cmd = Cmd::OnSheet { sheet: mcu.id.clone(), cmd: Box::new(Cmd::MoveSymbol { id: "U1".into(), x: to.x, y: to.y, unit: None }) };
        assert_eq!(cmd.domain(), Domain::Schematic, "an edit on a sheet is a schematic edit");
        b.apply(&cmd).unwrap();
        let after = b.design();
        assert_eq!(after.sheet_contents.as_ref().unwrap()[&file].symbols.iter().find(|s| s.id == "U1").unwrap().at, to);
        // The root and the other sheets are as they were.
        assert_eq!(after.schematic.as_ref().unwrap().sheets.len(), root_before.sheets.len());
        assert_eq!(after.schematic.as_ref().unwrap().wires.len(), root_before.wires.len());
        // The drawing verbs reach the sheet too.
        let add = Cmd::OnSheet { sheet: mcu.id.clone(), cmd: Box::new(Cmd::AddSchText { content: "note".into(), at: to, angle_millideg: 0, size_um: 1_270 }) };
        b.apply(&add).unwrap();
        assert_eq!(b.design().sheet_contents.as_ref().unwrap()[&file].texts.len(), 1);
        assert!(b.design().schematic.as_ref().unwrap().texts.is_empty(), "nothing leaked onto the root");
        // An unknown path is refused and changes nothing.
        let before = serde_json::to_string(b.design()).unwrap();
        let bad = Cmd::OnSheet { sheet: "no_such_sheet".into(), cmd: Box::new(Cmd::AddSchText { content: "x".into(), at: to, angle_millideg: 0, size_um: 1_270 }) };
        assert_eq!(b.apply(&bad).unwrap_err()[0].check, "ops_unknown_sheet");
        assert_eq!(serde_json::to_string(b.design()).unwrap(), before);
        // A failing inner command leaves the design untouched too.
        let missing = Cmd::OnSheet { sheet: mcu.id.clone(), cmd: Box::new(Cmd::MoveSymbol { id: "R1".into(), x: 0, y: 0, unit: None }) };
        assert!(b.apply(&missing).is_err(), "R1 is on the channels sheet, not the MCU's");
        assert_eq!(serde_json::to_string(b.design()).unwrap(), before);
    }

    /// Do Not Populate, the exclusions and a lock are part of the symbol: they go with it to its new sheet.
    #[test]
    fn reorganize_keeps_what_the_user_set_on_a_symbol() {
        let model = mcu30();
        let mut design: Design = eda_engine::derive_schematic(&model, &EngineOptions::new(1, "t")).unwrap();
        design.placement = Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
        let root = design.schematic.as_mut().unwrap();
        for s in root.symbols.iter_mut().filter(|s| s.id == "R3") {
            s.dnp = true;
            s.exclude_from_bom = true;
            s.exclude_from_sim = true;
        }
        root.extras.set_locked("R3", true);
        root.extras.set_locked("U1", true);
        let mut b = Board::new(design, &model, 100, 300);
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let d = b.design();
        let screens = d.sheet_contents.as_ref().unwrap();
        let r3 = screens.values().flat_map(|s| s.symbols.iter()).find(|s| s.id == "R3").unwrap();
        assert_eq!((r3.dnp, r3.exclude_from_bom, r3.exclude_from_board, r3.exclude_from_sim), (true, true, false, true));
        let locked_on = |reference: &str| screens.values().find(|s| s.symbols.iter().any(|x| x.id == reference)).unwrap().extras.is_locked(reference);
        assert!(locked_on("R3") && locked_on("U1"), "the lock moved with the symbol");
        assert!(!locked_on("R4"));
        assert!(d.schematic.as_ref().unwrap().extras.locked.is_empty(), "the root holds sheet symbols only");
    }

    /// The studio addresses everything it sends from the Schematic tab to the sheet in view; what is not a schematic edit is not stopped by it.
    #[test]
    fn a_command_of_another_editor_ignores_the_sheet_path() {
        let model = mcu30();
        let mut b = flat_board(&model);
        let cmd = Cmd::OnSheet { sheet: "gone".into(), cmd: Box::new(Cmd::Batch { cmds: vec![] }) };
        assert_eq!(cmd.domain(), Domain::Pcb);
        b.apply(&cmd).unwrap();
    }

    #[test]
    fn on_sheet_round_trips_as_json() {
        let c = Cmd::OnSheet { sheet: "a/b".into(), cmd: Box::new(Cmd::MoveSymbol { id: "U1".into(), x: 1, y: 2, unit: None }) };
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["op"], "on_sheet");
        assert_eq!(j["cmd"]["op"], "move_symbol");
        let back: Cmd = serde_json::from_value(j).unwrap();
        assert_eq!(back, c);
    }
}
