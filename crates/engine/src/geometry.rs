//! Shared symbol-box geometry: node sizing and port placement.
//!
//! Used by `derive_schematic` to build the layout graph, and by `eda-render`
//! to recompute the same box sizes and pin positions when drawing a
//! `Design` — keeping the two in lock-step without duplicating the rules.

use eda_layout::{Node, Point, Port, Side};
use eda_model::{Part, PinKind};

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

/// Box size (width, height) for a part with `pin_count` pins.
pub fn node_size(pin_count: usize) -> (i64, i64) {
    let extra = pin_count.saturating_sub(4);
    let pairs = extra.div_ceil(2);
    (BASE_WIDTH, BASE_HEIGHT + (pairs as i64) * HEIGHT_STEP)
}

/// True for pin names that read as a "control-ish" input to an IC (enable,
/// chip-select, reset...): `source_control_pin_unconnected` in `eda-intent`
/// uses the same pattern list (kept in lockstep by inspection/tests since
/// `eda-intent` cannot depend on this crate without a cycle).
pub fn is_control_pin_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["EN", "CE", "SHDN", "RESET", "RST", "CS"].iter().any(|p| n.contains(p))
}

fn is_ground_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("GND") || n.contains("VSS") || n.contains("AGND")
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
pub fn build_ports(part: &Part, width: i64, height: i64) -> (Vec<Port>, Vec<Option<usize>>) {
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

// ---------------------------------------------------------------- shared text-box estimation
//
// Used by both `eda-render` (to place ref/value/pin/net-label text with
// guaranteed clearance) and `eda-gates` (`schematic_text_overlap`) so the
// two always agree on what counts as an overlap.

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
const CHAR_WIDTH_FACTOR: f64 = 0.6;

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
