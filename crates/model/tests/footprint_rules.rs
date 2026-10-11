//! A footprint that cannot be fabricated must be rejected at the door.
//!
//! The drill case is why this file exists. A through-hole pad with no
//! declared drill used to reach two exporters that each invented one from
//! the copper -- the KiCad writer from `w.min(h) / 2`, the circuit-json
//! writer from `size.0 / 2` -- so a non-square pad shipped a different
//! hole depending on which exporter ran, and neither number came from the
//! part's datasheet.

use eda_model::{CheckStatus, Footprint, Pad, PadKind, PadShape};

fn failing_locations(f: &Footprint) -> Vec<String> {
    f.validate()
        .into_iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.location.unwrap_or_default())
        .collect()
}

fn smd(number: &str) -> Pad {
    Pad { opposite_side: false, number: number.into(), at: (0, 0), size: (1000, 600), shape: Default::default(), kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None }
}

fn tht(number: &str, drill: Option<i64>) -> Pad {
    Pad { opposite_side: false, number: number.into(), at: (0, 0), size: (1600, 1600), shape: Default::default(), kind: PadKind::ThroughHole, drill, drill_slot: None, rot: 0, roundrect_ratio: None }
}

fn fp(pads: Vec<Pad>) -> Footprint {
    Footprint { name: "TEST_FP".into(), pads, courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![] }
}

#[test]
fn an_ordinary_smd_footprint_is_valid() {
    // If this failed, the checks below would be describing normal parts
    // and no board would load at all.
    assert!(failing_locations(&fp(vec![smd("1"), smd("2")])).is_empty());
}

#[test]
fn a_through_hole_pad_with_a_real_drill_is_valid() {
    assert!(failing_locations(&fp(vec![tht("1", Some(800))])).is_empty());
}

#[test]
fn a_through_hole_pad_with_no_drill_is_rejected() {
    // The whole point: there is no right value to guess, because the hole
    // is a dimension of the physical lead.
    assert_eq!(failing_locations(&fp(vec![tht("1", None)])), vec!["TEST_FP.pad 1"]);
}

#[test]
fn a_drill_wider_than_its_pad_is_rejected() {
    // No annular ring: nothing for the plating to land on.
    assert_eq!(failing_locations(&fp(vec![tht("1", Some(1600))])), vec!["TEST_FP.pad 1"]);
}

#[test]
fn a_footprint_with_no_pads_is_rejected() {
    assert_eq!(failing_locations(&fp(vec![])), vec!["TEST_FP.pads"]);
}

#[test]
fn a_repeated_pad_number_is_allowed() {
    // KiCad itself allows a number to repeat -- a connector's shield tab
    // is commonly four physical pads all named the same pin, and every
    // one of them goes on that pin's net. An empty number (a mounting
    // hole) is just as normal.
    assert!(failing_locations(&fp(vec![smd("1"), smd("1"), smd("1"), smd("1")])).is_empty());
    assert!(failing_locations(&fp(vec![smd(""), smd("")])).is_empty());
}

#[test]
fn a_zero_sized_pad_is_rejected() {
    let mut p = smd("1");
    p.size = (0, 600);
    assert_eq!(failing_locations(&fp(vec![p])), vec!["TEST_FP.pad 1"]);
}

fn npth(number: &str, drill: Option<i64>, drill_slot: Option<(i64, i64)>) -> Pad {
    Pad { opposite_side: false, number: number.into(), at: (0, 0), size: (650, 650), shape: PadShape::Circle, kind: PadKind::NonPlatedHole, drill, drill_slot, rot: 0, roundrect_ratio: None }
}

#[test]
fn a_non_plated_hole_needs_a_drill_too() {
    assert_eq!(failing_locations(&fp(vec![npth("", None, None)])), vec!["TEST_FP.pad "]);
    assert!(failing_locations(&fp(vec![npth("", Some(650), None)])).is_empty());
}

#[test]
fn a_non_plated_hole_may_equal_its_pad_size() {
    // No annular ring by definition -- there is no copper -- so unlike a
    // plated through-hole, drill == size is normal, not an error.
    assert!(failing_locations(&fp(vec![npth("", Some(650), None)])).is_empty());
}

#[test]
fn a_slot_drill_round_trips_through_validate() {
    let mut p = tht("SH", None);
    p.size = (1000, 2100);
    p.drill_slot = Some((600, 1700));
    assert!(failing_locations(&fp(vec![p])).is_empty(), "a well-formed slot drill must validate");

    let mut too_big = tht("SH", None);
    too_big.size = (1000, 2100);
    too_big.drill_slot = Some((1000, 1700)); // width == pad width: no annular ring
    assert_eq!(failing_locations(&fp(vec![too_big])), vec!["TEST_FP.pad SH"]);
}

#[test]
fn a_pad_cannot_have_both_a_round_and_a_slot_drill() {
    let mut p = tht("1", Some(300));
    p.drill_slot = Some((300, 600));
    assert_eq!(failing_locations(&fp(vec![p])), vec!["TEST_FP.pad 1"]);
}

#[test]
fn roundrect_ratio_must_be_a_fraction_of_the_shorter_side() {
    let mut p = smd("1");
    p.roundrect_ratio = Some(0.25);
    assert!(failing_locations(&fp(vec![p])).is_empty());

    let mut too_big = smd("1");
    too_big.roundrect_ratio = Some(0.6);
    assert_eq!(failing_locations(&fp(vec![too_big])), vec!["TEST_FP.pad 1"]);

    let mut negative = smd("1");
    negative.roundrect_ratio = Some(-0.1);
    assert_eq!(failing_locations(&fp(vec![negative])), vec!["TEST_FP.pad 1"]);
}
