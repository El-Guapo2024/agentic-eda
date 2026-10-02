//! `POST /api/board_stats`: port of `pcbnew/board_statistics_report.cpp`'s
//! `ComputeBoardStatistics` / `FormatBoardStatisticsReport` and
//! `pcbnew/board_statistics.cpp`'s `CollectDrillLineItems` (KiCad snapshot
//! `8303b2ad`), driving `DIALOG_BOARD_STATISTICS`
//! (`web/studio/src/components/BoardStatisticsDialog.tsx`). Read-only --
//! "Board Statistics..." never mutates the board, so there is no `Cmd`.
//!
//! Geometry comes from `eda_drc::board::build` (world-space pad/track/via
//! shapes, the same ones DRC checks) and `eda_drc::fill::fill_all_zones`
//! (real zone fills), turned into polygons by `eda_zone_filler::shape` at
//! source's own `ARC_LOW_DEF` (0.02 mm) -- the same
//! `TransformShapeToPolygon(..., ARC_LOW_DEF, ERROR_INSIDE)` calls source
//! makes. Source's quirks are kept on purpose:
//! - copper area is a plain `SHAPE_POLY_SET::Area()` sum of every item's
//!   polygon -- overlapping copper double-counts -- *unless* "subtract holes
//!   from copper areas" is on, whose `BooleanSubtract` unions it first;
//! - courtyard area *is* unioned (`Simplify()`), and also includes every
//!   plated through-hole pad inflated by the default netclass clearance
//!   (and every NPTH hole) on *both* sides;
//! - the min track clearance walks every ordered (track|arc|via) pair on
//!   the same `GetLayer()` (a via's is its top layer) with different nets;
//! - an unset minimum (no tracks / no pairs / no round drills) stays at
//!   `INT_MAX` nm and prints as such, exactly like source's dialog does.
//!
//! Remaining model differences (not source deviations in the algorithm):
//! - Footprint type comes from the footprint's pad composition rather
//!   than a stored `FP_THROUGH_HOLE`/`FP_SMD` attribute, which placed
//!   parts don't carry in this IR (any PTH pad -> THT, else any SMD pad ->
//!   SMD, else Unspecified -- KiCad's own footprint-wizard default rule).
//! - Courtyards are this model's per-footprint courtyard box
//!   (`placed_courtyard`), not an arbitrary polygon.
//! - No connector pads, castellated/press-fit properties or microvias
//!   exist in the IR, so those rows are always 0.

use crate::board;
use eda_clipper2::Point64;
use eda_drc::board::{DrcBoard, DrcPad};
use eda_drc::kimath::Shape as DrcShape;
use eda_model::ir::{Design, Point, Side};
use eda_model::{ConstraintModel, PadKind};
use eda_shape_poly_set::{LineChain, ShapePolySet};
use eda_zone_filler::shape::{exact_polygon, shape_to_polygon, Shape as FillShape};
use serde_json::{json, Value};
use std::path::Path;

/// `ARC_LOW_DEF` (`pcbIUScale.mmToIU( 0.02 )`), in this IR's µm.
const ARC_LOW_DEF: i64 = 20;

/// `std::numeric_limits<int>::max()` IU (nm) -- source's "never set"
/// sentinel for the three minimums, expressed in µm.
const INT_MAX_UM: f64 = i32::MAX as f64 / 1000.0;

/// `BOARD_STATISTICS_OPTIONS`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub exclude_footprints_without_pads: bool,
    pub subtract_holes_from_board_area: bool,
    pub subtract_holes_from_copper_areas: bool,
}

impl Options {
    fn from_body(body: &[u8]) -> Self {
        let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        Options {
            exclude_footprints_without_pads: b("exclude_footprints_without_pads"),
            subtract_holes_from_board_area: b("subtract_holes_from_board_area"),
            subtract_holes_from_copper_areas: b("subtract_holes_from_copper_areas"),
        }
    }
}

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// Shoelace formula, µm² (`SHAPE_LINE_CHAIN::Area( true )` -- absolute).
fn polygon_area(pts: &[Point]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        sum += (a.x as f64) * (b.y as f64) - (b.x as f64) * (a.y as f64);
    }
    (sum / 2.0).abs()
}

fn bbox_of(pts: &[Point]) -> Option<(i64, i64, i64, i64)> {
    if pts.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some((x0, y0, x1, y1))
}

fn pt(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

/// Same mapping as `eda_drc::fill`'s private adapter.
fn fill_shape(s: &DrcShape) -> FillShape {
    match s {
        DrcShape::Circle { c, r } => FillShape::Circle { c: pt(*c), r: *r },
        DrcShape::Stadium { a, b, r } => FillShape::Stadium { a: pt(*a), b: pt(*b), r: *r },
        DrcShape::Rect { x0, y0, x1, y1 } => FillShape::Rect { x0: *x0, y0: *y0, x1: *x1, y1: *y1 },
        DrcShape::RoundRect { x0, y0, x1, y1, r } => FillShape::RoundRect { x0: *x0, y0: *y0, x1: *x1, y1: *y1, r: *r },
        DrcShape::Polygon { pts } => FillShape::Polygon { pts: pts.iter().map(|&p| pt(p)).collect() },
        DrcShape::Strokes { .. } => FillShape::Polygon { pts: Vec::new() },
    }
}

fn append(set: &mut ShapePolySet, chain: LineChain) {
    if chain.len() >= 3 {
        set.add_outline(chain);
    }
}

/// `PAD::TransformShapeToPolygon( aSet, layer, aClearance, ARC_LOW_DEF, ... )`.
fn append_shape(set: &mut ShapePolySet, s: &DrcShape, clearance: i64) {
    append(set, if clearance > 0 { shape_to_polygon(&fill_shape(s), clearance, ARC_LOW_DEF) } else { exact_polygon(&fill_shape(s), ARC_LOW_DEF) });
}

/// `PAD::GetEffectiveHoleShape()` area as source computes it for the board
/// area: `seg.Length() * width + PI/4 * width^2`.
fn hole_area(s: &DrcShape) -> f64 {
    match s {
        DrcShape::Circle { r, .. } => std::f64::consts::PI * (*r as f64).powi(2),
        DrcShape::Stadium { a, b, r } => {
            let w = 2.0 * *r as f64;
            (((b.x - a.x) as f64).hypot((b.y - a.y) as f64)) * w + std::f64::consts::PI * 0.25 * w * w
        }
        _ => 0.0,
    }
}

/// `DRILL_LINE_ITEM`.
#[derive(Debug, Clone, PartialEq)]
pub struct DrillLine {
    pub x_um: i64,
    pub y_um: i64,
    pub oval: bool,
    pub plated: bool,
    pub is_pad: bool,
    pub start_layer: Option<String>,
    pub stop_layer: Option<String>,
    pub qty: i64,
}

/// `CollectDrillLineItems`: pads (footprint order) then vias, `addOrIncrement`.
fn collect_drill_line_items(board: &DrcBoard) -> Vec<DrillLine> {
    let mut out: Vec<DrillLine> = Vec::new();
    let mut add = |d: DrillLine| {
        if let Some(e) = out.iter_mut().find(|e| DrillLine { qty: e.qty, ..d.clone() } == **e) {
            e.qty += 1;
        } else {
            out.push(DrillLine { qty: 1, ..d });
        }
    };
    for p in &board.pads {
        let (xs, ys, oval) = match (p.drill_round, p.drill_slot) {
            (Some(d), _) => (d, d, false),
            (None, Some((x, y))) => (x, y, true),
            _ => continue,
        };
        if xs <= 0 || ys <= 0 {
            continue;
        }
        // `GetLayerSet().CuStack()` front/back.
        let (top, bottom) = (p.layers.first().cloned(), p.layers.last().cloned());
        add(DrillLine { x_um: xs, y_um: ys, oval, plated: p.kind != PadKind::NonPlatedHole, is_pad: true, start_layer: top, stop_layer: bottom, qty: 0 });
    }
    for v in &board.vias {
        if v.drill <= 0 {
            continue;
        }
        let (top, bottom) = via_span(board, &v.from_layer, &v.to_layer);
        add(DrillLine { x_um: v.drill, y_um: v.drill, oval: false, plated: true, is_pad: false, start_layer: Some(top), stop_layer: Some(bottom), qty: 0 });
    }
    out
}

/// `PCB_VIA::TopLayer()`/`BottomLayer()`: the via's span ordered by the
/// board's copper stack.
fn via_span(board: &DrcBoard, a: &str, b: &str) -> (String, String) {
    let idx = |l: &str| board.layers.iter().position(|x| x == l).unwrap_or(usize::MAX);
    if idx(a) <= idx(b) {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// `VIATYPE` of a via in this IR (no microvia flag exists).
fn via_type(board: &DrcBoard, top: &str, bottom: &str) -> usize {
    let first = board.layers.first().map(String::as_str);
    let last = board.layers.last().map(String::as_str);
    match (Some(top) == first, Some(bottom) == last) {
        (true, true) => 0,                // THROUGH
        (true, false) | (false, true) => 1, // BLIND
        (false, false) => 2,              // BURIED
    }
}

fn pad_has_hole(p: &DrcPad) -> bool {
    p.hole.is_some()
}

pub fn compute(dir: &Path, body: &[u8]) -> Value {
    let (meta, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let opts = Options::from_body(body);
    let mut stats = compute_from(&design, &model, opts);
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if req.get("report").and_then(Value::as_bool).unwrap_or(false) {
        let units = req.get("units").and_then(Value::as_str).unwrap_or("mm");
        let date = req.get("date").and_then(Value::as_str).unwrap_or("");
        let name = std::path::PathBuf::from(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
        stats["report"] = Value::String(format_report(&stats, units, date, &name, &name));
        // `fn.SetName( fn.GetName() + wxT( "_report" ) ); fn.SetExt( wxT( "txt" ) );`
        stats["report_file_name"] = Value::String(format!("{name}_report.txt"));
    }
    stats
}

/// `ComputeBoardStatistics`. Split out from [`compute`] so it can be
/// exercised directly with a hand-built fixture.
fn compute_from(design: &Design, model: &ConstraintModel, opts: Options) -> Value {
    let board = eda_drc::board::build(design, model);
    let front = board.layers.first().cloned().unwrap_or_else(|| "F.Cu".to_string());
    let back = board.layers.last().cloned().unwrap_or_else(|| "B.Cu".to_string());

    // footprintEntries: THT, SMD, Unspecified -- (front, back).
    let mut fp_counts = [(0i64, 0i64); 3];
    // padEntries: PTH, SMD, CONN, NPTH.
    let mut pad_counts = [0i64; 4];
    // viaEntries: THROUGH, BLIND, BURIED, MICROVIA.
    let mut via_counts = [0i64; 4];

    if let Some(pl) = &design.placement {
        for fp in &pl.footprints {
            let Some(part) = model.part(&fp.id) else { continue };
            let Some(footprint) = model.footprint_of(part) else { continue };
            if opts.exclude_footprints_without_pads && footprint.pads.is_empty() {
                continue;
            }
            let has_tht = footprint.pads.iter().any(|p| p.kind == PadKind::ThroughHole);
            let has_smd = footprint.pads.iter().any(|p| p.kind == PadKind::Smd);
            let entry = if has_tht {
                0
            } else if has_smd {
                1
            } else {
                2
            };
            match fp.side {
                Side::Top => fp_counts[entry].0 += 1,
                Side::Bottom => fp_counts[entry].1 += 1,
            }
            for p in &footprint.pads {
                pad_counts[match p.kind {
                    PadKind::ThroughHole => 0,
                    PadKind::Smd => 1,
                    PadKind::NonPlatedHole => 3,
                }] += 1;
            }
        }
    }

    // Tracks: min width (PCB_TRACE_T only), pairwise clearance, via types.
    let mut min_track_width = i64::MAX;
    for t in &board.tracks {
        min_track_width = min_track_width.min(t.width);
    }
    // (layer, net, shape) for every PCB_TRACE_T/PCB_ARC_T/PCB_VIA_T, in
    // `aBoard->Tracks()` order.
    let mut track_items: Vec<(String, Option<String>, DrcShape)> = board.tracks.iter().map(|t| (t.layer.clone(), t.net.clone(), t.shape())).collect();
    for v in &board.vias {
        let (top, bottom) = via_span(&board, &v.from_layer, &v.to_layer);
        via_counts[via_type(&board, &top, &bottom)] += 1;
        track_items.push((top, v.net.clone(), v.shape()));
    }
    let mut min_clearance = i64::MAX;
    for (i, (la, na, sa)) in track_items.iter().enumerate() {
        for (j, (lb, nb, sb)) in track_items.iter().enumerate() {
            if i == j || la != lb || na == nb {
                continue;
            }
            // `Collide( b, minClearance, &actual )` with `minClearance`
            // starting at INT_MAX collides for every pair, so this is the
            // min `actual` (clamped at 0) over all of them.
            min_clearance = min_clearance.min(sa.clearance_to(sb).0);
        }
    }

    // Drills, sorted COMPARE( COL_COUNT, false ) -- by count, descending
    // (stable, so ties keep `CollectDrillLineItems` order).
    let mut drills = collect_drill_line_items(&board);
    drills.sort_by(|a, b| b.qty.cmp(&a.qty));
    let min_drill = drills.iter().filter(|d| !d.oval).map(|d| d.x_um).min().unwrap_or(i64::MAX);

    // Board outline / area.
    let outline = &board.outline;
    let has_outline = outline.len() >= 3;
    let mut board_area = 0.0;
    let (mut board_width, mut board_height) = (0, 0);
    if has_outline {
        board_area = polygon_area(outline);
        if opts.subtract_holes_from_board_area {
            for p in &board.pads {
                if let Some(h) = &p.hole {
                    board_area -= hole_area(h);
                }
            }
            for v in &board.vias {
                board_area -= std::f64::consts::PI * 0.25 * (v.drill as f64).powi(2);
            }
        }
        if let Some((x0, y0, x1, y1)) = bbox_of(outline) {
            board_width = x1 - x0;
            board_height = y1 - y0;
        }
    }

    // Courtyard areas.
    let mut front_area_set = ShapePolySet::new();
    let mut back_area_set = ShapePolySet::new();
    for f in &board.footprints {
        let (x0, y0, x1, y1) = f.courtyard;
        let rect = vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)];
        match f.side {
            Side::Top => append(&mut front_area_set, rect),
            Side::Bottom => append(&mut back_area_set, rect),
        }
    }
    // `std::min( minPadClearanceOuter, pad->GetOwnClearance( layer ) )` --
    // the default netclass clearance is this IR's floor for every pad.
    let pad_clearance = model.board.clearance;
    for p in board.pads.iter().filter(|p| pad_has_hole(p)) {
        if p.kind != PadKind::NonPlatedHole {
            append_shape(&mut front_area_set, &p.copper, pad_clearance);
            append_shape(&mut back_area_set, &p.copper, pad_clearance);
        } else if let Some(h) = &p.hole {
            append_shape(&mut front_area_set, h, 0);
            append_shape(&mut back_area_set, h, 0);
        }
    }
    front_area_set.simplify();
    back_area_set.simplify();
    let front_courtyard_area = front_area_set.area();
    let back_courtyard_area = back_area_set.area();
    let (front_density, back_density) =
        if has_outline { (front_courtyard_area * 100.0 / board_area, back_courtyard_area * 100.0 / board_area) } else { (0.0, 0.0) };

    // Copper areas: every non-footprint child (recursing into footprints'
    // pads), on F.Cu / B.Cu.
    let mut copper = [ShapePolySet::new(), ShapePolySet::new()];
    let mut holes = [ShapePolySet::new(), ShapePolySet::new()];
    let sides = [front.as_str(), back.as_str()];
    for p in &board.pads {
        for (k, l) in sides.iter().enumerate() {
            if p.layers.iter().any(|x| x == l) && p.kind != PadKind::NonPlatedHole {
                append_shape(&mut copper[k], &p.copper, 0);
            }
            if let Some(h) = &p.hole {
                append_shape(&mut holes[k], h, 0);
            }
        }
    }
    for t in &board.tracks {
        for (k, l) in sides.iter().enumerate() {
            if t.layer == *l {
                append_shape(&mut copper[k], &t.shape(), 0);
            }
        }
    }
    for v in &board.vias {
        let (top, bottom) = via_span(&board, &v.from_layer, &v.to_layer);
        let (ti, bi) = (board.layers.iter().position(|x| *x == top), board.layers.iter().position(|x| *x == bottom));
        for (k, l) in sides.iter().enumerate() {
            let li = board.layers.iter().position(|x| x == l);
            if let (Some(ti), Some(bi), Some(li)) = (ti, bi, li) {
                if li >= ti && li <= bi {
                    append_shape(&mut copper[k], &v.shape(), 0);
                    append_shape(&mut holes[k], &v.hole(), 0);
                }
            }
        }
    }
    for s in &board.shapes {
        for (k, l) in sides.iter().enumerate() {
            if s.layer() == *l {
                append_shape(&mut copper[k], &eda_drc::board::from_ir_shape(s), 0);
            }
        }
    }
    if !board.zones.is_empty() {
        let fills = eda_drc::fill::fill_all_zones(&board, &model.board);
        for z in &board.zones {
            for (k, l) in sides.iter().enumerate() {
                if z.layer == *l {
                    if let Some(fill) = fills.get(&z.id) {
                        for poly in &fill.polys {
                            copper[k].add_polygon(poly.clone());
                        }
                    }
                }
            }
        }
    }
    if opts.subtract_holes_from_copper_areas {
        let [hf, hb] = &holes;
        copper[0].boolean_subtract(hf);
        copper[1].boolean_subtract(hb);
    }
    let front_copper_area = copper[0].area();
    let back_copper_area = copper[1].area();

    // `GetStackupOrDefault().BuildBoardThicknessFromStackup()`; the default
    // stackup is built from `BOARD_DESIGN_SETTINGS`'s 1.6 mm.
    let thickness_um = model.stackup.as_ref().map(|s| s.layers.iter().filter_map(|l| l.thickness_mm).sum::<f64>() * 1000.0).unwrap_or(1600.0);

    let min_or_sentinel = |v: i64| if v == i64::MAX { INT_MAX_UM } else { v as f64 };

    let drill_rows: Vec<Value> = drills
        .iter()
        .map(|d| {
            json!({
                "qty": d.qty,
                "shape": if d.oval { "slot" } else { "round" },
                "x_um": d.x_um,
                "y_um": d.y_um,
                "plated": d.plated,
                "is_pad": d.is_pad,
                "start_layer": d.start_layer,
                "stop_layer": d.stop_layer,
            })
        })
        .collect();

    json!({
        "ok": true,
        "board": {
            "has_outline": has_outline,
            "width_um": board_width,
            "height_um": board_height,
            "area_um2": board_area,
            "front_copper_area_um2": front_copper_area,
            "back_copper_area_um2": back_copper_area,
            "front_courtyard_area_um2": front_courtyard_area,
            "back_courtyard_area_um2": back_courtyard_area,
            "front_density_pct": front_density,
            "back_density_pct": back_density,
            "min_track_width_um": min_or_sentinel(min_track_width),
            "min_clearance_um": min_or_sentinel(min_clearance),
            "min_drill_um": min_or_sentinel(min_drill),
            "thickness_um": thickness_um,
        },
        "footprints": [
            { "title": "THT:", "front": fp_counts[0].0, "back": fp_counts[0].1 },
            { "title": "SMD:", "front": fp_counts[1].0, "back": fp_counts[1].1 },
            { "title": "Unspecified:", "front": fp_counts[2].0, "back": fp_counts[2].1 },
        ],
        "pads": [
            { "title": "Through hole:", "qty": pad_counts[0] },
            { "title": "SMD:", "qty": pad_counts[1] },
            { "title": "Connector:", "qty": pad_counts[2] },
            { "title": "NPTH:", "qty": pad_counts[3] },
        ],
        "pad_properties": [
            { "title": "Castellated:", "qty": 0 },
            { "title": "Press-fit:", "qty": 0 },
        ],
        "vias": [
            { "title": "Through vias:", "qty": via_counts[0] },
            { "title": "Blind vias:", "qty": via_counts[1] },
            { "title": "Buried vias:", "qty": via_counts[2] },
            { "title": "Micro vias:", "qty": via_counts[3] },
        ],
        "drills": drill_rows,
    })
}

/// `EDA_DATA_TYPE`.
#[derive(Clone, Copy, PartialEq)]
enum DataType {
    Distance,
    Area,
}

/// `EDA_UNIT_UTILS::UI::MessageTextFromValue` (pcbnew IU scale), for a
/// value given in this IR's µm (or µm² for an area).
fn message_text_from_value(value_um: f64, units: &str, add_label: bool, ty: DataType) -> String {
    // To KiCad IU (nm), then `ToUserUnit` once per dimension.
    let (iu, per_unit) = match units {
        "mil" => (if ty == DataType::Area { value_um * 1e6 } else { value_um * 1e3 }, 25_400.0),
        "in" => (if ty == DataType::Area { value_um * 1e6 } else { value_um * 1e3 }, 25_400_000.0),
        _ => (if ty == DataType::Area { value_um * 1e6 } else { value_um * 1e3 }, 1_000_000.0),
    };
    let mut value = iu / per_unit;
    if ty == DataType::Area {
        value /= per_unit;
    }
    let short_form = ty == DataType::Area;
    let decimals = match (units, short_form) {
        ("mil", true) => 0,
        ("mil", false) => 2,
        ("in", true) => 3,
        ("in", false) => 4,
        (_, true) => 3,
        (_, false) => 4,
    };
    let mut text = format!("{value:.decimals$}");
    if value != 0.0 && !text.chars().any(|c| ('1'..='9').contains(&c)) {
        text = c_exp(value);
    }
    // Trim to 2-1/2 digits after the decimal place for short-form mm.
    if short_form && units == "mm" {
        let b = text.as_bytes();
        let n = b.len();
        if n > 4 && b[n - 4] == b'.' && b[n - 1] == b'0' {
            text.truncate(n - 1);
        }
    }
    if add_label {
        text += match units {
            "mil" => " mils",
            "in" => " in",
            _ => " mm",
        };
        if ty == DataType::Area {
            text += "²";
        }
    }
    text
}

/// printf's `%.3e` (Rust's `{:e}` has no zero-padded two-digit exponent).
fn c_exp(v: f64) -> String {
    let s = format!("{v:.3e}");
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, digits) = if let Some(d) = e.strip_prefix('-') { ('-', d) } else { ('+', e) };
            format!("{m}e{sign}{digits:0>2}")
        }
        None => s,
    }
}

/// `appendTable`.
fn append_table(rows: &[Vec<String>], first_col_as_label: bool, out: &mut String) {
    if rows.is_empty() {
        return;
    }
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    let mut widths = vec![0usize; cols];
    for row in rows {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(cell.chars().count());
        }
    }
    let cell = |row: &Vec<String>, c: usize| row.get(c).cloned().unwrap_or_default();
    let pad_left = |s: &str, w: usize| format!("{}{}", " ".repeat(w.saturating_sub(s.chars().count())), s);
    let pad_right = |s: &str, w: usize| format!("{}{}", s, " ".repeat(w.saturating_sub(s.chars().count())));
    let data_row = |row: &Vec<String>, label: bool, out: &mut String| {
        if label && first_col_as_label {
            out.push_str(&format!("|{}  |", pad_right(&cell(row, 0), widths[0])));
            for c in 1..cols {
                out.push_str(&format!(" {} |", pad_left(&cell(row, c), widths[c])));
            }
        } else {
            out.push('|');
            for c in 0..cols {
                out.push_str(&format!(" {} |", pad_left(&cell(row, c), widths[c])));
            }
        }
        out.push('\n');
    };
    data_row(&rows[0], false, out);
    out.push('|');
    for w in &widths {
        out.push_str(&"-".repeat((w + 2).max(3)));
        out.push('|');
    }
    out.push('\n');
    for row in &rows[1..] {
        data_row(row, true, out);
    }
}

/// `FormatBoardStatisticsReport`, from [`compute_from`]'s JSON.
fn format_report(s: &Value, units: &str, date: &str, project: &str, board_name: &str) -> String {
    let b = &s["board"];
    let f = |k: &str| b[k].as_f64().unwrap_or(0.0);
    let dist = |k: &str| message_text_from_value(f(k), units, true, DataType::Distance);
    let area = |k: &str| message_text_from_value(f(k), units, true, DataType::Area);
    let mut r = String::new();
    r += "PCB statistics report\n=====================\n";
    r += &format!("- Date: {date}\n- Project: {project}\n- Board name: {board_name}\n\n");
    r += "Board\n-----\n";
    if b["has_outline"].as_bool().unwrap_or(false) {
        r += &format!("- Width: {}\n", message_text_from_value(f("width_um"), units, true, DataType::Distance));
        r += &format!("- Height: {}\n", message_text_from_value(f("height_um"), units, true, DataType::Distance));
        r += &format!("- Area: {}\n", area("area_um2"));
    } else {
        r += "- Dimensions: unknown\n- Area: unknown\n";
    }
    r += &format!("- Front copper area: {}\n", area("front_copper_area_um2"));
    r += &format!("- Back copper area: {}\n", area("back_copper_area_um2"));
    r += &format!("- Min track clearance: {}\n", dist("min_clearance_um"));
    r += &format!("- Min track width: {}\n", dist("min_track_width_um"));
    r += &format!("- Min drill diameter: {}\n", dist("min_drill_um"));
    r += &format!("- Board stackup thickness: {}\n\n", dist("thickness_um"));
    r += &format!("- Front footprint area: {}\n", area("front_courtyard_area_um2"));
    r += &format!("- Back footprint area: {}\n", area("back_courtyard_area_um2"));
    let has = b["has_outline"].as_bool().unwrap_or(false);
    r += &format!("- Front component density: {}\n", if has { format!("{:.2} %", f("front_density_pct")) } else { "unknown".into() });
    r += &format!("- Back component density: {}\n", if has { format!("{:.2} %", f("back_density_pct")) } else { "unknown".into() });
    r += "Pads\n----\n";
    for key in ["pads", "pad_properties"] {
        for e in s[key].as_array().into_iter().flatten() {
            r += &format!("- {} {}\n", e["title"].as_str().unwrap_or(""), e["qty"]);
        }
    }
    r += "\nVias\n----\n";
    for e in s["vias"].as_array().into_iter().flatten() {
        r += &format!("- {} {}\n", e["title"].as_str().unwrap_or(""), e["qty"]);
    }
    r += "\nComponents\n----------\n\n";
    let mut rows = vec![vec![String::new(), "Front Side".into(), "Back Side".into(), "Total".into()]];
    let (mut ft, mut bt) = (0, 0);
    for e in s["footprints"].as_array().into_iter().flatten() {
        let (fr, bk) = (e["front"].as_i64().unwrap_or(0), e["back"].as_i64().unwrap_or(0));
        rows.push(vec![e["title"].as_str().unwrap_or("").to_string(), fr.to_string(), bk.to_string(), (fr + bk).to_string()]);
        ft += fr;
        bt += bk;
    }
    rows.push(vec!["Total:".into(), ft.to_string(), bt.to_string(), (ft + bt).to_string()]);
    append_table(&rows, true, &mut r);
    r += "\nDrill holes\n-----------\n\n";
    let mut rows = vec![["Count", "Shape", "X Size", "Y Size", "Plated", "Via/Pad", "Start Layer", "Stop Layer"].iter().map(|s| s.to_string()).collect::<Vec<_>>()];
    for d in s["drills"].as_array().into_iter().flatten() {
        let layer = |k: &str| d[k].as_str().map(str::to_string).unwrap_or_else(|| "N/A".into());
        rows.push(vec![
            d["qty"].to_string(),
            if d["shape"] == "slot" { "Slot" } else { "Round" }.to_string(),
            message_text_from_value(d["x_um"].as_f64().unwrap_or(0.0), units, true, DataType::Distance),
            message_text_from_value(d["y_um"].as_f64().unwrap_or(0.0), units, true, DataType::Distance),
            if d["plated"].as_bool().unwrap_or(false) { "PTH" } else { "NPTH" }.to_string(),
            if d["is_pad"].as_bool().unwrap_or(false) { "Pad" } else { "Via" }.to_string(),
            layer("start_layer"),
            layer("stop_layer"),
        ]);
    }
    append_table(&rows, false, &mut r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Provenance, RoutingSection};

    fn design_with_outline(outline: Vec<Point>) -> Design {
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            routing: None,
            placement: Some(PlacementSection { outline, footprints: Vec::new(), modules: Vec::new() }),
            drawings: None,
        }
    }

    fn rect(w: i64, h: i64) -> Vec<Point> {
        vec![Point { x: 0, y: 0 }, Point { x: w, y: 0 }, Point { x: w, y: h }, Point { x: 0, y: h }]
    }

    fn routing(mut v: Value) -> RoutingSection {
        for k in ["tracks", "vias"] {
            if v.get(k).is_none() {
                v[k] = json!([]);
            }
        }
        serde_json::from_value(v).unwrap()
    }

    fn model() -> ConstraintModel {
        let mut m = ConstraintModel::default();
        m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
        m
    }

    #[test]
    fn reports_board_dimensions_and_area_from_the_outline() {
        let design = design_with_outline(rect(100_000, 50_000));
        let stats = compute_from(&design, &model(), Options::default());
        assert_eq!(stats["ok"], true);
        assert_eq!(stats["board"]["has_outline"], true);
        assert_eq!(stats["board"]["width_um"], 100_000);
        assert_eq!(stats["board"]["height_um"], 50_000);
        assert_eq!(stats["board"]["area_um2"], 5_000_000_000.0);
        // No stackup: `GetStackupOrDefault` is 1.6 mm.
        assert_eq!(stats["board"]["thickness_um"], 1600.0);
    }

    #[test]
    fn unset_minimums_keep_sources_int_max_sentinel() {
        let design = design_with_outline(vec![]);
        let stats = compute_from(&design, &model(), Options::default());
        assert_eq!(stats["board"]["has_outline"], false);
        assert_eq!(stats["board"]["front_density_pct"], 0.0);
        assert_eq!(stats["board"]["min_clearance_um"], INT_MAX_UM);
        let r = format_report(&stats, "mm", "d", "p", "b");
        assert!(r.contains("- Min track clearance: 2147.4836 mm\n"), "{r}");
        assert!(r.contains("- Dimensions: unknown\n- Area: unknown\n"));
    }

    #[test]
    fn clearance_walks_tracks_and_vias_on_the_same_layer_with_different_nets() {
        let mut design = design_with_outline(rect(100_000, 100_000));
        design.routing = Some(routing(json!({
            "tracks": [
                { "net": "A", "layer": "F.Cu", "width": 200, "pts": [{"x": 0, "y": 0}, {"x": 10_000, "y": 0}] },
                { "net": "B", "layer": "F.Cu", "width": 200, "pts": [{"x": 0, "y": 1_000}, {"x": 10_000, "y": 1_000}] },
                { "net": "C", "layer": "B.Cu", "width": 100, "pts": [{"x": 0, "y": 300}, {"x": 10_000, "y": 300}] }
            ],
            "vias": [ { "net": "D", "at": {"x": 5_000, "y": 600}, "drill": 300, "diameter": 600, "from_layer": "F.Cu", "to_layer": "B.Cu" } ]
        })));
        let stats = compute_from(&design, &model(), Options::default());
        // Via (r=300 at y=600) vs track A (edge y=100): 600-300-100 = 200;
        // vs track B (edge y=900): 900-600-300... = 0 -> they overlap? 1000-100-600-300 = 0.
        assert_eq!(stats["board"]["min_clearance_um"], 0.0);
        assert_eq!(stats["board"]["min_track_width_um"], 100.0);
        assert_eq!(stats["vias"][0]["qty"], 1);
        assert_eq!(stats["board"]["min_drill_um"], 300.0);
        assert_eq!(stats["drills"][0]["start_layer"], "F.Cu");
        assert_eq!(stats["drills"][0]["is_pad"], false);
    }

    #[test]
    fn track_only_clearance_ignores_same_net_and_other_layers() {
        let mut design = design_with_outline(rect(100_000, 100_000));
        design.routing = Some(routing(json!({
            "tracks": [
                { "net": "A", "layer": "F.Cu", "width": 200, "pts": [{"x": 0, "y": 0}, {"x": 10_000, "y": 0}] },
                { "net": "A", "layer": "F.Cu", "width": 200, "pts": [{"x": 0, "y": 250}, {"x": 10_000, "y": 250}] },
                { "net": "B", "layer": "B.Cu", "width": 200, "pts": [{"x": 0, "y": 250}, {"x": 10_000, "y": 250}] },
                { "net": "C", "layer": "F.Cu", "width": 200, "pts": [{"x": 0, "y": 1_000}, {"x": 10_000, "y": 1_000}] }
            ]
        })));
        let stats = compute_from(&design, &model(), Options::default());
        // Nearest different-net same-layer pair: second A (y=250) vs C (y=1000): 750 - 200 = 550.
        assert_eq!(stats["board"]["min_clearance_um"], 550.0);
    }

    #[test]
    fn copper_area_double_counts_overlaps_unless_holes_are_subtracted() {
        let mut design = design_with_outline(rect(100_000, 100_000));
        design.routing = Some(routing(json!({
            "tracks": [
                { "net": "A", "layer": "F.Cu", "width": 1000, "pts": [{"x": 0, "y": 0}, {"x": 10_000, "y": 0}] },
                { "net": "A", "layer": "F.Cu", "width": 1000, "pts": [{"x": 0, "y": 0}, {"x": 10_000, "y": 0}] }
            ]
        })));
        let plain = compute_from(&design, &model(), Options::default());
        let merged = compute_from(&design, &model(), Options { subtract_holes_from_copper_areas: true, ..Default::default() });
        let (p, m) = (plain["board"]["front_copper_area_um2"].as_f64().unwrap(), merged["board"]["front_copper_area_um2"].as_f64().unwrap());
        // One stadium is ~10000*1000 + pi*500^2 ~= 10.785e6 um^2.
        assert!((m - 10.785e6).abs() < 0.06e6, "{m}"); // arcs polygonised inside the true circle (ERROR_INSIDE)
        assert!((p - 2.0 * m).abs() < 0.02e6, "{p} vs {m}");
        assert_eq!(plain["board"]["back_copper_area_um2"], 0.0);
    }

    #[test]
    fn board_area_subtracts_via_holes_when_asked() {
        let mut design = design_with_outline(rect(10_000, 10_000));
        design.routing = Some(routing(json!({
            "vias": [ { "net": "D", "at": {"x": 5_000, "y": 5_000}, "drill": 1000, "diameter": 2000, "from_layer": "F.Cu", "to_layer": "B.Cu" } ]
        })));
        let stats = compute_from(&design, &model(), Options { subtract_holes_from_board_area: true, ..Default::default() });
        let expect = 1e8 - std::f64::consts::PI * 0.25 * 1e6;
        assert!((stats["board"]["area_um2"].as_f64().unwrap() - expect).abs() < 1e-3);
    }

    #[test]
    fn drills_group_and_sort_by_count_descending_stably() {
        let mut design = design_with_outline(rect(100_000, 100_000));
        design.routing = Some(routing(json!({
            "vias": [
                { "net": "A", "at": {"x": 0, "y": 0}, "drill": 400, "diameter": 800, "from_layer": "F.Cu", "to_layer": "B.Cu" },
                { "net": "A", "at": {"x": 9000, "y": 0}, "drill": 300, "diameter": 600, "from_layer": "F.Cu", "to_layer": "B.Cu" },
                { "net": "A", "at": {"x": 19000, "y": 0}, "drill": 300, "diameter": 600, "from_layer": "B.Cu", "to_layer": "F.Cu" }
            ]
        })));
        let stats = compute_from(&design, &model(), Options::default());
        let d = stats["drills"].as_array().unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0]["x_um"].as_i64(), d[0]["qty"].as_i64()), (Some(300), Some(2)));
        assert_eq!((d[1]["x_um"].as_i64(), d[1]["qty"].as_i64()), (Some(400), Some(1)));
    }

    #[test]
    fn message_text_from_value_matches_kicad_formats() {
        assert_eq!(message_text_from_value(1600.0, "mm", true, DataType::Distance), "1.6000 mm");
        assert_eq!(message_text_from_value(254.0, "mil", true, DataType::Distance), "10.00 mils");
        assert_eq!(message_text_from_value(25_400.0, "in", true, DataType::Distance), "1.0000 in");
        // Area: short form, trailing zero of 3 decimals trimmed for mm.
        assert_eq!(message_text_from_value(5e9, "mm", true, DataType::Area), "5000.00 mm²");
        assert_eq!(message_text_from_value(1234.5e6, "mm", true, DataType::Area), "1234.50 mm²");
        assert_eq!(message_text_from_value(1234.567e6, "mm", true, DataType::Area), "1234.567 mm²");
        // A tiny non-zero value switches to %.3e.
        assert_eq!(message_text_from_value(0.01, "mm", false, DataType::Distance), "1.000e-05");
    }

    #[test]
    fn append_table_matches_sources_layout() {
        let rows = vec![
            vec!["".to_string(), "Front Side".into(), "Back Side".into(), "Total".into()],
            vec!["THT:".to_string(), "1".into(), "0".into(), "1".into()],
            vec!["Total:".to_string(), "12".into(), "0".into(), "12".into()],
        ];
        let mut out = String::new();
        append_table(&rows, true, &mut out);
        assert_eq!(
            out,
            "|        | Front Side | Back Side | Total |\n\
             |--------|------------|-----------|-------|\n\
             |THT:    |          1 |         0 |     1 |\n\
             |Total:  |         12 |         0 |    12 |\n"
        );
    }

    #[test]
    fn report_has_sources_sections_in_order() {
        let design = design_with_outline(rect(100_000, 50_000));
        let stats = compute_from(&design, &model(), Options::default());
        let r = format_report(&stats, "mm", "today", "proj", "board");
        let order = ["PCB statistics report\n=====================\n- Date: today\n- Project: proj\n- Board name: board\n\nBoard\n-----\n- Width: 100.0000 mm\n- Height: 50.0000 mm\n- Area: 5000.00 mm²\n", "Pads\n----\n- Through hole: 0\n", "\nVias\n----\n- Through vias: 0\n", "\nComponents\n----------\n\n", "\nDrill holes\n-----------\n\n| Count |"];
        let mut at = 0;
        for o in order {
            let i = r[at..].find(o).unwrap_or_else(|| panic!("missing {o:?} in\n{r}"));
            at += i + o.len();
        }
    }

    #[test]
    fn polygon_area_is_winding_independent() {
        let cw = [Point { x: 0, y: 0 }, Point { x: 0, y: 5_000 }, Point { x: 10_000, y: 5_000 }, Point { x: 10_000, y: 0 }];
        assert_eq!(polygon_area(&cw), 50_000_000.0);
        assert_eq!(polygon_area(&cw[..2]), 0.0);
    }
}
