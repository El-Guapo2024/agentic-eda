//! Tools > Reorganize into Module Sheets: the verb that turns a flat schematic into one sheet per functional module.
//!
//! The derivation is `eda_engine::derive_hierarchy` over the modules `eda_model::modules::infer_modules` finds; this file is what
//! makes it an edit of an existing design: what is kept (every symbol's reference, value, footprint, datasheet and library symbol,
//! the user fields, the ERC exclusions and pin map, the title block), and where the netlist the new sheets must draw comes from.

use eda_engine::nets::{trace_nets, ScreenIn};
use eda_engine::placed::pin_points_in;
use eda_engine::{derive_hierarchy, EngineOptions};
use eda_model::modules::infer_modules;
use eda_model::{CheckResult, Net};
use std::collections::{BTreeMap, BTreeSet};

use crate::sheet_units;
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
                let resolved = self.model.real_symbol_of_instance(&drawn, sym, part);
                for (number, at) in pin_points_in(&drawn, sym, part, resolved.as_ref()) {
                    pins.insert(format!("{}.{number}", sym.id), at);
                }
            }
            let mut screens = vec![ScreenIn { path: Vec::new(), sch: &mut drawn, file: String::new(), pins }];
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
                // a net the drawing names by its pins only (`Net-(R1-Pad1)`, KiCad's name for a net no label drives) takes the intent's name, which the new
                // sheets' labels then show
                let by_its_pins = |name: &str| name.starts_with("NET_") || name.starts_with("Net-(") || name.starts_with("unconnected-(");
                let mut used: BTreeSet<String> = traced.iter().filter(|n| !by_its_pins(&n.name)).map(|n| n.name.clone()).collect();
                for n in traced.iter_mut().filter(|n| by_its_pins(&n.name)) {
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
        // a symbol on the sheet is a part of the design
        for s in &root.symbols {
            if self.model.part(&s.id).is_none() {
                return Err(fail("ops_unknown_part", &s.id, format!("{} is on the sheet but is not a part of the design, so it cannot be placed in a module", s.id)));
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
        // A part with several units is laid out unit by unit (each unit a part of its own for the layout), the units in their part's module.
        let split = sheet_units::split(&model, &nets, &root);
        let keep = split.keep(&root);
        let modules = sheet_units::expand_modules(&modules, &split.expand);
        let p = &self.design.provenance;
        let opts = EngineOptions { seed: p.seed, engine_version: p.engine_version.clone(), intent_hash: p.intent_hash.clone() };
        let mut derived = derive_hierarchy(&split.model, &opts, &modules, &keep)?;

        // What the layout drew is the one reference with its units again, and the symbols keep the look they had.
        {
            let mut screens: Vec<&mut eda_model::ir::SchematicSection> = derived.sheet_contents.iter_mut().flat_map(|c| c.values_mut()).collect();
            for sec in screens.iter_mut() {
                sheet_units::restore(sec, &split.back);
                sheet_units::carry_field_looks(&root, sec);
            }
            sheet_units::carry_label_looks(&root, &mut screens);
        }
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
        Board::new(design, model, 100_000, 300_000)
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

    /// `traced` against `drawn_nets`: the same pins in the same nets, each named as KiCad names what is drawn -- the intent's name for a power net, the
    /// intent's name under a sheet's path (`/MCU (U1)/RESET`) for a labelled one, its best pin's (`Net-(D1-Pad1)`) for one no label names.
    fn assert_same_nets(got: &BTreeMap<String, Vec<String>>, want: &BTreeMap<String, Vec<String>>) {
        let by_pins = |m: &BTreeMap<String, Vec<String>>| -> BTreeMap<Vec<String>, String> { m.iter().map(|(n, p)| (p.clone(), n.clone())).collect() };
        let (got, want) = (by_pins(got), by_pins(want));
        assert_eq!(got.keys().collect::<Vec<_>>(), want.keys().collect::<Vec<_>>(), "the same nets, the same pins");
        for (pins, name) in &got {
            let intent = &want[pins];
            assert!(name == intent || (name.starts_with('/') && name.ends_with(&format!("/{intent}"))) || name.starts_with("Net-("), "{pins:?} is the intent's {intent}, traced as {name}");
        }
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
                    for (n, at) in eda_engine::placed::pin_points(sym, part, model.real_symbol_of_instance(s, sym, part).as_ref()) {
                        m.insert(format!("{}.{n}", sym.id), at);
                    }
                }
                m
            })
            .collect();
        let mut screens: Vec<ScreenIn> = Vec::new();
        for ((file, sch), p) in all.iter_mut().zip(pins) {
            screens.push(ScreenIn { path: Vec::new(), sch, file: file.clone(), pins: p });
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
        assert_same_nets(&traced(&model, d), &drawn_nets(&model));
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
        let mut b = Board::new(design, &stale, 100_000, 300_000).with_intent_nets(model.nets.clone());
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        assert_same_nets(&traced(&model, b.design()), &drawn_nets(&model));
        assert_eq!(b.design().schematic.as_ref().unwrap().sheets.len(), 3);
    }

    // ------------------------------------------------------------------ what Reorganize keeps of a symbol's look, and multi-unit parts

    use eda_model::ir::{field_key, FieldPlacement};
    use eda_model::sch_extras::LabelLook;
    use eda_model::symbol::{AlternateBody, LibPin, LibSymbol, SPoint};
    use eda_model::{Part, Pin, PinKind};

    /// The mcu30 flat design, with the root section edited by `edit` first.
    fn flat_board_with<'a>(model: &'a ConstraintModel, edit: impl FnOnce(&mut eda_model::ir::SchematicSection)) -> Board<'a> {
        let mut design: Design = eda_engine::derive_schematic(model, &EngineOptions::new(1, "t")).unwrap();
        design.placement = Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
        edit(design.schematic.as_mut().unwrap());
        Board::new(design, model, 100_000, 300_000)
    }

    fn screen_with<'d>(d: &'d Design, reference: &str) -> &'d eda_model::ir::SchematicSection {
        d.sheet_contents.as_ref().unwrap().values().find(|s| s.symbols.iter().any(|x| x.id == reference)).unwrap_or_else(|| panic!("no sheet has {reference}"))
    }

    fn placement(name: &str, dx: i64, dy: i64) -> FieldPlacement {
        FieldPlacement { dx, dy, ..FieldPlacement::at_origin(name) }
    }

    /// Fields the user placed stay where they were put; fields Autoplace Fields placed are placed again but keep their look; a label keeps the look
    /// its net's labels had.
    #[test]
    fn reorganize_keeps_the_places_of_fields_and_the_looks_of_fields_and_labels() {
        let model = mcu30();
        // R3: the user put the Reference far to the left and hid the Value. U1: Autoplace Fields' places, but the Value is bold and larger.
        let custom = vec![FieldPlacement { h: eda_model::ir::TextJustify::Right, ..placement("Reference", -6_350, 0) }, FieldPlacement { visible: false, ..placement("Value", 2_540, 5_080) }];
        let mut label_net = String::new();
        let labelled_on_new_sheets: BTreeSet<String> = {
            let mut probe = flat_board(&model);
            probe.apply(&Cmd::ReorganizeSheets).unwrap();
            probe.design().sheet_contents.as_ref().unwrap().values().flat_map(|s| s.labels.iter().map(|l| l.net.clone())).collect()
        };
        let mut b = flat_board_with(&model, |root| {
            root.field_layout.insert("R3".into(), custom.clone());
            root.extras.fields_autoplaced.remove("R3");
            let u1 = root.field_layout.get_mut("U1").expect("a derivation stores the places of every symbol's fields");
            let value = u1.iter_mut().find(|f| f.name == "Value").unwrap();
            value.bold = true;
            value.size_nm = 1_778;
            assert!(root.extras.fields_autoplaced.contains_key("U1"));
            // a label of a net, set in a look of its own
            // (one the new sheets draw a label for: a net between two modules)
            let l = root.labels.iter().find(|l| l.kind.is_local() && labelled_on_new_sheets.contains(&l.net)).expect("the flat design has a local label of a net the new sheets label").clone();
            label_net = l.net.clone();
            root.extras.label_looks.insert(l.id.clone(), LabelLook { size_nm: 2_032, bold: true, italic: false, auto_rotate: false });
        });
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let d = b.design();

        let r3 = screen_with(d, "R3");
        assert_eq!(r3.field_layout.get("R3"), Some(&custom), "the fields the user placed are where they were put");
        assert!(!r3.extras.fields_autoplaced.contains_key("R3"), "and still the user's");
        let r4 = screen_with(d, "R4");
        assert!(r4.extras.fields_autoplaced.contains_key("R4"), "the others were placed by Autoplace Fields and are again");

        let u1 = screen_with(d, "U1");
        let value = u1.field_layout["U1"].iter().find(|f| f.name == "Value").unwrap();
        assert!(value.bold && value.size_nm == 1_778, "Autoplace Fields placed it again, and it is still bold and larger: {value:?}");
        let reference = u1.field_layout["U1"].iter().find(|f| f.name == "Reference").unwrap();
        assert!(!reference.bold && reference.size_nm == 0);

        let looked: Vec<&eda_model::ir::NetLabel> = d.sheet_contents.as_ref().unwrap().values().flat_map(|s| s.labels.iter().map(move |l| (s, l))).filter(|(s, l)| l.net == label_net && s.extras.label_looks.contains_key(&l.id)).map(|(_, l)| l).collect();
        let all: usize = d.sheet_contents.as_ref().unwrap().values().flat_map(|s| s.labels.iter()).filter(|l| l.net == label_net).count();
        assert!(all > 0 && looked.len() == all, "every label of {label_net} has the net's look ({} of {all})", looked.len());
        let any = d.sheet_contents.as_ref().unwrap().values().find_map(|s| s.labels.iter().find(|l| l.net == label_net).map(|l| s.extras.label_looks[&l.id])).unwrap();
        assert_eq!((any.size_nm, any.bold), (2_032, true));
    }

    /// A symbol drawn in the alternate body style of its library symbol is laid out in it, and stays in it.
    #[test]
    fn reorganize_keeps_the_body_style_and_lays_the_symbol_out_in_it() {
        let mut model = mcu30();
        let mut led = eda_model::symbol::builtin("Device:LED").unwrap();
        led.lib_id = "Device:LED".into();
        // an alternate body whose pins sit a cell further out than the normal one's
        let wide = |p: &LibPin| LibPin { at: SPoint::new(p.at.x, p.at.y + p.at.y.signum() * 2.54), ..p.clone() };
        led.alternate = Some(Box::new(AlternateBody::new(led.graphics.clone(), led.pins.iter().map(wide).collect())));
        model.symbols.push(led);
        let mut b = flat_board_with(&model, |root| {
            root.extras.body_styles.insert(field_key("D1", 1), 2);
        });
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let d = b.design();
        let sheet = screen_with(d, "D1");
        assert_eq!(sheet.extras.body_styles.get("D1"), Some(&2), "D1 is still in the alternate body style");
        assert!(sheet.extras.body_styles.get("D2").is_none());
        // it is laid out in that body: the pins the tracer finds are where the wires end
        assert_same_nets(&traced(&model, d), &drawn_nets(&model));
    }

    /// A dual op-amp the way a library has it: unit 1 and unit 2 the amplifiers, unit 3 the power pins.
    fn dual_opamp() -> (LibSymbol, Part) {
        let pin = |number: &str, name: &str, unit: u32, y: f64, left: bool| LibPin {
            number: number.into(),
            name: name.into(),
            electrical_type: if unit == 3 { "power_in" } else { "passive" }.into(),
            shape: "line".into(),
            at: SPoint::new(if left { -7.62 } else { 7.62 }, y),
            angle_deg: if left { 0.0 } else { 180.0 },
            length_mm: 2.54,
            unit,
            hidden: false,
        };
        let sym = LibSymbol {
            lib_id: "Test:Dual3".into(),
            graphics: vec![eda_model::symbol::SymbolGraphic::Rectangle { unit: 0, start: SPoint::new(-5.08, 5.08), end: SPoint::new(5.08, -5.08), stroke_mm: 0.254, filled: false }],
            pins: vec![
                pin("1", "OUT1", 1, 0.0, false),
                pin("2", "IN1-", 1, 2.54, true),
                pin("3", "IN1+", 1, -2.54, true),
                pin("7", "OUT2", 2, 0.0, false),
                pin("6", "IN2-", 2, 2.54, true),
                pin("5", "IN2+", 2, -2.54, true),
                pin("8", "V+", 3, 2.54, false),
                pin("4", "V-", 3, -2.54, false),
            ],
            power: false,
            in_bom: true,
            on_board: true,
            datasheet: String::new(),
            description: String::new(),
            reference_prefix: "U".into(),
            unit_count: 3,
            pin_names_hidden: false,
            pin_numbers_hidden: false,
            pin_name_offset_mm: 0.508,
            alternate: None,
        };
        let pins = ["1", "2", "3", "4", "5", "6", "7", "8"].iter().map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect();
        let part = Part { reference: "U9".into(), mpn: None, lcsc: None, value: Some("TL072".into()), package: None, footprint: None, symbol: Some("Test:Dual3".into()), datasheet: None, pins, body_nm: None, edge: None };
        (sym, part)
    }

    /// mcu30 with a dual op-amp, wired to the rails and to itself.
    fn mcu30_with_opamp() -> ConstraintModel {
        let mut model = mcu30();
        let (sym, part) = dual_opamp();
        model.symbols.push(sym);
        model.parts.push(part);
        model.nets.iter_mut().find(|n| n.name == "VDD").unwrap().pins.push("U9.8".into());
        model.nets.iter_mut().find(|n| n.name == "GND").unwrap().pins.push("U9.4".into());
        model.nets.push(Net { name: "AMP_FB1".into(), pins: vec!["U9.1".into(), "U9.2".into()] });
        model.nets.push(Net { name: "AMP_FB2".into(), pins: vec!["U9.7".into(), "U9.6".into()] });
        model.nets.push(Net { name: "AMP_X".into(), pins: vec!["U9.3".into(), "U9.5".into()] });
        model
    }

    fn unit_of(model: &ConstraintModel, id: &str, unit: u32, at: Point) -> eda_model::ir::SymbolInstance {
        let mut s = eda_model::ir::SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: "Test:Dual3".into(), unit, value: "TL072".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
        s.value = model.part(id).and_then(|p| p.value.clone()).unwrap_or_default();
        s
    }

    #[test]
    fn reorganize_places_every_unit_of_a_multi_unit_part_in_the_same_module_and_the_nets_stay() {
        let model = mcu30_with_opamp();
        // the flat drawing is mcu30's, plus the op-amp's three units placed (and not wired: the nets are the model's)
        let flat = mcu30();
        let mut design: Design = eda_engine::derive_schematic(&flat, &EngineOptions::new(1, "t")).unwrap();
        design.placement = Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
        {
            let root = design.schematic.as_mut().unwrap();
            for (unit, x) in [(1, 300_000_000), (2, 330_000_000), (3, 360_000_000)] {
                root.symbols.push(unit_of(&model, "U9", unit, Point { x, y: 20_000_000 }));
            }
            root.symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
        }
        let mut b = Board::new(design, &model, 100_000, 300_000);
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let d = b.design();
        let units: Vec<(String, u32)> = d.sheet_contents.as_ref().unwrap().values().flat_map(|s| s.symbols.iter()).filter(|s| s.id == "U9").map(|s| (s.id.clone(), s.unit)).collect();
        let mut sorted = units.clone();
        sorted.sort();
        assert_eq!(sorted, vec![("U9".to_string(), 1), ("U9".to_string(), 2), ("U9".to_string(), 3)], "each unit placed once");
        let sheet = screen_with(d, "U9");
        assert_eq!(sheet.symbols.iter().filter(|s| s.id == "U9").count(), 3, "all three units are on one sheet");
        // the part is one part: its identity is kept, and no name made for the layout is left
        assert!(sheet.symbols.iter().filter(|s| s.id == "U9").all(|s| s.lib_id == "Test:Dual3" && s.value == "TL072"));
        for sec in d.sheet_contents.as_ref().unwrap().values() {
            let named: Vec<&str> = sec.symbols.iter().map(|s| s.id.as_str()).chain(sec.wires.iter().flat_map(|w| w.pins.iter().map(String::as_str))).chain(sec.power_symbols.iter().map(|p| p.pin.as_str())).chain(sec.no_connects.iter().map(|n| n.pin.as_str())).collect();
            assert!(named.iter().all(|n| !n.contains('#') || n.starts_with("#PWR") || n.starts_with("#FLG")), "no unit part's name is left in the design: {named:?}");
            assert!(!sec.user_fields.keys().any(|k| k.contains("U9#")) && !sec.extras.locked.iter().any(|k| k.contains("U9#")));
        }
        // each unit has the places of its own fields
        for key in ["U9", "U9#2", "U9#3"] {
            assert!(sheet.field_layout.contains_key(key), "{key}");
        }
        // the nets after are the nets before, drawn back from the units
        assert_same_nets(&traced(&model, d), &drawn_nets(&model));
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
        let add = Cmd::OnSheet { sheet: mcu.id.clone(), cmd: Box::new(Cmd::AddSchText { content: "note".into(), at: to, angle_millideg: 0, size_nm: 1_270 }) };
        b.apply(&add).unwrap();
        assert_eq!(b.design().sheet_contents.as_ref().unwrap()[&file].texts.len(), 1);
        assert!(b.design().schematic.as_ref().unwrap().texts.is_empty(), "nothing leaked onto the root");
        // An unknown path is refused and changes nothing.
        let before = serde_json::to_string(b.design()).unwrap();
        let bad = Cmd::OnSheet { sheet: "no_such_sheet".into(), cmd: Box::new(Cmd::AddSchText { content: "x".into(), at: to, angle_millideg: 0, size_nm: 1_270 }) };
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
        let mut b = Board::new(design, &model, 100_000, 300_000);
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

    /// A reference is one part for the whole design: what another sheet has counts when a symbol is added, renamed or numbered.
    #[test]
    fn a_reference_is_one_part_for_the_whole_design() {
        let model = mcu30();
        let mut b = flat_board(&model);
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let root = b.design().schematic.clone().unwrap();
        let mcu = root.sheets.iter().find(|s| s.name.starts_with("MCU")).unwrap().id.clone();
        let on_mcu = |cmd: Cmd| Cmd::OnSheet { sheet: mcu.clone(), cmd: Box::new(cmd) };
        let add = |id: &str| Cmd::AddSymbol { id: id.into(), lib_id: "Device:R".into(), at: Point { x: 10_160, y: 10_160 }, rot_millideg: 0, value: "1k".into(), footprint: String::new(), unit: 1 };
        // R1 is on the LED channels sheet: the MCU sheet cannot have another
        assert_eq!(b.apply(&on_mcu(add("R1"))).unwrap_err()[0].check, "ops_duplicate_symbol");
        // a part may not be renamed onto a reference another sheet has
        let rename = Cmd::RenameSymbol { id: "C1".into(), new_id: "R2".into() };
        assert_eq!(b.apply(&on_mcu(rename)).unwrap_err()[0].check, "ops_duplicate_symbol");
        // an unnumbered symbol is numbered after the references of every sheet (R1..R8 are the channels')
        b.apply(&on_mcu(add("R?"))).unwrap();
        b.apply(&on_mcu(Cmd::Annotate { reset_existing: false, order: Default::default(), ids: None, options: Default::default() })).unwrap();
        let screens = b.design().sheet_contents.clone().unwrap();
        let mcu_file = &root.sheets.iter().find(|s| s.id == mcu).unwrap().file;
        let mut on_mcu_sheet: Vec<&str> = screens[mcu_file].symbols.iter().map(|s| s.id.as_str()).filter(|r| r.starts_with('R')).collect();
        on_mcu_sheet.sort();
        assert_eq!(on_mcu_sheet, vec!["R9"], "the new resistor is R9: the channels sheet has R1..R8");
    }

    /// The Symbol Fields Table covers every sheet: one view of the design's symbols, and an edit goes to the sheet that has its symbol.
    #[test]
    fn the_fields_table_covers_every_sheet() {
        let model = mcu30();
        let mut b = flat_board(&model);
        b.apply(&Cmd::ReorganizeSheets).unwrap();
        let view = crate::fields_table::project_view(b.design()).unwrap();
        assert_eq!(view.symbols.len(), 30, "the root has no symbol of its own; the table lists all thirty");
        let edit = |id: &str, value: &str| crate::fields_table::FieldEdit { id: id.into(), field: "Value".into(), value: value.into() };
        // R3 is on the channels sheet, C5 on the MCU sheet; the edits are made from the root, with no sheet named
        b.apply(&Cmd::SetSymbolFields { edits: vec![edit("R3", "470"), edit("C5", "22nF")], add_fields: vec!["MPN".into()], rename_fields: vec![], remove_fields: vec![] }).unwrap();
        let screens = b.design().sheet_contents.clone().unwrap();
        let value_of = |reference: &str| screens.values().flat_map(|s| s.symbols.iter()).find(|s| s.id == reference).unwrap().value.clone();
        assert_eq!((value_of("R3").as_str(), value_of("C5").as_str()), ("470", "22nF"));
        assert!(screens.values().filter(|s| !s.symbols.is_empty()).all(|s| s.user_fields.values().all(|f| f.contains_key("MPN"))), "a new column is a column of every sheet");
        // a reference no sheet has is refused, and nothing changed
        let before = serde_json::to_string(b.design()).unwrap();
        let err = b.apply(&Cmd::SetSymbolFields { edits: vec![edit("R3", "1k"), edit("R99", "1k")], add_fields: vec![], rename_fields: vec![], remove_fields: vec![] }).unwrap_err();
        assert_eq!(err[0].check, "ops_bad_fields");
        assert_eq!(serde_json::to_string(b.design()).unwrap(), before);
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
