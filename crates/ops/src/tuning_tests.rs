//! `Cmd::CommitTuning`: a tuning pattern placed, re-tuned and removed in one undo step each, and kept honest when its tracks go.

use super::pcb_transform_tests::{design, drawings, ids, model, p, routing, track};
use super::*;
use eda_model::tuning::{TargetRange, TuningMode, TuningPattern};
use eda_model::units::mm;

fn pattern() -> TuningPattern {
    TuningPattern {
        id: String::new(),
        mode: TuningMode::Single,
        layer: "F.Cu".into(),
        origin: p(mm(1.0), mm(2.0)),
        end: p(mm(9.0), mm(2.0)),
        initial_side: Default::default(),
        min_amplitude: mm(0.2),
        max_amplitude: mm(1.0),
        spacing: mm(0.6),
        corner_radius_percent: 80,
        rounded: true,
        single_sided: false,
        override_custom_rules: false,
        target_length: TargetRange::around(mm(12.0)),
        target_skew: TargetRange { min: -100_000, opt: 0, max: 100_000 },
        is_time_domain: false,
        target_delay: TargetRange::delay_unconstrained(),
        target_skew_delay: TargetRange { min: -100_000_000, opt: 0, max: 100_000_000 },
        base_line: vec![p(mm(1.0), mm(2.0)), p(mm(9.0), mm(2.0))],
        base_line_coupled: vec![],
        last_track_width: mm(0.2),
        last_diff_pair_gap: Nm::ZERO,
        last_tuning_length: mm(12000.0),
            last_tuning_delay: 0,
        last_netname: "GND".into(),
        last_status: "tuned".into(),
        member_ids: vec![],
    }
}

fn tuned(cmd_tracks: Vec<Track>, members: Vec<usize>, pattern: Option<TuningPattern>, remove_pattern: Option<String>, remove_track_ids: Vec<String>) -> Cmd {
    Cmd::CommitTuning { pattern, remove_pattern, remove_track_ids, tracks: cmd_tracks, member_indices: members }
}

fn meander() -> Vec<Track> {
    vec![
        track("", "F.Cu", &[(Nm::ZERO, mm(2.0)), (mm(1.0), mm(2.0))]),
        track("", "F.Cu", &[(mm(1.0), mm(2.0)), (mm(1.0), mm(3.0)), (mm(3.0), mm(3.0)), (mm(3.0), mm(2.0))]),
        track("", "F.Cu", &[(mm(3.0), mm(2.0)), (mm(9.0), mm(2.0))]),
    ]
}

#[test]
fn a_pattern_is_added_with_the_tracks_of_its_meander_as_members() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let before = routing(&b).tracks.len();
    b.apply(&tuned(meander(), vec![1], Some(pattern()), None, vec![])).unwrap();
    assert_eq!(routing(&b).tracks.len(), before + 3);
    let patterns = &drawings(&b).tuning_patterns;
    assert_eq!(patterns.len(), 1);
    assert!(patterns[0].id.starts_with("tun_"), "{}", patterns[0].id);
    let member = routing(&b).tracks.iter().find(|t| t.pts.len() == 4).expect("the meander track");
    assert_eq!(patterns[0].member_ids, vec![member.id.clone()]);
}

#[test]
fn re_tuning_replaces_the_members_and_keeps_the_patterns_id() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&tuned(meander(), vec![1], Some(pattern()), None, vec![])).unwrap();
    let old = drawings(&b).tuning_patterns[0].clone();
    // the pattern comes back with a taller meander
    let taller = vec![
        track("", "F.Cu", &[(Nm::ZERO, mm(2.0)), (mm(1.0), mm(2.0))]),
        track("", "F.Cu", &[(mm(1.0), mm(2.0)), (mm(1.0), mm(4.0)), (mm(3.0), mm(4.0)), (mm(3.0), mm(2.0))]),
        track("", "F.Cu", &[(mm(3.0), mm(2.0)), (mm(9.0), mm(2.0))]),
    ];
    let mut next = pattern();
    next.id = old.id.clone();
    next.max_amplitude = mm(2.0);
    let remove: Vec<String> = routing(&b).tracks.iter().filter(|t| t.pts.first().map(|q| q.y) == Some(mm(2.0)) || t.pts.len() == 4).map(|t| t.id.clone()).collect();
    b.apply(&tuned(taller, vec![1], Some(next), Some(old.id.clone()), remove)).unwrap();
    let patterns = &drawings(&b).tuning_patterns;
    assert_eq!(patterns.len(), 1);
    assert_eq!((patterns[0].id.as_str(), patterns[0].max_amplitude), (old.id.as_str(), mm(2.0)));
    let member = routing(&b).tracks.iter().find(|t| t.pts.len() == 4).expect("the new meander");
    assert_eq!(member.pts[1], p(mm(1.0), mm(4.0)));
    assert_eq!(patterns[0].member_ids, vec![member.id.clone()]);
}

#[test]
fn removing_a_pattern_puts_the_baseline_back() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&tuned(meander(), vec![1], Some(pattern()), None, vec![])).unwrap();
    let id = drawings(&b).tuning_patterns[0].id.clone();
    let member = routing(&b).tracks.iter().find(|t| t.pts.len() == 4).unwrap().id.clone();
    b.apply(&tuned(vec![track("", "F.Cu", &[(mm(1.0), mm(2.0)), (mm(3.0), mm(2.0))])], vec![], None, Some(id), vec![member])).unwrap();
    assert!(drawings(&b).tuning_patterns.is_empty());
    assert!(routing(&b).tracks.iter().all(|t| t.pts.len() != 4));
    assert!(routing(&b).tracks.iter().any(|t| t.pts == vec![p(mm(1.0), mm(2.0)), p(mm(3.0), mm(2.0))]));
}

#[test]
fn a_pattern_without_a_member_is_refused_and_so_is_a_bad_track() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let e = b.apply(&tuned(meander(), vec![], Some(pattern()), None, vec![])).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_tuning");
    let e = b.apply(&tuned(meander(), vec![7], Some(pattern()), None, vec![])).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_tuning");
    let e = b.apply(&tuned(vec![track("", "In9.Cu", &[(Nm::ZERO, Nm::ZERO), (Nm::new(1), Nm::new(1))])], vec![0], Some(pattern()), None, vec![])).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_layer");
    let e = b.apply(&tuned(vec![], vec![], None, Some("tun_nope".into()), vec![])).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_tuning");
    assert!(drawings(&b).tuning_patterns.is_empty());
}

#[test]
fn a_member_deleted_by_another_verb_leaves_the_pattern_and_the_last_one_takes_it_along() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let two = vec![
        track("", "F.Cu", &[(Nm::ZERO, mm(2.0)), (mm(1.0), mm(2.0))]),
        track("", "F.Cu", &[(mm(1.0), mm(2.0)), (mm(1.0), mm(3.0)), (mm(3.0), mm(3.0)), (mm(3.0), mm(2.0))]),
        track("", "F.Cu", &[(mm(3.0), mm(2.0)), (mm(3.0), mm(5.0)), (mm(4.0), mm(5.0)), (mm(4.0), mm(2.0))]),
    ];
    b.apply(&tuned(two, vec![1, 2], Some(pattern()), None, vec![])).unwrap();
    let members = drawings(&b).tuning_patterns[0].member_ids.clone();
    assert_eq!(members.len(), 2);
    b.apply(&Cmd::DeleteTrack { id: members[0].clone() }).unwrap();
    assert_eq!(drawings(&b).tuning_patterns[0].member_ids, vec![members[1].clone()]);
    b.apply(&Cmd::DeleteTrack { id: members[1].clone() }).unwrap();
    assert!(drawings(&b).tuning_patterns.is_empty(), "no member left: no pattern");
    let _ = ids(&[]);
}

/// r2-pcb-edit "Mirror ... not yet": `PCB_TUNING_PATTERN::Mirror` (`baseMirror`): the base line mirrors, the origin and end are its ends, the initial side swaps.
#[test]
fn mirror_items_mirrors_a_tuning_pattern_as_base_mirror() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let mut pat = pattern();
    pat.id = "gen_1".into();
    d.drawings.get_or_insert_with(Default::default).tuning_patterns.push(pat);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::MirrorItems { ids: vec!["gen_1".into()], pivot: p(mm(0.0), mm(0.0)), direction: FlipDirection::LeftRight }).unwrap();
    let pat = &b.design().drawings.as_ref().unwrap().tuning_patterns[0];
    assert_eq!((pat.origin, pat.end), (p(mm(-1.0), mm(2.0)), p(mm(-9.0), mm(2.0))));
    assert_eq!(pat.base_line, vec![p(mm(-1.0), mm(2.0)), p(mm(-9.0), mm(2.0))]);
    assert_eq!(pat.initial_side, eda_model::tuning::PatternSide::Right, "LEFT becomes RIGHT");
}

/// A board with a pattern on F.Cu whose meander is its member, on four copper layers.
fn tuned_on_four_layers() -> Board<'static> {
    let m: &'static eda_model::ConstraintModel = Box::leak(Box::new(model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"])));
    let mut b = Board::new(design(), m, mm(0.1), mm(0.3));
    b.apply(&tuned(meander(), vec![1], Some(pattern()), None, vec![])).unwrap();
    b
}

#[test]
fn swap_layers_moves_a_tuning_pattern_one_way() {
    // The track goes to In1.Cu; the pattern's `GetLayer` is its first track's, In1.Cu, and {In1.Cu} maps onto itself (round 4 B-1)
    let mut b = tuned_on_four_layers();
    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "In1.Cu".into())] }).unwrap();
    assert_eq!(drawings(&b).tuning_patterns[0].layer, "In1.Cu");
}

#[test]
fn swap_layers_both_ways_puts_a_tuning_patterns_tracks_back_as_set_layer_does() {
    // Round 4 B-1: the track goes to B.Cu; the pattern's set is {B.Cu} (`GetLayer` is its first track's), mapped to {F.Cu}: `SetLayer( F.Cu )` moves the
    // pattern and its tracks there.
    let mut b = tuned_on_four_layers();
    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "B.Cu".into()), ("B.Cu".into(), "F.Cu".into())] }).unwrap();
    assert_eq!(drawings(&b).tuning_patterns[0].layer, "F.Cu");
    for member in &drawings(&b).tuning_patterns[0].member_ids {
        assert_eq!(routing(&b).tracks.iter().find(|t| &t.id == member).unwrap().layer, "F.Cu");
    }
}

#[test]
fn swap_layers_chained_moves_a_pattern_twice() {
    // F.Cu to In1.Cu and In1.Cu to In2.Cu: the tracks go to In1.Cu, then the set {In1.Cu} maps to {In2.Cu} and `SetLayer( In2.Cu )` takes everything there.
    let mut b = tuned_on_four_layers();
    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "In1.Cu".into()), ("In1.Cu".into(), "In2.Cu".into())] }).unwrap();
    assert_eq!(drawings(&b).tuning_patterns[0].layer, "In2.Cu");
    for member in &drawings(&b).tuning_patterns[0].member_ids {
        assert_eq!(routing(&b).tracks.iter().find(|t| &t.id == member).unwrap().layer, "In2.Cu");
    }
}
