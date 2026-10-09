//! A waived DRC violation as the derived `.kicad_pro` lists it (`board.design_settings.drc_exclusions`).
//!
//! KiCad keeps an exclusion as the text of the marker it waives -- `PCB_MARKER::SerializeToString` -- and matches a marker found by a
//! run to its exclusion by that text, exactly (`BOARD::ResolveDRCExclusions`): the check's settings key, the marker's position in
//! nanometres, and the uuids of the items it names. A text no marker matches is dropped when the board loads.
//!
//! The position is the catch: kicad-cli's report (`DRC_REPORT::WriteJsonReport`) says where the *items* of a violation are
//! (`EDA_ITEM::GetPosition`), never where the marker is, and most checks put the marker somewhere of their own (the contact point of
//! two items, the middle of a track). So an exclusion stored in `design.json` carries the positions worth trying
//! ([`DrcExclusion::positions_nm`]) and one text is written for each; the one that is the marker's is the one kicad-cli matches. Where the
//! marker sits at an item's position -- a via, a pad, a footprint, the start of an unconnected pair -- or in the middle of a track, that
//! is the whole story. Where it sits somewhere else the exclusion holds in the studio's own report and not in kicad-cli's.

use eda_model::ir::DrcExclusion;
use std::collections::BTreeSet;

/// `niluuid`: what `RC_ITEM::GetAuxItemID` is for a violation that names one item.
pub const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// `MARKER_BASE::MARKER_RATSNEST`: the marker type `PCB_MARKER::SerializeToString` writes for an unconnected pair.
const MARKER_RATSNEST: u8 = 4;

/// The checks whose marker text also carries the marker's layer, which a report does not say (`DRCE_COPPER_SLIVER`, `DRCE_STARVED_THERMAL`,
/// `DRCE_GENERIC_WARNING`, `DRCE_GENERIC_ERROR`) and the drawing-sheet variable check, whose items are not preserved between runs.
const LAYERED: &[&str] = &["copper_sliver", "starved_thermal", "generic-warning", "generic-error", "unresolved_variable"];

/// The texts to list in the project for one exclusion: `PCB_MARKER::SerializeToString` at each position worth trying, sorted and
/// listed once. Empty when the exclusion has no position to try, or its check's text has a part (the marker's layer) a report does not give.
pub fn marker_texts(e: &DrcExclusion) -> Vec<String> {
    if LAYERED.contains(&e.check.as_str()) {
        return Vec::new();
    }
    let main = e.items.first().map_or(NIL_UUID, String::as_str);
    let aux = e.items.get(1).map_or(NIL_UUID, String::as_str);
    let texts: BTreeSet<String> = e
        .positions_nm
        .iter()
        .map(|[x, y]| {
            if e.check == "unconnected_items" {
                // `DRCE_UNCONNECTED_ITEMS`: the marker layer is `F_Cu` when it has none, and a ratsnest marker's type is in the text.
                format!("{}|{x}|{y}|F.Cu|{MARKER_RATSNEST}|{main}|{aux}", e.check)
            } else {
                format!("{}|{x}|{y}|{main}|{aux}", e.check)
            }
        })
        .collect();
    texts.into_iter().collect()
}

/// The `drc_exclusions` array of the project's `board.design_settings`: `[text, comment]` for each text of each exclusion
/// (`BOARD_DESIGN_SETTINGS`' `drc_exclusions` `PARAM_LAMBDA`), in the order KiCad's own `std::set` keeps them.
pub fn project_entries(exclusions: &[DrcExclusion]) -> Vec<serde_json::Value> {
    let mut entries: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for e in exclusions {
        for text in marker_texts(e) {
            entries.insert(text, e.comment.clone());
        }
    }
    entries.into_iter().map(|(text, comment)| serde_json::json!([text, comment])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waived(check: &str, items: &[&str], positions: &[[i64; 2]], comment: &str) -> DrcExclusion {
        DrcExclusion { check: check.into(), items: items.iter().map(|s| s.to_string()).collect(), ids: vec![], positions_nm: positions.to_vec(), comment: comment.into() }
    }

    #[test]
    fn a_marker_text_is_the_check_the_position_and_the_two_items() {
        let two = marker_texts(&waived("clearance", &["u-1", "u-2"], &[[4_400_000, 22_225_000]], ""));
        assert_eq!(two, vec!["clearance|4400000|22225000|u-1|u-2".to_string()]);
        // One item: the aux item is the nil uuid.
        let one = marker_texts(&waived("annular_width", &["u-1"], &[[12_459_000, -18_390_000]], ""));
        assert_eq!(one, vec![format!("annular_width|12459000|-18390000|u-1|{NIL_UUID}")]);
    }

    #[test]
    fn an_unconnected_pair_carries_its_layer_and_marker_type() {
        let texts = marker_texts(&waived("unconnected_items", &["p-1", "p-2"], &[[9_025_000, 26_200_000], [9_562_000, 26_200_000]], ""));
        assert_eq!(texts, vec!["unconnected_items|9025000|26200000|F.Cu|4|p-1|p-2".to_string(), "unconnected_items|9562000|26200000|F.Cu|4|p-1|p-2".to_string()]);
    }

    #[test]
    fn every_position_to_try_is_a_text_and_a_check_whose_text_needs_a_layer_is_left_out() {
        let e = waived("clearance", &["a", "b"], &[[2, 1], [1, 1], [1, 1]], "");
        assert_eq!(marker_texts(&e).len(), 2, "the same position twice is one text");
        assert!(marker_texts(&waived("copper_sliver", &["a"], &[[1, 1]], "")).is_empty());
        assert!(marker_texts(&waived("starved_thermal", &["a", "b"], &[[1, 1]], "")).is_empty());
        assert!(marker_texts(&waived("clearance", &["a", "b"], &[], "")).is_empty(), "no position to try: the studio's own exclusion");
    }

    #[test]
    fn the_project_lists_each_text_with_its_comment_sorted() {
        let list = project_entries(&[waived("track_width", &["t"], &[[5, 5]], "thin on purpose"), waived("annular_width", &["v"], &[[1, 2]], "")]);
        assert_eq!(list, vec![serde_json::json!([format!("annular_width|1|2|v|{NIL_UUID}"), ""]), serde_json::json!([format!("track_width|5|5|t|{NIL_UUID}"), "thin on purpose"])]);
    }
}
