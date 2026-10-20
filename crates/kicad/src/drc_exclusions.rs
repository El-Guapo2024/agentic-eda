//! A waived DRC violation as the derived `.kicad_pro` lists it (`board.design_settings.drc_exclusions`).
//!
//! KiCad keeps an exclusion as the marker it waives and matches a marker found by a run to its exclusion by what the marker is: the kind of
//! finding, its position in nanometres and the items it names (`BOARD::ResolveDRCExclusions`). Up to the pinned source (`8303b2ad`) that was one
//! text, `PCB_MARKER::SerializeToString`, `<key>|<x>|<y>|<main uuid>|<aux uuid>`. The nightly the studio runs (`10.99.0-5334`) keeps it as a
//! structured record, `kiapi.board.DrcExclusion` (`DRC_EXCLUSION`), in the project as protobuf JSON:
//!
//! ```text
//! { "marker": { "errorType": "DRCET_ANNULAR_WIDTH", "position": { "xNm": "10000000", "yNm": "15000000" }, "items": [ { "value": "<uuid>" } ] },
//!   "comment": "..." }
//! ```
//!
//! (and a `layer` for a marker that has one). That nightly reads an old pipe-delimited text only out of a section that says it is at schema 2 or less
//! (`BOARD_DESIGN_SETTINGS::migrateSchema2to3` turns it into this record) and drops it from any other, so none is written: the project lists the records, and
//! its design settings say they are at schema 3 (`export_kicad_pro_for`). A record no marker matches is dropped when the board loads.
//!
//! The position is the catch: kicad-cli's report (`DRC_REPORT::WriteJsonReport`) says where the *items* of a violation are
//! (`EDA_ITEM::GetPosition`), never where the marker is, and most checks put the marker somewhere of their own (the contact point of
//! two items, the middle of a track). So an exclusion stored in `design.json` carries the positions worth trying
//! ([`DrcExclusion::positions_nm`]) and one record is written for each; the one that is the marker's is the one kicad-cli matches. Where the
//! marker sits at an item's position -- a via, a pad, a footprint, the start of an unconnected pair -- or in the middle of a track, that
//! is the whole story. Where it sits somewhere else the exclusion holds in the studio's own report and not in kicad-cli's.

use eda_model::ir::DrcExclusion;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// KiCad's names for the kinds of finding (`kiapi.board.DrcErrorType`, in libkiapi) by the check's settings key: `DRCE_*` of `pcbnew/drc/drc_item.cpp`
/// with the prefix `DRCET_`.
const ERROR_TYPES: &[(&str, &str)] = &[
    ("unconnected_items", "DRCET_UNCONNECTED_ITEMS"),
    ("shorting_items", "DRCET_SHORTING_ITEMS"),
    ("items_not_allowed", "DRCET_ALLOWED_ITEMS"),
    ("clearance", "DRCET_CLEARANCE"),
    ("creepage", "DRCET_CREEPAGE"),
    ("tracks_crossing", "DRCET_TRACKS_CROSSING"),
    ("copper_edge_clearance", "DRCET_EDGE_CLEARANCE"),
    ("zones_intersect", "DRCET_ZONES_INTERSECT"),
    ("isolated_copper", "DRCET_ISOLATED_COPPER"),
    ("starved_thermal", "DRCET_STARVED_THERMAL"),
    ("via_dangling", "DRCET_DANGLING_VIA"),
    ("track_dangling", "DRCET_DANGLING_TRACK"),
    ("hole_clearance", "DRCET_HOLE_CLEARANCE"),
    ("hole_to_hole", "DRCET_DRILLED_HOLES_TOO_CLOSE"),
    ("holes_co_located", "DRCET_DRILLED_HOLES_COLOCATED"),
    ("connection_width", "DRCET_CONNECTION_WIDTH"),
    ("track_width", "DRCET_TRACK_WIDTH"),
    ("track_angle", "DRCET_TRACK_ANGLE"),
    ("track_segment_length", "DRCET_TRACK_SEGMENT_LENGTH"),
    ("annular_width", "DRCET_ANNULAR_WIDTH"),
    ("drill_out_of_range", "DRCET_DRILL_OUT_OF_RANGE"),
    ("via_diameter", "DRCET_VIA_DIAMETER"),
    ("padstack", "DRCET_PADSTACK"),
    ("padstack_invalid", "DRCET_PADSTACK_INVALID"),
    ("microvia_drill_out_of_range", "DRCET_MICROVIA_DRILL_OUT_OF_RANGE"),
    ("courtyards_overlap", "DRCET_OVERLAPPING_FOOTPRINTS"),
    ("missing_courtyard", "DRCET_MISSING_COURTYARD"),
    ("malformed_courtyard", "DRCET_MALFORMED_COURTYARD"),
    ("pth_inside_courtyard", "DRCET_PTH_IN_COURTYARD"),
    ("npth_inside_courtyard", "DRCET_NPTH_IN_COURTYARD"),
    ("item_on_disabled_layer", "DRCET_DISABLED_LAYER_ITEM"),
    ("invalid_outline", "DRCET_INVALID_OUTLINE"),
    ("duplicate_footprints", "DRCET_DUPLICATE_FOOTPRINT"),
    ("missing_footprint", "DRCET_MISSING_FOOTPRINT"),
    ("extra_footprint", "DRCET_EXTRA_FOOTPRINT"),
    ("net_conflict", "DRCET_NET_CONFLICT"),
    ("footprint_symbol_mismatch", "DRCET_SCHEMATIC_PARITY"),
    ("footprint_filters_mismatch", "DRCET_FOOTPRINT_FILTERS"),
    ("footprint_symbol_field_mismatch", "DRCET_SCHEMATIC_FIELDS_PARITY"),
    ("lib_footprint_issues", "DRCET_LIB_FOOTPRINT_ISSUES"),
    ("lib_footprint_mismatch", "DRCET_LIB_FOOTPRINT_MISMATCH"),
    ("unresolved_variable", "DRCET_UNRESOLVED_VARIABLE"),
    ("assertion_failure", "DRCET_ASSERTION_FAILURE"),
    ("generic_warning", "DRCET_GENERIC_WARNING"),
    ("generic_error", "DRCET_GENERIC_ERROR"),
    ("copper_sliver", "DRCET_COPPER_SLIVER"),
    ("solder_mask_bridge", "DRCET_SOLDERMASK_BRIDGE"),
    ("silk_over_copper", "DRCET_SILK_MASK_CLEARANCE"),
    ("silk_edge_clearance", "DRCET_SILK_EDGE_CLEARANCE"),
    ("silk_overlap", "DRCET_SILK_CLEARANCE"),
    ("text_height", "DRCET_TEXT_HEIGHT"),
    ("text_thickness", "DRCET_TEXT_THICKNESS"),
    ("length_out_of_range", "DRCET_LENGTH_OUT_OF_RANGE"),
    ("skew_out_of_range", "DRCET_SKEW_OUT_OF_RANGE"),
    ("too_many_vias", "DRCET_VIA_COUNT_OUT_OF_RANGE"),
    ("diff_pair_gap_out_of_range", "DRCET_DIFF_PAIR_GAP_OUT_OF_RANGE"),
    ("diff_pair_uncoupled_length_too_long", "DRCET_DIFF_PAIR_UNCOUPLED_LENGTH_TOO_LONG"),
    ("footprint", "DRCET_FOOTPRINT"),
    ("footprint_type_mismatch", "DRCET_FOOTPRINT_TYPE_MISMATCH"),
    ("through_hole_pad_without_hole", "DRCET_PAD_TH_WITH_NO_HOLE"),
    ("mirrored_text_on_front_layer", "DRCET_MIRRORED_TEXT_ON_FRONT_LAYER"),
    ("nonmirrored_text_on_back_layer", "DRCET_NONMIRRORED_TEXT_ON_BACK_LAYER"),
    ("missing_tuning_profile", "DRCET_MISSING_TUNING_PROFILE"),
    ("tuning_profile_track_geometries", "DRCET_TUNING_PROFILE_IMPLICIT_RULES"),
    ("track_on_post_machined_layer", "DRCET_TRACK_ON_POST_MACHINED_LAYER"),
    ("track_not_centered_on_via", "DRCET_TRACK_NOT_CENTERED_ON_VIA"),
];

/// The kind of finding the check's marker is (`kiapi.board.DrcErrorType`).
pub fn error_type(check: &str) -> Option<&'static str> {
    ERROR_TYPES.iter().find(|(k, _)| *k == check).map(|(_, t)| *t)
}

/// The checks whose marker also carries the layer it is on, which a report does not say (`DRCE_COPPER_SLIVER`, `DRCE_STARVED_THERMAL`,
/// `DRCE_GENERIC_WARNING`, `DRCE_GENERIC_ERROR`) and the drawing-sheet variable check, whose items are not preserved between runs.
const LAYERED: &[&str] = &["copper_sliver", "starved_thermal", "generic-warning", "generic-error", "unresolved_variable"];

/// The markers (`kiapi.board.DrcMarker` as protobuf JSON) to list in the project for one exclusion, one for each position worth trying, sorted and
/// listed once. Empty when the exclusion has no position to try, its check is not one the project can name, or its marker has a part (the layer) a
/// report does not give.
pub fn markers(e: &DrcExclusion) -> Vec<Value> {
    let Some(error_type) = error_type(&e.check).filter(|_| !LAYERED.contains(&e.check.as_str())) else { return Vec::new() };
    let items: Vec<Value> = e.items.iter().map(|u| json!({ "value": u })).collect();
    let mut positions: Vec<[i64; 2]> = e.positions_nm.clone();
    positions.sort_unstable();
    positions.dedup();
    positions
        .into_iter()
        .map(|[x, y]| {
            // protobuf JSON prints a 64-bit integer as a string
            json!({ "errorType": error_type, "position": { "xNm": x.to_string(), "yNm": y.to_string() }, "items": items })
        })
        .collect()
}

/// The `drc_exclusions` array of the project's `board.design_settings`: `{ marker, comment }` for each marker of each exclusion
/// (`kiapi.board.DrcExclusion` as protobuf JSON), in a fixed order -- by check, then position, then items.
pub fn project_entries(exclusions: &[DrcExclusion]) -> Vec<Value> {
    let mut entries: BTreeMap<(String, [i64; 2], Vec<String>), Value> = BTreeMap::new();
    for e in exclusions {
        let mut positions = e.positions_nm.clone();
        positions.sort_unstable();
        positions.dedup();
        for (marker, at) in markers(e).into_iter().zip(positions) {
            let mut entry = json!({ "marker": marker });
            if !e.comment.is_empty() {
                entry["comment"] = json!(e.comment);
            }
            entries.insert((e.check.clone(), at, e.items.clone()), entry);
        }
    }
    entries.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waived(check: &str, items: &[&str], positions: &[[i64; 2]], comment: &str) -> DrcExclusion {
        DrcExclusion { check: check.into(), items: items.iter().map(|s| s.to_string()).collect(), ids: vec![], positions_nm: positions.to_vec(), comment: comment.into() }
    }

    #[test]
    fn a_marker_is_the_kind_of_finding_the_position_and_the_items_as_protobuf_json() {
        let two = markers(&waived("clearance", &["u-1", "u-2"], &[[4_400_000, 22_225_000]], ""));
        assert_eq!(two, vec![json!({ "errorType": "DRCET_CLEARANCE", "position": { "xNm": "4400000", "yNm": "22225000" }, "items": [{ "value": "u-1" }, { "value": "u-2" }] })]);
        // One item: one is named, no second one is made up.
        let one = markers(&waived("annular_width", &["u-1"], &[[12_459_000, -18_390_000]], ""));
        assert_eq!(one, vec![json!({ "errorType": "DRCET_ANNULAR_WIDTH", "position": { "xNm": "12459000", "yNm": "-18390000" }, "items": [{ "value": "u-1" }] })]);
    }

    #[test]
    fn an_unconnected_pair_is_a_marker_like_any_other() {
        // The nightly before this one wrote a layer and a marker type into the text of a ratsnest marker; a record is the kind, the position and the items.
        let m = markers(&waived("unconnected_items", &["p-1", "p-2"], &[[9_025_000, 26_200_000], [9_562_000, 26_200_000]], ""));
        assert_eq!(m.len(), 2);
        assert_eq!(m[0]["errorType"], "DRCET_UNCONNECTED_ITEMS");
        assert_eq!(m[1]["position"]["xNm"], "9562000");
    }

    #[test]
    fn every_position_to_try_is_a_record_and_a_check_whose_marker_needs_a_layer_is_left_out() {
        let e = waived("clearance", &["a", "b"], &[[2, 1], [1, 1], [1, 1]], "");
        assert_eq!(markers(&e).len(), 2, "the same position twice is one record");
        assert!(markers(&waived("copper_sliver", &["a"], &[[1, 1]], "")).is_empty());
        assert!(markers(&waived("starved_thermal", &["a", "b"], &[[1, 1]], "")).is_empty());
        assert!(markers(&waived("clearance", &["a", "b"], &[], "")).is_empty(), "no position to try: the studio's own exclusion");
        assert!(markers(&waived("not_a_check", &["a"], &[[1, 1]], "")).is_empty(), "a check the project cannot name");
    }

    #[test]
    fn the_project_lists_each_marker_with_its_comment_sorted() {
        let list = project_entries(&[waived("track_width", &["t"], &[[5, 5]], "thin on purpose"), waived("annular_width", &["v"], &[[1, 2]], "")]);
        assert_eq!(
            list,
            vec![
                json!({ "marker": { "errorType": "DRCET_ANNULAR_WIDTH", "position": { "xNm": "1", "yNm": "2" }, "items": [{ "value": "v" }] } }),
                json!({ "marker": { "errorType": "DRCET_TRACK_WIDTH", "position": { "xNm": "5", "yNm": "5" }, "items": [{ "value": "t" }] }, "comment": "thin on purpose" }),
            ]
        );
    }

    #[test]
    fn every_check_the_studio_lists_has_a_kind_of_finding_unless_the_nightly_added_it_since() {
        // `src/kicad/drc_checks.json` is KiCad's own list at the pinned source; the names are the library's `DRCET_*`.
        assert!(ERROR_TYPES.iter().all(|(_, t)| t.starts_with("DRCET_")));
        for check in ["clearance", "copper_edge_clearance", "hole_clearance", "track_width", "via_dangling", "track_dangling", "courtyards_overlap", "silk_overlap", "unconnected_items", "annular_width", "lib_footprint_mismatch"] {
            assert!(error_type(check).is_some(), "{check}");
        }
        assert_eq!(error_type("courtyards_overlap"), Some("DRCET_OVERLAPPING_FOOTPRINTS"));
        assert_eq!(error_type("copper_edge_clearance"), Some("DRCET_EDGE_CLEARANCE"));
    }
}
