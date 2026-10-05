//! Every check `eda-lint` can emit is named here, once.
//!
//! This exists because of a failure mode that already happened in this
//! project: a gate was written, wired in, reported clean, and was measuring
//! nothing. A check that silently stops firing is worse than no check,
//! because the run still says PASS and now something believes it.
//!
//! A registry cannot prove a check is *correct*. What it proves is that the
//! set of checks is deliberate: renaming one, deleting one, or adding one
//! without saying so breaks this test. Whoever does it has to come here and
//! state the intent, which is exactly the moment to notice that a check
//! disappeared by accident.

use std::collections::BTreeSet;

const REGISTERED: &[&str] = &[
    // -- placement (placement.rs, named in finding.rs) ---------------
    "placement_board_use",
    "placement_decoupling",
    "placement_edge_connector",
    "placement_net_compactness",
    "placement_proximity",
    "placement_refdes_clear",
    "placement_stub_crossings",
    // -- routing (routing.rs) ----------------------------------------
    "routing_track_width",
    // -- fab readiness (fab.rs) --------------------------------------
    "fab_no_lcsc",
    "fab_no_mpn",
    "fab_placement",
    "fab_unknown_part",
    "fab_unplaced_part",
    // -- schematic readability (schematic.rs) ------------------------
    "schematic_cluster_split",
    "schematic_column_overflow",
    "schematic_content_in_bounds",
    "schematic_flag_adjacent",
    "schematic_flow_direction",
    "schematic_label_far_from_part",
    "schematic_label_in_symbol",
    "schematic_label_over_wire",
    "schematic_missing_junction",
    "schematic_offgrid",
    "schematic_power_net_as_wire",
    "schematic_sheet_aspect",
    "schematic_sheet_density",
    "schematic_symbol_overlap",
    "schematic_text_overlap",
    "schematic_wire_crossing_count",
    "schematic_wire_detour",
    "schematic_wire_endpoint_off_pin",
    "schematic_wire_ink",
    "schematic_wire_length",
    "schematic_wire_not_orthogonal",
    "schematic_wire_overlap",
    "schematic_wire_through_symbol",
];

/// Scrape check-name literals out of the crate source. Deliberately crude:
/// it matches the naming convention every check follows (`<area>_<what>`),
/// so a check that invents a different shape of name is invisible to it.
/// That is a known limit -- the job is to notice a *registered* check
/// vanishing, and a convention-based scrape is enough for that.
fn emitted() -> BTreeSet<String> {
    const SOURCES: &[&str] = &[include_str!("../src/finding.rs"), include_str!("../src/fab.rs"), include_str!("../src/schematic.rs"), include_str!("../src/routing.rs"), include_str!("../src/placement.rs")];
    const PREFIXES: &[&str] = &["placement_", "routing_", "fab_", "schematic_"];
    let mut found = BTreeSet::new();
    for src in SOURCES {
        for (i, _) in src.match_indices('"') {
            let rest = &src[i + 1..];
            let Some(end) = rest.find('"') else { continue };
            let tok = &rest[..end];
            if PREFIXES.iter().any(|p| tok.starts_with(p)) && tok.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                found.insert(tok.to_string());
            }
        }
    }
    found
}

#[test]
fn every_emitted_check_is_registered() {
    let registered: BTreeSet<String> = REGISTERED.iter().map(|s| s.to_string()).collect();
    let unregistered: Vec<_> = emitted().difference(&registered).cloned().collect();
    assert!(
        unregistered.is_empty(),
        "emitted but not registered: {unregistered:?}\n\
         Add them to REGISTERED -- the list is how a check appearing or vanishing gets noticed."
    );
}

#[test]
fn every_registered_check_is_still_emitted() {
    // The direction that catches the real bug: a check unwired, renamed or
    // deleted, where the run keeps reporting PASS because nothing is
    // looking any more.
    let emitted = emitted();
    let missing: Vec<_> = REGISTERED.iter().filter(|c| !emitted.contains(**c)).collect();
    assert!(
        missing.is_empty(),
        "registered but no longer emitted: {missing:?}\n\
         Either removed on purpose (drop them here, and say why) or a check silently \
         stopped running while the suite kept passing."
    );
}

#[test]
fn the_registry_has_no_duplicates() {
    let set: BTreeSet<_> = REGISTERED.iter().collect();
    assert_eq!(set.len(), REGISTERED.len(), "duplicate entry in REGISTERED");
}
