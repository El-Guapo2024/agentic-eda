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
use eda_model::units::mm;

fn pad(number: &str, x: Nm, y: Nm) -> Pad {
    Pad { number: number.into(), at: (x, y), size: (mm(0.9), mm(0.9)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
}

/// `Test:R_A`: the footprint the resistor is on. Pads at +-1 mm, its reference on the fab layer.
fn r_a() -> Footprint {
    let mut art = eda_model::fp_art::FpArt::default();
    art.texts.push(FpText::new("${REFERENCE}", Point { x: Nm::ZERO, y: Nm::ZERO }, "F.Fab"));
    Footprint { name: "Test:R_A".into(), pads: vec![pad("1", mm(-1.0), Nm::ZERO), pad("2", mm(1.0), Nm::ZERO)], courtyard: Some((mm(1.7), mm(0.8))), model: Some("a.step".into()), art, ..Default::default() }
}

/// `Test:R_B`: the library's other footprint for it. Pads at +-1.5 mm and a third, its own text and model.
fn r_b() -> Footprint {
    let mut art = eda_model::fp_art::FpArt::default();
    art.texts.push(FpText::new("${REFERENCE}", Point { x: Nm::ZERO, y: Nm::ZERO }, "F.Fab"));
    art.texts.push(FpText::new("B", Point { x: Nm::ZERO, y: mm(2.0) }, "F.SilkS"));
    Footprint { name: "Test:R_B".into(), pads: vec![pad("1", mm(-1.5), Nm::ZERO), pad("2", mm(1.5), Nm::ZERO), pad("3", Nm::ZERO, mm(1.5))], courtyard: Some((mm(2.2), mm(1.2))), model: Some("b.step".into()), art, ..Default::default() }
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
    FootprintInstance { id: id.into(), at: p(x, mm(20.0)), rot, side, label: LabelSide::default() }
}

/// R1 on `Test:R_A` turned a quarter, with every kind of edit a footprint can carry; R2 beside it.
fn fixture() -> (ConstraintModel, Design) {
    let model = ConstraintModel { parts: vec![part("R1", "Test:R_A", 2000), part("R2", "Test:R_A", 2000)], nets: vec![net("N1", &["R1.1", "R2.1"]), net("N2", &["R1.2", "R2.2", "R1.3"])], footprints: vec![r_a()], ..Default::default() };
    let mut sym = symbol("R1", "Device:R", "10k", 1, p(Nm::ZERO, Nm::ZERO));
    sym.footprint = "Test:R_A".into();
    let mut sym2 = symbol("R2", "Device:R", "10k", 1, p(mm(10.0), Nm::ZERO));
    sym2.footprint = "Test:R_A".into();
    let mut edit = FootprintEdit::new("R1");
    let mut moved = FieldLayout::new(Point { x: Nm::ZERO, y: mm(-3.0) }, "F.SilkS");
    moved.size = (mm(0.7), mm(0.7));
    edit.reference = Some(moved);
    edit.fields.push(UserField { name: "Vendor".into(), text: "ACME".into(), layout: FieldLayout::new(Point { x: Nm::ZERO, y: mm(3.0) }, "F.Fab") });
    edit.attrs = Some(FootprintAttrs { kind: FootprintKind::ThroughHole, dnp: true, ..Default::default() });
    let mut pad_edit = PadEdit::none("1", 1);
    pad_edit.size = Some((mm(1.2), mm(1.2)));
    edit.set_pad(pad_edit);
    let design = Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(section(vec![sym, sym2], vec![])),
        nets: Some(vec![net("N1", &["R1.1", "R2.1"]), net("N2", &["R1.2", "R2.2", "R1.3"])]),
        placement: Some(PlacementSection { outline: vec![p(Nm::ZERO, Nm::ZERO), p(mm(100.0), Nm::ZERO), p(mm(100.0), mm(100.0)), p(Nm::ZERO, mm(100.0))], footprints: vec![pose("R1", mm(10.0), 90_000, Side::Top), pose("R2", mm(30.0), 0, Side::Top)], modules: vec![] }),
        routing: None,
        drawings: Some(DrawingsSection {
            footprint_edits: vec![edit],
            zone_overrides: vec![FootprintZoneOverrides {
                id: "R1".into(),
                footprint: Some(FootprintZoneFacts { zone_connection: Some(PadConnection::Full), clearance: Some(mm(0.25)) }),
                pads: vec![PadZoneOverride { number: "1".into(), clearance: Some(mm(0.3)), ..Default::default() }],
            }],
            footprint_extras: vec![FootprintExtra { id: "R1".into(), solder_mask_margin: Some(mm(0.04)), clearance: Some(mm(0.25)), ..Default::default() }],
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
    Cmd::ExchangeFootprints { references: references.iter().map(|s| s.to_string()).collect(), footprint: footprint.map(str::to_string), options, library: library.iter().map(|f| (f.name.clone(), f.clone())).collect(), library_items: Default::default() }
}

fn pose_of(d: &Design, id: &str) -> FootprintInstance {
    d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == id).cloned().unwrap()
}

#[test]
fn change_footprint_puts_the_part_on_the_new_one_and_keeps_its_pose_nets_lock_and_group() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    let d = b.design();
    let dr = d.drawings.as_ref().unwrap();
    let a = d.footprint_assignment("R1").expect("the part is assigned its footprint");
    assert_eq!(a.footprint, "Test:R_B");
    assert_eq!(a.definition.as_ref().map(|f| f.pads.len()), Some(3), "the board keeps its copy: the model does not know Test:R_B");
    assert!(d.footprint_assignment("R2").is_none(), "the other part is on its footprint still");
    // the pose: same side and label; the pads +-1000 against +-1500 have no shift that matches more than half of them, and the mean is nothing
    let pose = pose_of(d, "R1");
    assert_eq!((pose.at, pose.rot, pose.side), (p(mm(10.0), mm(20.0)), 90_000, Side::Top));
    // UI-LEFT P21: the pads are matched as `ExchangeFootprint` matches them (`matchItemsBySimilarity<PAD>`): pads 1 and 2 keep their nets, and the third pad,
    // which no old pad matches, has none (`SetNetCode( NETINFO_LIST::UNCONNECTED )`) until Update PCB from Schematic gives it its pin's
    assert_eq!(d.nets.as_ref().unwrap()[0].pins, ["R1.1", "R2.1"]);
    assert_eq!(d.nets.as_ref().unwrap()[1].pins, ["R1.2", "R2.2"]);
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
    let keep_everything = ExchangeOptions { match_pad_positions: true, remove_extra_texts: false, reset_text_layers: false, reset_text_effects: false, reset_text_positions: false, reset_text_content: false, reset_fabrication_attrs: false, reset_clearance_overrides: false, reset_3d_models: false, pads_by_number: false };
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_B"), keep_everything, &[r_b()])).unwrap();
    let d = b.design();
    let edit = d.footprint_edit("R1").unwrap();
    // the Reference stays where it was on the board, at its size
    let reference = edit.reference.as_ref().expect("the old Reference layout is kept");
    assert_eq!((reference.size, reference.layer.as_str()), ((mm(0.7), mm(0.7)), "F.SilkS"));
    assert_eq!(reference.board_position(&pose_of(d, "R1")), FieldLayout::new(Point { x: Nm::ZERO, y: mm(-3.0) }, "F.SilkS").board_position(&pose_of(&fixture().1, "R1")));
    assert!(edit.attrs.is_some_and(|a| a.dnp && a.kind == FootprintKind::ThroughHole), "the attributes as they were");
    // the footprint-level overrides stay, the pads' do not
    let dr = d.drawings.as_ref().unwrap();
    assert_eq!(dr.zone_overrides.len(), 1);
    assert!(dr.zone_overrides[0].pads.is_empty() && dr.zone_overrides[0].footprint.is_some_and(|f| f.clearance == Some(mm(0.25))));
    assert_eq!((dr.footprint_extras.len(), dr.footprint_extras[0].solder_mask_margin, dr.footprint_extras[0].clearance), (1, Some(mm(0.04)), Some(mm(0.25))));
    // the 3D model is the old footprint's
    assert_eq!(d.footprint_assignment("R1").unwrap().definition.as_ref().unwrap().model.as_deref(), Some("a.step"));
}

#[test]
fn remove_text_items_drops_what_the_new_footprint_has_no_match_for() {
    let (mut model, design) = fixture();
    // an extra text on the old footprint, which the model's footprint carries
    model.footprints[0].art.texts.push(FpText::new("keep me?", Point { x: mm(4.0), y: Nm::ZERO }, "F.SilkS"));
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    let kept: Vec<String> = b.design().footprint_assignment("R1").unwrap().definition.as_ref().unwrap().art.texts.iter().map(|t| t.text.clone()).collect();
    assert!(kept.contains(&"keep me?".to_string()), "kept when text items are not removed: {kept:?}");
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions { remove_extra_texts: true, ..ExchangeOptions::change() }, &[r_b()])).unwrap();
    let a = b.design().footprint_assignment("R1").unwrap();
    assert!(a.definition.as_ref().unwrap().art.texts.iter().all(|t| t.text != "keep me?"), "removed");
    assert!(b.design().footprint_edit("R1").is_none_or(|e| e.fields.is_empty()), "and the user field with it");
}

#[test]
fn match_pad_positions_moves_the_footprint_so_its_pads_stay_where_they_were() {
    let (model, design) = fixture();
    // the new footprint has its origin on pad 1
    let shifted = Footprint { name: "Test:R_S".into(), pads: vec![pad("1", Nm::ZERO, Nm::ZERO), pad("2", mm(2.0), Nm::ZERO)], ..Default::default() };
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
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
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_S"), ExchangeOptions { match_pad_positions: false, ..ExchangeOptions::change() }, &[shifted])).unwrap();
    let kept = pose_of(b.design(), "R1");
    assert_eq!((kept.at, kept.rot, kept.side), (was.at, was.rot, was.side));
}

#[test]
fn a_footprint_no_library_has_refuses_the_whole_command() {
    let (model, design) = fixture();
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    let e = b.apply(&exchange(&["R1", "R2"], Some("Test:Nowhere"), ExchangeOptions::change(), &[])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
    assert!(e[0].hint.as_deref().unwrap_or("").contains("not found"));
    let e = b.apply(&exchange(&["R9"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_part");
    let e = b.apply(&exchange(&[], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_to_change");
    assert_eq!(serde_json::to_string(b.design()).unwrap(), serde_json::to_string(&design).unwrap(), "a refused command changes nothing");
    // a footprint the model knows (another part is on it) needs no copy in the command
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_A"), ExchangeOptions::change(), &[])).unwrap();
}

#[test]
fn update_from_library_puts_the_library_copy_on_the_footprint_the_part_is_on() {
    let (model, mut design) = fixture();
    // R1 holds a stale copy of Test:R_A (an older pad size)
    let mut stale = r_a();
    stale.pads[0].size = (mm(0.5), mm(0.5));
    design.drawings.as_mut().unwrap().footprint_assignments.push(FootprintAssignment { reference: "R1".into(), footprint: "Test:R_A".into(), definition: Some(stale) });
    let mut model_with_copy = model.clone();
    model_with_copy.instance_footprints.insert("R1".into(), design.footprint_assignment("R1").unwrap().definition.clone().unwrap());
    let mut library_copy = r_a();
    library_copy.pads[0].size = (mm(1.0), mm(1.0));
    let mut b = Board::new(design, &model_with_copy, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], None, ExchangeOptions::update(), &[library_copy.clone()])).unwrap();
    let a = b.design().footprint_assignment("R1").unwrap();
    assert_eq!(a.footprint, "Test:R_A");
    assert_eq!(a.definition.as_ref().map(|f| f.pads[0].size), Some((mm(1.0), mm(1.0))), "the board's copy is the library's now");
    // the symbol's field is not touched by an update
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols[0].footprint, "Test:R_A");
    // the update options keep what the board has of the texts, the attributes and the pose
    let edit = b.design().footprint_edit("R1").unwrap();
    assert!(edit.reference.is_some() && edit.attrs.is_some(), "Update keeps the Reference layout and the attributes unless asked");
    // a footprint with no library copy is not found, as KiCad says
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let e = b.apply(&exchange(&["R1"], None, ExchangeOptions::update(), &[])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
}

#[test]
fn a_footprint_that_exists_only_on_the_board_keeps_the_nets_of_the_pads_the_new_one_has() {
    let (mut model, mut design) = fixture();
    model.parts.push(part("H1", "Test:R_A", 3000));
    let dr = design.drawings.as_mut().unwrap();
    dr.board_parts.push(BoardPart { reference: "H1".into(), value: Some("R_A".into()), footprint: "Test:R_A".into(), definition: None, pad_nets: vec![("1".into(), "N1".into()), ("2".into(), "N2".into()), ("9".into(), "N9".into())] });
    design.placement.as_mut().unwrap().footprints.push(pose("H1", mm(50.0), 0, Side::Bottom));
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
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
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_B"), ExchangeOptions::change(), &[r_b()])).unwrap();
    b.apply(&Cmd::RenameSymbol { id: "R1".into(), new_id: "R7".into() }).unwrap();
    let d = b.design();
    assert!(d.footprint_assignment("R1").is_none());
    assert_eq!(d.footprint_assignment("R7").unwrap().footprint, "Test:R_B");
}

/// UI-LEFT P21: Change Footprint matches the pads by KiCad's rules -- by number, then by place and shape (`matchItemsBySimilarity<PAD>`): a pad the new
/// footprint numbers otherwise takes the net and the pin name of the old pad it matches; Update PCB from Schematic's exchange (`replace`) gives the nets by
/// number, as its netlist does.
#[test]
fn change_footprint_carries_a_renumbered_pads_net_and_the_netlists_exchange_goes_by_number() {
    let (model, design) = fixture();
    // R_A's pads where they were, the second numbered "B"
    let mut renumbered = r_a();
    renumbered.name = "Test:R_AB".into();
    renumbered.pads[1].number = "B".into();
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_AB"), ExchangeOptions::change(), &[renumbered.clone()])).unwrap();
    let nets = b.design().nets.clone().unwrap();
    assert_eq!(nets[1].pins, ["R1.B", "R2.2"], "pad B is on the old pad 2's net");
    assert!(nets.iter().all(|n| !n.pins.contains(&"R1.2".to_string()) && !n.pins.contains(&"R1.3".to_string())));
    let pad_b = b.design().footprint_edit("R1").and_then(|e| e.pad("B", 1)).cloned().expect("the renumbered pad's pin name and type");
    assert_eq!(pad_b.pin_function.as_deref(), Some(""), "the old pad's (none here)");
    // the netlist's exchange: by number
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_AB"), ExchangeOptions::replace(), &[renumbered])).unwrap();
    assert_eq!(b.design().nets.clone().unwrap()[1].pins, ["R1.2", "R2.2", "R1.3"], "untouched: the schematic's pins by number");
}

/// UI-LEFT P30: what an exchange keeps of the board file's record of the footprint (`ExchangeFootprint`: `SetUuid`, `SetSheetfile`, `SetSheetname`, `SetFilters`,
/// `SetStaticComponentClass` always; the clearance overrides, the 3D body and the fields by their options), and that the footprint's items are the new footprint's.
#[test]
fn an_exchange_keeps_what_kicad_sets_from_the_old_footprint_and_takes_the_new_ones_items() {
    use crate::fp_exchange::exchanged_record;
    use eda_model::embedded_files::{EmbeddedFile, EmbeddedFileType};
    use eda_model::fp_items::FootprintItems;
    use eda_model::pcb_items::TextBox;
    let file = |kind: EmbeddedFileType, name: &str| EmbeddedFile { name: name.into(), kind, data: String::new(), checksum: "0".into() };
    let old = FootprintExtra {
        id: "U1".into(),
        uuid: Some("u-1".into()),
        sheetname: Some("/".into()),
        sheetfile: Some("a.kicad_sch".into()),
        component_classes: vec!["LOGO".into()],
        solder_mask_margin: Some(mm(0.05)),
        clearance: Some(mm(0.2)),
        allow_soldermask_bridges: true,
        tokens: vec![
            "(descr \"the board's\")".into(),
            "(property \"Datasheet\" \"http://x\" (at 0 0 0) (layer \"F.Fab\") (hide yes))".into(),
            "(property \"Description\" \"A logo\" (at 0 0 0) (layer \"F.Fab\") (hide yes))".into(),
            "(property \"ki_fp_filters\" \"R_*\")".into(),
            "(solder_paste_margin -0.03)".into(),
        ],
        source_name: "Lib:Logo".into(),
        courtyard_items: vec!["(fp_rect ..)".into()],
        extruded_body: Some(Default::default()),
        embedded: eda_model::embedded_files::EmbeddedFiles { fonts: false, files: vec![file(EmbeddedFileType::Model, "a.step"), file(EmbeddedFileType::Datasheet, "a.pdf")] },
        items: FootprintItems { text_boxes: vec![TextBox::new("F.SilkS", Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.0), y: mm(1.0) }, mm(1.0), mm(0.1))], ..Default::default() },
        ..Default::default()
    };
    let library = FootprintItems { text_boxes: vec![TextBox::new("F.Fab", Point { x: mm(1.0), y: Nm::ZERO }, Point { x: mm(3.0), y: mm(1.0) }, mm(1.0), mm(0.1))], ..Default::default() };
    let lib = crate::fp_on_board::LibraryRecord::of_items(library.clone());
    // Update's defaults: the texts stay, the overrides and the models are the library's
    let r = exchanged_record(Some(&old), true, "U1", &ExchangeOptions::update(), Some(&lib), false, "Lib:Logo").record.unwrap();
    assert_eq!((r.uuid.as_deref(), r.sheetname.as_deref(), r.sheetfile.as_deref(), r.component_classes.as_slice()), (Some("u-1"), Some("/"), Some("a.kicad_sch"), &["LOGO".to_string()][..]));
    assert_eq!(r.items, library, "the library's items, the board's own gone");
    assert_eq!(r.tokens, old.tokens[1..4].to_vec(), "the Datasheet and Description fields and the filters; the description and the paste margin are the library's");
    assert_eq!((r.source_name.as_str(), r.library_frame), ("Lib:Logo", true));
    assert_eq!((r.solder_mask_margin, r.clearance, r.allow_soldermask_bridges, r.extruded_body.is_some(), r.embedded.files.len()), (None, None, false, false, 0));
    assert!(r.courtyard_items.is_empty(), "the courtyard is the new footprint's");
    // every option off: the overrides, the body and the model files stay too, and the paste margin with the file's form
    let off = ExchangeOptions { reset_clearance_overrides: false, reset_3d_models: false, ..ExchangeOptions::update() };
    let r = exchanged_record(Some(&old), true, "U1", &off, None, false, "Lib:Logo").record.unwrap();
    assert_eq!((r.solder_mask_margin, r.clearance, r.allow_soldermask_bridges, r.extruded_body.is_some()), (Some(mm(0.05)), Some(mm(0.2)), true, true));
    assert_eq!(r.embedded.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a.step"], "the model files, not the datasheet");
    assert_eq!(r.tokens.last().map(String::as_str), Some("(solder_paste_margin -0.03)"));
    assert!(r.items.is_empty());
    // "update/reset text content" on (Change's default): the file's fields go, and with them the record's file form -- the writer gives the symbol's fields
    let changed = exchanged_record(Some(&old), true, "U1", &ExchangeOptions { reset_clearance_overrides: false, ..ExchangeOptions::change() }, Some(&lib), true, "Lib:Other");
    let r = changed.record.unwrap();
    assert!(r.tokens.is_empty() && !r.holds_file_form(), "{r:?}");
    assert_eq!(r.uuid.as_deref(), Some("u-1"));
    assert_eq!((changed.paste_margin, changed.paste_ratio), (Some(mm(-0.03)), None), "a paste margin kept goes into the footprint's edit");
    assert!(r.items_are_for("anything"), "a record of no file form: its items are the footprint's whatever it is on");
    // a record whose file form no longer describes the footprint (the part was put on another before) keeps none of its tokens
    let r = exchanged_record(Some(&old), false, "U1", &ExchangeOptions::update(), None, false, "Lib:Logo").record.unwrap();
    assert!(r.tokens.is_empty() && !r.holds_file_form());
    // a footprint made here with nothing to hold has no record; with the library's items it has one, in the library's frame
    assert_eq!(exchanged_record(None, false, "U1", &ExchangeOptions::change(), None, true, "Lib:Other").record, None);
    let r = exchanged_record(None, false, "U1", &ExchangeOptions::change(), Some(&lib), true, "Lib:Other").record.unwrap();
    assert_eq!((r.items == library, r.library_frame, r.holds_file_form()), (true, true, false));
}

/// Update Footprint from Library of a part on a built-in package reads the library copy of the footprint the board names (`aFootprint->GetFPID()`: a `0603` resistor is
/// `Resistor_SMD:R_0603_1608Metric` on the board, which the server's command carries), and the part stays on that name; Update Footprint on Board of a built-in's copy
/// leaves it on that name too, not on a footprint called `0603` that no library has (it was refused: "library footprint '0603' not found").
#[test]
fn a_built_in_packages_update_reads_the_footprint_the_board_names() {
    let mut r9 = part("R9", "unused", 2);
    r9.footprint = None;
    r9.package = Some("0603".into());
    let model = ConstraintModel { parts: vec![r9.clone()], ..Default::default() };
    assert_eq!(eda_model::footprint::footprint_lib_id(&r9), "Resistor_SMD:R_0603_1608Metric");
    let mut design = fixture().1;
    design.placement.as_mut().unwrap().footprints = vec![pose("R9", mm(40.0), 0, Side::Top)];
    design.drawings = Some(DrawingsSection::default());
    let mut installed = model.footprint_of(&r9).expect("the built-in");
    installed.name = "Resistor_SMD:R_0603_1608Metric".into();
    installed.pads[0].size = (mm(1.1), mm(1.0));
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R9"], None, ExchangeOptions::update(), &[installed.clone()])).expect("the update takes the installed footprint");
    let a = b.design().footprint_assignment("R9").expect("the board keeps the library's copy");
    assert_eq!((a.footprint.as_str(), a.definition.as_ref().map(|d| d.pads[0].size)), ("Resistor_SMD:R_0603_1608Metric", Some((mm(1.1), mm(1.0)))));

    // a part whose own copy is named after the package (a copy made by an edit of its texts on the board) is updated from the same footprint
    let mut legacy = design.clone();
    legacy.drawings.as_mut().unwrap().set_footprint_assignment(FootprintAssignment { reference: "R9".into(), footprint: "0603".into(), definition: model.footprint_of(&r9) });
    let mut m2 = model.clone();
    eda_model::fp_assign::apply_footprint_assignments(&legacy, &mut m2);
    let mut b = Board::new(legacy, &m2, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R9"], None, ExchangeOptions::update(), &[installed])).expect("the copy named 0603 is updated from Resistor_SMD:R_0603_1608Metric");

    // Ctrl+E and Update Footprint on Board: the part is on the footprint the board names
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::FpEditor(crate::fp_editor::FpEditorCmd::OpenFromBoard { reference: "R9".into() })).unwrap();
    let key = eda_model::ir::BoardFootprintCopy::key_of("R9");
    b.apply(&Cmd::AddFootprintText { footprint: key.clone(), text: eda_model::ir::Text { id: String::new(), content: "NOTE".into(), at: Point { x: Nm::ZERO, y: mm(1.5) }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: Default::default(), mirror: false } }).unwrap();
    b.apply(&Cmd::FpEditor(crate::fp_editor::FpEditorCmd::SaveToBoard { footprint: key })).unwrap();
    assert_eq!(b.design().footprint_assignment("R9").map(|a| a.footprint.as_str()), Some("Resistor_SMD:R_0603_1608Metric"));
}

/// The Footprint Editor's open of a name the model knows another footprint by (Edit Footprint's "Open in library" of a built-in `0603` whose part is on the installed
/// `Resistor_SMD:R_0603_1608Metric` now): the entry is under the name asked for, so the editor finds what it opened.
#[test]
fn a_footprint_opened_by_its_normalized_name_is_the_editors_under_that_name() {
    let mut installed = r_a();
    installed.name = "Resistor_SMD:R_0603_1608Metric".into();
    let model = ConstraintModel { footprints: vec![installed], ..Default::default() };
    let mut b = Board::new(fixture().1, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::OpenFootprintForEdit { name: "0603".into() }).unwrap();
    let lib = b.design().footprint_library.as_ref().unwrap();
    assert!(lib.by_name("0603").is_some_and(|f| f.pads.len() == 2), "{:?}", lib.footprints.iter().map(|f| &f.name).collect::<Vec<_>>());
}

/// UI-LEFT-3 P3-22, `ExchangeFootprint`'s `matchItemsBySimilarity<PAD>` and `newPad->SetNetCode( oldPad->GetNetCode() )`: each new pad takes the net of the old pad it
/// matched, pad by pad. R1 changed to a footprint whose two pads are unnumbered, where R_A's 1 and 2 were: each is matched by its place and keeps that pad's net, the
/// two unnumbered pads on two nets each with its own (`PadMaskInfo::net`); changed back, pads 1 and 2 take them again and no pad keeps a net of its own.
#[test]
fn unnumbered_pads_take_the_net_of_the_pad_they_match_by_place() {
    let (model, design) = fixture();
    let mut unnumbered = r_a();
    unnumbered.name = "Test:R_U".into();
    unnumbered.pads[0].number = String::new();
    unnumbered.pads[1].number = String::new();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&exchange(&["R1"], Some("Test:R_U"), ExchangeOptions::change(), &[unnumbered])).unwrap();
    let own: Vec<Option<String>> = b.design().drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "R1").map(|e| e.pads.iter().map(|p| p.net.clone()).collect()).unwrap_or_default();
    assert_eq!(own, [Some("N1".to_string()), Some("N2".to_string())], "each pad its matched pad's net");
    let nets = b.design().nets.clone().unwrap();
    assert!(nets.iter().filter(|n| n.pins.contains(&"R1.".to_string())).count() == 2 && nets.iter().all(|n| !n.pins.iter().any(|p| p == "R1.1" || p == "R1.2")), "{nets:?}");
    // and back: pads 1 and 2 matched by place to the two unnumbered pads, each with its net; one net per number again
    b.apply(&exchange(&["R1"], Some("Test:R_A"), ExchangeOptions::change(), &[r_a()])).unwrap();
    let own: Vec<Option<String>> = b.design().drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "R1").map(|e| e.pads.iter().map(|p| p.net.clone()).collect()).unwrap_or_default();
    assert!(own.iter().all(Option::is_none), "{own:?}");
    let nets = b.design().nets.clone().unwrap();
    let net_of = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default();
    assert_eq!([net_of("R1.1"), net_of("R1.2")], ["N1", "N2"]);
}
