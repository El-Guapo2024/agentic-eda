//! Remove Unused Pads (`RemoveUnusedPads`): the through-hole pads and the vias a choice picks take `remove_unused_layers` and give it back.

use super::pcb_transform_tests::{board, model, p, part};
use super::*;
use eda_model::fp_edit::{FootprintEdit, PadEdit};
use eda_model::padstack::UnconnectedLayers;
use eda_model::via_extra::{ViaExtra, ViaKind};
use eda_model::{ConstraintModel, PadKind};

const FOUR: [&str; 4] = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];

/// Two parts with `LOPSIDED` whose pads 1 and 3 are plated through-hole and 2 surface mount.
fn through_hole_model(layers: &[&str]) -> ConstraintModel {
    let mut m = model(layers);
    for pad in m.footprints[0].pads.iter_mut().filter(|p| p.number != "2") {
        pad.kind = PadKind::ThroughHole;
        pad.drill = Some(Nm::from_raw_shim(300_000));
    }
    m.parts = vec![part("U1"), part("U2")];
    m
}

fn remove(selected: Option<&[&str]>, vias: bool, pads: bool, keep_outside: bool, remove: bool) -> Cmd {
    Cmd::RemoveUnusedPads { selected: selected.map(|s| s.iter().map(|x| x.to_string()).collect()), vias, pads, keep_outside, remove }
}

fn edit_of<'a>(b: &'a Board<'_>, id: &str) -> Option<&'a FootprintEdit> {
    b.design().footprint_edit(id)
}

/// The padstack mode of a pad as the board has it: the edit's, else the library's.
fn mode_of(b: &Board<'_>, reference: &str, number: &str) -> UnconnectedLayers {
    edit_of(b, reference).and_then(|e| e.pad(number, 1)).and_then(|e| e.stack.as_ref()).map(|s| s.unconnected).unwrap_or_default()
}

fn via_mode(b: &Board<'_>, id: &str) -> UnconnectedLayers {
    b.design().drawings.as_ref().and_then(|d| d.via_extra(id)).map(ViaExtra::unconnected_layers).unwrap_or_default()
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    let e = b.apply(&cmd).unwrap_err();
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

#[test]
fn remove_unused_layers_marks_every_through_hole_pad_and_via_and_restore_all_layers_clears_them() {
    let m = through_hole_model(&FOUR);
    let mut b = board(&m);
    b.apply(&remove(None, true, true, true, true)).unwrap();
    for r in ["U1", "U2"] {
        assert_eq!(mode_of(&b, r, "1"), UnconnectedLayers::RemoveExceptStartAndEnd, "{r}.1 keeps its outside layers");
        assert_eq!(mode_of(&b, r, "3"), UnconnectedLayers::RemoveExceptStartAndEnd);
        assert!(edit_of(&b, r).unwrap().pad("2", 1).is_none(), "{r}.2 is surface mount: nothing to remove");
    }
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::RemoveExceptStartAndEnd);
    let tokens = &b.design().drawings.as_ref().unwrap().via_extra("via_a").unwrap().tokens;
    assert_eq!(tokens, &["(remove_unused_layers yes)", "(keep_end_layers yes)", "(zone_layer_connections)"]);

    // The same again changes nothing, and is said so rather than left as an empty undo step.
    assert!(refusal(&mut b, remove(None, true, true, true, true)).starts_with("ops_nothing_to_change"));

    // Without "Keep outside layers" the pad removes every layer nothing connects.
    b.apply(&remove(None, true, true, false, true)).unwrap();
    assert_eq!(mode_of(&b, "U1", "1"), UnconnectedLayers::RemoveAll);
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::RemoveAll);

    // "Restore All Layers" takes every setting away, and the edits and extras with them.
    b.apply(&remove(None, true, true, true, false)).unwrap();
    assert!(edit_of(&b, "U1").is_none() && edit_of(&b, "U2").is_none(), "no edit is left that says nothing");
    assert!(b.design().drawings.as_ref().unwrap().via_extra("via_a").is_none());
    assert!(refusal(&mut b, remove(None, true, true, true, false)).starts_with("ops_nothing_to_change"));
}

#[test]
fn the_choice_of_pads_and_vias_is_the_dialogs_checkboxes() {
    let m = through_hole_model(&FOUR);
    let mut b = board(&m);
    b.apply(&remove(None, false, true, true, true)).unwrap();
    assert_eq!(mode_of(&b, "U1", "1"), UnconnectedLayers::RemoveExceptStartAndEnd);
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::KeepAll, "Vias is unchecked");
    b.apply(&remove(None, true, false, true, true)).unwrap();
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::RemoveExceptStartAndEnd);
    assert!(refusal(&mut b, remove(None, false, false, true, true)).starts_with("ops_nothing_to_change"), "neither kind checked");
}

#[test]
fn selected_only_takes_what_the_selection_names() {
    let m = through_hole_model(&FOUR);
    // A footprint stands for its pads.
    let mut b = board(&m);
    b.apply(&remove(Some(&["U1", "trk_a", "zone_a"]), true, true, true, true)).unwrap();
    assert_eq!(mode_of(&b, "U1", "3"), UnconnectedLayers::RemoveExceptStartAndEnd);
    assert!(edit_of(&b, "U2").is_none(), "U2 is not selected");
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::KeepAll, "the via is not selected");
    // A pad is itself.
    let mut b = board(&m);
    b.apply(&remove(Some(&["U2.3"]), true, true, false, true)).unwrap();
    assert_eq!(mode_of(&b, "U2", "3"), UnconnectedLayers::RemoveAll);
    assert!(edit_of(&b, "U2").unwrap().pad("1", 1).is_none() && edit_of(&b, "U1").is_none());
    // A via is itself; an empty selection picks nothing.
    let mut b = board(&m);
    b.apply(&remove(Some(&["via_a"]), true, true, true, true)).unwrap();
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::RemoveExceptStartAndEnd);
    assert!(edit_of(&b, "U1").is_none());
    assert!(refusal(&mut b, remove(Some(&[]), true, true, true, true)).starts_with("ops_nothing_to_change"));
    // The surface-mount pad of a selected footprint is no through-hole pad.
    assert!(refusal(&mut b, remove(Some(&["U1.2"]), false, true, true, true)).starts_with("ops_nothing_to_change"));
}

#[test]
fn a_through_via_has_layers_to_remove_only_on_a_board_of_more_than_two_copper_layers() {
    let m = through_hole_model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert!(refusal(&mut b, remove(None, true, false, true, true)).starts_with("ops_nothing_to_change"));
    // The pads have a setting all the same: the dialog does not look at the stack for them.
    b.apply(&remove(None, true, true, true, true)).unwrap();
    assert_eq!(mode_of(&b, "U1", "1"), UnconnectedLayers::RemoveExceptStartAndEnd);
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::KeepAll);
}

#[test]
fn the_layer_depth_is_kicads_difference_of_layer_ids() {
    use super::unused_pads::layer_depth;
    // `BOARD::LayerDepth`: ids F_Cu 0, B_Cu 2, In1_Cu 4 ..., put in order; a back layer that is then the end counts as `F_Cu + count - 1`.
    assert_eq!(layer_depth("F.Cu", "B.Cu", 2), Some(1));
    assert_eq!(layer_depth("B.Cu", "F.Cu", 2), Some(1));
    assert_eq!(layer_depth("F.Cu", "B.Cu", 4), Some(3));
    assert_eq!(layer_depth("F.Cu", "In1.Cu", 4), Some(4));
    assert_eq!(layer_depth("In1.Cu", "In2.Cu", 4), Some(2), "two inner layers: the ids are two apart");
    assert_eq!(layer_depth("In1.Cu", "B.Cu", 4), Some(2), "swapped, the back layer is the start and stays 2");
    assert_eq!(layer_depth("B.Cu", "In1.Cu", 4), Some(2));
    assert_eq!(layer_depth("F.Cu", "Edge.Cuts", 4), None, "not a copper layer");
}

#[test]
fn a_blind_or_buried_via_is_taken_by_the_layer_depth_between_its_ends() {
    let m = through_hole_model(&FOUR);
    let b = board(&m);
    let dr = b.design().drawings.clone().unwrap();
    let mut d = b.design().clone();
    {
        let rt = d.routing.as_mut().unwrap();
        rt.vias[0].to_layer = "In1.Cu".into();
        rt.vias.push(super::pcb_transform_tests::via("via_b", (Nm::from_raw_shim(30_000_000), Nm::from_raw_shim(50_000_000)), "In1.Cu", "In2.Cu"));
        rt.vias.push(super::pcb_transform_tests::via("via_c", (Nm::from_raw_shim(40_000_000), Nm::from_raw_shim(50_000_000)), "In1.Cu", "B.Cu"));
        rt.vias.push(super::pcb_transform_tests::via("via_d", (Nm::from_raw_shim(50_000_000), Nm::from_raw_shim(50_000_000)), "F.Cu", "B.Cu"));
        let mut dr = dr.clone();
        dr.via_extras = ["via_a", "via_b", "via_c", "via_d"].iter().map(|id| ViaExtra { id: (*id).into(), kind: ViaKind::Blind, tokens: vec![] }).collect();
        d.drawings = Some(dr);
    }
    let mut b = Board::new(d, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    b.apply(&remove(None, true, false, true, true)).unwrap();
    // The dialog takes every via that spans more than a depth of 1, and with KiCad's layer ids that is every pair of layers but the front and the back of a board of
    // two copper layers.
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::RemoveExceptStartAndEnd, "F.Cu to In1.Cu");
    assert_eq!(via_mode(&b, "via_b"), UnconnectedLayers::RemoveExceptStartAndEnd, "In1.Cu to In2.Cu: depth 2");
    assert_eq!(via_mode(&b, "via_c"), UnconnectedLayers::RemoveExceptStartAndEnd, "In1.Cu to B.Cu: depth 2");
    assert_eq!(via_mode(&b, "via_d"), UnconnectedLayers::RemoveExceptStartAndEnd, "F.Cu to B.Cu on four layers: depth 3");
    // The via is still a blind one.
    assert_eq!(b.design().drawings.as_ref().unwrap().via_extra("via_a").unwrap().kind, ViaKind::Blind);
    let _ = p(Nm::ZERO, Nm::ZERO);

    // On two copper layers the front-to-back via that is not a through one has a depth of 1: nothing between its ends.
    let m = through_hole_model(&["F.Cu", "B.Cu"]);
    let b = board(&m);
    let mut d = b.design().clone();
    {
        let mut dr = d.drawings.clone().unwrap();
        dr.via_extras = vec![ViaExtra { id: "via_a".into(), kind: ViaKind::Blind, tokens: vec![] }];
        d.drawings = Some(dr);
    }
    let mut b = Board::new(d, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert!(refusal(&mut b, remove(None, true, false, true, true)).starts_with("ops_nothing_to_change"));
    assert_eq!(via_mode(&b, "via_a"), UnconnectedLayers::KeepAll);
}

#[test]
fn a_pad_that_already_has_a_padstack_keeps_it_and_the_setting_goes_with_it() {
    use eda_model::padstack::{PadLayerShape, PadStack, PadStackMode};
    let m = through_hole_model(&FOUR);
    let mut b = board(&m);
    // A front/inner/back stack the board already had on U1.1.
    let stack = PadStack { mode: PadStackMode::FrontInnerBack, inner: Some(PadLayerShape::plain(eda_model::PadShape::Circle, (Nm::from_raw_shim(900_000), Nm::from_raw_shim(900_000)))), back: Some(PadLayerShape::plain(eda_model::PadShape::Rect, (Nm::from_raw_shim(1_400_000), Nm::from_raw_shim(1_400_000)))), ..Default::default() };
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: PadEdit { number: "1".into(), nth: 1, stack: Some(stack.clone()), ..Default::default() } }).unwrap();
    b.apply(&remove(Some(&["U1"]), false, true, false, true)).unwrap();
    let e = edit_of(&b, "U1").unwrap().pad("1", 1).unwrap().stack.clone().unwrap();
    assert_eq!(e.mode, PadStackMode::FrontInnerBack);
    assert_eq!((e.inner.clone(), e.back.clone()), (stack.inner.clone(), stack.back.clone()), "the shapes are kept");
    assert_eq!(e.unconnected, UnconnectedLayers::RemoveAll);
    // Restoring the layers leaves the stack, not the edit that made the pad's padstack.
    b.apply(&remove(Some(&["U1"]), false, true, true, false)).unwrap();
    let e = edit_of(&b, "U1").unwrap().pad("1", 1).unwrap().stack.clone().unwrap();
    assert_eq!((e.mode, e.unconnected), (PadStackMode::FrontInnerBack, UnconnectedLayers::KeepAll));
}

#[test]
fn the_verb_takes_the_json_the_studio_sends() {
    let c: Cmd = serde_json::from_str(r#"{"op":"remove_unused_pads","selected":null,"vias":true,"pads":true,"keep_outside":true,"remove":false}"#).unwrap();
    assert!(matches!(c, Cmd::RemoveUnusedPads { selected: None, vias: true, pads: true, keep_outside: true, remove: false }));
    let c: Cmd = serde_json::from_str(r#"{"op":"remove_unused_pads","selected":["U1","via_a"],"remove":true}"#).unwrap();
    assert!(matches!(c, Cmd::RemoveUnusedPads { selected: Some(ref s), vias: false, pads: false, keep_outside: false, remove: true } if s.len() == 2));
}
