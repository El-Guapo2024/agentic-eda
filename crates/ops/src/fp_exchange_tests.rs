//! Change Footprint, Update Footprint from Library and the exchange the symbol's Footprint field asks for (`Cmd::ExchangeFootprints`): what stays with the part, what
//! goes with the old footprint, and what the dialog's options decide.
use super::*;
use crate::sch_annotate_tests::{p, section, symbol};
use eda_model::footprint::{Pad, PadKind, PadShape};
use eda_model::fp_assign::FootprintAssignment;
use eda_model::fp_art::FpText;
use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind, PadEdit, UserField};
use eda_model::ir::{BoardPart, FootprintExtra, FootprintZoneFacts, FootprintZoneOverrides, Group, PadConnection, PadZoneOverride, PlacementSection, Provenance};
use eda_model::{Net, Part, Pin, PinKind};

fn pad(number: &str, x: Nm, y: Nm) -> Pad {
    Pad { number: number.into(), at: (x, y), size: (900, 900), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
}

/// `Test:R_A`: the footprint the resistor is on. Pads at +-1 mm, its reference on the fab layer.
fn r_a() -> Footprint {
    let mut art = eda_model::fp_art::FpArt::default();
    art.texts.push(FpText::new("${REFERENCE}", Point { x: 0, y: 0 }, "F.Fab"));
    Footprint { name: "Test:R_A".into(), pads: vec![pad("1", -1_000, 0), pad("2", 1_000, 0)], courtyard: Some((1_700, 800)), model: Some("a.step".into()), art, ..Default::default() }
}

/// `Test:R_B`: the library's other footprint for it. Pads at +-1.5 mm and a third, its own text and model.
fn r_b() -> Footprint {
    let mut art = eda_model::fp_art::FpArt::default();
    art.texts.push(FpText::new("${REFERENCE}", Point { x: 0, y: 0 }, "F.Fab"));
    art.texts.push(FpText::new("B", Point { x: 0, y: 2_000 }, "F.SilkS"));
    Footprint { name: "Test:R_B".into(), pads: vec![pad("1", -1_500, 0), pad("2", 1_500, 0), pad("3", 0, 1_500)], courtyard: Some((2_200, 1_200)), model: Some("b.step".into()), art, ..Default::default() }
}

fn part(r: &str, footprint: &str, pins: usize) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: Some("10k".into()),
        package: None,
        footprint: Some(footprint.into()),
        symbol: None,
        datasheet: None,
        pins: (1..=pins).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_nm: None,
        edge: None,
    }
}

fn net(name: &str, pins: &[&str]) -> Net {
    Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
}

fn pose(id: &str, x: Nm, rot: u32, side: Side) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: p(x, 20_000), rot, side, label: LabelSide::default() }
}

/// R1 on `Test:R_A` turned a quarter, with every kind of edit a footprint can carry; R2 beside it.
fn fixture() -> (ConstraintModel, Design) {
    let model = ConstraintModel { parts: vec![part("R1", "Test:R_A", 2), part("R2", "Test:R_A", 2)], nets: vec![net("N1", &["R1.1", "R2.1"]), net("N2", &["R1.2", "R2.2", "R1.3"])], footprints: vec![r_a()], ..Default::default() };
    let mut sym = symbol("R1", "Device:R", "10k", 1, p(0, 0));
    sym.footprint = "Test:R_A".into();
    let mut sym2 = symbol("R2", "Device:R", "10k", 1, p(10_000, 0));
    sym2.footprint = "Test:R_A".into();
    let mut edit = FootprintEdit::new("R1");
    let mut moved = FieldLayout::new(Point { x: 0, y: -3_000 }, "F.SilkS");
    moved.size = (700, 700);
    edit.reference = Some(moved);
    edit.fields.push(UserField { name: "Vendor".into(), text: "ACME".into(), layout: FieldLayout::new(Point { x: 0, y: 3_000 }, "F.Fab") });
    edit.attrs = Some(FootprintAttrs { kind: FootprintKind::ThroughHole, dnp: true, ..Default::default() });
    let mut pad_edit = PadEdit::none("1", 1);
    pad_edit.size = Some((1_200, 1_200));
    edit.set_pad(pad_edit);
    let design = Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(section(vec![sym, sym2], vec![])),
        nets: Some(vec![net("N1", &["R1.1", "R2.1"]), net("N2", &["R1.2", "R2.2", "R1.3"])]),
        placement: Some(PlacementSection { outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)], footprints: vec![pose("R1", 10_000, 90_000, Side::Top), pose("R2", 30_000, 0, Side::Top)], modules: vec![] }),
        routing: None,
        drawings: Some(DrawingsSection {
            footprint_edits: vec![edit],
            zone_overrides: vec![FootprintZoneOverrides {
                id: "R1".into(),
                footprint: Some(FootprintZoneFacts { zone_connection: Some(PadConnection::Full), clearance: Some(250) }),
                pads: vec![PadZoneOverride { number: "1".into(), clearance: Some(300), ..Default::default() }],
            }],
            footprint_extras: vec![FootprintExtra { id: "R1".into(), solder_mask_margin: Some(40), clearance: Some(250), ..Default::default() }],
            locked_ids: vec!["R1".into()],
            groups: vec![Group { id: "grp_1".into(), name: "g".into(), member_ids: vec!["R1".into(), "R2".into()], design_block: String::new() }],
            ..Default::default()
        }),
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    };
    (model, design)
}

fn exchange(references: &[&str], footprint: Option<&str>, options: ExchangeOptions, library: &[Footprint]) -> Cmd {
    Cmd::ExchangeFootprints { references: references.iter().map(|s| s.to_string()).collect(), footprint: footprint.map(str::to_string), options, library: library.iter().map(|f| (f.name.clone(), f.clone())).collect() }
}

fn pose_of(d: &Design, id: &str) -> FootprintInstance {
    d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == id).cloned().unwrap()
}

#[test]
fn change_footprint_puts_the_part_on_the_new_one_and_keeps_its_pose_nets_lock_and_group() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    let d = b.design();
    let dr = d.drawings.as_ref().unwrap();
    let a = d.footprint_assignment("R1").expect("the part is assigned its footprint");
    assert_eq!(a.footprint, "Test:R_B");
    assert_eq!(a.definition.as_ref().map(|f| f.pads.len()), Some(3), "the board keeps its copy: the model does not know Test:R_B");
    assert!(d.footprint_assignment("R2").is_none(), "the other part is on its footprint still");
    // the pose: same side and label; the pads +-1000 against +-1500 have no shift that matches more than half of them, and the mean is nothing
    let pose = pose_of(d, "R1");
    assert_eq!((pose.at, pose.rot, pose.side), (p(10_000, 20_000), 90_000, Side::Top));
    // the schematic's nets are by pad number: R1 is on N1 and N2 as before, and its third pad (R1.3) has its net already
    assert_eq!(d.nets.as_ref().unwrap()[1].pins, ["R1.2", "R2.2", "R1.3"]);
    assert_eq!(dr.locked_ids, ["R1"]);
    assert_eq!(dr.groups[0].member_ids, ["R1", "R2"]);
    // the edits of the old pads are gone, the zone overrides and imported facts too (the options say so)
    let edit = d.footprint_edit("R1").expect("the user field is kept: extra text items are not removed in a change");
    assert!(edit.pads.is_empty(), "pad edits belonged to the old pads");
    assert_eq!(edit.fields.len(), 1);
    assert!(edit.reference.is_none(), "the Reference layout is the new footprint's default: text layout reset");
    assert!(edit.attrs.is_none(), "fabrication attributes reset");
    assert!(dr.zone_overrides.is_empty() && dr.footprint_extras.is_empty(), "clearance overrides reset");
    // the symbol names the footprint the board has
    assert_eq!(d.schematic.as_ref().unwrap().symbols[0].footprint, "Test:R_B");
    assert_eq!(d.schematic.as_ref().unwrap().symbols[1].footprint, "Test:R_A");
    // the footprint's own text the new one has no match for is kept; the new one's own "B" is there too
    let texts: Vec<&str> = a.definition.as_ref().unwrap().art.texts.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, ["${REFERENCE}", "B"]);
}

#[test]
fn the_options_decide_what_the_board_keeps_of_the_old_footprint() {
    let (model, design) = fixture();
    let keep_everything = ExchangeOptions { match_pad_positions: true, remove_extra_texts: false, reset_text_layers: false, reset_text_effects: false, reset_text_positions: false, reset_text_content: false, reset_fabrication_attrs: false, reset_clearance_overrides: false, reset_3d_models: false };
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_B"), keep_everything, &[r_b()])).unwrap();
    let d = b.design();
    let edit = d.footprint_edit("R1").unwrap();
    // the Reference stays where it was on the board, at its size
    let reference = edit.reference.as_ref().expect("the old Reference layout is kept");
    assert_eq!((reference.size, reference.layer.as_str()), ((700, 700), "F.SilkS"));
    assert_eq!(reference.board_position(&pose_of(d, "R1")), FieldLayout::new(Point { x: 0, y: -3_000 }, "F.SilkS").board_position(&pose_of(&fixture().1, "R1")));
    assert!(edit.attrs.is_some_and(|a| a.dnp && a.kind == FootprintKind::ThroughHole), "the attributes as they were");
    // the footprint-level overrides stay, the pads' do not
    let dr = d.drawings.as_ref().unwrap();
    assert_eq!(dr.zone_overrides.len(), 1);
    assert!(dr.zone_overrides[0].pads.is_empty() && dr.zone_overrides[0].footprint.is_some_and(|f| f.clearance == Some(250)));
    assert_eq!((dr.footprint_extras.len(), dr.footprint_extras[0].solder_mask_margin, dr.footprint_extras[0].clearance), (1, Some(40), Some(250)));
    // the 3D model is the old footprint's
    assert_eq!(d.footprint_assignment("R1").unwrap().definition.as_ref().unwrap().model.as_deref(), Some("a.step"));
}

#[test]
fn remove_text_items_drops_what_the_new_footprint_has_no_match_for() {
    let (mut model, design) = fixture();
    // an extra text on the old footprint, which the model's footprint carries
    model.footprints[0].art.texts.push(FpText::new("keep me?", Point { x: 4_000, y: 0 }, "F.SilkS"));
    let mut b = Board::new(design.clone(), &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    let kept: Vec<String> = b.design().footprint_assignment("R1").unwrap().definition.as_ref().unwrap().art.texts.iter().map(|t| t.text.clone()).collect();
    assert!(kept.contains(&"keep me?".to_string()), "kept when text items are not removed: {kept:?}");
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions { remove_extra_texts: true, ..ExchangeOptions::change() }, &[r_b()])).unwrap();
    let a = b.design().footprint_assignment("R1").unwrap();
    assert!(a.definition.as_ref().unwrap().art.texts.iter().all(|t| t.text != "keep me?"), "removed");
    assert!(b.design().footprint_edit("R1").is_none_or(|e| e.fields.is_empty()), "and the user field with it");
}

#[test]
fn match_pad_positions_moves_the_footprint_so_its_pads_stay_where_they_were() {
    let (model, design) = fixture();
    // the new footprint has its origin on pad 1
    let shifted = Footprint { name: "Test:R_S".into(), pads: vec![pad("1", 0, 0), pad("2", 2_000, 0)], ..Default::default() };
    let mut b = Board::new(design.clone(), &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_S"), ExchangeOptions::change(), &[shifted.clone()])).unwrap();
    let was = pose_of(&design, "R1");
    let now = pose_of(b.design(), "R1");
    for number in ["1", "2"] {
        let old_pad = r_a().pads.iter().find(|p| p.number == number).unwrap().clone();
        let new_pad = shifted.pads.iter().find(|p| p.number == number).unwrap().clone();
        assert_eq!(eda_model::footprint::to_board(&now, new_pad.at), eda_model::footprint::to_board(&was, old_pad.at), "pad {number} is where it was");
    }
    assert_ne!(now.at, was.at);
    // and left alone without the option
    let mut b = Board::new(design.clone(), &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_S"), ExchangeOptions { match_pad_positions: false, ..ExchangeOptions::change() }, &[shifted])).unwrap();
    let kept = pose_of(b.design(), "R1");
    assert_eq!((kept.at, kept.rot, kept.side), (was.at, was.rot, was.side));
}

#[test]
fn a_footprint_no_library_has_refuses_the_whole_command() {
    let (model, design) = fixture();
    let mut b = Board::new(design.clone(), &model, 100, 300);
    let e = b.apply(&exchange(&["R1", "R2"], Some("Test:Nowhere"), ExchangeOptions::change(), &[])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
    assert!(e[0].hint.as_deref().unwrap_or("").contains("not found"));
    let e = b.apply(&exchange(&["R9"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_part");
    let e = b.apply(&exchange(&[], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_to_change");
    assert_eq!(serde_json::to_string(b.design()).unwrap(), serde_json::to_string(&design).unwrap(), "a refused command changes nothing");
    // a footprint the model knows (another part is on it) needs no copy in the command
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_A"), ExchangeOptions::change(), &[])).unwrap();
}

#[test]
fn update_from_library_puts_the_library_copy_on_the_footprint_the_part_is_on() {
    let (model, mut design) = fixture();
    // R1 holds a stale copy of Test:R_A (an older pad size)
    let mut stale = r_a();
    stale.pads[0].size = (500, 500);
    design.drawings.as_mut().unwrap().footprint_assignments.push(FootprintAssignment { reference: "R1".into(), footprint: "Test:R_A".into(), definition: Some(stale) });
    let mut model_with_copy = model.clone();
    model_with_copy.instance_footprints.insert("R1".into(), design.footprint_assignment("R1").unwrap().definition.clone().unwrap());
    let mut library_copy = r_a();
    library_copy.pads[0].size = (1_000, 1_000);
    let mut b = Board::new(design, &model_with_copy, 100, 300);
    b.apply(&exchange(&["R1"], None, ExchangeOptions::update(), &[library_copy.clone()])).unwrap();
    let a = b.design().footprint_assignment("R1").unwrap();
    assert_eq!(a.footprint, "Test:R_A");
    assert_eq!(a.definition.as_ref().map(|f| f.pads[0].size), Some((1_000, 1_000)), "the board's copy is the library's now");
    // the symbol's field is not touched by an update
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols[0].footprint, "Test:R_A");
    // the update options keep what the board has of the texts, the attributes and the pose
    let edit = b.design().footprint_edit("R1").unwrap();
    assert!(edit.reference.is_some() && edit.attrs.is_some(), "Update keeps the Reference layout and the attributes unless asked");
    // a footprint with no library copy is not found, as KiCad says
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    let e = b.apply(&exchange(&["R1"], None, ExchangeOptions::update(), &[])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
}

#[test]
fn a_footprint_that_exists_only_on_the_board_keeps_the_nets_of_the_pads_the_new_one_has() {
    let (mut model, mut design) = fixture();
    model.parts.push(part("H1", "Test:R_A", 3));
    let dr = design.drawings.as_mut().unwrap();
    dr.board_parts.push(BoardPart { reference: "H1".into(), value: Some("R_A".into()), footprint: "Test:R_A".into(), definition: None, pad_nets: vec![("1".into(), "N1".into()), ("2".into(), "N2".into()), ("9".into(), "N9".into())] });
    design.placement.as_mut().unwrap().footprints.push(pose("H1", 50_000, 0, Side::Bottom));
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&exchange(&["H1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    let bp = &b.design().drawings.as_ref().unwrap().board_parts[0];
    assert_eq!(bp.footprint, "Test:R_B");
    assert_eq!(bp.pad_nets, [("1".to_string(), "N1".to_string()), ("2".to_string(), "N2".to_string())], "pad 9 is not on the new footprint");
    assert_eq!(bp.value.as_deref(), Some("R_B"), "a value that was the footprint's name follows it");
    assert_eq!(bp.definition.as_ref().map(|d| d.pads.len()), Some(3));
    assert_eq!(pose_of(b.design(), "H1").side, Side::Bottom, "it stays on its side");
}

#[test]
fn the_symbols_whose_footprint_field_differs_from_the_board_are_the_pending_changes() {
    let (model, mut design) = fixture();
    design.schematic.as_mut().unwrap().symbols[1].footprint = "Test:R_B".into();
    design.drawings.as_mut().unwrap().locked_ids.push("R2".into());
    let changes = pending_footprint_changes(&design, &model);
    assert_eq!(changes, [FootprintChange { reference: "R2".into(), from: "Test:R_A".into(), to: "Test:R_B".into(), locked: true }]);
    // a symbol with no field, or the same one, says nothing
    design.schematic.as_mut().unwrap().symbols[1].footprint = String::new();
    assert!(pending_footprint_changes(&design, &model).is_empty());
}

#[test]
fn a_rename_after_an_exchange_takes_the_assignment_with_the_part() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, 100, 300);
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    b.apply(&Cmd::RenameSymbol { id: "R1".into(), new_id: "R7".into() }).unwrap();
    let d = b.design();
    assert!(d.footprint_assignment("R1").is_none());
    assert_eq!(d.footprint_assignment("R7").unwrap().footprint, "Test:R_B");
}
