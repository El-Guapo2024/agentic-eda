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

/// An item's copper as the copper clearance test sees it on a layer: `PCB_TRACK::GetEffectiveShape` is a `SHAPE_SEGMENT` (the ends and the
/// width), `PCB_VIA::GetEffectiveShape` a `SHAPE_CIRCLE` (the centre and half the layer's diameter), and `PAD::GetEffectiveShape` a
/// `SHAPE_COMPOUND` of a `SHAPE_CIRCLE`, a `SHAPE_SEGMENT` (an oval), a `SHAPE_RECT` (a rectangle on the axes) or a `SHAPE_SIMPLE` (a turned
/// rectangle, a trapezoid), with a `SHAPE_SEGMENT` along each side of a rounded rectangle (`PAD::buildEffectiveShape`, KiCad 25ec2335).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copper {
    Segment { a: [i64; 2], b: [i64; 2], width: i64 },
    Circle { centre: [i64; 2], radius: i64 },
    /// `SHAPE_RECT` with no radius: its corner `p0` (the least x and y) and its size.
    Rect { p0: [i64; 2], w: i64, h: i64 },
    /// `SHAPE_SIMPLE`: a closed polygon.
    Simple { pts: Vec<[i64; 2]> },
    /// `SHAPE_COMPOUND`, never nested.
    Compound(Vec<Copper>),
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
    match (check, &main, &aux) {
        ("tracks_crossing", Copper::Segment { a, b, .. }, Copper::Segment { a: c, b: d, .. }) => seg_intersect([*a, *b], [*c, *d]),
        // `itemShape->Collide( otherShape, clearance, &actual, &pos )`: whether they collide is the report's to say; where is the shapes'
        ("clearance" | "shorting_items", _, _) => collide(&main, &aux).map(|(_, at)| at),
        _ => None,
    }
}

/// `collideShapes( aA, aB, clearance, &actual, &location )` (KiCad 25ec2335 `shape_collisions.cpp`) for two shapes known to collide: `(actual, location)`.
/// A compound is each of its shapes against the other (`collideCompoundSubshapes`), the least `actual` winning and the first of equals kept. `None` for a
/// pair this port does not place (two polygons, a rectangle and a polygon), and for a compound with one of those.
pub fn collide(a: &Copper, b: &Copper) -> Option<(i64, [i64; 2])> {
    match (a, b) {
        (Copper::Compound(pa), Copper::Compound(pb)) => least(pa.iter().flat_map(|x| pb.iter().map(move |y| collide_single(x, y)))),
        (Copper::Compound(pa), _) => least(pa.iter().map(|x| collide_single(x, b))),
        (_, Copper::Compound(pb)) => least(pb.iter().map(|y| collide_single(a, y))),
        _ => collide_single(a, b),
    }
}

/// The first of the least `actual`s (`if( actual < currentActual )`), or `None` when one of them is not known.
fn least(results: impl Iterator<Item = Option<(i64, [i64; 2])>>) -> Option<(i64, [i64; 2])> {
    let mut best: Option<(i64, [i64; 2])> = None;
    for r in results {
        let r = r?;
        if best.is_none_or(|b| r.0 < b.0) {
            best = Some(r);
        }
    }
    best
}

/// `collideSingleShapes`: which `Collide( T_a, T_b )` a pair goes to (`CollCase`, `CollCaseReversed`).
fn collide_single(a: &Copper, b: &Copper) -> Option<(i64, [i64; 2])> {
    use Copper::*;
    match (a, b) {
        (Circle { centre: p, radius: r }, Circle { centre: q, radius: s }) => {
            let d = [q[0] - p[0], q[1] - p[1]];
            let dist_sq = sq(d);
            // `VECTOR2I::operator/( double )`: `KiROUND` of each half
            Some(((isqrt_trunc(dist_sq) - r - s).max(0), [ki_round_half(p[0] + q[0]), ki_round_half(p[1] + q[1])]))
        }
        (Circle { centre, radius }, Segment { a, b, width }) | (Segment { a, b, width }, Circle { centre, radius }) => circle_segment(*centre, *radius, [*a, *b], *width),
        (Segment { a, b, width }, Segment { a: c, b: d, width: w2 }) => {
            // `aA.Collide( aB.GetSeg(), .. )`, `SHAPE_SEGMENT::Collide( const SEG& )` (a zero-length other: `Collide( const VECTOR2I& )`)
            let s = [*a, *b];
            let (dist_sq, at) = if c == d { (seg_sq_distance_point(s, *c), seg_nearest_point(s, *c)) } else { (seg_sq_distance(s, [*c, *d]), seg_nearest_to_seg(s, [*c, *d])) };
            let actual = (isqrt_trunc(dist_sq) - (width + 1) / 2).max(0);
            Some(((actual - w2 / 2).max(0), at))
        }
        (Rect { p0, w, h }, Segment { a, b, width }) | (Segment { a, b, width }, Rect { p0, w, h }) => rect_segment(*p0, *w, *h, [*a, *b], *width),
        (Rect { p0, w, h }, Circle { centre, radius }) | (Circle { centre, radius }, Rect { p0, w, h }) => Some(rect_circle(*p0, *w, *h, *centre, *radius)),
        (Simple { pts }, Segment { a, b, width }) | (Segment { a, b, width }, Simple { pts }) => {
            let (actual, at) = chain_seg(pts, [*a, *b]);
            Some(((actual - width / 2).max(0), at))
        }
        (Simple { pts }, Circle { centre, radius }) | (Circle { centre, radius }, Simple { pts }) => circle_chain(*centre, *radius, pts),
        // `Collide( const SHAPE_RECT&, const SHAPE_RECT& )` with an output asked: the two `Outline()`s as line chains
        (Rect { p0, w, h }, Rect { p0: q0, w: w2, h: h2 }) => Some(chain_chain(&rect_outline(*p0, *w, *h), &rect_outline(*q0, *w2, *h2))),
        // `Collide( const SHAPE_RECT&, const SHAPE_LINE_CHAIN_BASE& )` (`CollCase< SHAPE_RECT, SHAPE_LINE_CHAIN_BASE >( aB, aA )` the other way round)
        (Rect { p0, w, h }, Simple { pts }) | (Simple { pts }, Rect { p0, w, h }) => Some(rect_chain(*p0, *w, *h, pts)),
        (Simple { pts }, Simple { pts: other }) => Some(chain_chain(pts, other)),
        _ => None,
    }
}

/// `SHAPE_RECT::Outline()` of a rectangle with no radius (`TransformToPolygon`): `p0`, `(x + w, y)`, `(x + w, y + h)`, `(x, y + h)`, closed.
fn rect_outline(p0: [i64; 2], w: i64, h: i64) -> Vec<[i64; 2]> {
    vec![p0, [p0[0] + w, p0[1]], [p0[0] + w, p0[1] + h], [p0[0], p0[1] + h]]
}

/// `SEG::Collide( aSeg, clearance, &dist )`'s `dist`: 0 when they cross or touch, else the integer root of the least end-to-segment square.
fn seg_dist(a: [[i64; 2]; 2], b: [[i64; 2]; 2]) -> i64 {
    if a[0] == a[1] {
        return isqrt_floor(seg_sq_distance_point(b, a[0]));
    }
    if b[0] == b[1] {
        return isqrt_floor(seg_sq_distance_point(a, b[0]));
    }
    if seg_intersect(a, b).is_some() {
        return 0;
    }
    let m = [seg_sq_distance_point(a, b[0]), seg_sq_distance_point(a, b[1]), seg_sq_distance_point(b, a[0]), seg_sq_distance_point(b, a[1])].into_iter().min().unwrap_or(0);
    isqrt_floor(m)
}

/// `isqrt`: the floor of the root.
fn isqrt_floor(v: i128) -> i64 {
    let mut r = (v as f64).sqrt() as i128;
    while r * r > v {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= v {
        r += 1;
    }
    r as i64
}

fn sides(pts: &[[i64; 2]]) -> Vec<[[i64; 2]; 2]> {
    (0..pts.len()).map(|i| [pts[i], pts[(i + 1) % pts.len()]]).collect()
}

/// `Collide( const SHAPE_LINE_CHAIN_BASE&, const SHAPE_LINE_CHAIN_BASE& )` of two closed chains, with an output asked: `aA`'s first point inside `aB`
/// is the place, else `aB`'s first point inside `aA`; else each chain's segments sorted by their start (`seg_sort`: x, then y) and every pair tried, the
/// least `dist` winning (`a_seg.NearestPoint( b_seg )`), the first of equals, the inner loop left at 0 (the segment index is used only past 32 x 64
/// segments, and keeps this order: "Candidate IDs restore the legacy order").
fn chain_chain(a: &[[i64; 2]], b: &[[i64; 2]]) -> (i64, [i64; 2]) {
    if let Some(&p) = a.first() {
        if point_inside(b, p) {
            return (0, p);
        }
    }
    if let Some(&p) = b.first() {
        if point_inside(a, p) {
            return (0, p);
        }
    }
    let sort = |mut v: Vec<[[i64; 2]; 2]>| {
        v.sort_by(|x, y| (x[0][0], x[0][1]).cmp(&(y[0][0], y[0][1])));
        v
    };
    let (sa, sb) = (sort(sides(a)), sort(sides(b)));
    let mut closest = i64::MAX;
    let mut nearest = a.first().copied().unwrap_or([0, 0]);
    for x in &sa {
        for y in &sb {
            let d = seg_dist(*x, *y);
            if d < closest {
                nearest = seg_nearest_to_seg(*x, *y);
                closest = d;
            }
            if closest == 0 {
                break;
            }
        }
    }
    (closest, nearest)
}

/// `Collide( const SHAPE_RECT&, const SHAPE_LINE_CHAIN_BASE& )` with no radius: the rectangle's centre inside the chain is the place, else each of the
/// chain's segments against the rectangle (`SHAPE_RECT::Collide( SEG )`), the least winning, the first of equals.
fn rect_chain(p0: [i64; 2], w: i64, h: i64, pts: &[[i64; 2]]) -> (i64, [i64; 2]) {
    let centre = [p0[0] + w / 2, p0[1] + h / 2];
    if point_inside(pts, centre) {
        return (0, centre);
    }
    let mut best = (i64::MAX, centre);
    for s in sides(pts) {
        if let Some((d, at)) = rect_segment(p0, w, h, s, 0) {
            if d < best.0 {
                best = (d, at);
                if d == 0 {
                    break;
                }
            }
        }
    }
    best
}

fn sq(v: [i64; 2]) -> i128 {
    (v[0] as i128).pow(2) + (v[1] as i128).pow(2)
}

/// `(int) sqrt( dist_sq )`.
fn isqrt_trunc(v: i128) -> i64 {
    (v as f64).sqrt() as i64
}

/// `Collide( const SHAPE_CIRCLE&, const SHAPE_SEGMENT& )`: `SHAPE_CIRCLE::Collide( aSeg.GetSeg(), .. )` -- the segment's nearest point to the centre, or
/// where the circle crosses it when the centre is on it -- and the segment's half width off the actual.
fn circle_segment(centre: [i64; 2], radius: i64, s: [[i64; 2]; 2], width: i64) -> Option<(i64, [i64; 2])> {
    let (actual, at) = circle_seg(centre, radius, s);
    Some(((actual - width / 2).max(0), at))
}

/// `SHAPE_CIRCLE::Collide( const SEG& )`.
fn circle_seg(centre: [i64; 2], radius: i64, s: [[i64; 2]; 2]) -> (i64, [i64; 2]) {
    let pn = seg_nearest_point(s, centre);
    let dist_sq = sq([pn[0] - centre[0], pn[1] - centre[1]]);
    let mut at = pn;
    if dist_sq == 0 {
        if let Some(first) = circle_intersect_seg(centre, radius, s).first() {
            at = *first;
        }
    }
    ((isqrt_trunc(dist_sq) - radius).max(0), at)
}

/// `CIRCLE::Intersect( const SEG& )`: `IntersectLine`'s points that the segment contains (`SEG::Contains`, a squared distance of 3 at most).
fn circle_intersect_seg(centre: [i64; 2], radius: i64, s: [[i64; 2]; 2]) -> Vec<[i64; 2]> {
    let [a, b] = s;
    let d = [b[0] - a[0], b[1] - a[1]];
    let l_sq = sq(d);
    // `SEG::LineProject`
    let m = if l_sq == 0 {
        a
    } else {
        let t = d[0] as i128 * (centre[0] - a[0]) as i128 + d[1] as i128 * (centre[1] - a[1]) as i128;
        [a[0] + rescale(t, d[0] as i128, l_sq) as i64, a[1] + rescale(t, d[1] as i128, l_sq) as i64]
    };
    let om = rounded_hypot(m[0] - centre[0], m[1] - centre[1]);
    const MIN_PRECISION_IU: i64 = 2;
    let points = if om > radius + MIN_PRECISION_IU {
        Vec::new()
    } else if om >= radius - MIN_PRECISION_IU {
        vec![m]
    } else {
        let m_to_1 = ((radius as i128 * radius as i128 - om as i128 * om as i128) as f64).sqrt() as i64;
        let v = resize(d, m_to_1);
        vec![[m[0] + v[0], m[1] + v[1]], [m[0] - v[0], m[1] - v[1]]]
    };
    points.into_iter().filter(|p| seg_sq_distance_point(s, *p) <= 3).collect()
}

/// `RoundedHypot`: the nearest integer to the length.
fn rounded_hypot(x: i64, y: i64) -> i64 {
    let v = sq([x, y]);
    let mut r = (v as f64).sqrt() as i128;
    while r * r > v {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= v {
        r += 1;
    }
    // round: r or r + 1, whichever square is nearer (`(r + 0.5)^2 = r^2 + r + 0.25`)
    if v - r * r > r {
        r += 1;
    }
    r as i64
}

/// `VECTOR2<int>::Resize( aNewLength )`.
fn resize(v: [i64; 2], len: i64) -> [i64; 2] {
    if v == [0, 0] {
        return [0, 0];
    }
    let (nx, ny) = if v[0].abs() == v[1].abs() {
        let h = len.abs() as f64 * std::f64::consts::FRAC_1_SQRT_2;
        (h, h)
    } else {
        let (x_sq, y_sq) = ((v[0] as i128).pow(2), (v[1] as i128).pow(2));
        let l_sq = x_sq + y_sq;
        let n_sq = (len as i128).pow(2);
        ((rescale(n_sq, x_sq, l_sq) as f64).sqrt(), (rescale(n_sq, y_sq, l_sq) as f64).sqrt())
    };
    let kr = |f: f64| f.round() as i64;
    let sign = if len < 0 { -1 } else { 1 };
    [if v[0] < 0 { -kr(nx) } else { kr(nx) } * sign, if v[1] < 0 { -kr(ny) } else { kr(ny) } * sign]
}

/// `Collide( const SHAPE_RECT&, const SHAPE_SEGMENT& )`: `SHAPE_RECT::Collide( aSeg.GetSeg(), .. )` -- an end inside the box is the place, else the side
/// nearest the segment (of equals, the one whose point is nearer the segment's start) -- and the segment's half width off the actual.
fn rect_segment(p0: [i64; 2], w: i64, h: i64, s: [[i64; 2]; 2], width: i64) -> Option<(i64, [i64; 2])> {
    let contains = |p: [i64; 2]| p[0] >= p0[0] && p[0] <= p0[0] + w && p[1] >= p0[1] && p[1] <= p0[1] + h;
    if contains(s[0]) {
        return Some((0, s[0]));
    }
    if contains(s[1]) {
        return Some((0, s[1]));
    }
    let corners = [p0, [p0[0], p0[1] + h], [p0[0] + w, p0[1] + h], [p0[0] + w, p0[1]], p0];
    let mut closest = i128::MAX;
    let mut nearest = p0;
    for i in 0..4 {
        let side = [corners[i], corners[i + 1]];
        let d = seg_sq_distance(side, s);
        if d < closest {
            nearest = seg_nearest_to_seg(side, s);
            closest = d;
        } else if d == closest {
            let near = seg_nearest_to_seg(side, s);
            if sq([near[0] - s[0][0], near[1] - s[0][1]]) < sq([nearest[0] - s[0][0], nearest[1] - s[0][1]]) {
                nearest = near;
            }
        }
    }
    Some(((isqrt_trunc(closest) - width / 2).max(0), nearest))
}

/// `Collide( const SHAPE_RECT&, const SHAPE_CIRCLE& )` with no corner radius: the nearest point of the box's sides to the centre (inside or out).
fn rect_circle(p0: [i64; 2], w: i64, h: i64, c: [i64; 2], r: i64) -> (i64, [i64; 2]) {
    let vts = [p0, [p0[0], p0[1] + h], [p0[0] + w, p0[1] + h], [p0[0] + w, p0[1]], p0];
    let mut nearest_sq = i128::MAX;
    let mut nearest = p0;
    for i in 0..4 {
        let pn = seg_nearest_point([vts[i], vts[i + 1]], c);
        let d = sq([pn[0] - c[0], pn[1] - c[1]]);
        if d < nearest_sq {
            nearest = pn;
            nearest_sq = d;
            if nearest_sq == 0 {
                break;
            }
        }
    }
    ((isqrt_trunc(nearest_sq) - r).max(0), nearest)
}

/// `SHAPE_LINE_CHAIN_BASE::Collide( const SEG& )` on a closed polygon: the segment's start inside it is the place, else the nearest of its sides
/// (`s.NearestPoint( aSeg )`, the first of equals).
fn chain_seg(pts: &[[i64; 2]], s: [[i64; 2]; 2]) -> (i64, [i64; 2]) {
    if point_inside(pts, s[0]) {
        return (0, s[0]);
    }
    let mut closest = i128::MAX;
    let mut nearest = s[0];
    for i in 0..pts.len() {
        let side = [pts[i], pts[(i + 1) % pts.len()]];
        let d = seg_sq_distance(side, s);
        if d < closest {
            nearest = seg_nearest_to_seg(side, s);
            closest = d;
            if closest == 0 {
                break;
            }
        }
    }
    (isqrt_trunc(closest), nearest)
}

/// `Collide( const SHAPE_CIRCLE&, const SHAPE_LINE_CHAIN_BASE& )`: the centre inside the polygon is the place, else the side the circle is nearest
/// (`SHAPE_CIRCLE::Collide( SEG )` on each, the first of equals).
fn circle_chain(c: [i64; 2], r: i64, pts: &[[i64; 2]]) -> Option<(i64, [i64; 2])> {
    if point_inside(pts, c) {
        return Some((0, c));
    }
    let mut best: Option<(i64, [i64; 2])> = None;
    for i in 0..pts.len() {
        let (actual, at) = circle_seg(c, r, [pts[i], pts[(i + 1) % pts.len()]]);
        if best.is_none_or(|b| actual < b.0) {
            best = Some((actual, at));
            if actual == 0 {
                break;
            }
        }
    }
    best
}

/// `SHAPE_LINE_CHAIN_BASE::PointInside` (accuracy 0): the crossings of a ray in +x, `rescale` for the crossing's x.
fn point_inside(pts: &[[i64; 2]], p: [i64; 2]) -> bool {
    if pts.len() < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..pts.len() {
        let (p1, p2) = (pts[i], pts[(i + 1) % pts.len()]);
        let diff = [p2[0] - p1[0], p2[1] - p1[1]];
        if diff[1] == 0 {
            continue;
        }
        let d = rescale(diff[0] as i128, (p[1] - p1[1]) as i128, diff[1] as i128);
        if ((p1[1] >= p[1]) != (p2[1] >= p[1])) && ((p[0] - p1[0]) as i128) < d {
            inside = !inside;
        }
    }
    inside
}

/// `SEG::SquaredDistance( const VECTOR2I& )`: the ends' squares, and inside the Lagrange identity `KiROUND( cross^2 / |ab|^2 )`.
fn seg_sq_distance_point(s: [[i64; 2]; 2], p: [i64; 2]) -> i128 {
    let [a, b] = s;
    let ab = [(b[0] - a[0]) as i128, (b[1] - a[1]) as i128];
    let ap = [(p[0] - a[0]) as i128, (p[1] - a[1]) as i128];
    let e = ap[0] * ab[0] + ap[1] * ab[1];
    if e <= 0 {
        return ap[0] * ap[0] + ap[1] * ap[1];
    }
    let f = ab[0] * ab[0] + ab[1] * ab[1];
    if e >= f {
        let bp = [(p[0] - b[0]) as i128, (p[1] - b[1]) as i128];
        return bp[0] * bp[0] + bp[1] * bp[1];
    }
    let c = cross(ab, ap) as f64;
    (c * c / f as f64).round() as i128
}

/// `SEG::SquaredDistance( const SEG& )`: a point's for a zero-length one, 0 when they cross, else the least of the four end distances.
fn seg_sq_distance(s: [[i64; 2]; 2], o: [[i64; 2]; 2]) -> i128 {
    if s[0] == s[1] {
        return seg_sq_distance_point(o, s[0]);
    }
    if o[0] == o[1] {
        return seg_sq_distance_point(s, o[0]);
    }
    if seg_intersect(s, o).is_some() {
        return 0;
    }
    let d = |p: [i64; 2], q: [i64; 2]| sq([p[0] - q[0], p[1] - q[1]]);
    [d(seg_nearest_point(o, s[0]), s[0]), d(seg_nearest_point(o, s[1]), s[1]), d(seg_nearest_point(s, o[0]), o[0]), d(seg_nearest_point(s, o[1]), o[1])].into_iter().min().unwrap_or(0)
}

/// `RotatePoint( int* pX, int* pY, const EDA_ANGLE& )` (`trigo.cpp`): exact at the right angles, else `KiROUND` of the turned coordinates.
pub fn rotate_point(p: [i64; 2], degrees: f64) -> [i64; 2] {
    let a = degrees.rem_euclid(360.0);
    let [x, y] = p;
    if a == 0.0 {
        [x, y]
    } else if a == 90.0 {
        [y, -x]
    } else if a == 180.0 {
        [-x, -y]
    } else if a == 270.0 {
        [-y, x]
    } else {
        let (s, c) = a.to_radians().sin_cos();
        [((y as f64) * s + (x as f64) * c).round() as i64, ((y as f64) * c - (x as f64) * s).round() as i64]
    }
}

/// The copper of every track segment, via and pad of a `.kicad_pcb` (the derived board kicad-cli judged) by its uuid, as the clearance test takes it on a
/// layer: a track's `SHAPE_SEGMENT`, a via's `SHAPE_CIRCLE`, and a pad's compound (`PAD::buildEffectiveShape`: a circle; an oval's segment; a rectangle's or a
/// rounded rectangle's inset `SHAPE_RECT`, or `SHAPE_SIMPLE` when turned off the axes, and the rounded one's four side segments; a trapezoid's
/// `SHAPE_SIMPLE`), at `ShapePos` (the footprint's position plus the pad's offset turned by the footprint, `SetFPRelativePosition`). A pad of another shape
/// (a chamfered rectangle, a custom pad) or with its copper off its hole (`(offset ..)`) is left out, as is an arc track: their markers stay the studio's.
pub fn board_copper(pcb_text: &str) -> std::collections::HashMap<String, Copper> {
    board_shapes(pcb_text).copper
}

/// The shapes of a `.kicad_pcb` a DRC marker is placed by, each by its item's uuid ([`board_copper`]'s copper, the holes, the board edges).
#[derive(Debug, Clone, Default)]
pub struct BoardShapes {
    pub copper: std::collections::HashMap<String, Copper>,
    /// `GetEffectiveHoleShape`: a `SHAPE_SEGMENT` from `pos - half_len` to `pos + half_len`, as wide as the drill's narrow side (a round hole's length is 0;
    /// `PAD::BuildEffectiveShapes`, `PCB_VIA::GetEffectiveHoleShape`).
    pub holes: std::collections::HashMap<String, Copper>,
    /// A board-edge shape's `GetEffectiveShape( Edge_Cuts )`: its stroke's segments (`EDA_SHAPE::MakeEffectiveShapes` of a line, a rectangle's four sides
    /// from `GetRectCorners`, a polygon's sides), each as wide as the line.
    pub edges: std::collections::HashMap<String, Copper>,
}

/// Where KiCad 25ec2335 puts the marker of `check` between the items `main` and `aux` (the report's order) of `board`:
/// - `clearance`, `shorting_items`: `Collide( main, aux )` (`testSingleLayerItemAgainstItem` after its uuid order, `testPadAgainstItem`);
/// - `tracks_crossing`: the crossing ([`contact_point`]);
/// - `hole_clearance`: `a_shape[ii]->Collide( holeShape )`, the main item's copper against the aux item's hole (`SetItems( a[ii], b[ii] )`);
/// - `copper_edge_clearance`: `itemShape->Collide( edge->GetEffectiveShape( Edge_Cuts ) )`, the aux item against the main item, the edge
///   (`DRC_TEST_PROVIDER_EDGE_CLEARANCE::testAgainstEdge`: `SetItems( edge, item )`).
/// `None` when a shape is not known or the pair is not one this port places.
pub fn marker_at(check: &str, main: &str, aux: &str, board: &BoardShapes) -> Option<[i64; 2]> {
    match check {
        "clearance" | "shorting_items" | "tracks_crossing" => contact_point(check, board.copper.get(main)?.clone(), board.copper.get(aux)?.clone()),
        "hole_clearance" => collide(board.copper.get(main)?, board.holes.get(aux)?).map(|(_, at)| at),
        "copper_edge_clearance" => collide(board.copper.get(aux)?, board.edges.get(main)?).map(|(_, at)| at),
        _ => None,
    }
}

/// [`BoardShapes`] of a `.kicad_pcb` text.
pub fn board_shapes(pcb_text: &str) -> BoardShapes {
    use crate::sexpr::{find, find_all, tag, txt};
    let mut shapes = BoardShapes::default();
    let Ok(tree) = crate::sexpr::parse(pcb_text) else { return shapes };
    let Some(top) = tree.as_list() else { return shapes };
    let out = &mut shapes.copper;
    let holes = &mut shapes.holes;
    let edges = &mut shapes.edges;
    let nm = |t: Option<&str>| -> Option<i64> { t.and_then(|v| v.parse::<f64>().ok()).map(|mm| (mm * 1_000_000.0).round() as i64) };
    let xy = |l: &[crate::sexpr::Sexpr]| -> Option<[i64; 2]> { Some([nm(txt(l, 1))?, nm(txt(l, 2))?]) };
    let uuid = |l: &[crate::sexpr::Sexpr]| find(l, "uuid").or_else(|| find(l, "tstamp")).and_then(|u| txt(u, 1)).map(String::from);
    for item in top.iter().skip(1) {
        let Some(l) = item.as_list() else { continue };
        match tag(l) {
            Some("segment") => {
                if let (Some(a), Some(b), Some(w), Some(u)) = (find(l, "start").and_then(xy), find(l, "end").and_then(xy), find(l, "width").and_then(|w| nm(txt(w, 1))), uuid(l)) {
                    out.insert(u, Copper::Segment { a, b, width: w });
                }
            }
            Some("via") => {
                if let (Some(c), Some(d), Some(u)) = (find(l, "at").and_then(xy), find(l, "size").and_then(|w| nm(txt(w, 1))), uuid(l)) {
                    if let Some(drill) = find(l, "drill").and_then(|w| nm(txt(w, 1))) {
                        holes.insert(u.clone(), Copper::Segment { a: c, b: c, width: drill });
                    }
                    out.insert(u, Copper::Circle { centre: c, radius: d / 2 });
                }
            }
            Some("gr_line" | "gr_rect" | "gr_poly") if find(l, "layer").and_then(|y| txt(y, 1)) == Some("Edge.Cuts") => {
                let Some(u) = uuid(l) else { continue };
                let width = find(l, "stroke").and_then(|st| find(st, "width")).or_else(|| find(l, "width")).and_then(|w| nm(txt(w, 1))).unwrap_or(0);
                let pts: Vec<[i64; 2]> = match tag(l) {
                    Some("gr_line") => [find(l, "start").and_then(xy), find(l, "end").and_then(xy)].into_iter().flatten().collect(),
                    Some("gr_rect") => match (find(l, "start").and_then(xy), find(l, "end").and_then(xy)) {
                        // `GetRectCorners`: the start, (end.x, start.y), the end, (start.x, end.y); a rounded one is not placed
                        (Some(a), Some(b)) if find(l, "radius").is_none() => vec![a, [b[0], a[1]], b, [a[0], b[1]], a],
                        _ => Vec::new(),
                    },
                    _ => {
                        let mut p: Vec<[i64; 2]> = find(l, "pts").map(|ps| find_all(ps, "xy").filter_map(xy).collect()).unwrap_or_default();
                        if find(l, "pts").is_some_and(|ps| find(ps, "arc").is_some()) {
                            p.clear();
                        }
                        if let Some(first) = p.first().copied() {
                            p.push(first);
                        }
                        p
                    }
                };
                if pts.len() >= 2 {
                    edges.insert(u, Copper::Compound(pts.windows(2).map(|w| Copper::Segment { a: w[0], b: w[1], width }).collect()));
                }
            }
            Some("footprint") => {
                let Some(at) = find(l, "at") else { continue };
                let Some(fp_pos) = xy(at) else { continue };
                let fp_angle = txt(at, 3).and_then(|a| a.parse::<f64>().ok()).unwrap_or(0.0);
                for pad in find_all(l, "pad") {
                    let (Some(u), Some(pat)) = (uuid(pad), find(pad, "at")) else { continue };
                    let Some(local) = xy(pat) else { continue };
                    let angle = txt(pat, 3).and_then(|a| a.parse::<f64>().ok()).unwrap_or(0.0);
                    let off = rotate_point(local, fp_angle);
                    let pos = [fp_pos[0] + off[0], fp_pos[1] + off[1]];
                    let has_offset = find(pad, "drill").and_then(|d| find(d, "offset")).and_then(xy).is_some_and(|o| o != [0, 0]);
                    let Some(size) = find(pad, "size").and_then(xy) else { continue };
                    let ratio = find(pad, "roundrect_rratio").and_then(|r| txt(r, 1)).and_then(|r| r.parse::<f64>().ok()).unwrap_or(0.0);
                    let delta = find(pad, "rect_delta").and_then(xy).unwrap_or([0, 0]);
                    // the hole, at the pad's position (`PAD::BuildEffectiveShapes`)
                    if let Some(drill) = find(pad, "drill") {
                        let oval = txt(drill, 1) == Some("oval");
                        let (dx, dy) = if oval { (nm(txt(drill, 2)), nm(txt(drill, 3))) } else { (nm(txt(drill, 1)), nm(txt(drill, 1))) };
                        if let (Some(dx), Some(dy)) = (dx, dy.or(dx)) {
                            let half = [dx / 2, dy / 2];
                            let (hw, len) = if oval { let hw = half[0].min(half[1]); (hw, rotate_point([half[0] - hw, half[1] - hw], angle)) } else { (half[0], [0, 0]) };
                            if dx > 0 {
                                holes.insert(u.clone(), Copper::Segment { a: [pos[0] - len[0], pos[1] - len[1]], b: [pos[0] + len[0], pos[1] + len[1]], width: hw * 2 });
                            }
                        }
                    }
                    // `ShapePos`: the position plus the drill's offset turned by the pad's orientation
                    let shape_pos = if has_offset {
                        let o = rotate_point(find(pad, "drill").and_then(|d| find(d, "offset")).and_then(xy).unwrap_or([0, 0]), angle);
                        [pos[0] + o[0], pos[1] + o[1]]
                    } else {
                        pos
                    };
                    let chamfer = find(pad, "chamfer").map(|c| {
                        let bit = |name: &str| match name {
                            "top_left" => 1,
                            "top_right" => 2,
                            "bottom_left" => 4,
                            "bottom_right" => 8,
                            _ => 0,
                        };
                        let corners = c.iter().skip(1).filter_map(|a| a.text()).map(bit).fold(0, |a, b| a | b);
                        let ratio = find(pad, "chamfer_ratio").and_then(|r| txt(r, 1)).and_then(|r| r.parse::<f64>().ok()).unwrap_or(0.2);
                        (ratio, corners)
                    });
                    let shape = match (txt(pad, 3).unwrap_or(""), chamfer) {
                        ("roundrect" | "rect", Some((cratio, corners))) if corners != 0 => chamfered_compound(shape_pos, size, angle, ratio, cratio, corners),
                        ("custom", _) => custom_compound(pad, shape_pos, size, angle, &nm),
                        (kind, _) => pad_compound(kind, shape_pos, size, angle, ratio, delta),
                    };
                    if let Some(shape) = shape {
                        out.insert(u, shape);
                    }
                }
            }
            _ => {}
        }
    }
    shapes
}

/// A chamfered rectangle pad (`PAD_SHAPE::CHAMFERED_RECT`): `TransformRoundChamferedRectToPolygon`'s corner list -- the four corners, each chamfered one
/// replaced by its two cut points `KiROUND( ratio * shorter side )` along the sides, consecutive duplicates dropped when two chamfers meet -- turned and
/// moved, one `SHAPE_SIMPLE`. `None` when a corner that is not chamfered is rounded (its arc is approximated to the board's error, not ported).
pub fn chamfered_compound(pos: [i64; 2], size: [i64; 2], angle: f64, rratio: f64, chamfer_ratio: f64, corners_mask: u32) -> Option<Copper> {
    let radius = (size[0].min(size[1]) as f64 * rratio).round() as i64;
    let half = [size[0] / 2, size[1] / 2];
    let mut corners: Vec<([i64; 2], i64)> = vec![([-half[0], -half[1]], radius), ([half[0], -half[1]], radius), ([half[0], half[1]], radius), ([-half[0], half[1]], radius)];
    let shorter = size[0].min(size[1]);
    let chamfer = ((chamfer_ratio * shorter as f64).round() as i64).max(0);
    let ids = [1u32, 2, 8, 4]; // top left, top right, bottom right, bottom left
    let sign = [0i64, 1, -1, 0, 0, -1, 1, 0];
    let mut pos_i = 0usize;
    for cc in 0..4usize {
        if corners_mask & ids[cc] == 0 {
            pos_i += 1;
            continue;
        }
        corners[pos_i].1 = 0;
        if chamfer == 0 {
            pos_i += 1;
            continue;
        }
        let c = corners[pos_i];
        corners.insert(pos_i + 1, c);
        let k = |i: i64| sign[(i.rem_euclid(8)) as usize];
        corners[pos_i].0[0] += k(2 * cc as i64) * chamfer;
        corners[pos_i].0[1] += k(2 * cc as i64 - 2) * chamfer;
        corners[pos_i + 1].0[0] += k(2 * cc as i64 + 1) * chamfer;
        corners[pos_i + 1].0[1] += k(2 * cc as i64 - 1) * chamfer;
        pos_i += 2;
    }
    if corners.iter().any(|(_, r)| *r > 0) {
        return None;
    }
    let mut pts: Vec<[i64; 2]> = corners.into_iter().map(|(p, _)| p).collect();
    if corners_mask.count_ones() > 1 && 2 * chamfer >= shorter {
        // `CornerListRemoveDuplicates`
        pts.dedup();
        while pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
    }
    let pts = pts.into_iter().map(|p| {
        let t = rotate_point(p, angle);
        [t[0] + pos[0], t[1] + pos[1]]
    });
    Some(Copper::Compound(vec![Copper::Simple { pts: pts.collect() }]))
}

/// A custom pad (`PAD_SHAPE::CUSTOM`): its anchor -- a circle or a rectangle of the pad's size (`(options (anchor ..))`) -- and each primitive's
/// `MakeEffectiveShapes` (a filled polygon's `SHAPE_SIMPLE` and, with a width, its sides; a line's segment; a filled rectangle's `SHAPE_SIMPLE` and sides;
/// a filled circle of no width), turned by the pad's orientation and moved to its `ShapePos`. `None` for a primitive this port does not build (an arc, a
/// circle with a stroke, a curve).
fn custom_compound(pad: &[crate::sexpr::Sexpr], pos: [i64; 2], size: [i64; 2], angle: f64, nm: &dyn Fn(Option<&str>) -> Option<i64>) -> Option<Copper> {
    use crate::sexpr::{find, find_all, tag, txt};
    let anchor = find(pad, "options").and_then(|o| find(o, "anchor")).and_then(|a| txt(a, 1)).unwrap_or("rect");
    let Copper::Compound(mut parts) = pad_compound(if anchor == "circle" { "circle" } else { "rect" }, pos, size, angle, 0.0, [0, 0])? else { return None };
    let place = |p: [i64; 2]| {
        let t = rotate_point(p, angle);
        [t[0] + pos[0], t[1] + pos[1]]
    };
    let xy = |l: &[crate::sexpr::Sexpr]| -> Option<[i64; 2]> { Some([nm(txt(l, 1))?, nm(txt(l, 2))?]) };
    let Some(prims) = find(pad, "primitives") else { return Some(Copper::Compound(parts)) };
    for prim in prims.iter().skip(1) {
        let Some(l) = prim.as_list() else { continue };
        let width = find(l, "width").or_else(|| find(l, "stroke").and_then(|st| find(st, "width"))).and_then(|w| nm(txt(w, 1))).unwrap_or(0);
        let filled = find(l, "fill").and_then(|f| txt(f, 1)).is_some_and(|f| f == "yes" || f == "solid");
        let outline = |pts: Vec<[i64; 2]>, parts: &mut Vec<Copper>| {
            let pts: Vec<[i64; 2]> = pts.into_iter().map(place).collect();
            if filled {
                parts.push(Copper::Simple { pts: pts.clone() });
            }
            if width > 0 || !filled {
                for s in sides(&pts) {
                    parts.push(Copper::Segment { a: s[0], b: s[1], width });
                }
            }
        };
        match tag(l) {
            Some("gr_poly") => {
                let pts: Vec<[i64; 2]> = find(l, "pts").map(|ps| find_all(ps, "xy").filter_map(xy).collect()).unwrap_or_default();
                if pts.len() < 2 || find(l, "pts").is_some_and(|ps| find(ps, "arc").is_some()) {
                    return None;
                }
                outline(pts, &mut parts);
            }
            Some("gr_rect") => {
                let (a, b) = (find(l, "start").and_then(xy)?, find(l, "end").and_then(xy)?);
                outline(vec![a, [b[0], a[1]], b, [a[0], b[1]]], &mut parts);
            }
            Some("gr_line") => {
                let (a, b) = (find(l, "start").and_then(xy)?, find(l, "end").and_then(xy)?);
                parts.push(Copper::Segment { a: place(a), b: place(b), width });
            }
            Some("gr_circle") if filled && width == 0 => {
                let (c, e) = (find(l, "center").and_then(xy)?, find(l, "end").and_then(xy)?);
                parts.push(Copper::Circle { centre: place(c), radius: rounded_hypot(e[0] - c[0], e[1] - c[1]) });
            }
            _ => return None,
        }
    }
    Some(Copper::Compound(parts))
}

/// `PAD::buildEffectiveShape` of a pad of `shape` at `pos` (its `ShapePos`), `size`, turned `angle` degrees: `None` for a shape this port does not build.
pub fn pad_compound(shape: &str, pos: [i64; 2], size: [i64; 2], angle: f64, rratio: f64, trap_delta: [i64; 2]) -> Option<Copper> {
    let parts = match shape {
        "circle" => vec![Copper::Circle { centre: pos, radius: size[0] / 2 }],
        "oval" if size[0] == size[1] => vec![Copper::Circle { centre: pos, radius: size[0] / 2 }],
        "oval" => {
            let half = [size[0] / 2, size[1] / 2];
            let hw = half[0].min(half[1]);
            let len = rotate_point([half[0] - hw, half[1] - hw], angle);
            vec![Copper::Segment { a: [pos[0] - len[0], pos[1] - len[1]], b: [pos[0] + len[0], pos[1] + len[1]], width: hw * 2 }]
        }
        "rect" | "roundrect" | "trapezoid" => {
            // `GetRoundRectCornerRadius`: `KiROUND( std::min( size.x, size.y ) * ratio )`
            let r = if shape == "roundrect" { (size[0].min(size[1]) as f64 * rratio).round() as i64 } else { 0 };
            let mut half = [size[0] / 2, size[1] / 2];
            let mut td = [0i64, 0];
            if r > 0 {
                half = [half[0] - r, half[1] - r];
                // "For roundrect pad very near a circle, use only a circle": `pcbIUScale.mmToIU( 0.0001 )`
                let min_len = 100;
                if half[0] < min_len && half[1] < min_len {
                    return Some(Copper::Compound(vec![Copper::Circle { centre: pos, radius: r }]));
                }
            } else if shape == "trapezoid" {
                td = [trap_delta[0] / 2, trap_delta[1] / 2];
            }
            let corners: Vec<[i64; 2]> = [[-half[0] - td[1], half[1] + td[0]], [half[0] + td[1], half[1] - td[0]], [half[0] - td[1], -half[1] + td[0]], [-half[0] + td[1], -half[1] - td[0]]]
                .into_iter()
                .map(|c| {
                    let t = rotate_point(c, angle);
                    [t[0] + pos[0], t[1] + pos[1]]
                })
                .collect();
            let c = &corners;
            let axis = (c[0][1] == c[1][1] && c[1][0] == c[2][0] && c[2][1] == c[3][1] && c[3][0] == c[0][0]) || (c[0][0] == c[1][0] && c[1][1] == c[2][1] && c[2][0] == c[3][0] && c[3][1] == c[0][1]);
            let mut parts = vec![if axis {
                Copper::Rect { p0: [c[2][0].min(c[0][0]), c[2][1].min(c[0][1])], w: (c[2][0] - c[0][0]).abs(), h: (c[2][1] - c[0][1]).abs() }
            } else {
                Copper::Simple { pts: corners.clone() }
            }];
            if r > 0 {
                for i in 0..4 {
                    parts.push(Copper::Segment { a: c[i], b: c[(i + 1) % 4], width: r * 2 });
                }
            }
            parts
        }
        _ => return None,
    };
    Some(Copper::Compound(parts))
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

    /// UI-LEFT-2 BB1: a pad's compound as `PAD::buildEffectiveShape` makes it, and where a track, a via or a hole meets it (proved against kicad-cli in
    /// `review_tests::a_pad_clearance_reaches_kicad_cli`).
    #[test]
    fn a_pads_marker_is_where_its_compound_meets_the_other_item() {
        // a 0.8 mm rounded rectangle, a quarter rounded: the inset rectangle and a 0.4 mm wide segment along each side
        let rr = pad_compound("roundrect", [6_000_000, 5_000_000], [800_000, 800_000], 0.0, 0.25, [0, 0]).unwrap();
        let Copper::Compound(parts) = &rr else { panic!() };
        assert_eq!(parts[0], Copper::Rect { p0: [5_800_000, 4_800_000], w: 400_000, h: 400_000 });
        assert_eq!(parts[1], Copper::Segment { a: [5_800_000, 5_200_000], b: [6_200_000, 5_200_000], width: 400_000 });
        assert_eq!(parts.len(), 5);
        // a plain rectangle turned 45 degrees is a polygon; on the axes a `SHAPE_RECT`
        assert!(matches!(&pad_compound("rect", [0, 0], [800_000, 400_000], 45.0, 0.0, [0, 0]).unwrap(), Copper::Compound(p) if matches!(p[0], Copper::Simple { .. })));
        let rect = pad_compound("rect", [6_000_000, 5_000_000], [800_000, 800_000], 90.0, 0.0, [0, 0]).unwrap();
        // a track ending right of it: the side's nearest point to the track (`SHAPE_RECT::Collide( SEG )`)
        let track = Copper::Segment { a: [6_650_000, 5_000_000], b: [14_000_000, 5_000_000], width: 200_000 };
        assert_eq!(collide(&track, &rect), Some((150_000, [6_400_000, 5_000_000])));
        assert_eq!(collide(&rect, &track), Some((150_000, [6_400_000, 5_000_000])), "either order: `CollCase< SHAPE_RECT, SHAPE_SEGMENT >( aB, aA )`");
        // a via below it: the bottom side's point nearest the centre
        let via = Copper::Circle { centre: [6_000_000, 5_850_000], radius: 300_000 };
        assert_eq!(collide(&via, &rect).map(|r| r.1), Some([6_000_000, 5_400_000]));
        // an oval: its segment's nearest point to the track, and a circle pad's middle with a via (`( aA.GetCenter() + aB.GetCenter() ) / 2`)
        let oval = pad_compound("oval", [0, 0], [800_000, 1_200_000], 0.0, 0.0, [0, 0]).unwrap();
        assert_eq!(oval, Copper::Compound(vec![Copper::Segment { a: [0, -200_000], b: [0, 200_000], width: 800_000 }]));
        assert_eq!(collide(&Copper::Segment { a: [500_000, 1_000_000], b: [900_000, 1_000_000], width: 100_000 }, &oval).map(|r| r.1), Some([500_000, 1_000_000]));
        let circle = pad_compound("circle", [0, 0], [800_000, 800_000], 0.0, 0.0, [0, 0]).unwrap();
        assert_eq!(collide(&circle, &Copper::Circle { centre: [1_000_001, 0], radius: 300_000 }).map(|r| r.1), Some([500_001, 0]));
        // a track through a circle's centre: where the circle crosses it (`m_circle.Intersect( aSeg )[0]`)
        assert_eq!(collide(&circle, &Copper::Segment { a: [-1_000_000, 0], b: [1_000_000, 0], width: 100_000 }).map(|r| r.1), Some([400_000, 0]));
        // two polygons (`Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )`): the first one's first point inside the other is the place
        let turned = pad_compound("rect", [0, 0], [800_000, 400_000], 30.0, 0.0, [0, 0]).unwrap();
        assert_eq!(collide(&turned, &turned), Some((0, [-346_410, 200_000])));
        // two rectangles apart: the outlines' sides sorted by their start, the least distance, `a_seg.NearestPoint( b_seg )`
        let a = Copper::Rect { p0: [0, 0], w: 100, h: 100 };
        let b = Copper::Rect { p0: [150, 20], w: 100, h: 100 };
        assert_eq!(collide(&a, &b), Some((50, [100, 20])));
        // a chamfered square with its top left corner cut: five corners, turned and moved
        let ch = chamfered_compound([1000, 1000], [400, 400], 0.0, 0.0, 0.25, 1).unwrap();
        assert_eq!(ch, Copper::Compound(vec![Copper::Simple { pts: vec![[800, 900], [900, 800], [1200, 800], [1200, 1200], [800, 1200]] }]));
        assert_eq!(chamfered_compound([0, 0], [400, 400], 0.0, 0.25, 0.25, 1), None, "a rounded corner beside the chamfer is not built");
    }

    #[test]
    fn a_boards_pads_holes_and_edges_are_read_by_uuid() {
        let pcb = r#"(kicad_pcb (version 20250101)
            (footprint "x:y" (layer "F.Cu") (uuid "fp") (at 10 20 90)
              (pad "1" thru_hole circle (at 1 0 90) (size 1.6 1.6) (drill 0.8) (layers "*.Cu") (uuid "p1"))
              (pad "2" smd roundrect (at -1 0 90) (size 1 0.6) (layers "F.Cu") (roundrect_rratio 0.25) (uuid "p2"))
              (pad "3" thru_hole oval (at 0 2 90) (size 1 2) (drill oval 0.6 1.2) (layers "*.Cu") (uuid "p3")))
            (via (at 3 4) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "v1"))
            (gr_rect (start 0 0) (end 30 40) (stroke (width 0.05) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "e1")))"#;
        let b = board_shapes(pcb);
        // `SetFPRelativePosition`: the pad's offset turned with the footprint: (1, 0) by 90 degrees is (0, -1)
        assert_eq!(b.copper["p1"], Copper::Compound(vec![Copper::Circle { centre: [10_000_000, 19_000_000], radius: 800_000 }]));
        assert_eq!(b.holes["p1"], Copper::Segment { a: [10_000_000, 19_000_000], b: [10_000_000, 19_000_000], width: 800_000 });
        // an oval hole turned with the pad: (0, 0.3) by 90 degrees is (0.3, 0)
        assert_eq!(b.holes["p3"], Copper::Segment { a: [11_700_000, 20_000_000], b: [12_300_000, 20_000_000], width: 600_000 });
        assert!(matches!(&b.copper["p2"], Copper::Compound(p) if p.len() == 5));
        assert_eq!(b.copper["v1"], Copper::Circle { centre: [3_000_000, 4_000_000], radius: 300_000 });
        assert_eq!(b.holes["v1"], Copper::Segment { a: [3_000_000, 4_000_000], b: [3_000_000, 4_000_000], width: 300_000 });
        let Copper::Compound(sides) = &b.edges["e1"] else { panic!() };
        assert_eq!(sides[0], Copper::Segment { a: [0, 0], b: [30_000_000, 0], width: 50_000 }, "`GetRectCorners` order, the stroke's width");
        assert_eq!(sides.len(), 4);
        // the edge check names the edge first and places the marker on the item's side of the meeting
        let track = Copper::Segment { a: [5_000_000, 5_000_000], b: [5_000_000, 39_800_000], width: 200_000 };
        let mut with_track = b.clone();
        with_track.copper.insert("t".into(), track);
        assert_eq!(marker_at("copper_edge_clearance", "e1", "t", &with_track), Some([5_000_000, 39_800_000]));
        assert_eq!(marker_at("hole_clearance", "t", "v1", &with_track), Some([5_000_000, 5_000_000]));
        assert_eq!(marker_at("silk_overlap", "t", "v1", &with_track), None);
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
