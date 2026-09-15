//! A footprint that cannot be fabricated must be rejected at the door.
//!
//! The drill case is why this file exists. A through-hole pad with no
//! declared drill used to reach two exporters that each invented one from
//! the copper -- the KiCad writer from `w.min(h) / 2`, the circuit-json
//! writer from `size.0 / 2` -- so a non-square pad shipped a different
//! hole depending on which exporter ran, and neither number came from the
//! part's datasheet.

use eda_model::{CheckStatus, Footprint, Pad, PadKind};

fn failing_locations(f: &Footprint) -> Vec<String> {
    f.validate()
        .into_iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| c.location.unwrap_or_default())
        .collect()
}

fn smd(number: &str) -> Pad {
    Pad { number: number.into(), at: (0, 0), size: (1000, 600), shape: Default::default(), kind: PadKind::Smd, drill: None }
}

fn tht(number: &str, drill: Option<i64>) -> Pad {
    Pad { number: number.into(), at: (0, 0), size: (1600, 1600), shape: Default::default(), kind: PadKind::ThroughHole, drill }
}

fn fp(pads: Vec<Pad>) -> Footprint {
    Footprint { name: "TEST_FP".into(), pads, courtyard: None }
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
fn a_repeated_pad_number_is_rejected() {
    // Two pads called "1" means the netlist cannot say which one a net
    // lands on.
    assert_eq!(failing_locations(&fp(vec![smd("1"), smd("1")])), vec!["TEST_FP.pads"]);
}

#[test]
fn a_zero_sized_pad_is_rejected() {
    let mut p = smd("1");
    p.size = (0, 600);
    assert_eq!(failing_locations(&fp(vec![p])), vec!["TEST_FP.pad 1"]);
}
