//! A free pad deleted and turned over on its own ([`crate::free_pads`], "Allow free pads"): `EDIT_TOOL::DeleteItems`' `case PCB_PAD_T` and `PAD::Flip`.

use super::pcb_transform_tests::{design, ids, p, part};
use super::*;
use eda_model::fp_edit::{edited_footprint, FootprintEdit, PadEdit, RemovedPad};
use eda_model::ir::{ChamferCorners, FootprintExtra, FootprintGroup, PadMaskInfo};
use eda_model::padstack::{PadLayerShape, PadStack, PadStackMode};
use eda_model::units::mm;
use eda_model::{Footprint, Pad, PadKind, PadShape};

/// Four pads, three of them numbered 2 (a connector's shield, say).
fn shielded() -> Footprint {
    let pad = |n: &str, x: f64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (mm(x), Nm::ZERO), size: (mm(0.6), mm(0.6)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
    Footprint { name: "LOPSIDED".into(), pads: vec![pad("1", -3.0), pad("2", -1.0), pad("2", 1.0), pad("2", 3.0)], courtyard: Some((mm(4.0), mm(2.0))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
}

fn shielded_model() -> ConstraintModel {
    let mut m = super::pcb_transform_tests::model(&["F.Cu", "B.Cu"]);
    m.footprints = vec![shielded()];
    m.parts = vec![part("U1"), part("U2")];
    m
}

fn record(name: &str) -> PadMaskInfo {
    PadMaskInfo { pin_function: name.into(), layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()], ..Default::default() }
}

fn with_records(d: &mut Design) {
    let dr = d.drawings.as_mut().unwrap();
    dr.footprint_extras.push(FootprintExtra {
        id: "U1".into(),
        pads: vec![record("A"), record("B"), record("C"), record("D")],
        groups: vec![FootprintGroup { name: "g".into(), locked: false, members: vec!["pad:2#3".into(), "pad:2#2".into(), "pad:1#1".into()] }],
        ..Default::default()
    });
    let mut big = PadEdit::none("2", 3);
    big.size = Some((mm(1.0), mm(1.0)));
    let mut first = PadEdit::none("2", 1);
    first.solder_mask_margin = Some(mm(0.05));
    dr.footprint_edits.push(FootprintEdit { id: "U1".into(), pads: vec![first, big], ..Default::default() });
}

fn edit<'a>(b: &'a Board<'_>) -> &'a FootprintEdit {
    b.design().footprint_edit("U1").unwrap()
}
fn records(b: &Board<'_>) -> Vec<String> {
    b.design().drawings.as_ref().unwrap().footprint_extras[0].pads.iter().map(|p| p.pin_function.clone()).collect()
}

/// UI-LEFT-3 (Stage 28's left): Delete of a free pad takes it off its footprint; the board file's records, the later pads' edits and the footprint's groups follow,
/// so the k-th pad numbered 2 is still `U1.2#k` and its record is still its own.
#[test]
fn a_free_pad_deleted_leaves_its_footprint_and_the_pads_after_it_keep_their_edits_and_records() {
    let m = shielded_model();
    let mut d = design();
    with_records(&mut d);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let removed = |b: &Board<'_>| edit(b).removed_pads.iter().map(|r| (r.index, r.number.clone())).collect::<Vec<_>>();

    // the middle pad numbered 2 (`U1.2#2`, place 2)
    b.apply(&Cmd::RemoveBoardPad { pad: "U1.2#2".into() }).unwrap();
    assert_eq!(removed(&b), [(2, "2".to_string())]);
    assert_eq!(records(&b), ["A", "B", "D"], "the pad's record goes, the others keep theirs");
    assert_eq!(edit(&b).pad("2", 2).and_then(|e| e.size), Some((mm(1.0), mm(1.0))), "the third pad numbered 2 is the second now, with its edit");
    assert!(edit(&b).pad("2", 3).is_none());
    assert_eq!(edit(&b).pad("2", 1).and_then(|e| e.solder_mask_margin), Some(mm(0.05)), "a pad before it keeps its edit");
    let members = &b.design().drawings.as_ref().unwrap().footprint_extras[0].groups[0].members;
    assert_eq!(members, &["pad:2#2", "pad:1#1"], "the group loses the pad and names the later one by its new place");
    // the footprint as the board has it: three pads, the edited one where the last was
    let fp = edited_footprint(&shielded(), edit(&b));
    assert_eq!(fp.pads.iter().map(|p| p.at.0).collect::<Vec<_>>(), [mm(-3.0), mm(-1.0), mm(3.0)]);
    assert_eq!(fp.pads[2].size, (mm(1.0), mm(1.0)));

    // the first pad numbered 2 (place 1); then the one left (place 3 of the whole footprint)
    b.apply(&Cmd::RemoveBoardPad { pad: "U1.2".into() }).unwrap();
    assert_eq!(removed(&b), [(1, "2".to_string()), (2, "2".to_string())]);
    assert_eq!(records(&b), ["A", "D"]);
    assert_eq!(edit(&b).pad("2", 1).and_then(|e| e.size), Some((mm(1.0), mm(1.0))));
    b.apply(&Cmd::RemoveBoardPad { pad: "U1.2".into() }).unwrap();
    assert_eq!(removed(&b), [(1, "2".to_string()), (2, "2".to_string()), (3, "2".to_string())]);
    assert_eq!(records(&b), ["A"]);
    assert!(edit(&b).pads.is_empty(), "the edits went with their pads");
    assert_eq!(edited_footprint(&shielded(), edit(&b)).pads.len(), 1);
    // no pad numbered 2 is left; one that never was is refused
    assert!(b.apply(&Cmd::RemoveBoardPad { pad: "U1.2".into() }).is_err());
    assert!(b.apply(&Cmd::RemoveBoardPad { pad: "U9.1".into() }).is_err());
    // a removal whose place no longer holds its number (the footprint changed under it) leaves the pad
    let stale = FootprintEdit { id: "U1".into(), removed_pads: vec![RemovedPad { index: 0, number: "9".into() }], ..Default::default() };
    assert_eq!(edited_footprint(&shielded(), &stale).pads.len(), 4);
}

/// `PAD::Flip` of a free pad alone: about a lone pad's own position its place stays, its orientation relative to the footprint is negated, its offset and chamfered
/// corners mirror, its padstack's front and back trade shapes, its copper goes to the other side, and its record's layers turn over.
#[test]
fn a_free_pad_flipped_goes_to_the_other_side_alone() {
    let m = shielded_model();
    let mut d = design();
    with_records(&mut d);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // pad 1 a chamfered rectangle, turned 30 degrees, its copper offset, with a front/inner/back stack whose back is an oval
    let mut e = PadEdit::none("1", 1);
    e.shape = Some(PadShape::ChamferedRect);
    e.chamfer_corners = Some(ChamferCorners { top_left: true, bottom_left: true, ..Default::default() });
    e.rot = Some(30_000);
    e.offset = Some(p(mm(0.1), mm(0.2)));
    e.stack = Some(PadStack { mode: PadStackMode::FrontInnerBack, back: Some(PadLayerShape::plain(PadShape::Oval, (mm(0.8), mm(0.4)))), ..Default::default() });
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).unwrap();
    // U1 at (30, 20) mm: pad 1 at (27, 20) on the board
    b.apply(&Cmd::FlipItems { ids: ids(&["U1.1"]), pivot: p(mm(27.0), mm(20.0)), direction: FlipDirection::LeftRight }).unwrap();
    let pe = edit(&b).pad("1", 1).cloned().unwrap();
    assert_eq!(pe.at, Some(p(mm(-3.0), Nm::ZERO)), "about its own position the pad stays");
    assert_eq!(pe.rot, Some(330_000), "SetFPRelativeOrientation( -GetFPRelativeOrientation() )");
    assert_eq!(pe.opposite_side, Some(true), "the SMD pad's copper is on the other side");
    assert_eq!((pe.shape, pe.size), (Some(PadShape::Oval), Some((mm(0.8), mm(0.4)))), "the back's shape is the front's now");
    let back = pe.stack.as_ref().and_then(|s| s.back.clone()).unwrap();
    assert_eq!(back.shape, PadShape::ChamferedRect);
    assert_eq!(back.offset, Some((mm(-0.1), mm(0.2))), "the offset mirrored left-right");
    assert_eq!(back.chamfer_corners, ChamferCorners { top_right: true, bottom_right: true, ..Default::default() }, "the corners mirrored");
    let rec = &b.design().drawings.as_ref().unwrap().footprint_extras[0].pads[0];
    assert_eq!(rec.layers, ["B.Cu", "B.Paste", "B.Mask"], "the record's layers turned over");
    // turned over again: back on its footprint's side, as it was
    b.apply(&Cmd::FlipItems { ids: ids(&["U1.1"]), pivot: p(mm(27.0), mm(20.0)), direction: FlipDirection::LeftRight }).unwrap();
    let pe = edit(&b).pad("1", 1).cloned().unwrap();
    assert_eq!((pe.opposite_side, pe.shape, pe.rot), (Some(false), Some(PadShape::ChamferedRect), Some(30_000)));
    assert_eq!(pe.offset, Some(p(mm(0.1), mm(0.2))));
    // a pad of another footprint and the footprint itself: untouched
    assert!(b.design().footprint_edit("U2").is_none());
    assert_eq!(super::pcb_transform_tests::pose(&b, "U1").side, eda_model::ir::Side::Top);
}
