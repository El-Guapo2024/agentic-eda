//! Every gate this crate can emit is named here, once.
//!
//! This exists because of a failure mode that already happened: a gate was
//! written, wired in, reported clean, and was measuring nothing -- it
//! looked up footprints in a list that real parts never came from. A gate
//! that silently stops firing is worse than no gate, because the run still
//! says PASS and now something believes it.
//!
//! A registry cannot prove a gate is *correct*. What it proves is that the
//! set of gates is deliberate: renaming one, deleting one, or adding one
//! without saying so breaks this test. Whoever does it has to come here
//! and state the intent, which is exactly the moment to notice that a gate
//! disappeared by accident.

use std::collections::BTreeSet;

/// Check names emitted anywhere in `eda-gates`.
const REGISTERED: &[&str] = &[
    // -- placement --------------------------------------------------
    "placement_board_use",
    "placement_coverage",
    "placement_decoupling",
    "placement_courtyard_overlap",
    "placement_edge_connector",
    "placement_footprint",
    "placement_isolation",
    "placement_net_compactness",
    "placement_outline",
    "placement_present",
    "placement_proximity",
    "placement_refdes_clear",
    "placement_region",
    "placement_separation",
    "placement_stub_crossings",
    "placement_within_outline",
    // -- routing ----------------------------------------------------
    "routing_bend_count",
    "routing_between_smd_pads",
    "routing_clearance",
    "routing_connectivity",
    "routing_detour_ratio",
    "routing_edge_clearance",
    "routing_footprint",
    "routing_over_refdes",
    "routing_pass_through_pad",
    "routing_present",
    "routing_stackup",
    "routing_track_width",
    "routing_unnecessary_via",
    "routing_via_in_pad",
    "routing_within_outline",
    // -- schematic --------------------------------------------------
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

/// Scrape check-name literals out of the crate source.
///
/// Deliberately crude: it matches the naming convention every gate here
/// follows (`<stage>_<what>`), so a gate that invents a different shape of
/// name is invisible to it. That is a known limit, not an oversight -- the
/// job is to notice a *registered* gate vanishing, and a convention-based
/// scrape is enough for that.
fn emitted() -> BTreeSet<String> {
    const SOURCES: &[&str] = &[include_str!("../src/lib.rs"), include_str!("../src/pcb.rs")];
    const PREFIXES: &[&str] = &["placement_", "routing_", "schematic_"];
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
fn every_emitted_gate_is_registered() {
    let registered: BTreeSet<String> = REGISTERED.iter().map(|s| s.to_string()).collect();
    let unregistered: Vec<_> = emitted().difference(&registered).cloned().collect();
    assert!(
        unregistered.is_empty(),
        "emitted but not registered: {unregistered:?}\n\
         Add them to REGISTERED -- the list is how a gate appearing or vanishing gets noticed."
    );
}

#[test]
fn every_registered_gate_is_still_emitted() {
    // The direction that catches the real bug: a gate unwired, renamed or
    // deleted, where the run keeps reporting PASS because nothing is
    // looking any more.
    let emitted = emitted();
    let missing: Vec<_> = REGISTERED.iter().filter(|c| !emitted.contains(**c)).collect();
    assert!(
        missing.is_empty(),
        "registered but no longer emitted: {missing:?}\n\
         Either removed on purpose (drop them here, and say why) or a gate silently \
         stopped running while the suite kept passing."
    );
}

#[test]
fn the_registry_has_no_duplicates() {
    let set: BTreeSet<_> = REGISTERED.iter().collect();
    assert_eq!(set.len(), REGISTERED.len(), "duplicate entry in REGISTERED");
}
