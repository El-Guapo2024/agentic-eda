//! A rename that carries the board's part (`Board::rename_part`): Edit Reference, Annotate and Increment Annotations leave nothing under the old reference.
use super::*;
use crate::sch_annotate_tests::{p, section, symbol};
use eda_model::fp_assign::PartRename;
use eda_model::fp_edit::{FootprintEdit, PadEdit};
use eda_model::ir::{FootprintExtra, FootprintZoneOverrides, Group, ModuleRegion, PlacementSection, Provenance};
use eda_model::{Net, Part, Pin, PinKind};

fn part(r: &str, pkg: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(pkg.into()),
        footprint: None,
        symbol: None,
        datasheet: None,
        pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
        body_um: None,
        edge: None,
    }
}

fn pose(id: &str, x: Um) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: p(x, 20_000), rot: 0, side: Side::Top, label: LabelSide::default() }
}

fn net(name: &str, pins: &[&str]) -> eda_model::Net {
    Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
}

/// R1, R2 and U1 on a board with a stored schematic, routed from R1.2 to U1.1, with a net list, a floorplan block, and every kind of edit a footprint can have.
fn fixture() -> (ConstraintModel, Design) {
    let model = ConstraintModel { parts: vec![part("R1", "0603"), part("R2", "0603"), part("U1", "SOIC-8")], nets: vec![net("N1", &["R1.2", "U1.1"]), net("N2", &["R2.1", "U1.2"])], ..Default::default() };
    let sym = |id: &str, x: Um| symbol(id, "Device:R", "10k", 1, p(x, 0));
    let mut pad_edit = PadEdit::none("1", 1);
    pad_edit.size = Some((900, 900));
    let mut edit = FootprintEdit::new("U1");
    edit.set_pad(pad_edit);
    let design = Design {
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(section(vec![sym("R1", 0), sym("R2", 10_000), sym("U1", 20_000)], vec![])),
        nets: Some(vec![net("N1", &["R1.2", "U1.1"]), net("N2", &["R2.1", "U1.2"])]),
        placement: Some(PlacementSection {
            outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)],
            footprints: vec![pose("R1", 10_000), pose("R2", 20_000), pose("U1", 40_000)],
            modules: vec![ModuleRegion { name: "main".into(), refs: vec!["R1".into(), "U1".into()], rect: (0, 0, 50_000, 50_000) }],
        }),
        routing: Some(RoutingSection {
            tracks: vec![Track { id: "trk_1".into(), net: "N1".into(), pins: vec!["R1.2".into(), "U1.1".into()], layer: "F.Cu".into(), width: 200, pts: vec![p(11_000, 20_000), p(39_000, 20_000)], arc_mid_offset: None }],
            vias: vec![],
            zones: vec![],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: Some(DrawingsSection {
            footprint_edits: vec![edit],
            footprint_extras: vec![FootprintExtra { id: "R1".into(), ..Default::default() }],
            zone_overrides: vec![FootprintZoneOverrides { id: "U1".into(), ..Default::default() }],
            locked_ids: vec!["R1".into(), "U1".into()],
            groups: vec![Group { id: "grp_1".into(), name: "g".into(), member_ids: vec!["R1".into(), "U1".into(), "trk_1".into()] }],
            ..Default::default()
        }),
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    };
    (model, design)
}

/// Every string of the design that is, or starts like, an id of `reference`: `U1`, `U1.3`, `U1:Reference`. The list of renames is where the old reference
/// is meant to stay, so it is not looked in.
fn mentions(d: &Design, reference: &str) -> Vec<String> {
    let mut d = d.clone();
    if let Some(dr) = d.drawings.as_mut() {
        dr.renamed_parts.clear();
    }
    let text = serde_json::to_string(&d).unwrap();
    let mut found = Vec::new();
    for suffix in ["\"", ".", ":"] {
        let needle = format!("\"{reference}{suffix}");
        if text.contains(&needle) {
            found.push(needle);
        }
    }
    found
}

/// The model the loader builds for `design` from the intent's `model` (`board::build_model`): the renames laid over it.
fn loaded(model: &ConstraintModel, design: &Design) -> ConstraintModel {
    let mut m = model.clone();
    eda_model::fp_assign::apply_part_renames(design, &mut m);
    m
}

#[test]
fn renaming_a_symbol_moves_the_boards_part_with_it_and_leaves_nothing_under_the_old_reference() {
    let (model, design) = fixture();
    let before = design.clone();
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&Cmd::RenameSymbol { id: "U1".into(), new_id: "U7".into() }).unwrap();
    let d = b.design();
    assert!(mentions(d, "U1").is_empty(), "stranded: {:?}", mentions(d, "U1"));
    assert!(!mentions(d, "U7").is_empty());
    let pl = d.placement.as_ref().unwrap();
    assert_eq!(pl.footprints.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["R1", "R2", "U7"], "the pose, still sorted");
    assert_eq!(pl.footprints[2].at, p(40_000, 20_000), "where it was");
    assert_eq!(pl.modules[0].refs, ["R1", "U7"]);
    assert_eq!(d.routing.as_ref().unwrap().tracks[0].pins, ["R1.2", "U7.1"], "the route still ends on its pad");
    assert_eq!(d.nets.as_ref().unwrap()[0].pins, ["R1.2", "U7.1"]);
    assert_eq!(d.nets.as_ref().unwrap()[1].pins, ["R2.1", "U7.2"]);
    let dr = d.drawings.as_ref().unwrap();
    assert_eq!(dr.footprint_edits[0].id, "U7");
    assert_eq!(dr.footprint_edits[0].pad("1", 1).unwrap().size, Some((900, 900)), "its pad edit");
    assert_eq!(dr.zone_overrides[0].id, "U7");
    assert_eq!(dr.locked_ids, ["R1", "U7"]);
    assert_eq!(dr.groups[0].member_ids, ["R1", "U7", "trk_1"]);
    assert_eq!(dr.footprint_extras[0].id, "R1", "the other parts are untouched");
    assert_eq!(dr.renamed_parts, [PartRename { from: "U1".into(), to: "U7".into() }], "the intent's part is recorded for the loader");
    // the symbol, as before
    assert!(d.schematic.as_ref().unwrap().symbols.iter().any(|s| s.id == "U7") && d.schematic.as_ref().unwrap().symbols.iter().all(|s| s.id != "U1"));
    // the design before the command is as it was (the verb is one apply: a step the history can take back whole)
    assert!(!mentions(&before, "U1").is_empty());
}

#[test]
fn a_second_rename_moves_the_one_entry_on_and_the_way_back_drops_it() {
    let (intent, design) = fixture();
    let mut b = Board::new(design, &intent, 100, 300);
    b.apply(&Cmd::RenameSymbol { id: "U1".into(), new_id: "U7".into() }).unwrap();
    // the next command sees the model the loader builds: the intent's U1 is U7 in it
    let design = b.into_design();
    let model = loaded(&intent, &design);
    assert_eq!(model.parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), ["R1", "R2", "U7"], "no part left under U1");
    assert_eq!(model.nets[0].pins, ["R1.2", "U7.1"]);
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&Cmd::RenameSymbol { id: "U7".into(), new_id: "U9".into() }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().renamed_parts, [PartRename { from: "U1".into(), to: "U9".into() }]);
    assert!(mentions(b.design(), "U7").is_empty());
    let design = b.into_design();
    let model = loaded(&intent, &design);
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&Cmd::RenameSymbol { id: "U9".into(), new_id: "U1".into() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().renamed_parts.is_empty(), "back under the intent's own reference");
    assert!(!mentions(b.design(), "U1").is_empty());
    assert!(mentions(b.design(), "U9").is_empty());
}

#[test]
fn a_footprint_made_on_the_board_is_not_taken_by_a_rename() {
    let (mut model, mut design) = fixture();
    model.parts.push(part("H1", "0603"));
    design.drawings.as_mut().unwrap().board_parts.push(eda_model::ir::BoardPart { reference: "H1".into(), value: None, footprint: "MountingHole:MH".into(), definition: None, pad_nets: vec![] });
    design.placement.as_mut().unwrap().footprints.push(pose("H1", 60_000));
    let mut b = Board::new(design, &model, 100, 300);
    let e = b.apply(&Cmd::RenameSymbol { id: "R2".into(), new_id: "H1".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_duplicate_symbol");
    assert!(e[0].hint.as_deref().unwrap_or("").contains("footprint"), "{e:?}");
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols.iter().filter(|s| s.id == "R2").count(), 1, "a refused rename changes nothing");
}

#[test]
fn increment_annotations_moves_the_parts_in_one_go_so_no_two_share_a_reference() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&Cmd::IncrementAnnotations { start: "R1".into(), increment: 1 }).unwrap();
    let d = b.design();
    assert_eq!(d.placement.as_ref().unwrap().footprints.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["R2", "R3", "U1"]);
    assert_eq!(d.placement.as_ref().unwrap().footprints[0].at, p(10_000, 20_000), "R1 became R2 where it stood");
    assert_eq!(d.routing.as_ref().unwrap().tracks[0].pins, ["R2.2", "U1.1"]);
    assert_eq!(d.nets.as_ref().unwrap()[1].pins, ["R3.1", "U1.2"], "R2.1 -> R3.1");
    let dr = d.drawings.as_ref().unwrap();
    assert_eq!(dr.renamed_parts, [PartRename { from: "R1".into(), to: "R2".into() }, PartRename { from: "R2".into(), to: "R3".into() }], "simultaneous, not a chain");
    assert_eq!(dr.locked_ids, ["R2", "U1"]);
    assert!(d.placement.as_ref().unwrap().footprints.iter().all(|f| f.id != "R1"), "nothing is left under R1");
}

#[test]
fn annotate_with_reset_numbers_the_parts_by_place_and_the_board_follows_each() {
    let (model, mut design) = fixture();
    // R1 right of R2 on the sheet: a reset in reading order swaps their numbers
    let sch = design.schematic.as_mut().unwrap();
    sch.symbols[0].at = p(30_000, 0);
    let mut b = Board::new(design, &model, 100, 300);
    let opts = AnnotateOptions { scope: Some(AnnotateScope::All), ..Default::default() };
    b.apply(&Cmd::Annotate { reset_existing: true, order: AnnotateOrder::XThenY, ids: None, options: opts }).unwrap();
    let d = b.design();
    let at = |id: &str| d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == id).map(|f| f.at.x);
    // the part that was R1 (pose x=10000, symbol now at x=30000) is the second resistor: R2; the former R2 (pose x=20000) is R1
    assert_eq!((at("R1"), at("R2")), (Some(20_000), Some(10_000)), "the poses followed their symbols");
    assert_eq!(d.routing.as_ref().unwrap().tracks[0].pins, ["R2.2", "U1.1"], "the route stays on the pad of the same part");
    assert_eq!(d.nets.as_ref().unwrap()[0].pins, ["R2.2", "U1.1"]);
    assert_eq!(d.nets.as_ref().unwrap()[1].pins, ["R1.1", "U1.2"]);
    let renames: Vec<(&str, &str)> = d.drawings.as_ref().unwrap().renamed_parts.iter().map(|r| (r.from.as_str(), r.to.as_str())).collect();
    assert_eq!(renames, [("R1", "R2"), ("R2", "R1")], "a swap is two entries, applied together");
    assert_eq!(d.placement.as_ref().unwrap().footprints.len(), 3, "no pose lost, none made");
}

#[test]
fn a_rename_is_a_step_of_both_editors() {
    let rename = Cmd::RenameSymbol { id: "U1".into(), new_id: "U2".into() };
    assert!(rename.spans_both_editors());
    assert_eq!(rename.domain(), Domain::Schematic, "it is still a schematic command (its tab, its connectivity)");
    assert!(Cmd::IncrementAnnotations { start: "R1".into(), increment: 1 }.spans_both_editors());
    assert!(Cmd::Annotate { reset_existing: false, order: AnnotateOrder::default(), ids: None, options: Default::default() }.spans_both_editors());
    assert!(Cmd::OnSheet { sheet: "a".into(), cmd: Box::new(rename.clone()) }.spans_both_editors());
    assert!(Cmd::Batch { cmds: vec![Cmd::AddJunction { at: p(0, 0) }, rename] }.spans_both_editors());
    assert!(!Cmd::MoveSymbol { id: "U1".into(), x: 0, y: 0, unit: None }.spans_both_editors());
    assert!(!Cmd::EditSymbolFields { id: "U1".into(), value: Some("1k".into()), footprint: None, datasheet: None }.spans_both_editors());
}
