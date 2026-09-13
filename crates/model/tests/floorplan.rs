//! The floorplan stage: does it recover the blocks the intent implies, and
//! does it refuse the plans it cannot honour?

use eda_model::floorplan::{partition, plan};
use eda_model::{ConstraintModel, Part, PlacementRule};

fn part(reference: &str, footprint: &str) -> Part {
    let mut p: Part = serde_yaml::from_str(&format!("reference: {reference}")).expect("part");
    p.footprint = Some(footprint.to_string());
    p
}

fn prox(a: &str, b: &str) -> PlacementRule {
    PlacementRule::Proximity { a: a.into(), b: b.into(), max_mm: 2.0, reason: None }
}

/// An MCU with two decoupling caps, and a regulator with one: two rules
/// groups, so two modules, and the unrelated part stays out of both.
fn two_block_model() -> ConstraintModel {
    let mut m = ConstraintModel::default();
    m.parts = ["U1", "C1", "C2", "U2", "C3", "R9"].iter().map(|r| part(r, if r.starts_with('U') { "SOIC-8" } else { "R_0402_1005Metric" })).collect();
    m.placement_rules = vec![prox("U1", "C1"), prox("U1", "C2"), prox("U2", "C3")];
    m
}

#[test]
fn proximity_rules_become_modules() {
    let (mods, free) = partition(&two_block_model());
    assert_eq!(mods.len(), 2, "two rule groups, two modules: {mods:?}");
    let mut seen: Vec<Vec<String>> = mods.iter().map(|m| m.refs.clone()).collect();
    seen.sort();
    assert_eq!(seen, vec![vec!["C1", "C2", "U1"], vec!["C3", "U2"]]);
    assert_eq!(free, vec!["R9"], "a part no rule and no net ties down stays free");
}

#[test]
fn a_net_to_exactly_one_module_absorbs_the_part() {
    let mut m = two_block_model();
    m.nets = vec![eda_model::Net { name: "SENSE".into(), pins: vec!["R9.1".into(), "U2.3".into()] }];
    let (mods, free) = partition(&m);
    assert!(free.is_empty(), "R9's only net lands in one module, so it joins it: {free:?}");
    let u2 = mods.iter().find(|x| x.refs.contains(&"U2".to_string())).expect("U2 module");
    assert!(u2.refs.contains(&"R9".to_string()), "{:?}", u2.refs);
}

#[test]
fn a_net_pulled_by_two_modules_leaves_the_part_free() {
    let mut m = two_block_model();
    m.nets = vec![eda_model::Net { name: "SHARED".into(), pins: vec!["R9.1".into(), "U1.3".into(), "U2.3".into()] }];
    let (_, free) = partition(&m);
    assert_eq!(free, vec!["R9"], "ambiguous ownership is admitted, not guessed");
}

#[test]
fn a_board_too_small_for_its_modules_is_a_hard_fail() {
    let m = two_block_model();
    let err = plan(&m, (0, 0, 4_000, 4_000), 500, 0).expect_err("4x4 mm cannot hold two blocks");
    assert!(err.contains("of board"), "{err}");
}

#[test]
fn modules_never_overlap_and_stay_on_the_board() {
    let m = two_block_model();
    let fp = plan(&m, (0, 0, 60_000, 40_000), 500, 7).expect("plan");
    assert_eq!(fp.modules.len(), 2);
    for (i, a) in fp.modules.iter().enumerate() {
        assert!(a.rect.0 >= 0 && a.rect.1 >= 0 && a.rect.2 <= 60_000 && a.rect.3 <= 40_000, "{a:?}");
        for b in &fp.modules[i + 1..] {
            let dx = a.rect.2.min(b.rect.2) - a.rect.0.max(b.rect.0);
            let dy = a.rect.3.min(b.rect.3) - a.rect.1.max(b.rect.1);
            assert!(dx <= 0 || dy <= 0, "{} and {} overlap", a.name, b.name);
        }
    }
    // Every part in a module is reachable through `region_of`.
    for m in &fp.modules {
        for r in &m.refs {
            assert_eq!(fp.region_of(r), Some(m.rect));
        }
    }
}
