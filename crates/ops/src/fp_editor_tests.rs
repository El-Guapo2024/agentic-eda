//! The Footprint Editor's verbs of UI-LEFT F3, F4, F5 and F7 (`fp_editor.rs`, and the looks of `fp_item set_graphic` / `set_text`): a board footprint opened in the editor
//! and put back on that one instance, Footprint Properties' settings, Set Anchor's courtyard and kept forms.
use super::*;
use crate::fp_editor::FpEditorCmd;
use crate::sch_annotate_tests::{p, section, symbol};
use eda_model::footprint::{Pad, PadKind, PadShape};
use eda_model::fp_art::FpText;
use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind, PadEdit, UserField};
use eda_model::fp_settings::FootprintSettings;
use eda_model::ir::{BoardFootprintCopy, FootprintExtra, FootprintZoneFacts, FootprintZoneOverrides, PadConnection, PadZoneOverride, PlacementSection, Provenance};
use eda_model::units::mm;
use eda_model::{Net, Part, Pin, PinKind};

fn pad(number: &str, x: Nm, y: Nm) -> Pad {
    Pad { number: number.into(), at: (x, y), size: (mm(0.9), mm(0.9)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
}

/// `Test:R_A`: pads at +-1 mm, a `${REFERENCE}` on the fab layer and a silkscreen line.
fn r_a() -> Footprint {
    let mut art = eda_model::fp_art::FpArt::default();
    art.texts.push(FpText::new("${REFERENCE}", Point { x: Nm::ZERO, y: Nm::ZERO }, "F.Fab"));
    Footprint { name: "Test:R_A".into(), pads: vec![pad("1", mm(-1.0), Nm::ZERO), pad("2", mm(1.0), Nm::ZERO)], courtyard: Some((mm(1.7), mm(0.8))), art, ..Default::default() }
}

fn part(r: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: Some("10k".into()),
        package: None,
        footprint: Some("Test:R_A".into()),
        symbol: None,
        datasheet: None,
        pins: (1..=2).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_nm: None,
        edge: None,
    }
}

fn pose(id: &str, x: Nm, rot: u32, side: Side) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: p(x, mm(20.0)), rot, side, label: LabelSide::default() }
}

/// R1 (turned a quarter, with a pad edit, a user field, attributes and overrides) and R2 (on the bottom), both on `Test:R_A`.
fn fixture() -> (ConstraintModel, Design) {
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() };
    let model = ConstraintModel { parts: vec![part("R1"), part("R2")], nets: vec![net("N1", &["R1.1", "R2.1"]), net("N2", &["R1.2", "R2.2"])], footprints: vec![r_a()], ..Default::default() };
    let mut sym = symbol("R1", "Device:R", "10k", 1, p(Nm::ZERO, Nm::ZERO));
    sym.footprint = "Test:R_A".into();
    let mut sym2 = symbol("R2", "Device:R", "10k", 1, p(mm(10.0), Nm::ZERO));
    sym2.footprint = "Test:R_A".into();
    let mut edit = FootprintEdit::new("R1");
    edit.reference = Some(FieldLayout::new(Point { x: Nm::ZERO, y: mm(-3.0) }, "F.SilkS"));
    edit.fields.push(UserField { name: "Vendor".into(), text: "ACME".into(), layout: FieldLayout::new(Point { x: Nm::ZERO, y: mm(3.0) }, "F.Fab") });
    edit.attrs = Some(FootprintAttrs { kind: FootprintKind::Smd, exclude_from_pos_files: true, ..Default::default() });
    let mut pe = PadEdit::none("1", 1);
    pe.solder_mask_margin = Some(mm(0.02));
    edit.set_pad(pe);
    let design = Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(section(vec![sym, sym2], vec![])),
        nets: Some(vec![net("N1", &["R1.1", "R2.1"]), net("N2", &["R1.2", "R2.2"])]),
        placement: Some(PlacementSection { outline: vec![p(Nm::ZERO, Nm::ZERO), p(mm(100.0), Nm::ZERO), p(mm(100.0), mm(100.0)), p(Nm::ZERO, mm(100.0))], footprints: vec![pose("R1", mm(10.0), 90_000, Side::Top), pose("R2", mm(30.0), 0, Side::Bottom)], modules: vec![] }),
        routing: None,
        drawings: Some(DrawingsSection {
            footprint_edits: vec![edit],
            zone_overrides: vec![FootprintZoneOverrides { id: "R1".into(), footprint: Some(FootprintZoneFacts { zone_connection: Some(PadConnection::Full), clearance: Some(mm(0.25)) }), pads: vec![PadZoneOverride { number: "2".into(), clearance: Some(mm(0.3)), ..Default::default() }] }],
            footprint_extras: vec![FootprintExtra { id: "R1".into(), solder_mask_margin: Some(mm(0.04)), net_tie_pad_groups: vec!["1, 2".into()], ..Default::default() }],
            ..Default::default()
        }),
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    };
    (model, design)
}

fn fp_cmd(c: FpEditorCmd) -> Cmd {
    Cmd::FpEditor(c)
}

fn copy_of<'d>(d: &'d Design, reference: &str) -> &'d BoardFootprintCopy {
    d.footprint_library.as_ref().unwrap().board_copy(&BoardFootprintCopy::key_of(reference)).expect("the board copy")
}

#[test]
fn edit_footprint_copies_the_board_instance_with_its_fields_attributes_and_overrides() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    assert_eq!(fp_cmd(FpEditorCmd::OpenFromBoard { reference: "R1".into() }).domain(), Domain::FootprintEditor);
    b.apply(&fp_cmd(FpEditorCmd::OpenFromBoard { reference: "R1".into() })).unwrap();
    let d = b.design();
    let c = copy_of(d, "R1");
    assert_eq!((c.reference.as_str(), c.fpid.as_str(), c.footprint.name.as_str()), ("R1", "Test:R_A", "<board>/R1"));
    let fp = &c.footprint;
    assert_eq!(fp.pads.len(), 2);
    // the Reference shows the part's reference where the board has it; the Value the part's value
    let r = fp.reference.as_ref().unwrap();
    assert_eq!((r.text.as_deref(), r.at, r.layer.as_str()), (Some("R1"), Point { x: Nm::ZERO, y: mm(-3.0) }, "F.SilkS"));
    assert_eq!(fp.value.as_ref().unwrap().text.as_deref(), Some("10k"));
    assert_eq!(fp.fields.iter().map(|f| (f.name.as_str(), f.value.as_str())).collect::<Vec<_>>(), [("Vendor", "ACME")]);
    assert!(fp.attributes.smd && fp.attributes.exclude_from_position_files && !fp.attributes.through_hole);
    // the footprint's and the pads' overrides
    assert_eq!(fp.zone_connection, Some(PadConnection::Full));
    assert_eq!((fp.settings.clearance, fp.settings.solder_mask_margin), (Some(mm(0.25)), Some(mm(0.04))));
    assert_eq!(fp.settings.net_tie_pad_groups, vec!["1, 2".to_string()]);
    let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
    assert_eq!(p2.clearance_override, Some(mm(0.3)));
    assert_eq!(fp.pads.iter().find(|p| p.number == "1").unwrap().kicad.solder_mask_margin, Some(mm(0.02)));
    // the footprint's drawings
    assert!(fp.texts.iter().any(|t| t.content == "${REFERENCE}"));
    // nothing became a library entry
    assert!(d.footprint_library.as_ref().unwrap().footprints.is_empty());
    // a bottom-side footprint's fields come to the front, as `LoadFootprintFromBoard`'s flip turns them
    b.apply(&fp_cmd(FpEditorCmd::OpenFromBoard { reference: "R2".into() })).unwrap();
    let lib = b.design().footprint_library.as_ref().unwrap();
    assert_eq!(lib.board_copies.len(), 1, "the editor holds one footprint");
    let r2 = copy_of(b.design(), "R2").footprint.reference.clone().unwrap();
    assert_eq!(r2.layer, "F.SilkS");
    assert!(!r2.mirror);
    assert!(b.apply(&fp_cmd(FpEditorCmd::OpenFromBoard { reference: "R9".into() })).is_err());
}

#[test]
fn update_footprint_on_board_changes_only_the_linked_instance() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&fp_cmd(FpEditorCmd::OpenFromBoard { reference: "R1".into() })).unwrap();
    let key = BoardFootprintCopy::key_of("R1");
    // edit the copy with the editor's own verbs: a third pad, a bigger value text, the attributes
    let mut third = b.design().footprint_library.as_ref().unwrap().by_name(&key).unwrap().pads[0].clone();
    third.number = "3".into();
    third.at = Point { x: Nm::ZERO, y: mm(1.5) };
    b.apply(&Cmd::AddPad { footprint: key.clone(), pad: third }).unwrap();
    b.apply(&fp_cmd(FpEditorCmd::SetSettings { footprint: key.clone(), settings: FootprintSettings { clearance: Some(mm(0.15)), ..Default::default() } })).unwrap();
    let save = fp_cmd(FpEditorCmd::SaveToBoard { footprint: key.clone() });
    assert_eq!(save.domain(), Domain::Pcb);
    assert!(save.spans_both_editors());
    b.apply(&save).unwrap();
    let d = b.design();
    let a = d.footprint_assignment("R1").expect("R1 takes the edited footprint");
    assert_eq!(a.footprint, "Test:R_A");
    assert_eq!(a.definition.as_ref().map(|f| f.pads.len()), Some(3));
    assert!(d.footprint_assignment("R2").is_none(), "R2, on the same footprint, is left as it was");
    // the pads did not move, so neither did the footprint
    let pose = d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "R1").unwrap();
    assert_eq!((pose.at, pose.rot), (p(mm(10.0), mm(20.0)), 90_000));
    // what the copy carried beyond the pads is the instance's: the Reference layout (its text is the part's), the user field, the attributes, the overrides
    let edit = d.footprint_edit("R1").unwrap();
    let r = edit.reference.as_ref().unwrap();
    assert_eq!((r.at, r.text.as_deref()), (Point { x: Nm::ZERO, y: mm(-3.0) }, None));
    assert_eq!(edit.fields.len(), 1);
    assert_eq!(edit.attrs.map(|a| a.exclude_from_pos_files), Some(true));
    assert_eq!(edit.pad("1", 1).and_then(|p| p.solder_mask_margin), Some(mm(0.02)));
    let dr = d.drawings.as_ref().unwrap();
    let z = dr.zone_overrides.iter().find(|z| z.id == "R1").unwrap();
    assert_eq!(z.footprint, Some(FootprintZoneFacts { zone_connection: Some(PadConnection::Full), clearance: Some(mm(0.15)) }));
    assert_eq!(z.pads.iter().find(|p| p.number == "2").and_then(|p| p.clearance), Some(mm(0.3)));
    assert!(dr.footprint_extras.iter().all(|e| e.id != "R1"), "the settings were replaced whole: no mask margin and no net ties now");
    // the library has no entry, the copy is still the editor's
    assert!(d.footprint_library.as_ref().unwrap().footprints.is_empty());
    assert!(d.footprint_library.as_ref().unwrap().board_copy(&key).is_some());
}

#[test]
fn update_footprint_on_board_matches_pad_positions_and_refuses_a_footprint_gone_from_the_board() {
    let (model, design) = fixture();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&fp_cmd(FpEditorCmd::OpenFromBoard { reference: "R2".into() })).unwrap();
    let key = BoardFootprintCopy::key_of("R2");
    // the anchor moves 1 mm to the right: every pad is 1 mm further left in the footprint's frame, so the footprint moves for its pads to stay put
    b.apply(&Cmd::SetFootprintAnchor { name: key.clone(), at: Point { x: mm(1.0), y: Nm::ZERO } }).unwrap();
    b.apply(&fp_cmd(FpEditorCmd::SaveToBoard { footprint: key.clone() })).unwrap();
    let pose = b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "R2").cloned().unwrap();
    // R2 is on the bottom: the frame is mirrored, so the footprint goes 1 mm to the board's left
    assert_eq!((pose.at, pose.side), (p(mm(29.0), mm(20.0)), Side::Bottom));
    // the instance is deleted from the board: "Unable to find the footprint on the main board.\nCannot save."
    let mut gone = b.design().clone();
    gone.placement.as_mut().unwrap().footprints.retain(|f| f.id != "R2");
    let mut b2 = Board::new(gone, &model, mm(0.1), mm(0.3));
    let err = b2.apply(&fp_cmd(FpEditorCmd::SaveToBoard { footprint: key })).unwrap_err();
    assert_eq!(err[0].hint.as_deref(), Some("Unable to find the footprint on the main board.\nCannot save."));
}

#[test]
fn footprint_properties_settings_are_checked_and_replace_an_older_designs_kept_forms() {
    let m = ConstraintModel::default();
    let mut b = Board::new(crate::tests::empty_design(), &m, mm(0.1), mm(0.3));
    let mut fp = eda_model::ir::LibraryFootprint::new_empty("Lib:Two");
    for (n, x) in [("1", -1.0), ("2", 1.0)] {
        let mut lp = eda_model::ir::LibraryPad::from_engine_pad(&pad(n, mm(x), Nm::ZERO));
        lp.id = String::new();
        fp.pads.push(lp);
    }
    // an older design kept the footprint's clearance and net ties as the file's forms
    fp.kicad.tokens = vec!["(clearance 0.2)".into(), "(net_tie_pad_groups \"1,2\")".into(), "(stackup (layer \"F.Cu\"))".into()];
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    let set = |s: FootprintSettings| fp_cmd(FpEditorCmd::SetSettings { footprint: "Lib:Two".into(), settings: s });
    let err = b.apply(&set(FootprintSettings { jumper_pad_groups: vec![vec!["1".into(), "7".into()]], ..Default::default() })).unwrap_err();
    assert_eq!(err[0].hint.as_deref(), Some("Pad '7' in jumper pad group 1 does not exist in this footprint."));
    assert!(b.apply(&set(FootprintSettings { clearance: Some(mm(-0.1)), ..Default::default() })).is_err());
    assert!(b.apply(&set(FootprintSettings { private_layers: vec!["F.SilkS".into()], ..Default::default() })).is_err(), "only a user layer can be private");
    b.apply(&set(FootprintSettings {
        solder_paste_margin_ratio: Some(-0.05),
        private_layers: vec!["User.1".into()],
        net_tie_pad_groups: vec!["1, 2".into(), String::new()],
        jumper_pad_groups: vec![vec!["2".into(), "1".into()]],
        ..Default::default()
    }))
    .unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Lib:Two").unwrap();
    assert_eq!(fp.kicad.tokens, vec!["(stackup (layer \"F.Cu\"))".to_string()], "the older forms of the settings are gone, the rest stays");
    assert_eq!(fp.settings.clearance, None);
    assert_eq!(fp.settings.net_tie_pad_groups, vec!["1, 2".to_string()], "an empty row is no group");
    assert_eq!(fp.settings.jumper_pad_groups, vec![vec!["1".to_string(), "2".to_string()]]);
    assert_eq!(fp.settings.private_layers, vec!["User.1".to_string()]);
}

#[test]
fn set_anchor_moves_the_courtyard_and_the_items_kept_as_text_but_not_a_point() {
    let m = ConstraintModel::default();
    let mut b = Board::new(crate::tests::empty_design(), &m, mm(0.1), mm(0.3));
    let mut fp = eda_model::ir::LibraryFootprint::new_empty("Lib:Kept");
    fp.courtyard = Some((mm(2.0), mm(1.0)));
    fp.kicad.tokens = vec![
        "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (locked yes) (hatch edge 0.5) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))".into(),
        "(point (at 1 1) (size 1) (layer \"F.Fab\"))".into(),
        "(clearance 0.1)".into(),
    ];
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    b.apply(&Cmd::SetFootprintAnchor { name: "Lib:Kept".into(), at: Point { x: mm(1.0), y: mm(-0.5) } }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Lib:Kept").unwrap();
    // no courtyard graphics: the smallest box about the origin that holds the old one moved by (-1, +0.5)
    assert_eq!(fp.courtyard, Some((mm(3.0), mm(1.5))));
    assert_eq!(fp.kicad.tokens[0], "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (locked yes) (hatch edge 0.5) (polygon (pts (xy -1 0.5) (xy 0 0.5) (xy 0 1.5))))");
    assert_eq!(fp.kicad.tokens[1], "(point (at 1 1) (size 1) (layer \"F.Fab\"))", "`MoveAnchorPosition` leaves the points");
    assert_eq!(fp.kicad.tokens[2], "(clearance 0.1)");
    // with courtyard graphics the box is theirs, moved
    let mut fp2 = eda_model::ir::LibraryFootprint::new_empty("Lib:Crt");
    fp2.courtyard = Some((mm(2.0), mm(1.0)));
    fp2.graphics.push(Shape::Rect { id: String::new(), layer: "F.CrtYd".into(), stroke_width: mm(0.05), filled: false, start: Point { x: mm(-2.0), y: mm(-1.0) }, end: Point { x: mm(2.0), y: mm(1.0) } });
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp2, overwrite: false }).unwrap();
    b.apply(&Cmd::SetFootprintAnchor { name: "Lib:Crt".into(), at: Point { x: mm(2.0), y: Nm::ZERO } }).unwrap();
    assert_eq!(b.design().footprint_library.as_ref().unwrap().by_name("Lib:Crt").unwrap().courtyard, Some((mm(4.0), mm(1.0))));
}

#[test]
fn shape_and_text_dialogs_set_the_looks_kept_by_id() {
    use crate::fp_items::{FpItemCmd, ShapeLook, TextLook};
    let m = ConstraintModel::default();
    let mut b = Board::new(crate::tests::empty_design(), &m, mm(0.1), mm(0.3));
    let mut fp = eda_model::ir::LibraryFootprint::new_empty("Lib:Looks");
    fp.graphics.push(Shape::Rect { id: "g".into(), layer: "F.SilkS".into(), stroke_width: mm(0.1), filled: false, start: Point { x: mm(-1.0), y: mm(-1.0) }, end: Point { x: mm(1.0), y: mm(1.0) } });
    fp.texts.push(Text { id: "t".into(), content: "x".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: Default::default(), mirror: false });
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    let lib = |b: &Board| b.design().footprint_library.as_ref().unwrap().by_name("Lib:Looks").unwrap().clone();
    let g = lib(&b).graphics[0].clone();
    let t = lib(&b).texts[0].clone();
    b.apply(&Cmd::FpItem(FpItemCmd::SetGraphic { footprint: "Lib:Looks".into(), id: g.id().into(), shape: g.clone(), look: Some(ShapeLook { line_style: "dash".into(), hatch: Some(eda_model::fp_art::Hatch::Hatch) }) })).unwrap();
    assert!(b.apply(&Cmd::FpItem(FpItemCmd::SetGraphic { footprint: "Lib:Looks".into(), id: g.id().into(), shape: g.clone(), look: Some(ShapeLook { line_style: "wavy".into(), hatch: None }) })).is_err());
    b.apply(&Cmd::FpItem(FpItemCmd::SetText { footprint: "Lib:Looks".into(), id: t.id.clone(), text: t.clone(), look: Some(TextLook { bold: true, italic: true, valign: 1, width_nm: Some(mm(1.5)), knockout: false, keep_upright: false }) })).unwrap();
    let fp = lib(&b);
    assert_eq!(fp.kicad.stroke_types.get(g.id()).map(String::as_str), Some("dash"));
    assert_eq!(fp.kicad.hatches.get(g.id()), Some(&eda_model::fp_art::Hatch::Hatch));
    let e = fp.kicad.text_extra(&t.id).unwrap();
    assert!(e.bold && e.italic && e.valign == eda_model::ir::TextVAlign::Bottom && e.width_nm == Some(mm(1.5)));
    assert!(fp.kicad.unlocked_texts.contains(&t.id));
    // without a look the dialog's edit keeps them
    b.apply(&Cmd::FpItem(FpItemCmd::SetText { footprint: "Lib:Looks".into(), id: t.id.clone(), text: Text { content: "y".into(), ..t.clone() }, look: None })).unwrap();
    assert!(lib(&b).kicad.text_extra(&t.id).unwrap().bold);
    // solid and no hatch take them away
    b.apply(&Cmd::FpItem(FpItemCmd::SetGraphic { footprint: "Lib:Looks".into(), id: g.id().into(), shape: g.clone(), look: Some(ShapeLook { line_style: "solid".into(), hatch: None }) })).unwrap();
    let fp = lib(&b);
    assert!(!fp.kicad.stroke_types.contains_key(g.id()) && !fp.kicad.hatches.contains_key(g.id()));
}
