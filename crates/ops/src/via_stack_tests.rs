//! UI-LEFT-3 X3-1: KiCad 25ec2335's `PCB_VIA_STACK` -- placement, the dialog's edit, Regenerate, Remove, Create Stack from Selection, Move / Flip and the members'
//! upkeep (the behaviours of KiCad's `qa/tests/pcbnew/test_via_stack_saveload.cpp` that are about the board, not the file).

use super::*;
use crate::pcb_items::PcbItemCmd;
use crate::pcb_transform_tests::{board, model};
use crate::Cmd;
use eda_model::units::mm;
use eda_model::via_extra::ViaKind;

fn six() -> eda_model::ConstraintModel {
    model(&["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"])
}

fn p(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

fn stacked(start: &str, end: &str, at: Point) -> MicroviaStack {
    let mut s = MicroviaStack::new(at);
    s.start_layer = start.into();
    s.end_layer = end.into();
    s.via_size_nm = mm(0.25);
    s.via_drill_nm = mm(0.1);
    s.net = "GND".into();
    s
}

fn place(s: MicroviaStack) -> Cmd {
    Cmd::PcbItem(PcbItemCmd::SetViaStack { id: None, stack: s, current: Default::default(), allow_drc_violations: false, hidden_layers: vec![] })
}

fn stacks(b: &Board<'_>) -> Vec<MicroviaStack> {
    b.design().drawings.as_ref().map(|d| d.board_items.via_stacks.clone()).unwrap_or_default()
}

fn member_vias(b: &Board<'_>, s: &MicroviaStack) -> Vec<Via> {
    b.design().routing.as_ref().unwrap().vias.iter().filter(|v| s.member_ids.contains(&v.id)).cloned().collect()
}

fn member_tracks(b: &Board<'_>, s: &MicroviaStack) -> Vec<Track> {
    b.design().routing.as_ref().unwrap().tracks.iter().filter(|t| s.member_ids.contains(&t.id)).cloned().collect()
}

fn extra<'a>(b: &'a Board<'_>, id: &str) -> &'a ViaExtra {
    b.design().drawings.as_ref().unwrap().via_extras.iter().find(|x| x.id == id).unwrap()
}

/// `BuildMembers` of a stacked stack: one microvia per adjacent pair, coaxial, filled, and capped only where a hop reaches an outer layer.
#[test]
fn a_stacked_stack_is_one_filled_microvia_per_layer_pair() {
    let m = six();
    let mut b = board(&m);
    let mut s = stacked("F.Cu", "In2.Cu", p(70.0, 70.0));
    s.capped = true;
    b.apply(&place(s)).unwrap();
    let st = &stacks(&b)[0];
    assert!(st.id.starts_with("vsk_"));
    let mut vias = member_vias(&b, st);
    vias.sort_by(|a, b| a.from_layer.cmp(&b.from_layer));
    assert_eq!(vias.len(), 2);
    assert!(vias.iter().all(|v| v.at == p(70.0, 70.0) && v.diameter == mm(0.25) && v.drill == mm(0.1) && v.net == "GND"));
    let spans: Vec<(&str, &str)> = vias.iter().map(|v| (v.from_layer.as_str(), v.to_layer.as_str())).collect();
    assert_eq!(spans, [("F.Cu", "In1.Cu"), ("In1.Cu", "In2.Cu")]);
    for v in &vias {
        let e = extra(&b, &v.id);
        assert_eq!(e.kind, ViaKind::Micro);
        assert_eq!(e.props().filling, Some(true));
        assert_eq!(e.props().capping, Some(v.from_layer == "F.Cu"), "only the hop that reaches F.Cu has a face to cap");
    }
}

/// A staggered stack: each hop at its own place, a trace of the net's class width on each landing layer, filled only when asked; a hop past the drawn ones goes on
/// along the last step by the pitch (`hopPos`).
#[test]
fn a_staggered_stack_steps_by_its_hops_and_joins_them_with_traces() {
    let m = six();
    let mut b = board(&m);
    let mut s = stacked("In1.Cu", "In4.Cu", p(70.0, 70.0));
    s.style = ViaStackStyle::Staggered;
    s.pitch_nm = mm(0.5);
    s.filled = false;
    s.hops = Some(vec![p(70.0, 70.0), p(70.5, 70.0)]);
    b.apply(&place(s)).unwrap();
    let st = &stacks(&b)[0];
    let mut vias = member_vias(&b, st);
    vias.sort_by_key(|v| v.at);
    assert_eq!(vias.iter().map(|v| v.at).collect::<Vec<_>>(), [p(70.0, 70.0), p(70.5, 70.0), p(71.0, 70.0)]);
    assert!(vias.iter().all(|v| extra(&b, &v.id).props().filling == Some(false)));
    let mut traces = member_tracks(&b, st);
    traces.sort_by(|a, b| a.layer.cmp(&b.layer));
    assert_eq!(traces.iter().map(|t| (t.layer.as_str(), t.pts.clone())).collect::<Vec<_>>(), [("In2.Cu", vec![p(70.0, 70.0), p(70.5, 70.0)]), ("In3.Cu", vec![p(70.5, 70.0), p(71.0, 70.0)])]);
    assert!(traces.iter().all(|t| t.width == m.board.width_of("GND").max(m.board.track_width_min_nm) && t.net == "GND"));
}

/// The Microvia Stack dialog's OK: a new pitch re-spaces the hops and the members are reshaped in place (`reuseMembers`: their ids stay); a new span replaces them.
#[test]
fn the_dialog_respaces_the_hops_and_keeps_the_members_it_can() {
    let m = six();
    let mut b = board(&m);
    let mut s = stacked("In1.Cu", "In3.Cu", p(70.0, 70.0));
    s.style = ViaStackStyle::Staggered;
    s.pitch_nm = mm(0.5);
    s.hops = Some(vec![p(70.0, 70.0), p(70.0, 70.5)]);
    b.apply(&place(s)).unwrap();
    let st = stacks(&b)[0].clone();
    let edit = |s: &MicroviaStack| Cmd::PcbItem(PcbItemCmd::SetViaStack { id: Some(st.id.clone()), stack: s.clone(), current: Default::default(), allow_drc_violations: false, hidden_layers: vec![] });
    let mut wider = st.clone();
    wider.pitch_nm = mm(1.0);
    b.apply(&edit(&wider)).unwrap();
    let after = stacks(&b)[0].clone();
    assert_eq!(after.hops, Some(vec![p(70.0, 70.0), p(70.0, 71.0)]), "re-spaced along each hop's own direction");
    assert_eq!(after.member_ids, st.member_ids, "same hops, same layers: reshaped in place");
    assert!(member_vias(&b, &after).iter().any(|v| v.at == p(70.0, 71.0)));
    let mut deeper = after.clone();
    deeper.end_layer = "In4.Cu".into();
    b.apply(&edit(&deeper)).unwrap();
    let last = stacks(&b)[0].clone();
    assert_eq!(member_vias(&b, &last).len(), 3);
    assert_ne!(last.member_ids, after.member_ids);
    // the replaced members are gone from the board
    assert!(b.design().routing.as_ref().unwrap().vias.iter().all(|v| !after.member_ids.contains(&v.id) || last.member_ids.contains(&v.id)));
    // a span the board does not have is refused with the tool's message
    let mut bad = last.clone();
    bad.end_layer = "In9.Cu".into();
    assert_eq!(b.apply(&edit(&bad)).unwrap_err()[0].hint.as_deref().unwrap(), STACK_LAYERS_MISSING);
}

/// `Remove`: the stack and its members; a member deleted on its own leaves the stack, which goes with its last one.
#[test]
fn removing_a_stack_removes_its_members_and_a_stack_with_none_is_gone() {
    let m = six();
    let mut b = board(&m);
    b.apply(&place(stacked("F.Cu", "In2.Cu", p(70.0, 70.0)))).unwrap();
    let st = stacks(&b)[0].clone();
    let vias_before = b.design().routing.as_ref().unwrap().vias.len();
    b.apply(&Cmd::PcbItem(PcbItemCmd::DeleteViaStack { id: st.id.clone() })).unwrap();
    assert!(stacks(&b).is_empty());
    assert_eq!(b.design().routing.as_ref().unwrap().vias.len(), vias_before - 2);
    assert!(b.design().drawings.as_ref().unwrap().via_extras.iter().all(|x| !st.member_ids.contains(&x.id)));

    b.apply(&place(stacked("F.Cu", "In2.Cu", p(70.0, 70.0)))).unwrap();
    let st = stacks(&b)[0].clone();
    b.apply(&Cmd::DeleteVia { id: st.member_ids[0].clone() }).unwrap();
    assert_eq!(stacks(&b)[0].member_ids, st.member_ids[1..]);
    b.apply(&Cmd::DeleteVia { id: st.member_ids[1].clone() }).unwrap();
    assert!(stacks(&b).is_empty());
}

/// Move and Flip: the vias go with the stack as members, and its position and hops follow; a Flip turns the span over.
#[test]
fn a_stack_moves_and_flips_with_its_members() {
    let m = six();
    let mut b = board(&m);
    let mut s = stacked("F.Cu", "In2.Cu", p(70.0, 70.0));
    s.style = ViaStackStyle::Staggered;
    s.pitch_nm = mm(0.5);
    s.hops = Some(vec![p(70.0, 70.0), p(70.5, 70.0)]);
    b.apply(&place(s)).unwrap();
    let id = stacks(&b)[0].id.clone();
    b.apply(&Cmd::MoveItems { ids: vec![id.clone()], dx: mm(1.0), dy: mm(2.0) }).unwrap();
    let st = stacks(&b)[0].clone();
    assert_eq!((st.origin, st.hops.clone()), (p(71.0, 72.0), Some(vec![p(71.0, 72.0), p(71.5, 72.0)])));
    let mut at: Vec<Point> = member_vias(&b, &st).iter().map(|v| v.at).collect();
    at.sort();
    assert_eq!(at, [p(71.0, 72.0), p(71.5, 72.0)]);
    b.apply(&Cmd::FlipItems { ids: vec![id], pivot: p(71.0, 72.0), direction: crate::pcb_transform::FlipDirection::LeftRight }).unwrap();
    let st = stacks(&b)[0].clone();
    assert_eq!((st.start_layer.as_str(), st.end_layer.as_str()), ("B.Cu", "In3.Cu"));
    assert_eq!(st.hops, Some(vec![p(71.0, 72.0), p(70.5, 72.0)]));
    let mut spans: Vec<(String, String)> = member_vias(&b, &st).iter().map(|v| if v.from_layer == "B.Cu" || v.from_layer > v.to_layer && v.to_layer != "B.Cu" { (v.to_layer.clone(), v.from_layer.clone()) } else { (v.from_layer.clone(), v.to_layer.clone()) }).collect();
    spans.sort();
    assert_eq!(spans, [("In3.Cu".to_string(), "In4.Cu".to_string()), ("In4.Cu".to_string(), "B.Cu".to_string())]);
}

/// Create Stack from Selection (`CreateFromItems`): the settings read off the vias, only the traces that join two hops on a landing layer taken, and KiCad's refusals.
#[test]
fn a_selection_of_microvias_becomes_a_stack() {
    let m = six();
    let mut b = board(&m);
    // three loose microvias of a staggered column and their two traces, plus a stray trace
    let add_via = |b: &mut Board<'_>, at: Point, a: &str, z: &str| b.apply(&Cmd::AddVia { net: "GND".into(), x: at.x, y: at.y, drill: mm(0.1), diameter: mm(0.25), from_layer: a.into(), to_layer: z.into() }).unwrap();
    add_via(&mut b, p(80.0, 80.0), "F.Cu", "In1.Cu");
    add_via(&mut b, p(80.5, 80.0), "In1.Cu", "In2.Cu");
    add_via(&mut b, p(81.0, 80.0), "In2.Cu", "In3.Cu");
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "In1.Cu".into(), width: mm(0.1), pts: vec![p(80.0, 80.0), p(80.5, 80.0)] }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "In2.Cu".into(), width: mm(0.1), pts: vec![p(80.5, 80.0), p(81.0, 80.0)] }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "In2.Cu".into(), width: mm(0.1), pts: vec![p(81.0, 80.0), p(85.0, 80.0)] }).unwrap();
    let rt = b.design().routing.as_ref().unwrap().clone();
    let vias: Vec<String> = rt.vias.iter().filter(|v| v.at.x >= mm(80.0)).map(|v| v.id.clone()).collect();
    let tracks: Vec<String> = rt.tracks.iter().filter(|t| t.pts[0].x >= mm(80.0)).map(|t| t.id.clone()).collect();
    let make = |ids: Vec<String>| Cmd::PcbItem(PcbItemCmd::MakeViaStack { ids });
    // not microvias yet: refused
    assert_eq!(b.apply(&make(vias.clone())).unwrap_err()[0].hint.as_deref().unwrap(), MAKE_STACK_REFUSED);
    for v in &vias {
        b.drawings_mut().via_extras.push(ViaExtra { id: v.clone(), kind: ViaKind::Micro, tokens: vec!["(filling no)".into()] });
    }
    b.apply(&make(vias.iter().chain(&tracks).cloned().collect())).unwrap();
    let st = stacks(&b)[0].clone();
    assert_eq!((st.start_layer.as_str(), st.end_layer.as_str(), st.style, st.pitch_nm, st.filled), ("F.Cu", "In3.Cu", ViaStackStyle::Staggered, mm(0.5), false));
    assert_eq!((st.via_size_nm, st.via_drill_nm, st.net.as_str()), (mm(0.25), mm(0.1), "GND"));
    assert_eq!(st.member_ids.len(), 5, "three vias and the two traces that join hops; the stray one stays loose");
    // a via already in a stack is not taken again
    assert_eq!(b.apply(&make(vias[..2].to_vec())).unwrap_err()[0].hint.as_deref().unwrap(), MAKE_STACK_REFUSED);
}

/// `placeStack`: "Refuse placement over foreign copper, matching the plain via tool" -- unless DRC violations are allowed.
#[test]
fn a_stack_over_another_nets_copper_is_refused() {
    let m = six();
    let mut b = board(&m);
    // `trk_a` is GND on F.Cu from (10, 40) to (20, 40): a VCC stack on it is refused
    let mut s = stacked("F.Cu", "In1.Cu", p(15.0, 40.0));
    s.net = "VCC".into();
    assert_eq!(b.apply(&place(s.clone())).unwrap_err()[0].hint.as_deref().unwrap(), STACK_VIOLATES_DRC);
    b.apply(&Cmd::PcbItem(PcbItemCmd::SetViaStack { id: None, stack: s, current: Default::default(), allow_drc_violations: true, hidden_layers: vec![] })).unwrap();
    assert_eq!(stacks(&b).len(), 1);
}

/// "Use netclass values": the class's microvia sizes, the board's via size when the class has none.
#[test]
fn a_netclass_stack_takes_the_class_microvia_sizes() {
    let mut m = six();
    m.board.default_class = Some(eda_model::NetClass { name: "Default".into(), nets: vec![], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: Some(mm(0.3)), microvia_drill: Some(mm(0.15)), diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
    let mut b = board(&m);
    let mut s = stacked("F.Cu", "In1.Cu", p(70.0, 70.0));
    s.use_netclass = true;
    s.via_size_nm = Nm::ZERO;
    s.via_drill_nm = Nm::ZERO;
    b.apply(&place(s)).unwrap();
    let v = member_vias(&b, &stacks(&b)[0]);
    assert_eq!((v[0].diameter, v[0].drill), (mm(0.3), mm(0.15)));
}
