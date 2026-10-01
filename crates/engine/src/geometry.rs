//! Shared symbol-box geometry: node sizing and port placement.
//!
//! Used by `derive_schematic` to build the layout graph, and by `eda-render`
//! to recompute the same box sizes and pin positions when drawing a
//! `Design` — keeping the two in lock-step without duplicating the rules.

use eda_layout::{Node, Point, Port, Side};
use eda_model::ir::Wire;
use eda_model::{Part, PinKind};

/// Every point where >=3 same-net wire-segment endpoints coincide — a real
/// electrical junction (several wires sharing an endpoint, or a >2-way star
/// hub), as opposed to a pass-through vertex inside a single polyline
/// (which only ever touches 2 segments). Shared by `eda-render`
/// (`render_junctions`, which draws a dot at each) and `eda-gates`
/// (`schematic_missing_junction`, which fails if the rendered set of dots
/// would ever disagree with this) so the two can never drift apart on what
/// counts as a junction.
pub fn wire_junction_points(wires: &[Wire]) -> std::collections::BTreeSet<(String, eda_model::ir::Point)> {
    let mut touches: std::collections::BTreeMap<(String, eda_model::ir::Point), usize> = std::collections::BTreeMap::new();
    for w in wires {
        if w.pts.len() < 2 {
            continue;
        }
        for pair in w.pts.windows(2) {
            for pt in [pair[0], pair[1]] {
                *touches.entry((w.net.clone(), pt)).or_insert(0) += 1;
            }
        }
    }
    let mut out: std::collections::BTreeSet<(String, eda_model::ir::Point)> =
        touches.into_iter().filter(|(_, n)| *n >= 3).map(|(k, _)| k).collect();

    // A "T" meeting: one wire's endpoint lands, mid-span, on a same-net
    // wire's segment interior, rather than on a shared vertex — the routed
    // stub simply stops on top of the through-wire instead of both wires
    // sharing a bend point there. The coincident-vertex counting above
    // can't see this (the through-wire never has that point in its own
    // `pts`, so it never contributes a touch), but it's exactly as much a
    // real junction electrically, so it must get a dot too.
    for w in wires {
        if w.pts.is_empty() {
            continue;
        }
        for &p in [w.pts[0], *w.pts.last().unwrap()].iter() {
            for other in wires {
                if other.net != w.net || std::ptr::eq(other, w) {
                    continue;
                }
                if other.pts.contains(&p) {
                    continue; // shared vertex, already counted above
                }
                if other.pts.windows(2).any(|seg| point_on_segment_interior(p, seg[0], seg[1])) {
                    out.insert((w.net.clone(), p));
                }
            }
        }
    }
    out
}

/// True if `p` lies strictly inside the orthogonal segment `a`-`b`
/// (excludes the segment's own endpoints, which are handled by the
/// coincident-vertex counting in [`wire_junction_points`]).
fn point_on_segment_interior(p: eda_model::ir::Point, a: eda_model::ir::Point, b: eda_model::ir::Point) -> bool {
    if a.x == b.x {
        p.x == a.x && p.y > a.y.min(b.y) && p.y < a.y.max(b.y)
    } else if a.y == b.y {
        p.y == a.y && p.x > a.x.min(b.x) && p.x < a.x.max(b.x)
    } else {
        false
    }
}

/// Grid used by the layout engine; ports and node sizes are chosen as
/// multiples of this so everything lands on-grid.
pub const GRID: i64 = eda_layout::DEFAULT_GRID; // 1270 um

/// Pin-stub length (um): re-exported from eda-layout, the canonical source,
/// so callers don't need to know which crate actually owns the constant.
pub const STUB: i64 = eda_layout::graph::STUB_LEN;

/// Absolute location of the tip of `node`'s `port_idx`-th pin stub, given
/// its top-left corner. Thin wrapper around [`eda_layout::Node::stub_tip`]
/// — the one canonical implementation — kept here so engine/render/gates
/// code can call through `geometry::stub_tip` without reaching into
/// eda-layout directly.
pub fn stub_tip(node: &Node, top_left: Point, port_idx: usize) -> Point {
    node.stub_tip(top_left, port_idx)
}

pub const BASE_WIDTH: i64 = 10_160; // 10.16mm, 8 * GRID
pub const BASE_HEIGHT: i64 = 7_620; // 7.62mm, 6 * GRID
pub const HEIGHT_STEP: i64 = 2_540; // 2.54mm per pair of pins beyond 4

/// Box size (width, height) for `part`.
///
/// Height only ever depends on pin count (unchanged from the original
/// heuristic): `BASE_HEIGHT` plus one `HEIGHT_STEP` per pair of pins beyond
/// 4, giving west/east ports (and north/south stub rows) enough room that a
/// pin-name text box (one `PIN_FONT_MM`-tall line) never collides with its
/// neighbor's row.
///
/// Width starts from `BASE_WIDTH` but grows to fit whichever is worse:
/// - **north/south crowding**: `build_ports` spaces same-side pins evenly
///   across the box width (`distribute_offsets`), so a wide pin name
///   (`VDDA`, `GND2`, ...) on a busy top/bottom row can run into its
///   neighbor's name even though the row itself has plenty of pins — the
///   `VDDVDDA/DC`-style merge this whole gate/render pass exists to catch.
///   Widened so every adjacent same-side pair's pitch clears both names'
///   half-widths plus a full text-height gap.
/// - **west/east reach**: a long west pin name and a long east pin name on
///   the same row both grow inward from their own edge (`PIN_TEXT_MARGIN_MM`
///   in), and on a narrow box they meet in the middle. Widened so the two
///   longest west/east names, plus their margins and a text-height gap
///   between them, both fit without the box getting narrower than they need.
///
/// Only `eda-gates`/`eda-render` call this (see module doc), so `eda-layout`
/// (never touched by this pass) only ever sees the resulting `(width,
/// height)` as opaque `Node` dimensions — it has no idea *why* a box is this
/// size, only that it is.
/// The box edges (library frame: mm, +y **up**) a resolved real library
/// symbol implies -- the real-geometry replacement for the synthetic
/// system's own procedurally-sized box, consumed by both
/// [`build_ports_from_real_symbol`] (as the reference corner each pin's
/// `Port::offset` is measured from) and [`node_size`] (as the box to pack).
///
/// A side with at least one real pin takes its edge from *that pin's own
/// electrical point* (`LibPin::at`), offset by this project's fixed
/// [`eda_layout::graph::STUB_LEN`] -- **not** from the body graphics, and
/// not from the pin's own `length_mm` back toward the body. This matters
/// because `eda_layout::Node::stub_tip` (what a wire actually terminates
/// at) always extends a `Port` exactly `STUB_LEN` past this box's own
/// edge, by a fixed amount every other part of this workspace already
/// assumes; a real pin's own `length_mm` is routinely something else
/// entirely (`Device:C`'s own plates sit 3.3mm from its pins, not the
/// 1.27mm `STUB_LEN` a synthetic part's stub always is). Deriving the edge
/// from "pin point minus the fixed stub", instead of from the body or the
/// pin's own real length, is what makes `stub_tip` land exactly back on
/// `at` regardless of the real symbol's own geometry -- verified against
/// `Device:C` (plates at +-0.508mm, pins at +-3.81mm/length 3.302mm): the
/// naive graphics-plus-pins bbox this function used to return put the
/// edge at the plates' own +-1.524mm, 1.27mm off the schematic grid and
/// nowhere near either the plates or a stub-reachable point; this does
/// neither.
///
/// A side with no real pin at all (rare -- every part this project places
/// has at least one real electrical connection somewhere) falls back to
/// the body graphics' own extent on that side, since there is no pin
/// position to anchor a stub-reachable edge to.
pub fn real_symbol_bbox(sym: &eda_model::symbol::LibSymbol) -> (f64, f64, f64, f64) {
    use eda_model::symbol::SymbolGraphic;
    let (mut gx0, mut gy0, mut gx1, mut gy1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut feed = |x: f64, y: f64| {
        gx0 = gx0.min(x);
        gy0 = gy0.min(y);
        gx1 = gx1.max(x);
        gy1 = gy1.max(y);
    };
    for g in &sym.graphics {
        match g {
            SymbolGraphic::Rectangle { start, end, .. } => {
                feed(start.x, start.y);
                feed(end.x, end.y);
            }
            SymbolGraphic::Polyline { pts, .. } => {
                for p in pts {
                    feed(p.x, p.y);
                }
            }
            SymbolGraphic::Circle { center, radius_mm, .. } => {
                feed(center.x - radius_mm, center.y - radius_mm);
                feed(center.x + radius_mm, center.y + radius_mm);
            }
            SymbolGraphic::Arc { start, mid, end, .. } => {
                feed(start.x, start.y);
                feed(mid.x, mid.y);
                feed(end.x, end.y);
            }
            SymbolGraphic::Text { at, .. } => feed(at.x, at.y),
        }
    }
    if !gx0.is_finite() {
        // No graphics at all (power symbols, mostly): nothing to fall
        // back to, but stay finite rather than propagate infinity into a
        // side with no pins either (shouldn't happen in practice).
        gx0 = 0.0;
        gy0 = 0.0;
        gx1 = 0.0;
        gy1 = 0.0;
    }

    let stub_mm = STUB as f64 / 1000.0;
    let (mut left, mut right, mut bottom, mut top): (Option<f64>, Option<f64>, Option<f64>, Option<f64>) = (None, None, None, None);
    for pin in &sym.pins {
        match side_from_pin_angle(pin.angle_deg) {
            // Same-side pins share one perpendicular coordinate in every
            // real symbol this project places (that is what "being on the
            // same side" means visually); where it ever doesn't, picking
            // any one of them is no more wrong than any other, so `min`/
            // `max` here is just a deterministic choice, not a hedge.
            Side::Left => left = Some(left.map_or(pin.at.x + stub_mm, |v: f64| v.max(pin.at.x + stub_mm))),
            Side::Right => right = Some(right.map_or(pin.at.x - stub_mm, |v: f64| v.min(pin.at.x - stub_mm))),
            Side::Bottom => bottom = Some(bottom.map_or(pin.at.y + stub_mm, |v: f64| v.max(pin.at.y + stub_mm))),
            Side::Top => top = Some(top.map_or(pin.at.y - stub_mm, |v: f64| v.min(pin.at.y - stub_mm))),
        }
    }
    let x0 = left.unwrap_or(gx0);
    let x1 = right.unwrap_or(gx1);
    let y0 = bottom.unwrap_or(gy0);
    let y1 = top.unwrap_or(gy1);

    // Snap outward to the schematic grid. A real library's own *pins* are
    // always grid-aligned from the symbol's origin (true of every one of
    // this project's own built-ins, Device:R/C/L/D/LED and the parametric
    // connector, and of real KiCad libraries by long-standing convention)
    // and `STUB_LEN` itself is exactly one grid cell, so a pin-derived
    // edge is already grid-aligned; snapping only ever matters for a
    // graphics-derived fallback edge, and is harmless (a no-op) otherwise.
    let grid_mm = GRID as f64 / 1000.0;
    let x0 = (x0 / grid_mm).floor() * grid_mm;
    let y0 = (y0 / grid_mm).floor() * grid_mm;
    let x1 = (x1 / grid_mm).ceil() * grid_mm;
    let y1 = (y1 / grid_mm).ceil() * grid_mm;
    (x0, y0, x1, y1)
}

/// Which side a real pin's own stub attaches to, from its KiCad
/// `angle_deg` (0/90/180/270: the direction from the pin's outer point
/// *toward* the symbol body, in the library's own +y-**up** frame).
/// Negating the Y component reads the same direction in sheet space (+y
/// **down**), and the surviving axis says which edge the pin's outer point
/// sits beyond: pointing down (toward the body) means the outer point is
/// above the body, i.e. a Top pin; pointing right means a Left pin; and so
/// on. Verified against `Device:R` (pin 1 at library `(0, 3.81)`, angle
/// 270 -- library-south, i.e. sheet-down, toward the body below it -- is
/// exactly the Top pin the synthetic layout would also give a 2-pin
/// vertical passive).
fn side_from_pin_angle(angle_deg: f64) -> Side {
    match ((angle_deg.rem_euclid(360.0) / 90.0).round() as i64).rem_euclid(4) {
        0 => Side::Left,
        1 => Side::Bottom,
        2 => Side::Right,
        _ => Side::Top,
    }
}

/// Box size (width, height) for `part`: when `resolved` is a real library
/// symbol, its own bounding box ([`real_symbol_bbox`]), rounded up to
/// [`GRID`] purely for layout packing (wires still terminate at each pin's
/// own exact, un-rounded point -- see [`build_ports`] -- so rounding this
/// box can never pull a wire off its pin). Otherwise the synthetic,
/// procedurally-sized box below.
pub fn node_size(part: &Part, resolved: Option<&eda_model::symbol::LibSymbol>) -> (i64, i64) {
    if let Some(sym) = resolved {
        let (x0, y0, x1, y1) = real_symbol_bbox(sym);
        let width_um = ((x1 - x0).max(0.0) * 1000.0).round() as i64;
        let height_um = ((y1 - y0).max(0.0) * 1000.0).round() as i64;
        let width = ((width_um.max(1) + GRID - 1) / GRID) * GRID;
        let height = ((height_um.max(1) + GRID - 1) / GRID) * GRID;
        // NOT floored to the old synthetic minimum, deliberately: a real
        // symbol's box edges on a side with a real pin are load-bearing
        // for `Node::stub_tip` (its Bottom/Right ports measure from
        // `top_left + (width or height)`, so inflating either would move
        // a bottom/right pin's computed stub tip away from where
        // `baked_real_point` actually draws that same pin, breaking
        // connectivity exactly the way the original Y-axis bug this task
        // already fixed once did). Ref/value text placement's own need
        // for a less cramped box (`ref_slot_local`/`value_slot_local`) is
        // handled there instead, without touching the box real geometry
        // everything else relies on.
        return (width, height);
    }
    let pin_count = part.pins.len();
    let extra = pin_count.saturating_sub(4);
    let pairs = extra.div_ceil(2);
    let height = BASE_HEIGHT + (pairs as i64) * HEIGHT_STEP;

    // Pin->side classification never depends on the box's own width (only
    // on pin kind/name — see `build_ports`'s doc comment), so a throwaway
    // `BASE_WIDTH` first pass safely tells us which pins land on which side
    // before we know the final width.
    let (ports, pin_port) = build_ports(part, BASE_WIDTH, height, None);
    let mut north_names: Vec<&str> = Vec::new();
    let mut south_names: Vec<&str> = Vec::new();
    let mut west_names: Vec<&str> = Vec::new();
    let mut east_names: Vec<&str> = Vec::new();
    for (i, port_idx) in pin_port.iter().enumerate() {
        let Some(pi) = port_idx else { continue };
        let pin = &part.pins[i];
        let name = pin.name.as_deref().unwrap_or(&pin.number);
        match ports[*pi].side {
            Side::Top => north_names.push(name),
            Side::Bottom => south_names.push(name),
            Side::Left => west_names.push(name),
            Side::Right => east_names.push(name),
        }
    }

    let name_width_mm = |s: &str| s.chars().count() as f64 * CHAR_WIDTH_FACTOR * PIN_FONT_MM;

    // Minimum box width so evenly-spaced same-side names never crowd:
    // pitch = width/(n+1) must clear each adjacent pair's half-widths plus
    // one text-height gap, for the tightest (worst) adjacent pair.
    let width_for_row = |names: &[&str]| -> f64 {
        if names.len() < 2 {
            return 0.0;
        }
        let worst_pitch = names
            .windows(2)
            .map(|w| (name_width_mm(w[0]) + name_width_mm(w[1])) / 2.0 + PIN_FONT_MM)
            .fold(0.0_f64, f64::max);
        worst_pitch * (names.len() as f64 + 1.0)
    };
    let width_from_rows = width_for_row(&north_names).max(width_for_row(&south_names));

    // Minimum box width so the longest west name and longest east name
    // (each growing inward from its own edge by `PIN_TEXT_MARGIN_MM`) don't
    // meet in the middle, with one text-height gap held clear between them.
    let longest = |names: &[&str]| -> f64 { names.iter().map(|n| name_width_mm(n)).fold(0.0_f64, f64::max) };
    let west_reach = if west_names.is_empty() { 0.0 } else { longest(&west_names) + PIN_TEXT_MARGIN_MM };
    let east_reach = if east_names.is_empty() { 0.0 } else { longest(&east_names) + PIN_TEXT_MARGIN_MM };
    let width_from_reach = if west_names.is_empty() || east_names.is_empty() { 0.0 } else { west_reach + east_reach + PIN_FONT_MM };

    let width_mm = (BASE_WIDTH as f64 / 1000.0).max(width_from_rows).max(width_from_reach);
    let width_um = (width_mm * 1000.0).ceil() as i64;
    let width = ((width_um + GRID - 1) / GRID) * GRID;

    (width, height)
}

/// True for pin names that read as a "control-ish" input to an IC (enable,
/// chip-select, reset...): `source_control_pin_unconnected` in `eda-intent`
/// uses the same pattern list (kept in lockstep by inspection/tests since
/// `eda-intent` cannot depend on this crate without a cycle).
/// True for a part that `eda-render` draws as a 2-pin passive glyph
/// (resistor/capacitor/inductor/diode) rather than a generic IC box: exactly
/// two pins, and reference/package/value starts with R/C/L/D. Shared between
/// `eda-render` (glyph choice) and `eda-gates` (text-placement gates must
/// agree on which parts get the tighter passive anchoring) so the two never
/// drift apart on what counts as a passive.
pub fn is_two_pin_passive(part: &Part) -> bool {
    if part.pins.len() != 2 {
        return false;
    }
    let candidates = [Some(part.reference.as_str()), part.package.as_deref(), part.value.as_deref()];
    for c in candidates.into_iter().flatten() {
        let Some(first) = c.trim_start_matches(['+', '-']).chars().next() else { continue };
        if matches!(first.to_ascii_uppercase(), 'R' | 'C' | 'L' | 'D') {
            return true;
        }
    }
    false
}

pub fn is_control_pin_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["EN", "CE", "SHDN", "RESET", "RST", "CS"].iter().any(|p| n.contains(p))
}

fn is_ground_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("GND") || n.contains("VSS") || n.contains("AGND")
}

/// True for a net name that reads as power/ground rail (GND*/AGND*/VSS*, or
/// V*/+*/VCC/VDD/VIN/VOUT/"NV"-style rail names like `3V3`/`5V0`) — mirrors
/// `eda-render`'s `label_kind` glyph-selection rule (Ground or Power, not
/// Flag) so `eda-engine` (which nets get wired vs. flagged) and
/// `eda-render`/`eda-gates` (which glyph is drawn / which wire lengths are
/// gated) always agree on what counts as a power/ground net.
pub fn is_power_or_ground_net_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let trimmed = upper.trim_start_matches('+');
    const RAILS: &[&str] = &[
        "GND", "AGND", "DGND", "VSS", "VCC", "VDD", "VDDA", "VBAT", "VBUS", "VSYS", "3V3", "5V", "1V8", "12V",
    ];
    RAILS.iter().any(|r| *r == trimmed)
        || trimmed.starts_with("GND")
        || trimmed.starts_with("AGND")
        || trimmed.starts_with("DGND")
        || trimmed.starts_with("VSS")
}

/// Builds the port list for a part and returns the per-pin-index ->
/// port-index mapping (`None` for NC pins).
///
/// Side assignment is name+kind driven (not kind-alone): a `Ground`-kind
/// pin, or any pin whose name reads as GND/VSS/AGND, goes South. A
/// `Power`-kind pin named like an input (`*VIN*`/`*IN*`) goes West, one
/// named like an output (`*VOUT*`/`*OUT*`) goes East, and any other
/// `Power`-kind pin (a bare rail: VCC/VDD/V+) goes North. Signal/Passive
/// pins follow the same IN/OUT name convention, then control-ish names
/// (EN/CE/SHDN/RESET/CS) go West (below the named inputs); anything left
/// unmatched (e.g. numbered passive leads with no directional name) falls
/// back to an even West/East split, preserving the original layout for
/// simple 2-pin passives.
pub fn build_ports(part: &Part, width: i64, height: i64, resolved: Option<&eda_model::symbol::LibSymbol>) -> (Vec<Port>, Vec<Option<usize>>) {
    if let Some(sym) = resolved {
        return build_ports_from_real_symbol(part, sym);
    }
    let mut north = Vec::new();
    let mut south = Vec::new();
    let mut west_named = Vec::new();
    let mut west_ctrl = Vec::new();
    let mut east = Vec::new();
    let mut sigpass_fallback = Vec::new();

    for (i, pin) in part.pins.iter().enumerate() {
        let name = pin.name.clone().unwrap_or_default();
        match pin.kind {
            PinKind::Nc => {}
            PinKind::Ground => south.push(i),
            PinKind::Power => {
                if is_ground_name(&name) {
                    south.push(i);
                } else if name.to_ascii_uppercase().contains("IN") {
                    west_named.push(i);
                } else if name.to_ascii_uppercase().contains("OUT") {
                    east.push(i);
                } else {
                    north.push(i);
                }
            }
            PinKind::Signal | PinKind::Passive => {
                let upper = name.to_ascii_uppercase();
                if is_ground_name(&name) {
                    south.push(i);
                } else if upper.contains("OUT") {
                    east.push(i);
                } else if upper.contains("IN") {
                    west_named.push(i);
                } else if is_control_pin_name(&name) {
                    west_ctrl.push(i);
                } else {
                    sigpass_fallback.push(i);
                }
            }
        }
    }

    // Fallback (unnamed/generic) sigpass pins split evenly west/east, same
    // as the original behavior, so simple 2-pin passives are unaffected.
    let west_count = sigpass_fallback.len() / 2;
    let (fb_west, fb_east) = sigpass_fallback.split_at(west_count);

    let mut west = Vec::new();
    west.extend_from_slice(&west_named); // inputs first
    west.extend_from_slice(&west_ctrl); // control pins below inputs
    west.extend_from_slice(fb_west);
    let mut east_all = east.clone();
    east_all.extend_from_slice(fb_east);

    let north_offsets = distribute_offsets(north.len(), width);
    let south_offsets = distribute_offsets(south.len(), width);
    let west_offsets = distribute_offsets(west.len(), height);
    let east_offsets = distribute_offsets(east_all.len(), height);

    let mut ports = Vec::new();
    let mut pin_port = vec![None; part.pins.len()];

    for (k, &pin_i) in north.iter().enumerate() {
        ports.push(Port { side: Side::Top, offset: north_offsets[k] });
        pin_port[pin_i] = Some(ports.len() - 1);
    }
    for (k, &pin_i) in south.iter().enumerate() {
        ports.push(Port { side: Side::Bottom, offset: south_offsets[k] });
        pin_port[pin_i] = Some(ports.len() - 1);
    }
    for (k, &pin_i) in west.iter().enumerate() {
        ports.push(Port { side: Side::Left, offset: west_offsets[k] });
        pin_port[pin_i] = Some(ports.len() - 1);
    }
    for (k, &pin_i) in east_all.iter().enumerate() {
        ports.push(Port { side: Side::Right, offset: east_offsets[k] });
        pin_port[pin_i] = Some(ports.len() - 1);
    }

    (ports, pin_port)
}

/// [`build_ports`]'s real-symbol path: one port per non-`Nc` pin, placed at
/// its own real body-attachment point (`LibPin::at` moved `length_mm`
/// toward the body along `angle_deg`) rather than evenly distributed by
/// the synthetic heuristic -- a connector's pin 1 lands exactly where
/// `Device:R`'s/`Connector_Generic`'s own library data puts it, not
/// wherever an even split would. `part.pins` (our own intent order) is
/// matched to the real symbol's pins by number (`pin_by_number`), not by
/// list position, since a real library's own pin order need not match
/// ours; a pin number the real symbol doesn't have gets no port at all
/// (defensive -- real/intent data should always agree on pin numbers for
/// the same `lib_id`).
fn build_ports_from_real_symbol(part: &Part, sym: &eda_model::symbol::LibSymbol) -> (Vec<Port>, Vec<Option<usize>>) {
    let (x0, _y0, _x1, y1) = real_symbol_bbox(sym);
    struct Placed {
        pin_i: usize,
        side: Side,
        offset: i64,
    }
    let mut placed: Vec<Placed> = Vec::new();
    for (i, pin) in part.pins.iter().enumerate() {
        if pin.kind == PinKind::Nc {
            continue; // no port; positioned via `nc_pin_local_points` instead
        }
        let Some(real_pin) = sym.pin_by_number(&pin.number) else { continue };
        let side = side_from_pin_angle(real_pin.angle_deg);
        let theta = real_pin.angle_deg.to_radians();
        let body_x = real_pin.at.x + real_pin.length_mm * theta.cos();
        let body_y = real_pin.at.y + real_pin.length_mm * theta.sin();
        // Top/Bottom offsets run left-to-right (library +x, unaffected by
        // the sheet's y-flip): distance from the box's own left edge.
        // Left/Right offsets run top-to-bottom in *sheet* space, which is
        // bottom-to-top in the library's +y-up frame: distance from the
        // box's library-frame top (`y1`, its largest y).
        let offset_mm = match side {
            Side::Top | Side::Bottom => body_x - x0,
            Side::Left | Side::Right => y1 - body_y,
        };
        let offset = ((offset_mm * 1000.0).round() as i64).max(0);
        placed.push(Placed { pin_i: i, side, offset });
    }

    let mut ports = Vec::new();
    let mut pin_port = vec![None; part.pins.len()];
    for want in [Side::Top, Side::Bottom, Side::Left, Side::Right] {
        let mut group: Vec<&Placed> = placed.iter().filter(|p| p.side == want).collect();
        group.sort_by_key(|p| p.offset);
        for p in group {
            ports.push(Port { side: p.side, offset: p.offset });
            pin_port[p.pin_i] = Some(ports.len() - 1);
        }
    }
    (ports, pin_port)
}

// ---------------------------------------------------------------- shared text-box estimation
//
// Used by both `eda-render` (to place ref/value/pin/net-label text with
// guaranteed clearance) and `eda-gates` (`schematic_text_overlap`) so the
// two always agree on what counts as an overlap.

// ---------------------------------------------------------------- reserved label slots
//
// Every symbol reserves two text slots along the *screen-vertical* axis:
// a refdes slot above the box and a value slot below it. Both are sized and
// positioned by the constants below and are handed to `eda-layout` (via
// `LayoutOptions::label_slot_above/below`) so that channel-row selection,
// `try_direct_path` and the maze repair all treat them as solid obstacles.
// `eda-render` draws the text at exactly these positions and `eda-gates`
// (`collect_text_boxes`) recomputes them with the same function, so the
// three crates can never disagree about where a label is.

pub const REF_FONT_MM: f64 = 1.6;
pub const VALUE_FONT_MM: f64 = 1.4;

/// True when `part`'s real library pins are both on the box's Top/Bottom
/// edges -- a vertically-drawn 2-pin device, which is how every passive this
/// project resolves to a real symbol (`Device:R`/`C`/`L`/`D`/LED) actually
/// draws. See [`ref_slot_local`]'s doc comment for why this matters.
fn is_vertical_two_pin(resolved: Option<&eda_model::symbol::LibSymbol>) -> bool {
    let Some(sym) = resolved else { return false };
    sym.pins.len() == 2 && sym.pins.iter().all(|p| matches!(side_from_pin_angle(p.angle_deg), Side::Top | Side::Bottom))
}

/// Baseline of the refdes slot, in the symbol's local mm space (box
/// top-left = 0,0). Left-aligned at the box's left edge.
///
/// The two-pin-passive mid-height formula (`h/2 - 1`) assumes its pins sit
/// to the sides (Left/Right) of that midpoint, clear of it -- true of the
/// synthetic layout's own even West/East split for an unresolved 2-pin
/// part, but not of a *real* vertical passive's pins, which real library
/// data (and `side_from_pin_angle`) puts on Top/Bottom instead: there, the
/// box is routinely too short (`Device:C`'s real height is 5.08mm) for any
/// mid-height y to clear both the Top pin's text (near the box's own top
/// edge) and the Bottom pin's (near its bottom edge) at once
/// (`schematic_text_overlap` on `CIN`/`COUT`'s own ref vs. pin "1" text).
/// Falling back to the IC/default placement (just above the box, `-0.3`)
/// for this case clears both -- confirmed against `Device:C`'s own 5.08mm
/// box, whose Top-pin text starts 0.92mm down, well clear of a `-0.3`-baseline
/// ref label's own bottom edge at 0.02mm.
pub fn ref_slot_local(part: &Part, height_um: i64, resolved: Option<&eda_model::symbol::LibSymbol>) -> (f64, f64) {
    let h = height_um as f64 / 1000.0;
    let h_for_text = h.max(BASE_HEIGHT as f64 / 1000.0);
    let y = if is_two_pin_passive(part) && !is_vertical_two_pin(resolved) { h_for_text / 2.0 - 1.0 } else { -0.3 };
    (0.0, y)
}

/// Baseline of the value slot, in the symbol's local mm space.
///
/// Left-aligned at the box's left edge and pushed below the south pin-stub
/// band (`STUB` = 1.27mm) so the slot is a clean horizontal band that no
/// stub, bus run, or escape leg has to share. The old placement was
/// `(width + 1.6, height + 1.8)` — i.e. *outside the box to the right*,
/// which put the text squarely in the inter-layer routing channel; that
/// single offset accounted for 14/14 of l3's and 29/30 of l4's
/// `schematic_label_over_wire` failures.
pub fn value_slot_local(_part: &Part, height_um: i64) -> (f64, f64) {
    let h = height_um as f64 / 1000.0;
    // Clear of, in order: the box, the south pin-stub band (`STUB`), and a
    // south net-label glyph's text (a GND flag's baseline sits 1.8mm below
    // the pin, so its box bottom is 2.08mm below it) — otherwise the value
    // and the rail label share the band and `schematic_text_overlap` fires.
    (0.0, h + 3.4)
}

/// Font size (mm) the value text is drawn at: `VALUE_FONT_MM`, shrunk just
/// enough that the run fits inside the symbol box's own width.
///
/// The reserved value slot is only as wide as the box (that is all
/// `eda-layout` can reserve — the perpendicular axis is the layer axis,
/// whose spacing is set by channel demand, not by text), so a long MPN set
/// at the nominal size would poke sideways out of the slot and straight
/// into the inter-layer routing channel. Shrinking rather than truncating
/// keeps the value readable and loses no information.
pub fn value_font_mm(text: &str, width_um: i64) -> f64 {
    let n = text.chars().count().max(1) as f64;
    let avail = width_um as f64 / 1000.0;
    (avail / (CHAR_WIDTH_FACTOR * n)).min(VALUE_FONT_MM).max(0.7)
}

/// Height (um) reserved above the box for the refdes slot.
pub const SLOT_ABOVE_UM: i64 = 2 * GRID; // 2.54mm
/// Height (um) reserved below the box for the value slot: enough to cover
/// the value baseline (`h + STUB + 1.6`) plus its descender (0.2 * font).
pub const SLOT_BELOW_UM: i64 = 3 * GRID; // 3.81mm

/// Horizontal-anchor convention, matching SVG `text-anchor`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HAnchor {
    Start,
    Middle,
    End,
}

/// An axis-aligned text bounding-box estimate, in mm.
#[derive(Clone, Copy, Debug)]
pub struct TextBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl TextBox {
    pub fn overlaps(&self, other: &TextBox) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }
}

/// Per-character width factor (fraction of font-size) used to estimate text
/// width: `0.6 * font_size_mm` per character, as specified.
pub const CHAR_WIDTH_FACTOR: f64 = 0.6;

/// Estimates a text run's bounding box in mm from its SVG anchor point,
/// baseline y, content, font size, and horizontal anchor mode. Vertical
/// extent is estimated as one font-size tall, split ~0.8 above / ~0.2 below
/// the baseline (typical latin ascent/descent).
pub fn text_bbox(anchor_x: f64, baseline_y: f64, text: &str, font_mm: f64, anchor: HAnchor) -> TextBox {
    let w = text.chars().count() as f64 * CHAR_WIDTH_FACTOR * font_mm;
    let (x0, x1) = match anchor {
        HAnchor::Start => (anchor_x, anchor_x + w),
        HAnchor::Middle => (anchor_x - w / 2.0, anchor_x + w / 2.0),
        HAnchor::End => (anchor_x - w, anchor_x),
    };
    TextBox { x0, y0: baseline_y - 0.8 * font_mm, x1, y1: baseline_y + 0.2 * font_mm }
}

/// Font size (mm) pin-name/number text is drawn at, shared by `eda-render`
/// (drawing) and `eda-gates` (`schematic_text_overlap`) so the two agree on
/// how much space a pin's text actually occupies.
pub const PIN_FONT_MM: f64 = 1.1;
/// How far inside the box a pin's name text sits from the port point, in mm
/// — shared by `eda-render` and `eda-gates` for the same reason.
pub const PIN_TEXT_MARGIN_MM: f64 = 0.8;

/// Offset (mm, relative to the port's on-box point) and horizontal anchor
/// for a pin's name/number text, keyed purely by which side of the box the
/// port sits on. The single source `eda-render` draws pin text from and
/// `eda-gates` re-derives it from, so a pin-name text box can never drift
/// between the two.
pub fn pin_text_offset(side: Side) -> (f64, f64, HAnchor) {
    match side {
        Side::Top => (0.0, PIN_TEXT_MARGIN_MM + 1.0, HAnchor::Middle),
        Side::Bottom => (0.0, -PIN_TEXT_MARGIN_MM, HAnchor::Middle),
        Side::Left => (PIN_TEXT_MARGIN_MM, 0.4, HAnchor::Start),
        Side::Right => (-PIN_TEXT_MARGIN_MM, 0.4, HAnchor::End),
    }
}

/// A port's on-box attach point, in the symbol's local (box-space) um
/// coordinates (box top-left = 0,0). Shared by `eda-render` (pin stub +
/// text placement) and `eda-gates` (`pin_text_box`) so the two can never
/// disagree on where a pin sits on the box.
pub fn port_local_point(port: &Port, width: i64, height: i64) -> (f64, f64) {
    match port.side {
        Side::Top => (port.offset as f64, 0.0),
        Side::Bottom => (port.offset as f64, height as f64),
        Side::Left => (0.0, port.offset as f64),
        Side::Right => (width as f64, port.offset as f64),
    }
}

/// The exact `TextBox` a pin's name/number text occupies, in absolute mm:
/// `port`'s local attach point (via [`port_local_point`]) translated by the
/// symbol's absolute top-left (mm), offset by [`pin_text_offset`], run
/// through [`text_bbox`] at [`PIN_FONT_MM`]. `eda-render` draws the glyph
/// here and `eda-gates` checks overlap here — one function, so the two can
/// never disagree on where a pin's text sits or how wide it is.
pub fn pin_text_box(port: &Port, width: i64, height: i64, sym_x_mm: f64, sym_y_mm: f64, label: &str) -> TextBox {
    let (lx, ly) = port_local_point(port, width, height);
    let (dx, dy, anchor) = pin_text_offset(port.side);
    let x = sym_x_mm + lx / 1000.0 + dx;
    let y = sym_y_mm + ly / 1000.0 + dy;
    text_bbox(x, y, label, PIN_FONT_MM, anchor)
}

/// True if the orthogonal wire segment (mm) passes through the interior of
/// the axis-aligned box (mm). Shared by `eda-render` (to steer a value label
/// away from a wire) and `eda-gates` (`schematic_label_over_wire`) so the
/// two agree on what counts as "drawn on top of a wire".
pub fn segment_crosses_box(ax: f64, ay: f64, bx: f64, by: f64, b: &TextBox) -> bool {
    if (ax - bx).abs() < 1e-9 {
        ax > b.x0 && ax < b.x1 && ay.min(by) < b.y1 && ay.max(by) > b.y0
    } else if (ay - by).abs() < 1e-9 {
        ay > b.y0 && ay < b.y1 && ax.min(bx) < b.x1 && ax.max(bx) > b.x0
    } else {
        false
    }
}

/// Picks a value-label baseline y (in the symbol's local mm space, box
/// top-left = 0,0) that clears every wire segment, trying a short list of
/// increasingly-distant candidates below the box before giving up and
/// returning the last (closest-fit) one. `sym_x`/`sym_y` is the symbol's
/// absolute top-left (mm), needed to translate local candidate boxes into
/// the same space `wires` are in.
///
/// A south-side pin always has a wire (or at least its stub) running
/// horizontally `STUB` (1.27mm) below the box — a fixed-offset label placed
/// naively there would always land on top of it, so this tries a few
/// vertical steps out until one is clear.
/// Like [`resolve_text_y`] but also nudges the label further right when a
/// dense board's vertical routing corridor runs right past the box (a
/// long vertical wire at a near-fixed x defeats any amount of y-shifting,
/// since it stays in the label's x-range at every y). Returns the resolved
/// `(x_local, y_local)`.
pub fn resolve_value_pos(sym_x: f64, sym_y: f64, base_x_local: f64, base_y_local: f64, text: &str, font_mm: f64, wires: &[Wire]) -> (f64, f64) {
    resolve_value_pos_obs(sym_x, sym_y, base_x_local, base_y_local, text, font_mm, wires, &[])
}

/// Like [`resolve_value_pos`] but also steers clear of `obstacles` — other
/// symbols' already-placed ref/value text boxes (see
/// `resolve_label_pos`'s doc comment: same "try a few candidates" scheme,
/// extended from label-vs-wire to also cover ref/value-vs-ref/value so
/// `schematic_text_overlap` can be a hard Fail). Callers processing symbols
/// in a fixed order (e.g. `sch.symbols`) should accumulate each symbol's
/// placed boxes into `obstacles` as they go, so later symbols steer clear of
/// earlier ones.
pub fn resolve_value_pos_obs(sym_x: f64, sym_y: f64, base_x_local: f64, base_y_local: f64, text: &str, font_mm: f64, wires: &[Wire], obstacles: &[TextBox]) -> (f64, f64) {
    let x_step = 2.2;
    for xi in 0..3 {
        let x_local = base_x_local + (xi as f64) * x_step;
        for yi in 0..4 {
            let y_local = base_y_local + (yi as f64) * 1.6;
            let bbox = text_bbox(sym_x + x_local, sym_y + y_local, text, font_mm, HAnchor::Start);
            let clear_wires = wires.iter().flat_map(|w| w.pts.windows(2)).all(|seg| {
                let (ax, ay) = (seg[0].x as f64 / 1000.0, seg[0].y as f64 / 1000.0);
                let (bx, by) = (seg[1].x as f64 / 1000.0, seg[1].y as f64 / 1000.0);
                !segment_crosses_box(ax, ay, bx, by, &bbox)
            });
            let clear_obs = obstacles.iter().all(|o| !bbox.overlaps(o));
            if clear_wires && clear_obs {
                return (x_local, y_local);
            }
        }
    }
    (base_x_local + 2.0 * x_step, base_y_local + 3.0 * 1.6)
}

pub fn resolve_value_y(sym_x: f64, sym_y: f64, value_x_local: f64, base_y_local: f64, text: &str, font_mm: f64, wires: &[Wire]) -> f64 {
    resolve_text_y(sym_x, sym_y, value_x_local, base_y_local, 1.0, text, font_mm, HAnchor::Start, wires)
}

/// General form of [`resolve_value_y`]: steps the baseline by `step_dir_sign
/// * 1.6mm` per retry (positive to search further below, negative to search
/// further above — for a ref label, which sits above the box), trying a few
/// candidates before giving up and returning the furthest one tried.
#[allow(clippy::too_many_arguments)]
pub fn resolve_text_y(
    sym_x: f64,
    sym_y: f64,
    x_local: f64,
    base_y_local: f64,
    step_dir_sign: f64,
    text: &str,
    font_mm: f64,
    anchor: HAnchor,
    wires: &[Wire],
) -> f64 {
    resolve_text_y_obs(sym_x, sym_y, x_local, base_y_local, step_dir_sign, text, font_mm, anchor, wires, &[])
}

/// Like [`resolve_text_y`] but also steers clear of `obstacles` (see
/// [`resolve_value_pos_obs`]).
#[allow(clippy::too_many_arguments)]
pub fn resolve_text_y_obs(
    sym_x: f64,
    sym_y: f64,
    x_local: f64,
    base_y_local: f64,
    step_dir_sign: f64,
    text: &str,
    font_mm: f64,
    anchor: HAnchor,
    wires: &[Wire],
    obstacles: &[TextBox],
) -> f64 {
    let step = 1.6 * step_dir_sign; // mm per retry
    for i in 0..4 {
        let y_local = base_y_local + (i as f64) * step;
        let bbox = text_bbox(sym_x + x_local, sym_y + y_local, text, font_mm, anchor);
        let clear_wires = wires.iter().flat_map(|w| w.pts.windows(2)).all(|seg| {
            let (ax, ay) = (seg[0].x as f64 / 1000.0, seg[0].y as f64 / 1000.0);
            let (bx, by) = (seg[1].x as f64 / 1000.0, seg[1].y as f64 / 1000.0);
            !segment_crosses_box(ax, ay, bx, by, &bbox)
        });
        let clear_obs = obstacles.iter().all(|o| !bbox.overlaps(o));
        if clear_wires && clear_obs {
            return y_local;
        }
    }
    base_y_local + 3.0 * step
}

/// Resolves the final absolute-mm position of a net-label glyph anchored at
/// `pin` (um): the glyph's fixed vertical bend point (`bend_dy` mm from the
/// pin, e.g. a ground symbol's drop or a power flag's stem-top) and text
/// baseline (`ty_dy` mm from the pin) are fixed by the glyph style, but the
/// horizontal position is retried at a short list of increasing jogs (0,
/// then alternating +/-1.8mm, +/-3.6mm) — the same "try a few candidates,
/// keep the first clear one" scheme `resolve_text_y`/`resolve_value_pos` use
/// for label-vs-wire clearance, extended here to check against arbitrary
/// obstacle text boxes (other refs/values/labels), so `schematic_text_overlap`
/// (label-vs-refdes, label-vs-label) can be a hard failure rather than a
/// permanent warning. Returns `(jog_x, bend_y, baseline_y)`, all absolute mm;
/// the leader line runs pin -> (pin_x, bend_y) -> (jog_x, bend_y) when
/// `jog_x != pin_x`, and the glyph/text are drawn at `jog_x`.
pub fn resolve_label_pos(pin_x_um: i64, pin_y_um: i64, bend_dy: f64, ty_dy: f64, text: &str, font_mm: f64, obstacles: &[TextBox]) -> (f64, f64, f64) {
    resolve_label_pos_obs(pin_x_um, pin_y_um, bend_dy, ty_dy, text, font_mm, obstacles, &[], &[])
}

/// Like [`resolve_label_pos`] but also steers clear of `wires` — a label
/// that only avoided other text boxes could still land on top of a wire
/// segment (the `schematic_label_over_wire` defect), which is exactly what
/// happened on the denser ladder boards before this obstacle was added.
/// Tries the same x-jog ladder first, then additionally retries at a
/// lower/higher baseline (2x the ty offset) for each jog, since on a dense
/// sheet no purely-horizontal jog may be clear.
/// Clearance a placed net-label text claims *as an obstacle* for the next
/// label: `eda-gates`' `schematic_flag_adjacent` fails two same-net flag
/// texts that sit closer than one text height (1.3mm), so a label box
/// already on the sheet has to repel the next one by that much, not merely
/// avoid overlapping it. Padding is vertical only — labels stack in columns
/// above/below their pins, and the horizontal jog candidates in
/// `resolve_label_pos_obs` already spread side-by-side labels apart.
/// A signal-net "flag" label draws its italic name inside a pentagon tag
/// outline that pokes `TAG_NOSE_MM` past the text on each side plus a
/// `TAG_PAD_MM` breathing margin (see `eda-render`'s `render_net_label`
/// `LabelKind::Flag` arm) — real ink a reader sees, so overlap/obstacle math
/// for a Flag label must use this padded box, not the bare text box, or two
/// tags can visually run their pointer noses together while every text-only
/// bounding box still reports clear. Ground/Power labels draw a plain
/// symbol (no pentagon), so they keep using the bare text box.
pub const TAG_PAD_MM: f64 = 0.25;
pub const TAG_NOSE_MM: f64 = 0.45;

/// Expands a Flag label's raw text box to the pentagon tag outline actually
/// drawn around it. Shared by `eda-render` (drawing, and obstacle-pushing)
/// and `eda-gates` (`schematic_text_overlap`) so the two agree on what
/// "clear" means for a tag, not just for the name inside it.
pub fn tag_box(b: TextBox) -> TextBox {
    TextBox { x0: b.x0 - TAG_PAD_MM - TAG_NOSE_MM, x1: b.x1 + TAG_PAD_MM + TAG_NOSE_MM, y0: b.y0 - TAG_PAD_MM, y1: b.y1 + TAG_PAD_MM }
}

pub const LABEL_CLEARANCE_MM: f64 = 1.35;

/// Pads a label's box by `LABEL_CLEARANCE_MM` on every side (not just
/// vertically) before it's used as a placement obstacle for the *next*
/// label: `schematic_flag_adjacent` fails two same-net labels sitting less
/// than one text height apart on *either* axis (`dist = max(dx, dy)`), so a
/// horizontally-adjacent near-miss needs the same repulsion a
/// vertically-adjacent one already got, or the placement search (which only
/// avoids outright bbox overlap) can happily park a label just inside that
/// margin.
pub fn label_obstacle(b: TextBox) -> TextBox {
    TextBox { x0: b.x0 - LABEL_CLEARANCE_MM, x1: b.x1 + LABEL_CLEARANCE_MM, y0: b.y0 - LABEL_CLEARANCE_MM, y1: b.y1 + LABEL_CLEARANCE_MM }
}

pub fn resolve_label_pos_obs(
    pin_x_um: i64,
    pin_y_um: i64,
    bend_dy: f64,
    ty_dy: f64,
    text: &str,
    font_mm: f64,
    obstacles: &[TextBox],
    boxes: &[TextBox],
    wires: &[Wire],
) -> (f64, f64, f64) {
    resolve_label_pos_obs_padded(pin_x_um, pin_y_um, bend_dy, ty_dy, text, font_mm, 0.0, 0.0, obstacles, boxes, wires)
}

/// Like [`resolve_label_pos_obs`], but every clearance test inflates the
/// candidate box by `pad_x`/`pad_y` on each side first. A Flag label's
/// visible ink is its pentagon tag outline (see [`tag_box`]), not the bare
/// text — searching with the bare box would happily park a tag's nose right
/// on top of a neighbor it never actually tested against. Passing
/// `pad_x`/`pad_y` of 0.0 recovers exactly [`resolve_label_pos_obs`]'s
/// behavior (used by Ground/Power, which draw a plain symbol with no nose).
pub fn resolve_label_pos_obs_padded(
    pin_x_um: i64,
    pin_y_um: i64,
    bend_dy: f64,
    ty_dy: f64,
    text: &str,
    font_mm: f64,
    pad_x: f64,
    pad_y: f64,
    obstacles: &[TextBox],
    boxes: &[TextBox],
    wires: &[Wire],
) -> (f64, f64, f64) {
    let x = pin_x_um as f64 / 1000.0;
    let y = pin_y_um as f64 / 1000.0;
    let bend_y = y + bend_dy;
    let base_ty = y + ty_dy;
    let step = 1.8;
    let padded = |b: TextBox| -> TextBox { TextBox { x0: b.x0 - pad_x, x1: b.x1 + pad_x, y0: b.y0 - pad_y, y1: b.y1 + pad_y } };
    // Labels crowd hard now that functional blocks are packed tight, and a
    // label that finds no clear slot lands on top of its neighbour
    // (`schematic_text_overlap`/`schematic_flag_adjacent`). Search wider —
    // and, when nothing is clear, fall back to the *least* colliding
    // candidate (among those that stay outside every symbol box — see
    // `boxes` below) rather than a fixed jog that may be the worst one.
    let half = step / 2.0;
    // Multiples of `step` out to +-16: a very crowded side (many same-side
    // pins each with their own net label, e.g. a 16-channel PWM driver) can
    // exhaust a narrower range before finding an actually-clear slot,
    // leaving the "least colliding" fallback below to settle for a real
    // (if small) overlap.
    let x_candidates: Vec<f64> = std::iter::once(0.0)
        .chain((1..=16).map(|i| i as f64 * half))
        .chain((1..=16).map(|i| -(i as f64) * half))
        .collect();
    let y_extra_sign = if ty_dy >= 0.0 { 1.0 } else { -1.0 };
    // Only the natural (outward) direction is ever tried: stepping the
    // *opposite* way used to be a second search tier, but that direction
    // walks straight back over the pin and into the symbol's own box on a
    // tall box with a mid-height pin — exactly the "tag landed inside the
    // body" defect this function exists to prevent. Growing further in the
    // one genuinely outward direction (twice the old range, since it no
    // longer shares the budget with a doomed opposite tier) still finds
    // real slots on a crowded side without ever risking a fold-back.
    let y_candidates: Vec<f64> = (0..=14).map(|i| y_extra_sign * i as f64 * 1.6).collect();
    let safe = |bbox: &TextBox| -> bool { boxes.iter().all(|b| !bbox.overlaps(b)) };
    let clear = |jog_x: f64, ty: f64| -> Option<TextBox> {
        let bbox = padded(text_bbox(jog_x, ty, text, font_mm, HAnchor::Middle));
        if !safe(&bbox) {
            return None;
        }
        let clear_obs = obstacles.iter().all(|o| !bbox.overlaps(o));
        let clear_wires = wires.iter().flat_map(|w| w.pts.windows(2)).all(|seg| {
            let (ax, ay) = (seg[0].x as f64 / 1000.0, seg[0].y as f64 / 1000.0);
            let (bx, by) = (seg[1].x as f64 / 1000.0, seg[1].y as f64 / 1000.0);
            !segment_crosses_box(ax, ay, bx, by, &bbox)
        });
        if clear_obs && clear_wires { Some(bbox) } else { None }
    };
    let mut best: Option<(f64, f64, f64)> = None; // (penalty, jog_x, ty) — always box-safe
    for dy in &y_candidates {
        let dy = *dy;
        for dx in &x_candidates {
            let dx = *dx;
            let jog_x = x + dx;
            let ty = base_ty + dy;
            if clear(jog_x, ty).is_some() {
                return (jog_x, bend_y, ty);
            }
            let bbox = padded(text_bbox(jog_x, ty, text, font_mm, HAnchor::Middle));
            if !safe(&bbox) {
                // Never a candidate for the fallback either: entering a
                // symbol's box is not an acceptable "least bad" outcome.
                continue;
            }
            let overlap: f64 = obstacles
                .iter()
                .map(|o| {
                    let w = (bbox.x1.min(o.x1) - bbox.x0.max(o.x0)).max(0.0);
                    let h = (bbox.y1.min(o.y1) - bbox.y0.max(o.y0)).max(0.0);
                    w * h
                })
                .sum();
            // Break ties toward the smallest jog, so an uncrowded label
            // still sits directly over its pin.
            let penalty = overlap * 1000.0 + dx.abs() + dy.abs();
            if best.map(|(p, _, _)| penalty < p).unwrap_or(true) {
                best = Some((penalty, jog_x, ty));
            }
        }
    }
    if let Some((_, jog_x, ty)) = best {
        return (jog_x, bend_y, ty);
    }
    // No box-safe candidate anywhere in the searched range (should not
    // happen on any board-scale symbol): keep stepping further out along
    // the one outward direction until clear of every box, rather than ever
    // returning a position inside one.
    for i in 15..200 {
        let ty = base_ty + y_extra_sign * i as f64 * 1.6;
        let bbox = padded(text_bbox(x, ty, text, font_mm, HAnchor::Middle));
        if safe(&bbox) {
            return (x, bend_y, ty);
        }
    }
    (x, bend_y, base_ty + y_extra_sign * 200.0 * 1.6)
}

/// Axis-aligned box rectangle (mm) for a symbol whose top-left sits at
/// `(sym_x_mm, sym_y_mm)` and whose size (um) is `(width_um, height_um)` —
/// the same box `eda-render` draws and `eda-gates`' `schematic_symbol_overlap`
/// already checks, reused here as a hard placement obstacle so a net-label
/// tag can never land on top of *any* symbol's body, not just its own.
pub fn symbol_box_mm(sym_x_mm: f64, sym_y_mm: f64, width_um: i64, height_um: i64) -> TextBox {
    TextBox { x0: sym_x_mm, y0: sym_y_mm, x1: sym_x_mm + width_um as f64 / 1000.0, y1: sym_y_mm + height_um as f64 / 1000.0 }
}

/// Recovers which side of one of `boxes` a net-label's pin-anchor point
/// sits on, by finding the box whose boundary plane passes through
/// `(x, y)` (mm), within `eps` of float slop from the um->mm conversion —
/// a `NetLabel` only carries the anchor point, not the side, and the point
/// is always placed exactly on its owning pin's on-box attach point by
/// construction (`eda_engine::derive_schematic`), so this is exact, not a
/// heuristic. Shared by `eda-render` (to pick the label's outward search
/// axis) and `eda-gates` (to recompute the identical position for
/// `schematic_label_in_symbol`/`schematic_text_overlap`) so the two can
/// never disagree about which way a label should have grown.
pub fn side_of_boundary_point(boxes: &[TextBox], x: f64, y: f64, eps: f64) -> Option<Side> {
    for b in boxes {
        if (x - b.x0).abs() < eps && y > b.y0 - eps && y < b.y1 + eps {
            return Some(Side::Left);
        }
        if (x - b.x1).abs() < eps && y > b.y0 - eps && y < b.y1 + eps {
            return Some(Side::Right);
        }
        if (y - b.y0).abs() < eps && x > b.x0 - eps && x < b.x1 + eps {
            return Some(Side::Top);
        }
        if (y - b.y1).abs() < eps && x > b.x0 - eps && x < b.x1 + eps {
            return Some(Side::Bottom);
        }
    }
    None
}

/// Side-aware placement for a signal-net Flag tag: unlike
/// [`resolve_label_pos_obs_padded`] (which always grows the label straight
/// up/down, correct only for a Top/Bottom-side pin), this grows strictly
/// along the pin's own outward normal — up for Top, down for Bottom, left
/// for Left, right for Right — and only ever *slides* along the
/// perpendicular (the side the pin sits on) to dodge a crowded neighbour,
/// never doubling back over the pin toward the box. This is what actually
/// fixes a West/East-side pin's tag landing mid-body on a tall box: the old
/// scheme's "grow upward" made sense only by coincidence, for pins that
/// happened to sit near the box's top edge.
///
/// For a Left/Right-side pin the text is additionally anchored `End`/`Start`
/// (growing away from the box) rather than `Middle` — a long name
/// (`UART_LINK_TX`) centered on an anchor a fixed 1.8mm outside the box
/// would still reach back over the box by half its own width, which is
/// exactly the defect seen on U8. Top/Bottom-side pins keep `Middle`: they
/// only ever grow further away vertically, so a wide name safely straddles
/// the pin horizontally.
///
/// `bend_dist`/`text_dist` are positive magnitudes (mm) along the outward
/// normal — the leader's fixed bend point and the text's base distance,
/// both further from the pin than the box edge itself, so `bend_dist` need
/// not be searched: any positive distance along the outward normal from a
/// point already on the box boundary is, by construction, outside that box.
/// Returns `(bend_x, bend_y, text_x, text_y)`, all absolute mm.
#[allow(clippy::too_many_arguments)]
pub fn resolve_label_pos_side(
    pin_x_um: i64,
    pin_y_um: i64,
    side: Side,
    bend_dist: f64,
    text_dist: f64,
    text: &str,
    font_mm: f64,
    pad_x: f64,
    pad_y: f64,
    obstacles: &[TextBox],
    boxes: &[TextBox],
    wires: &[Wire],
) -> (f64, f64, f64, f64) {
    let x = pin_x_um as f64 / 1000.0;
    let y = pin_y_um as f64 / 1000.0;
    let (out_ax, out_ay) = match side {
        Side::Top => (0.0, -1.0),
        Side::Bottom => (0.0, 1.0),
        Side::Left => (-1.0, 0.0),
        Side::Right => (1.0, 0.0),
    };
    let (perp_ax, perp_ay) = (-out_ay, out_ax);
    let anchor = match side {
        Side::Top | Side::Bottom => HAnchor::Middle,
        Side::Left => HAnchor::End,
        Side::Right => HAnchor::Start,
    };
    let bend_x = x + out_ax * bend_dist;
    let bend_y = y + out_ay * bend_dist;

    let padded = |b: TextBox| -> TextBox { TextBox { x0: b.x0 - pad_x, x1: b.x1 + pad_x, y0: b.y0 - pad_y, y1: b.y1 + pad_y } };
    let step = 1.8;
    let half = step / 2.0;
    let perp_candidates: Vec<f64> = std::iter::once(0.0)
        .chain((1..=16).map(|i| i as f64 * half))
        .chain((1..=16).map(|i| -(i as f64) * half))
        .collect();
    // Only grow further outward when crowded, never back toward the pin
    // (see doc comment) — the direct fix for the "landed mid-body" defect.
    let out_candidates: Vec<f64> = (0..=14).map(|i| i as f64 * 1.6).collect();

    let point_at = |out_extra: f64, perp: f64| -> (f64, f64) {
        let d = text_dist + out_extra;
        (x + out_ax * d + perp_ax * perp, y + out_ay * d + perp_ay * perp)
    };
    let safe = |bbox: &TextBox| -> bool { boxes.iter().all(|b| !bbox.overlaps(b)) };
    let clear = |tx: f64, ty: f64| -> Option<TextBox> {
        let bbox = padded(text_bbox(tx, ty, text, font_mm, anchor));
        if !safe(&bbox) {
            return None;
        }
        let clear_obs = obstacles.iter().all(|o| !bbox.overlaps(o));
        let clear_wires = wires.iter().flat_map(|w| w.pts.windows(2)).all(|seg| {
            let (ax, ay) = (seg[0].x as f64 / 1000.0, seg[0].y as f64 / 1000.0);
            let (bx, by) = (seg[1].x as f64 / 1000.0, seg[1].y as f64 / 1000.0);
            !segment_crosses_box(ax, ay, bx, by, &bbox)
        });
        if clear_obs && clear_wires { Some(bbox) } else { None }
    };
    let mut best: Option<(f64, f64, f64)> = None; // (penalty, tx, ty) — always box-safe
    for out_extra in &out_candidates {
        for perp in &perp_candidates {
            let (tx, ty) = point_at(*out_extra, *perp);
            if clear(tx, ty).is_some() {
                return (bend_x, bend_y, tx, ty);
            }
            let bbox = padded(text_bbox(tx, ty, text, font_mm, anchor));
            if !safe(&bbox) {
                continue;
            }
            let overlap: f64 = obstacles
                .iter()
                .map(|o| {
                    let w = (bbox.x1.min(o.x1) - bbox.x0.max(o.x0)).max(0.0);
                    let h = (bbox.y1.min(o.y1) - bbox.y0.max(o.y0)).max(0.0);
                    w * h
                })
                .sum();
            let penalty = overlap * 1000.0 + out_extra.abs() + perp.abs();
            if best.map(|(p, _, _)| penalty < p).unwrap_or(true) {
                best = Some((penalty, tx, ty));
            }
        }
    }
    if let Some((_, tx, ty)) = best {
        return (bend_x, bend_y, tx, ty);
    }
    // No box-safe candidate anywhere in range: keep pushing straight
    // outward (never back toward the box) until clear.
    for i in 15..200 {
        let d = text_dist + i as f64 * 1.6;
        let (tx, ty) = (x + out_ax * d, y + out_ay * d);
        let bbox = padded(text_bbox(tx, ty, text, font_mm, anchor));
        if safe(&bbox) {
            return (bend_x, bend_y, tx, ty);
        }
    }
    let d = text_dist + 200.0 * 1.6;
    (bend_x, bend_y, x + out_ax * d, y + out_ay * d)
}

/// Which glyph a net label gets, decided purely from its net name
/// (case-insensitive): `GND*`/`AGND*`/`VSS*` draws a ground symbol,
/// `V*`/`+*`/`VCC`/`VDD` draws a power symbol, anything else is a plain
/// italic "flag" tag. Shared by `eda-render` (drawing) and `eda-gates`
/// (every gate that needs to know a label's shape) via [`resolve_net_label`]
/// so the two can never classify a net differently.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetLabelKind {
    Ground,
    Power,
    Flag,
}

pub fn net_label_kind(net: &str) -> NetLabelKind {
    let upper = net.to_uppercase();
    if upper.starts_with("GND") || upper.starts_with("AGND") || upper.starts_with("VSS") {
        NetLabelKind::Ground
    } else if upper.starts_with('V') || upper.starts_with('+') || upper == "VCC" || upper == "VDD" {
        NetLabelKind::Power
    } else {
        NetLabelKind::Flag
    }
}

/// A net label's fully-resolved on-sheet geometry, as decided by
/// [`resolve_net_label`] — everything `eda-render` needs to draw the glyph,
/// and everything a bounds/overlap gate needs to measure it, without either
/// side re-deriving jogs/bends independently.
#[derive(Clone, Copy, Debug)]
pub enum ResolvedNetLabel {
    Ground { x: f64, y: f64, drop: f64, jog_x: f64, ty: f64 },
    Power { x: f64, y: f64, stem_top: f64, jog_x: f64, ty: f64 },
    Flag { x: f64, y: f64, bend_x: f64, bend_y: f64, dog_x: f64, dog_y: f64, jog_x: f64, ty: f64, side: Side, anchor: HAnchor },
}

/// Resolves a net label's on-sheet position *and* its "tight" ink box (bare
/// text for Ground/Power, the pentagon [`tag_box`] for a Flag) — the single
/// routine `eda-render`'s `render_net_label` draws from and `eda-gates`'
/// `collect_text_boxes`/`schematic_content_in_bounds` measure from, so a
/// label can never end up positioned two different ways by the two crates.
/// Pushes the resulting tight ink box, padded via [`label_obstacle`], onto
/// `obstacles` before returning, exactly like the callers' own copies of
/// this logic always have, so a later label in the same sheet still repels
/// this one by the same margin.
pub fn resolve_net_label(
    pin_x_um: i64,
    pin_y_um: i64,
    net: &str,
    font_mm: f64,
    obstacles: &mut Vec<TextBox>,
    boxes: &[TextBox],
    wires: &[Wire],
) -> (ResolvedNetLabel, TextBox) {
    let x = pin_x_um as f64 / 1000.0;
    let y = pin_y_um as f64 / 1000.0;
    match net_label_kind(net) {
        NetLabelKind::Ground => {
            let (jog_x, drop, ty) = resolve_label_pos_obs(pin_x_um, pin_y_um, 0.5, 0.5 + 0.5 + 0.8, net, font_mm, obstacles, boxes, wires);
            let ink = text_bbox(jog_x, ty, net, font_mm, HAnchor::Middle);
            obstacles.push(label_obstacle(ink));
            (ResolvedNetLabel::Ground { x, y, drop, jog_x, ty }, ink)
        }
        NetLabelKind::Power => {
            let (jog_x, stem_top, ty) = resolve_label_pos_obs(pin_x_um, pin_y_um, -1.6, -1.6 - 0.4, net, font_mm, obstacles, boxes, wires);
            let ink = text_bbox(jog_x, ty, net, font_mm, HAnchor::Middle);
            obstacles.push(label_obstacle(ink));
            (ResolvedNetLabel::Power { x, y, stem_top, jog_x, ty }, ink)
        }
        NetLabelKind::Flag => {
            let side = side_of_boundary_point(boxes, x, y, 1e-3).unwrap_or(Side::Bottom);
            let (bend_x, bend_y, jog_x, ty) =
                resolve_label_pos_side(pin_x_um, pin_y_um, side, 1.5, 1.8, net, font_mm, TAG_PAD_MM + TAG_NOSE_MM, TAG_PAD_MM, obstacles, boxes, wires);
            let anchor = match side {
                Side::Top | Side::Bottom => HAnchor::Middle,
                Side::Left => HAnchor::End,
                Side::Right => HAnchor::Start,
            };
            let bbox = text_bbox(jog_x, ty, net, font_mm, anchor);
            let ink = tag_box(bbox);
            obstacles.push(label_obstacle(ink));
            let (dog_x, dog_y) = match side {
                Side::Top | Side::Bottom => (jog_x, bend_y),
                Side::Left | Side::Right => (bend_x, ty),
            };
            (ResolvedNetLabel::Flag { x, y, bend_x, bend_y, dog_x, dog_y, jog_x, ty, side, anchor }, ink)
        }
    }
}

/// The extra ink points (mm) a Ground glyph's drop bars add beyond its drop
/// point, given the resolved `jog_x`/drop-y: shared by `eda-render`
/// (drawing) and [`net_label_extent`] so the two agree on how wide the bars
/// reach.
pub fn ground_glyph_points(jog_x: f64, drop: f64) -> [(f64, f64); 6] {
    let d2 = drop + 0.25;
    let d3 = drop + 0.5;
    [(jog_x - 0.75, drop), (jog_x + 0.75, drop), (jog_x - 0.45, d2), (jog_x + 0.45, d2), (jog_x - 0.15, d3), (jog_x + 0.15, d3)]
}

/// The extra ink points (mm) a Power glyph's arrowhead adds beyond its stem
/// top, given the resolved `jog_x`/stem-top-y: shared the same way as
/// [`ground_glyph_points`].
pub fn power_glyph_points(jog_x: f64, stem_top: f64) -> [(f64, f64); 2] {
    let ay = stem_top + 0.6;
    [(jog_x - 0.5, ay), (jog_x + 0.5, ay)]
}

/// Full ink extent (mm) of a net label's *drawn glyph*, beyond its tight
/// text/tag box `ink`: the leader line(s) from pin to tag/glyph, plus (for
/// Ground/Power) the bars/arrowhead. Shared by `eda-render` (to grow the
/// sheet's own bounding box in `render_schematic` so nothing it draws is
/// ever clipped by the viewBox) and `eda-gates`' `schematic_content_in_bounds`
/// (to fail when something is), so the two can never disagree about how far
/// a label's ink actually reaches.
pub fn net_label_extent(resolved: &ResolvedNetLabel, ink: TextBox) -> TextBox {
    let mut b = ink;
    let mut feed = |x: f64, y: f64| {
        b.x0 = b.x0.min(x);
        b.x1 = b.x1.max(x);
        b.y0 = b.y0.min(y);
        b.y1 = b.y1.max(y);
    };
    match *resolved {
        ResolvedNetLabel::Ground { x, y, drop, jog_x, .. } => {
            feed(x, y);
            feed(x, drop);
            feed(jog_x, drop);
            for (px, py) in ground_glyph_points(jog_x, drop) {
                feed(px, py);
            }
        }
        ResolvedNetLabel::Power { x, y, stem_top, jog_x, .. } => {
            feed(x, y);
            feed(x, stem_top);
            feed(jog_x, stem_top);
            for (px, py) in power_glyph_points(jog_x, stem_top) {
                feed(px, py);
            }
        }
        ResolvedNetLabel::Flag { x, y, bend_x, bend_y, dog_x, dog_y, .. } => {
            feed(x, y);
            feed(bend_x, bend_y);
            feed(dog_x, dog_y);
        }
    }
    b
}

/// Local (box-space, um) attach points for a part's `nc`-kind pins, keyed
/// by pin index: a nib on the box's own south edge, one `GRID` step past
/// whichever real ports [`build_ports`] already placed there.
///
/// An `nc` pin never joins a net (no `Wire` ever names it), so it has no
/// business in [`build_ports`]'s port list — this is a deliberately
/// separate, additive function so the *existing* port assignment, and
/// every box-size/aspect/density/overlap gate built on it, is untouched by
/// whether a part happens to declare an `nc` pin or how many. What still
/// has to be true is that the pin is drawn *somewhere* on the symbol (not
/// simply omitted): a real component's unused pin keeps its real
/// electrical type and gets an explicit no-connect flag placed on it
/// (`SchematicSection::no_connects`) — that flag is what tells KiCad's ERC
/// the dangling pin is intentional, not the pin's type.
pub fn nc_pin_local_points(part: &Part, width: i64, height: i64, resolved: Option<&eda_model::symbol::LibSymbol>) -> Vec<(usize, Point)> {
    if let Some(sym) = resolved {
        // The real symbol's own point for this pin number, not a synthetic
        // slot: the no-connect flag must coincide with wherever the real,
        // drawn-verbatim graphics actually put the pin, or the flag and
        // the pin read as two unrelated things.
        let (x0, _y0, _x1, y1) = real_symbol_bbox(sym);
        let mut out = Vec::new();
        for (i, p) in part.pins.iter().enumerate() {
            if p.kind != PinKind::Nc {
                continue;
            }
            let Some(real_pin) = sym.pin_by_number(&p.number) else { continue };
            let x = ((real_pin.at.x - x0) * 1000.0).round() as i64;
            let y = ((y1 - real_pin.at.y) * 1000.0).round() as i64;
            out.push((i, Point { x, y }));
        }
        return out;
    }
    let (ports, _) = build_ports(part, width, height, None);
    let south_max_x = ports.iter().filter(|p| p.side == Side::Bottom).map(|p| p.offset).max().unwrap_or(0);
    let mut x = south_max_x + GRID;
    let mut out = Vec::new();
    for (i, p) in part.pins.iter().enumerate() {
        if p.kind == PinKind::Nc {
            out.push((i, Point { x, y: height }));
            x += GRID;
        }
    }
    out
}

/// Evenly spaces `n` offsets along a side of length `length`, snapped to
/// `GRID` and clamped to stay strictly within the side (never on a corner).
pub fn distribute_offsets(n: usize, length: i64) -> Vec<i64> {
    if n == 0 {
        return Vec::new();
    }
    let max_off = (((length / GRID) - 1).max(1)) * GRID;
    (0..n)
        .map(|i| {
            let raw = length * (i as i64 + 1) / (n as i64 + 1);
            let snapped = ((raw + GRID / 2) / GRID) * GRID;
            snapped.clamp(GRID, max_off)
        })
        .collect()
}
