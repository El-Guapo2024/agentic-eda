//! Geographical Reannotate on the board (`Cmd::ReannotateBoard`): the references renumbered by place, the symbols and everything the board keeps under a reference
//! going with them, at once.
use super::*;
use crate::sch_annotate_tests::{p, section, symbol};
use eda_model::fp_edit::{FootprintEdit, PadEdit};
use eda_model::ir::{BoardPart, Group, ModuleRegion, PlacementSection, Provenance};
use eda_model::reannotate::{ReannotateOptions, Scope};
use eda_model::{Net, Part, Pin, PinKind};

fn part(r: &str, value: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: Some(value.into()),
        package: Some("0603".into()),
        footprint: None,
        symbol: None,
        datasheet: None,
        pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
        body_nm: None,
        edge: None,
    }
}

fn pose(id: &str, x: Nm, y: Nm, side: Side) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: p(x, y), rot: 0, side, label: LabelSide::default() }
}

fn net(name: &str, pins: &[&str]) -> Net {
    Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
}

/// Four resistors in two rows -- R2 R1 on the first, R4 R3 on the second, left to right -- and U1 beside them, with a routed net, edits, a lock and a group on R1, a symbol for each
/// part, and values that say which part is which (R1 is "1k").
fn fixture() -> (ConstraintModel, Design) {
    let model = ConstraintModel {
        parts: vec![part("R1", "1k"), part("R2", "2k"), part("R3", "3k"), part("R4", "4k"), part("U1", "mcu")],
        nets: vec![net("N1", &["R1.2", "U1.1"]), net("N2", &["R2.1", "U1.2"])],
        ..Default::default()
    };
    let sym = |id: &str, x: Nm| symbol(id, "Device:R", "10k", 1, p(x, 0));
    let mut pad_edit = PadEdit::none("1", 1);
    pad_edit.size = Some((900, 900));
    let mut edit = FootprintEdit::new("R1");
    edit.set_pad(pad_edit);
    let design = Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(section(vec![sym("R1", 0), sym("R2", 10_000), sym("R3", 20_000), sym("R4", 30_000), sym("U1", 40_000)], vec![])),
        nets: Some(vec![net("N1", &["R1.2", "U1.1"]), net("N2", &["R2.1", "U1.2"])]),
        placement: Some(PlacementSection {
            outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)],
            footprints: vec![
                pose("R1", 30_000, 20_000, Side::Top),
                pose("R2", 10_000, 20_000, Side::Top),
                pose("R3", 30_000, 40_000, Side::Top),
                pose("R4", 10_000, 40_000, Side::Top),
                pose("U1", 60_000, 30_000, Side::Top),
            ],
            modules: vec![ModuleRegion { name: "main".into(), refs: vec!["R1".into(), "U1".into()], rect: (0, 0, 50_000, 50_000) }],
        }),
        routing: Some(RoutingSection {
            tracks: vec![Track { id: "trk_1".into(), net: "N1".into(), pins: vec!["R1.2".into(), "U1.1".into()], layer: "F.Cu".into(), width: 200, pts: vec![p(31_000, 20_000), p(59_000, 30_000)], arc_mid_offset: None }],
            vias: vec![],
            zones: vec![],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: Some(DrawingsSection {
            footprint_edits: vec![edit],
            locked_ids: vec!["R1".into()],
            groups: vec![Group { id: "grp_1".into(), name: "g".into(), member_ids: vec!["R1".into(), "trk_1".into()], design_block: String::new() }],
            ..Default::default()
        }),
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    };
    (model, design)
}

/// The model the loader builds for `design` from the intent's `model`: the renames laid over it.
fn loaded(model: &ConstraintModel, design: &Design) -> ConstraintModel {
    let mut m = model.clone();
    eda_model::fp_assign::apply_part_renames(design, &mut m);
    m
}

fn at(d: &Design, id: &str) -> Option<(Nm, Nm)> {
    d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == id).map(|f| (f.at.x, f.at.y))
}

fn reannotate(options: ReannotateOptions, selection: &[&str]) -> Cmd {
    Cmd::ReannotateBoard { options, selection: selection.iter().map(|s| s.to_string()).collect() }
}

#[test]
fn the_footprints_are_renumbered_by_place_and_the_symbols_and_the_boards_parts_go_with_them() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    // top to bottom, left to right: the first row is R2 (x 10) then R1 (x 30), the second R4 then R3
    b.apply(&reannotate(ReannotateOptions::default(), &[])).unwrap();
    let d = b.design();
    assert_eq!((at(d, "R1"), at(d, "R2"), at(d, "R3"), at(d, "R4")), (Some((10_000, 20_000)), Some((30_000, 20_000)), Some((10_000, 40_000)), Some((30_000, 40_000))), "each pose is the place the number is for");
    assert_eq!(at(d, "U1"), Some((60_000, 30_000)), "the only U is U1 still");
    assert_eq!(d.placement.as_ref().unwrap().footprints.len(), 5, "no pose lost, none made");
    // the symbols took the numbers: the one drawn first (it was R1) is R2 now
    let sch = d.schematic.as_ref().unwrap();
    let id_at = |x: Nm| sch.symbols.iter().find(|s| s.at.x == x).map(|s| s.id.as_str());
    assert_eq!((id_at(0), id_at(10_000), id_at(20_000), id_at(30_000), id_at(40_000)), (Some("R2"), Some("R1"), Some("R4"), Some("R3"), Some("U1")));
    // what was R1's is R2's: the route's pad, the net's pin, the lock, the edit, the group
    assert_eq!(d.routing.as_ref().unwrap().tracks[0].pins, ["R2.2", "U1.1"], "the route stays on the pad of the same part");
    assert_eq!(d.nets.as_ref().unwrap()[0].pins, ["R2.2", "U1.1"]);
    assert_eq!(d.nets.as_ref().unwrap()[1].pins, ["R1.1", "U1.2"], "R2.1 is R1.1 now");
    let dr = d.drawings.as_ref().unwrap();
    assert_eq!(dr.locked_ids, ["R2"]);
    assert_eq!(dr.footprint_edits.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["R2"]);
    assert_eq!(dr.footprint_edits[0].pad("1", 1).unwrap().size, Some((900, 900)), "its pad edit");
    assert_eq!(dr.groups[0].member_ids, ["R2", "trk_1"]);
    assert_eq!(d.placement.as_ref().unwrap().modules[0].refs, ["R2", "U1"]);
    // the intent's parts are told to the loader as swaps, each pair applied together
    let renames: BTreeSet<(&str, &str)> = dr.renamed_parts.iter().map(|r| (r.from.as_str(), r.to.as_str())).collect();
    assert_eq!(renames, BTreeSet::from([("R1", "R2"), ("R2", "R1"), ("R3", "R4"), ("R4", "R3")]));
    // and the model the loader builds from them has each value under the new reference
    let m = loaded(&model, d);
    let value = |r: &str| m.part(r).and_then(|p| p.value.clone());
    assert_eq!((value("R1"), value("R2"), value("R3"), value("R4"), value("U1")), (Some("2k".into()), Some("1k".into()), Some("4k".into()), Some("3k".into()), Some("mcu".into())));
    assert_eq!(m.nets[0].pins, ["R2.2", "U1.1"]);
}

#[test]
fn the_direction_decides_which_place_gets_the_first_number() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    // left to right, top to bottom: the left column (R2 above R4), then the right (R1 above R3)
    b.apply(&reannotate(ReannotateOptions { direction: 4, ..Default::default() }, &[])).unwrap();
    let d = b.design();
    assert_eq!((at(d, "R1"), at(d, "R2"), at(d, "R3"), at(d, "R4")), (Some((10_000, 20_000)), Some((10_000, 40_000)), Some((30_000, 20_000)), Some((30_000, 40_000))));
}

#[test]
fn a_back_footprint_is_numbered_on_its_own_side_from_its_own_start() {
    let (model, mut design) = fixture();
    design.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "R3").unwrap().side = Side::Bottom;
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&reannotate(ReannotateOptions { back_start: 100, ..Default::default() }, &[])).unwrap();
    let d = b.design();
    // the front: R2 (x 10) R1 (x 30) then R4 -> R1 R2 R3; the back's R3 starts at 100
    assert_eq!((at(d, "R1"), at(d, "R2"), at(d, "R3"), at(d, "R100")), (Some((10_000, 20_000)), Some((30_000, 20_000)), Some((10_000, 40_000)), Some((30_000, 40_000))));
    assert_eq!(at(d, "R4"), None, "no part is left under R4");
    assert!(d.schematic.as_ref().unwrap().symbols.iter().any(|s| s.id == "R100" && s.at.x == 20_000), "the symbol of the former R3");
    assert_eq!(d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "R100").unwrap().side, Side::Bottom);
    let m = loaded(&model, d);
    assert_eq!(m.part("R100").and_then(|p| p.value.clone()), Some("3k".into()));
    assert!(m.part("R3").is_some_and(|p| p.value.as_deref() == Some("4k")), "R3 is the former R4");
}

#[test]
fn a_part_that_is_not_on_the_board_keeps_its_number_and_is_not_taken() {
    let (model, mut design) = fixture();
    // R2 has a symbol but is not placed: its number is not given out
    design.placement.as_mut().unwrap().footprints.retain(|f| f.id != "R2");
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&reannotate(ReannotateOptions::default(), &[])).unwrap();
    let d = b.design();
    // placed: R1 (30,20), R4 (10,40), R3 (30,40) -> by place R1, R4, R3 -> R1, R3 (2 is taken), R4
    assert_eq!((at(d, "R1"), at(d, "R3"), at(d, "R4")), (Some((30_000, 20_000)), Some((10_000, 40_000)), Some((30_000, 40_000))));
    assert_eq!(at(d, "R2"), None, "still not placed");
    assert_eq!(d.schematic.as_ref().unwrap().symbols.iter().filter(|s| s.id == "R2").count(), 1, "its symbol is as it was");
}

#[test]
fn only_the_selection_is_renumbered_and_the_others_keep_their_numbers() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    // R4 alone, scope Selection: R1, R2 and R3 keep theirs, so R4 gets the first free number, 4: nothing changes
    b.apply(&reannotate(ReannotateOptions { scope: Scope::Selection, ..Default::default() }, &["R4"])).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().renamed_parts, [], "R4 is where the free number is");
    // R3 and R4 selected, the others kept: they take 3 and 4 by place (R4 left of R3)
    let mut b = Board::new(fixture().1, &model, 100, 300);
    b.apply(&reannotate(ReannotateOptions { scope: Scope::Selection, ..Default::default() }, &["R3", "R4"])).unwrap();
    assert_eq!(at(b.design(), "R3"), Some((10_000, 40_000)));
    assert_eq!(at(b.design(), "R4"), Some((30_000, 40_000)));
    assert_eq!(at(b.design(), "R1"), Some((30_000, 20_000)), "R1 is not selected: it is where it was, under its number");
    assert_eq!(b.design().drawings.as_ref().unwrap().renamed_parts.len(), 2, "a swap of the two");
}

#[test]
fn a_footprint_that_exists_only_on_the_board_is_renumbered_with_the_rest() {
    let (mut model, mut design) = fixture();
    model.parts.push(part("H1", "hole"));
    model.parts.push(part("H2", "hole"));
    let dr = design.drawings.as_mut().unwrap();
    for h in ["H1", "H2"] {
        dr.board_parts.push(BoardPart { reference: h.into(), value: None, footprint: "MountingHole:MH".into(), definition: None, pad_nets: vec![] });
    }
    let pl = design.placement.as_mut().unwrap();
    pl.footprints.push(pose("H1", 80_000, 10_000, Side::Top));
    pl.footprints.push(pose("H2", 5_000, 10_000, Side::Top));
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&reannotate(ReannotateOptions::default(), &[])).unwrap();
    let d = b.design();
    assert_eq!((at(d, "H1"), at(d, "H2")), (Some((5_000, 10_000)), Some((80_000, 10_000))), "the hole on the left is H1 now");
    assert_eq!(d.drawings.as_ref().unwrap().board_parts.iter().map(|h| h.reference.as_str()).collect::<Vec<_>>().len(), 2, "both holes are still there");
    assert!(d.schematic.as_ref().unwrap().symbols.iter().all(|s| !s.id.starts_with('H')), "no symbol was made for them");
}

#[test]
fn the_plan_changes_nothing_and_a_run_with_nothing_to_renumber_does_nothing() {
    let (model, design) = fixture();
    let b = Board::new(design.clone(), &model, 100, 300);
    let plan = b.reannotate_plan(&ReannotateOptions::default(), &[]);
    assert!(plan.ok());
    assert_eq!(plan.renames().len(), 4, "R1 <-> R2 and R3 <-> R4");
    assert_eq!(serde_json::to_string(b.design()).unwrap(), serde_json::to_string(&design).unwrap(), "the plan is a look");
    // the numbering by place of a board already numbered that way
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&reannotate(ReannotateOptions::default(), &[])).unwrap();
    let once = b.into_design();
    let model2 = loaded(&model, &once);
    let mut b = Board::new(once.clone(), &model2, 100, 300);
    b.apply(&reannotate(ReannotateOptions::default(), &[])).unwrap();
    assert_eq!(serde_json::to_string(b.design()).unwrap(), serde_json::to_string(&once).unwrap(), "a second run finds it numbered");
}

#[test]
fn it_is_a_step_of_both_editors_and_a_pcb_command() {
    let cmd = reannotate(ReannotateOptions::default(), &[]);
    assert!(cmd.spans_both_editors());
    assert_ne!(cmd.domain(), Domain::Schematic);
    assert!(Cmd::Batch { cmds: vec![cmd.clone()] }.spans_both_editors());
}
