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
//! is the whole story. Where it sits at the contact point of two tracks or vias (`clearance`, `shorting_items`, `tracks_crossing`), the point
//! is KiCad's own arithmetic on the two shapes, ported here ([`contact_point`]). Where it sits somewhere else (a pad's or a zone's contact,
//! a courtyard's overlap) the exclusion holds in the studio's own report and not in kicad-cli's.
//!
//! A record no marker matches costs nothing on a DRC run: kicad-cli's DRC job records the exclusions of the markers the load made, deletes every
//! marker, runs the tests and then marks the new ones a record matches (`PCBNEW_JOBS_HANDLER::JobExportDrc`: `RecordDRCExclusions`, `DeleteMARKERs`,
//! `RunTests`, `ResolveDRCExclusions( false )`), and a record that matched none is dropped without a marker of its own. So every position worth
//! trying is written, and for the checks whose marker also names its layer (`copper_sliver`, `starved_thermal`: `PCB_MARKER`'s `ToProto` stores
//! the layer of these, KiCad 25ec2335) each position on each copper layer of the board ([`markers_on`]).

use eda_model::units::Nm;
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
const LAYERED: &[&str] = &["copper_sliver", "starved_thermal", "generic_warning", "generic_error", "unresolved_variable"];

/// The layered checks a record can still be written for: one per copper layer the board has, as `PCB_MARKER`'s `ToProto` stores the marker's layer
/// for them (KiCad 25ec2335 `pcb_marker.cpp`: `storeLayer` for `DRCE_COPPER_SLIVER` and `DRCE_STARVED_THERMAL`, `UNDEFINED_LAYER` written as `F_Cu`).
/// The generic checks name no item a run keeps, and the drawing sheet's variable check is a marker of the drawing sheet.
const LAYERED_BY_COPPER: &[&str] = &["copper_sliver", "starved_thermal"];

/// `kiapi.board.types.BoardLayer` of a copper layer by its canonical name: `BL_F_Cu`, `BL_In1_Cu` .. `BL_In30_Cu`, `BL_B_Cu`
/// (`api/proto/board/board_types.proto`, `ToProtoEnum<PCB_LAYER_ID, BoardLayer>`). `None` for any other layer.
pub fn board_layer(name: &str) -> Option<String> {
    match name {
        "F.Cu" => Some("BL_F_Cu".into()),
        "B.Cu" => Some("BL_B_Cu".into()),
        _ => {
            let n: u32 = name.strip_prefix("In")?.strip_suffix(".Cu")?.parse().ok()?;
            (1..=30).contains(&n).then(|| format!("BL_In{n}_Cu"))
        }
    }
}

/// The markers (`kiapi.board.DrcMarker` as protobuf JSON) to list in the project for one exclusion, one for each position worth trying, sorted and
/// listed once. Empty when the exclusion has no position to try, its check is not one the project can name, or its marker has a part (the layer) a
/// report does not give. [`markers_on`] with no copper layers.
pub fn markers(e: &DrcExclusion) -> Vec<Value> {
    markers_on(e, &[]).into_iter().map(|(m, _)| m).collect()
}

/// [`markers`] on a board whose copper layers are `copper` (canonical names, `F.Cu` first): a check whose marker carries its layer
/// ([`LAYERED_BY_COPPER`]) gets one marker per position per copper layer; each comes with the position it is at.
pub fn markers_on(e: &DrcExclusion, copper: &[String]) -> Vec<(Value, [Nm; 2])> {
    let Some(error_type) = error_type(&e.check) else { return Vec::new() };
    let layers: Vec<Option<String>> = if LAYERED_BY_COPPER.contains(&e.check.as_str()) {
        copper.iter().filter_map(|l| board_layer(l)).map(Some).collect()
    } else if LAYERED.contains(&e.check.as_str()) {
        Vec::new()
    } else {
        vec![None]
    };
    let items: Vec<Value> = e.items.iter().map(|u| json!({ "value": u })).collect();
    let mut positions: Vec<[Nm; 2]> = e.positions_nm.clone();
    positions.sort_unstable();
    positions.dedup();
    let mut out = Vec::new();
    for [x, y] in positions {
        for layer in &layers {
            // protobuf JSON prints a 64-bit integer as a string
            let mut m = json!({ "errorType": error_type, "position": { "xNm": x.to_string(), "yNm": y.to_string() }, "items": items });
            if let Some(layer) = layer {
                m["layer"] = json!(layer);
            }
            out.push((m, [x, y]));
        }
    }
    out
}

/// The `drc_exclusions` array of the project's `board.design_settings`: `{ marker, comment }` for each marker of each exclusion
/// (`kiapi.board.DrcExclusion` as protobuf JSON), in a fixed order -- by check, then position, then items. [`project_entries_on`] with no copper layers.
pub fn project_entries(exclusions: &[DrcExclusion]) -> Vec<Value> {
    project_entries_on(exclusions, &[])
}

/// [`project_entries`] on a board whose copper layers are `copper` (see [`markers_on`]).
pub fn project_entries_on(exclusions: &[DrcExclusion], copper: &[String]) -> Vec<Value> {
    let mut entries: BTreeMap<(String, [Nm; 2], Vec<String>, String), Value> = BTreeMap::new();
    for e in exclusions {
        for (marker, at) in markers_on(e, copper) {
            let layer = marker["layer"].as_str().unwrap_or("").to_string();
            let mut entry = json!({ "marker": marker });
            if !e.comment.is_empty() {
                entry["comment"] = json!(e.comment);
            }
            entries.insert((e.check.clone(), at, e.items.clone(), layer), entry);
        }
    }
    entries.into_values().collect()
}

// ------------------------------------------------------------------------------------------------ where two copper items' marker sits

/// A track segment or a via as the copper clearance test sees it on a layer: `PCB_TRACK::GetEffectiveShape` is a `SHAPE_SEGMENT` (the ends and the
/// width), `PCB_VIA::GetEffectiveShape` a `SHAPE_CIRCLE` (the centre and half the layer's diameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copper {
    Segment { a: [i64; 2], b: [i64; 2], width: i64 },
    Circle { centre: [i64; 2], radius: i64 },
}

/// Where KiCad puts the marker of `check` between two copper items, `main` and `aux` in the order the report names them (the marker's own:
/// `DRC_TEST_PROVIDER_COPPER_CLEARANCE::testSingleLayerItemAgainstItem` swaps the two so that `item->m_Uuid < other->m_Uuid` and then calls
/// `SetItems( item, other )`), in nanometres. KiCad 25ec2335:
/// - `tracks_crossing` (two tracks): `trackSeg.Intersect( otherSeg )`, the crossing;
/// - `clearance` and `shorting_items`: the location `itemShape->Collide( otherShape, clearance, &actual, &pos )` gives (`shape_collisions.cpp`),
///   which is `SHAPE_SEGMENT::Collide( SEG )`'s `m_seg.NearestPoint( aSeg )` for two tracks, `SHAPE_CIRCLE::Collide( SEG )`'s nearest point of the
///   track to the via's centre for a via and a track (in either order: `CollCaseReversed`), and `( aA.GetCenter() + aB.GetCenter() ) / 2` for two vias.
///   (The clearance value decides only whether they collide, not where.)
///
/// `None` for another check or pair. A via whose centre lies on the track takes the circle's first crossing of the track in KiCad
/// (`m_circle.Intersect( aSeg )[0]`), which is not ported: the nearest point is given, and kicad-cli does not match it.
pub fn contact_point(check: &str, main: Copper, aux: Copper) -> Option<[i64; 2]> {
    use Copper::{Circle, Segment};
    match (check, main, aux) {
        ("tracks_crossing", Segment { a, b, .. }, Segment { a: c, b: d, .. }) => seg_intersect([a, b], [c, d]),
        ("clearance" | "shorting_items", Segment { a, b, .. }, Segment { a: c, b: d, .. }) => {
            // SHAPE_SEGMENT::Collide( const SEG& ): a zero-length other is a point, `m_seg.NearestPoint( aP )`
            Some(if c == d { seg_nearest_point([a, b], c) } else { seg_nearest_to_seg([a, b], [c, d]) })
        }
        ("clearance" | "shorting_items", Circle { centre, .. }, Segment { a, b, .. }) | ("clearance" | "shorting_items", Segment { a, b, .. }, Circle { centre, .. }) => Some(seg_nearest_point([a, b], centre)),
        // `VECTOR2I::operator/( double )`: `KiROUND` of each half
        ("clearance" | "shorting_items", Circle { centre: p, .. }, Circle { centre: q, .. }) => Some([ki_round_half(p[0] + q[0]), ki_round_half(p[1] + q[1])]),
        _ => None,
    }
}

/// `KiROUND( v / 2.0 )`: half of `v`, rounded half away from zero.
fn ki_round_half(v: i64) -> i64 {
    if v >= 0 {
        (v + 1) / 2
    } else {
        (v - 1) / 2
    }
}

/// `rescale( aNumerator, aValue, aDenominator )` (`libs/kimath/src/math/util.cpp`, the `__int128` branch): `aNumerator * aValue / aDenominator`
/// rounded to the nearest, half away from zero.
fn rescale(numerator: i128, value: i128, denominator: i128) -> i128 {
    let n = numerator * value;
    if (n < 0) ^ (denominator < 0) {
        (n - denominator / 2) / denominator
    } else {
        (n + denominator / 2) / denominator
    }
}

fn cross(u: [i128; 2], v: [i128; 2]) -> i128 {
    u[0] * v[1] - u[1] * v[0]
}

/// `SEG::NearestPoint( const VECTOR2I& aP )` (KiCad 25ec2335 `seg.cpp`).
fn seg_nearest_point(s: [[i64; 2]; 2], p: [i64; 2]) -> [i64; 2] {
    let [a, b] = s;
    let d = [(b[0] - a[0]) as i128, (b[1] - a[1]) as i128];
    let l_squared = d[0] * d[0] + d[1] * d[1];
    if l_squared == 0 {
        return a;
    }
    let pa = [(p[0] - a[0]) as i128, (p[1] - a[1]) as i128];
    let t = d[0] * pa[0] + d[1] * pa[1];
    if t < 0 {
        return a;
    } else if t > l_squared {
        return b;
    }
    [a[0] + rescale(t, d[0], l_squared) as i64, a[1] + rescale(t, d[1], l_squared) as i64]
}

/// `SEG::NearestPoint( const SEG& aSeg )`: the crossing if they cross, else of the four end-to-segment distances the smallest, its point on this
/// segment (this segment's own end, or the nearest point of this segment to the other's end), the first of equals.
fn seg_nearest_to_seg(s: [[i64; 2]; 2], o: [[i64; 2]; 2]) -> [i64; 2] {
    if let Some(p) = seg_intersect(s, o) {
        return p;
    }
    let [a, b] = s;
    let origin = [seg_nearest_point(o, a), seg_nearest_point(o, b), seg_nearest_point(s, o[0]), seg_nearest_point(s, o[1])];
    let out = [a, b, origin[2], origin[3]];
    let sq = |p: [i64; 2], q: [i64; 2]| ((p[0] - q[0]) as i128).pow(2) + ((p[1] - q[1]) as i128).pow(2);
    let dist = [sq(origin[0], a), sq(origin[1], b), sq(origin[2], o[0]), sq(origin[3], o[1])];
    let mut min_i = 0;
    for i in 0..4 {
        if dist[i] < dist[min_i] {
            min_i = i;
        }
    }
    out[min_i]
}

/// `SEG::Intersect( aSeg )` with its defaults (`aIgnoreEndpoints` and `aLines` false): `SEG::intersects`, KiCad 25ec2335 -- the bounding boxes, then
/// the determinant `dir2.Cross( dir1 )`; parallel segments meet only on one line (`dir1.Cross( offset ) == 0`), where `checkCollinearOverlap` takes the
/// middle of the overlap on the axis of the longer extent; otherwise both parameters must lie in the segments and the point is
/// `aSeg.A + rescale( param1, dir2, determinant )`.
fn seg_intersect(s: [[i64; 2]; 2], o: [[i64; 2]; 2]) -> Option<[i64; 2]> {
    let [a, b] = s;
    let [c, d] = o;
    if a[0].max(b[0]) < c[0].min(d[0]) || c[0].max(d[0]) < a[0].min(b[0]) || a[1].max(b[1]) < c[1].min(d[1]) || c[1].max(d[1]) < a[1].min(b[1]) {
        return None;
    }
    let dir1 = [(b[0] - a[0]) as i128, (b[1] - a[1]) as i128];
    let dir2 = [(d[0] - c[0]) as i128, (d[1] - c[1]) as i128];
    let offset = [(c[0] - a[0]) as i128, (c[1] - a[1]) as i128];
    let determinant = cross(dir2, dir1);
    if determinant == 0 {
        if cross(dir1, offset) != 0 {
            return None;
        }
        // checkCollinearOverlap( aSeg, use_x_axis, false, aPt )
        let use_x = dir1[0].abs() >= dir1[1].abs();
        let (ax, bx) = if use_x { (0, 1) } else { (1, 0) };
        let (s1s, s1e, s2s, s2e, c1s, c1e) = (a[ax], b[ax], c[ax], d[ax], a[bx], b[bx]);
        let (s1min, s1max, s2min, s2max) = (s1s.min(s1e), s1s.max(s1e), s2s.min(s2e), s2s.max(s2e));
        if !(s1max >= s2min && s2max >= s1min) {
            return None;
        }
        let proj = (s1min.max(s2min) + s1max.min(s2max)) / 2;
        let other = if s1e != s1s { c1s + rescale((proj - s1s) as i128, (c1e - c1s) as i128, (s1e - s1s) as i128) as i64 } else { c1s };
        return Some(if use_x { [proj, other] } else { [other, proj] });
    }
    let param2 = cross(dir2, offset);
    let param1 = cross(dir1, offset);
    if determinant > 0 {
        if param1 < 0 || param1 > determinant || param2 < 0 || param2 > determinant {
            return None;
        }
    } else if param1 > 0 || param1 < determinant || param2 > 0 || param2 < determinant {
        return None;
    }
    Some([c[0] + rescale(param1, dir2[0], determinant) as i64, c[1] + rescale(param1, dir2[1], determinant) as i64])
}

/// One exclusion as a project lists it: the check's settings key, the file uuids of its items, the marker's position (nm) and the comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub check: String,
    pub items: Vec<String>,
    pub at: [Nm; 2],
    pub comment: String,
}

/// `KIID::niluuid`, which a marker with one item names as its second.
const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// The exclusions of a project's `board.design_settings.drc_exclusions`, in every form KiCad has written them (`BOARD_DESIGN_SETTINGS`'s `drc_exclusions` parameter and
/// `migrateSchema2to3`): a record (`kiapi.board.DrcExclusion` as protobuf JSON: `{ "marker": { "error_type"|"errorType", "position": { "x_nm"|"xNm", .. }, "items":
/// [ { "value" } ] }, "comment" }`, the names as the proto has them or in camel case, as `JsonStringToMessage` takes both), a `[ "<marker>", "<comment>" ]` pair (KiCad 8 and 9)
/// or the marker's text alone (KiCad 7). The text is `PCB_MARKER::SerializeToString`'s, read as `DeserializeFromString` reads it: `key|x|y|main|aux` and the checks that
/// write it otherwise (`copper_sliver`, `generic_warning`, `generic_error`: `key|x|y|main|layer`; `unconnected_items`: `key|x|y|layer|type|main|aux`, before 9.0.4
/// `key|x|y|main|aux`). An entry that names no item, or a check this port does not know, is left out.
pub fn read_project(pro_text: &str) -> Vec<Listed> {
    let Ok(pro) = serde_json::from_str::<Value>(pro_text) else { return Vec::new() };
    let Some(entries) = pro.pointer("/board/design_settings/drc_exclusions").and_then(Value::as_array) else { return Vec::new() };
    entries.iter().filter_map(listed).filter(|l| !l.items.is_empty()).collect()
}

fn listed(entry: &Value) -> Option<Listed> {
    match entry {
        Value::Object(_) => {
            let marker = entry.get("marker")?;
            let field = |v: &Value, a: &str, b: &str| v.get(a).or_else(|| v.get(b)).cloned();
            let error_type = field(marker, "error_type", "errorType")?;
            let check = ERROR_TYPES.iter().find(|(_, t)| Some(*t) == error_type.as_str()).map(|(k, _)| k.to_string())?;
            let pos = marker.get("position").cloned().unwrap_or(Value::Null);
            // protobuf JSON prints a 64-bit integer as a string; a number is taken too.
            let nm = |v: Option<Value>| Nm::new(v.and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0));
            let at = [nm(field(&pos, "x_nm", "xNm")), nm(field(&pos, "y_nm", "yNm"))];
            let items = marker.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("value").and_then(Value::as_str)).filter(|u| *u != NIL_UUID).map(String::from).collect()).unwrap_or_default();
            Some(Listed { check, items, at, comment: entry.get("comment").and_then(Value::as_str).unwrap_or("").to_string() })
        }
        Value::Array(pair) => {
            let mut l = from_text(pair.first()?.as_str()?)?;
            l.comment = pair.get(1).and_then(Value::as_str).unwrap_or("").to_string();
            Some(l)
        }
        Value::String(text) => from_text(text),
        _ => None,
    }
}

/// `PCB_MARKER::DeserializeFromString`.
fn from_text(text: &str) -> Option<Listed> {
    let props: Vec<&str> = text.split('|').collect();
    let check = props.first()?.to_string();
    error_type(&check)?;
    let num = |i: usize| Nm::new(props.get(i).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0));
    let item = |i: usize| props.get(i).map(|s| s.trim()).filter(|s| !s.is_empty() && *s != NIL_UUID).map(String::from);
    let items: Vec<String> = match check.as_str() {
        "copper_sliver" | "generic_warning" | "generic_error" => item(3).into_iter().collect(),
        "unconnected_items" if props.len() < 6 => item(3).into_iter().chain(item(4)).collect(),
        "unconnected_items" => item(5).into_iter().chain(item(6)).collect(),
        _ => item(3).into_iter().chain(item(4)).collect(),
    };
    Some(Listed { check, items, at: [num(1), num(2)], comment: String::new() })
}

/// A KiCad project's own DRC exclusions as the design's (`DrawingsSection::drc_exclusions`), for a board imported from that project: each item named by the uuid
/// it had in the file is found by the id the import gave it (`item_ids`, `ImportNotes::item_ids`), and then by the uuid the derived board gives it (`derived`, the
/// map `export_kicad_pcb_mapped` returns), so the derived project lists the exclusion as kicad-cli will match it; the marker's position is the one position to try. An
/// exclusion with an item the import does not have is left out, as KiCad drops one no marker matches. Several records of one (check, items) are one exclusion with
/// their positions.
pub fn from_project(pro_text: &str, item_ids: &BTreeMap<String, String>, derived: &std::collections::HashMap<String, String>) -> Vec<DrcExclusion> {
    let mut uuid_of: BTreeMap<&str, &str> = BTreeMap::new();
    let mut by_uuid: Vec<(&String, &String)> = derived.iter().collect();
    by_uuid.sort();
    for (uuid, id) in by_uuid {
        uuid_of.entry(id.as_str()).or_insert(uuid.as_str());
    }
    let mut out: BTreeMap<(String, Vec<String>), DrcExclusion> = BTreeMap::new();
    for l in read_project(pro_text) {
        let Some(ids) = l.items.iter().map(|u| item_ids.get(u).cloned()).collect::<Option<Vec<String>>>() else { continue };
        let Some(items) = ids.iter().map(|id| uuid_of.get(id.as_str()).map(|u| u.to_string())).collect::<Option<Vec<String>>>() else { continue };
        let e = out.entry((l.check.clone(), items.clone())).or_insert_with(|| DrcExclusion { check: l.check.clone(), items, ids, positions_nm: Vec::new(), comment: l.comment.clone() });
        e.positions_nm.push(l.at);
        if e.comment.is_empty() {
            e.comment = l.comment;
        }
    }
    out.into_values().collect()
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
    use super::*;

    fn waived(check: &str, items: &[&str], positions: &[[Nm; 2]], comment: &str) -> DrcExclusion {
        DrcExclusion { check: check.into(), items: items.iter().map(|s| s.to_string()).collect(), ids: vec![], positions_nm: positions.to_vec(), comment: comment.into() }
    }

    #[test]
    fn a_marker_is_the_kind_of_finding_the_position_and_the_items_as_protobuf_json() {
        let two = markers(&waived("clearance", &["u-1", "u-2"], &[[mm(4.4), mm(22.225)]], ""));
        assert_eq!(two, vec![json!({ "errorType": "DRCET_CLEARANCE", "position": { "xNm": "4400000", "yNm": "22225000" }, "items": [{ "value": "u-1" }, { "value": "u-2" }] })]);
        // One item: one is named, no second one is made up.
        let one = markers(&waived("annular_width", &["u-1"], &[[mm(12.459), mm(-18.39)]], ""));
        assert_eq!(one, vec![json!({ "errorType": "DRCET_ANNULAR_WIDTH", "position": { "xNm": "12459000", "yNm": "-18390000" }, "items": [{ "value": "u-1" }] })]);
    }

    #[test]
    fn an_unconnected_pair_is_a_marker_like_any_other() {
        // The nightly before this one wrote a layer and a marker type into the text of a ratsnest marker; a record is the kind, the position and the items.
        let m = markers(&waived("unconnected_items", &["p-1", "p-2"], &[[mm(9.025), mm(26.2)], [mm(9.562), mm(26.2)]], ""));
        assert_eq!(m.len(), 2);
        assert_eq!(m[0]["errorType"], "DRCET_UNCONNECTED_ITEMS");
        assert_eq!(m[1]["position"]["xNm"], "9562000");
    }

    #[test]
    fn every_position_to_try_is_a_record_and_a_check_whose_marker_needs_a_layer_is_left_out() {
        let e = waived("clearance", &["a", "b"], &[[Nm::new(2), Nm::new(1)], [Nm::new(1), Nm::new(1)], [Nm::new(1), Nm::new(1)]], "");
        assert_eq!(markers(&e).len(), 2, "the same position twice is one record");
        assert!(markers(&waived("copper_sliver", &["a"], &[[Nm::new(1), Nm::new(1)]], "")).is_empty());
        assert!(markers(&waived("starved_thermal", &["a", "b"], &[[Nm::new(1), Nm::new(1)]], "")).is_empty());
        assert!(markers(&waived("clearance", &["a", "b"], &[], "")).is_empty(), "no position to try: the studio's own exclusion");
        assert!(markers(&waived("not_a_check", &["a"], &[[Nm::new(1), Nm::new(1)]], "")).is_empty(), "a check the project cannot name");
    }

    #[test]
    fn the_project_lists_each_marker_with_its_comment_sorted() {
        let list = project_entries(&[waived("track_width", &["t"], &[[Nm::new(5), Nm::new(5)]], "thin on purpose"), waived("annular_width", &["v"], &[[Nm::new(1), Nm::new(2)]], "")]);
        assert_eq!(
            list,
            vec![
                json!({ "marker": { "errorType": "DRCET_ANNULAR_WIDTH", "position": { "xNm": "1", "yNm": "2" }, "items": [{ "value": "v" }] } }),
                json!({ "marker": { "errorType": "DRCET_TRACK_WIDTH", "position": { "xNm": "5", "yNm": "5" }, "items": [{ "value": "t" }] }, "comment": "thin on purpose" }),
            ]
        );
    }

    #[test]
    fn a_check_whose_marker_names_its_layer_is_listed_on_each_copper_layer() {
        let copper: Vec<String> = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"].iter().map(|s| s.to_string()).collect();
        let starved = waived("starved_thermal", &["zone", "pad"], &[[Nm::new(7), Nm::new(8)]], "few spokes");
        let m = markers_on(&starved, &copper);
        let layers: Vec<&str> = m.iter().map(|(v, _)| v["layer"].as_str().unwrap()).collect();
        assert_eq!(layers, ["BL_F_Cu", "BL_In1_Cu", "BL_In2_Cu", "BL_B_Cu"], "one record per copper layer, named as `BoardLayer` names it");
        assert!(m.iter().all(|(v, at)| v["errorType"] == "DRCET_STARVED_THERMAL" && *at == [Nm::new(7), Nm::new(8)]));
        assert_eq!(project_entries_on(&[starved.clone()], &copper).len(), 4);
        assert!(project_entries_on(&[starved], &copper).iter().all(|e| e["comment"] == "few spokes"));
        // The rest carry no layer, whatever the board.
        assert!(markers_on(&waived("clearance", &["a", "b"], &[[Nm::new(1), Nm::new(1)]], ""), &copper).iter().all(|(v, _)| v.get("layer").is_none()));
        // The generic checks and the drawing sheet's variables are still the studio's own.
        assert!(markers_on(&waived("generic_error", &["a"], &[[Nm::new(1), Nm::new(1)]], ""), &copper).is_empty());
        assert_eq!(board_layer("In30.Cu").as_deref(), Some("BL_In30_Cu"));
        assert_eq!(board_layer("F.SilkS"), None);
    }

    #[test]
    fn the_marker_between_two_tracks_or_vias_is_where_kicads_collision_puts_it() {
        let seg = |a: [i64; 2], b: [i64; 2]| Copper::Segment { a, b, width: 200 };
        let via = |c: [i64; 2]| Copper::Circle { centre: c, radius: 300 };
        // `trackSeg.Intersect( otherSeg )`: the crossing
        assert_eq!(contact_point("tracks_crossing", seg([0, 0], [10, 10]), seg([0, 10], [10, 0])), Some([5, 5]));
        assert_eq!(contact_point("tracks_crossing", seg([0, 0], [10, 0]), seg([0, 5], [10, 5])), None, "parallel tracks do not cross");
        // two collinear overlapping tracks: the middle of the overlap (`checkCollinearOverlap`)
        assert_eq!(contact_point("tracks_crossing", seg([0, 0], [10, 0]), seg([4, 0], [20, 0])), Some([7, 0]));
        // `m_seg.NearestPoint( aSeg )`: of the four distances the least, the first of equals -- here the other's start seen from this one
        assert_eq!(contact_point("clearance", seg([0, 0], [10, 0]), seg([2, 3], [8, 3])), Some([2, 0]));
        assert_eq!(contact_point("clearance", seg([0, 0], [10, 0]), seg([12, 3], [20, 3])), Some([10, 0]), "this one's own end");
        assert_eq!(contact_point("shorting_items", seg([0, 0], [10, 0]), seg([5, -5], [5, 5])), Some([5, 0]), "touching: the crossing");
        // a via and a track, in either order: the track's nearest point to the via's centre (`SHAPE_CIRCLE::Collide( SEG )`)
        assert_eq!(contact_point("clearance", via([5, 4]), seg([0, 0], [10, 0])), Some([5, 0]));
        assert_eq!(contact_point("clearance", seg([0, 0], [10, 0]), via([12, 4])), Some([10, 0]));
        // a slanted track: `rescale` rounds half away from zero
        assert_eq!(contact_point("clearance", via([1, 0]), seg([0, 0], [3, 2])), Some([1, 0]), "t = 3, |d|^2 = 13: 9/13 and 6/13 round to 1 and 0");
        // two vias: the middle, each half rounded away from zero (`VECTOR2I::operator/( double )`)
        assert_eq!(contact_point("clearance", via([1, 1]), via([4, 2])), Some([3, 2]));
        assert_eq!(contact_point("clearance", via([-1, 0]), via([-2, 0])), Some([-2, 0]));
        // another check, or a pair this port does not place
        assert_eq!(contact_point("hole_clearance", via([1, 1]), via([4, 2])), None);
        assert_eq!(contact_point("tracks_crossing", via([1, 1]), seg([0, 0], [4, 4])), None);
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
