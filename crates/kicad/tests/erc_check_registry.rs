//! Every check `check_erc` can emit is named here, once -- the same
//! anti-regression registry `eda-gates` keeps for its own placement/routing
//! checks (see that crate's `tests/check_registry.rs`, whose own schematic
//! entries moved here along with the checks themselves).
//!
//! This exists because of a failure mode that already happened elsewhere in
//! this project: a gate was written, wired in, reported clean, and was
//! measuring nothing. A check that silently stops firing is worse than no
//! check, because the run still says PASS and now something believes it.
//!
//! A registry cannot prove a check is *correct*. What it proves is that the
//! set of checks is deliberate: renaming one, deleting one, or adding one
//! without saying so breaks this test. Whoever does it has to come here and
//! state the intent, which is exactly the moment to notice that a check
//! disappeared by accident.

use std::collections::BTreeSet;

/// Check names emitted anywhere in `eda_kicad::check_erc` -- both its own
/// pin-electrical checks and the readability/style checks folded in from
/// the former `eda-gates::check_schematic` (see `erc_style.rs`).
const REGISTERED: &[&str] = &[
    // -- electrical (erc.rs) -----------------------------------------
    "duplicate_reference",
    "endpoint_off_grid",
    "footprint_link_issues",
    "isolated_pin_label",
    "label_dangling",
    "lib_symbol_issues",
    "lib_symbol_mismatch",
    "no_connect_connected",
    "no_connect_dangling",
    "pin_not_connected",
    "pin_not_driven",
    "pin_to_pin",
    "power_pin_not_driven",
    "unconnected_wire_endpoint",
    "wire_dangling",
    // -- multi-unit symbols, GAPS.md #21 (erc.rs::check_multi_unit_symbols) --
    "different_unit_footprint",
    "different_unit_net",
    "extra_units",
    "missing_bidi_pin",
    "missing_input_pin",
    "missing_power_pin",
    "missing_unit",
    "unit_value_mismatch",
    // -- hierarchy, GAPS.md #6/#20 (erc.rs::check_hierarchy) --
    "duplicate_sheet_names",
    "hier_label_mismatch",
    // -- readability/style (erc_style.rs) -----------------------------
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

/// Scrape check-name literals out of the crate source. Deliberately crude
/// (see `eda-gates`' own copy of this function, which this mirrors): a
/// quoted string that is either a bare `pin_*`/`no_connect_*`/
/// `lib_symbol_*`/`*_dangling`/`*_reference` electrical check name, or
/// carries the `schematic_` style-check prefix.
fn emitted() -> BTreeSet<String> {
    const SOURCES: &[&str] = &[include_str!("../src/erc.rs"), include_str!("../src/erc_style.rs")];
    let mut found = BTreeSet::new();
    for src in SOURCES {
        for (i, _) in src.match_indices('"') {
            let rest = &src[i + 1..];
            let Some(end) = rest.find('"') else { continue };
            let tok = &rest[..end];
            let is_candidate = tok.starts_with("schematic_") || REGISTERED.contains(&tok);
            if is_candidate && tok.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
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
