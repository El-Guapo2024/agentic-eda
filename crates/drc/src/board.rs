//! Flattens a `Design`/`ConstraintModel` into the plain lists of
//! board-space items every test provider works from -- the same role
//! KiCad's own `BOARD` class plays for `DRC_ENGINE`. Built once per run.

use crate::kimath::Shape;
use eda_model::footprint::placed_courtyard;
use eda_model::ir::{Design, LabelSide, Point, Shape as IrShape, Side, Text, Um};
use eda_model::{ConstraintModel, Pad, PadKind, PadShape};
use std::collections::HashMap;

/// A silkscreen item for the silk-clearance/silk-edge/silk-over-copper
/// checks: a free-standing drawing shape or text on `F.SilkS`/`B.SilkS`, or
/// -- since this model has no explicit per-footprint silk graphics -- a
/// footprint's own reference-designator label, using [`exported_refdes_shape`]
/// (*not* `eda_model::footprint::placed_refdes_box`: that box is placement's
/// own coarser keepout approximation -- different gap constant, different
/// per-character width formula, font size from a board setting rather than
/// the fixed 1 mm the exporter always writes -- so reusing it here silently
/// disagreed with what a real board, and kicad-cli, actually sees; see the
/// task report). Refdes labels are, in practice, the single biggest source
/// of real silk violations (dense boards where a label overlaps a
/// neighbour's pad or courtyard), so leaving them out would silently
/// under-report against the oracle.
pub struct SilkItem {
    pub id: String,
    pub desc: String,
    pub layer: String,
    pub shape: Shape,
}

/// An `eda_model::ir::Shape` reduced to a DRC [`Shape`] -- a circle is
/// treated as a filled stadium of zero length (its stroke, if any,
/// inflating the radius), a rectangle/polygon as its outline; exact enough
/// for clearance purposes without rasterizing an arc.
pub fn from_ir_shape(s: &IrShape) -> Shape {
    match s {
        IrShape::Segment { start, end, stroke_width, .. } => Shape::Stadium { a: *start, b: *end, r: stroke_width / 2 },
        IrShape::Rect { start, end, .. } => {
            let (x0, x1) = (start.x.min(end.x), start.x.max(end.x));
            let (y0, y1) = (start.y.min(end.y), start.y.max(end.y));
            Shape::Rect { x0, y0, x1, y1 }
        }
        IrShape::Circle { center, end, stroke_width, .. } => Shape::Circle { c: *center, r: crate::kimath::dist(*center, *end) + stroke_width / 2 },
        IrShape::Arc { start, end, stroke_width, .. } => Shape::Stadium { a: *start, b: *end, r: stroke_width / 2 },
        IrShape::Polygon { pts, .. } => Shape::Polygon { pts: pts.clone() },
    }
}

pub struct DrcPad {
    /// `"<footprint-ref>.<pad-number>"`, unique per physical pad (pad
    /// numbers repeat within a footprint -- see `footprint::PlacedPad`).
    pub id: String,
    pub footprint_ref: String,
    pub number: String,
    pub net: Option<String>,
    pub center: Point,
    pub side: Side,
    pub kind: PadKind,
    /// Copper layers this pad is flashed on -- mirrors `eda_kicad`'s own
    /// exporter (`*.Cu` for through-hole/non-plated, one outer layer for
    /// SMD), so a DRC run against our own exported `.kicad_pcb` agrees with
    /// what kicad-cli actually sees.
    pub layers: Vec<String>,
    pub copper: Shape,
    /// Hole shape (round or slot), board space; `None` for a pure SMD pad.
    pub hole: Option<Shape>,
    pub drill_round: Option<Um>,
    /// Board-space axis-aligned slot drill extent (see `rotated_extent_by`),
    /// `None` for a round-drilled or SMD pad.
    pub drill_slot: Option<(Um, Um)>,
}

pub struct DrcTrackSeg {
    pub id: String,
    pub net: Option<String>,
    pub layer: String,
    pub width: Um,
    pub a: Point,
    pub b: Point,
    /// `Some(mid)` for a `PCB_ARC` (`a`/`b` are its start/end): one DRC
    /// item for the whole arc, as in KiCad, rather than one per chord of
    /// the IR's tessellated `pts` (see `eda_model::ir::Track::arc`).
    pub arc_mid: Option<Point>,
}

pub struct DrcVia {
    pub id: String,
    pub net: Option<String>,
    pub at: Point,
    pub drill: Um,
    pub diameter: Um,
    pub from_layer: String,
    pub to_layer: String,
}

pub struct DrcZone {
    pub id: String,
    pub net: Option<String>,
    pub layer: String,
    pub outline: Vec<Point>,
    // ---- fill settings, carried alongside so `eda_drc::fill::fill_all_zones`
    // can run from a `DrcBoard` alone (no separate `Design`/`ConstraintModel`
    // pass needed, and no risk of it recursively calling `board::build`
    // again) -- see that module's doc comment. Mirrors `eda_model::ir::Zone`'s
    // own fields of the same name.
    pub priority: u32,
    pub clearance: Um,
    pub min_thickness: Um,
    pub thermal_gap: Um,
    pub thermal_spoke_width: Um,
    pub pad_connection: eda_model::ir::PadConnection,
    pub island_removal_mode: eda_model::ir::IslandRemovalMode,
    pub min_island_area: i64,
    /// `ZONE::IsTeardropArea()`: tested "as tracks, not zones" -- left out
    /// of zone-vs-zone and tested against other zones instead
    /// (`testTeardropClearances`).
    pub teardrop: bool,
}

/// A rule area / keepout (`ZONE::GetIsRuleArea`, task item 3) -- kept
/// entirely separate from [`DrcZone`] (never pushed into `DrcBoard::zones`)
/// so every existing provider that already iterates `board.zones` assuming
/// "this is real copper" keeps working unchanged; a keepout has no fill
/// settings and is not copper.
pub struct DrcKeepout {
    pub id: String,
    pub layer: String,
    pub outline: Vec<Point>,
    pub no_tracks: bool,
    pub no_vias: bool,
    pub no_pads: bool,
    pub no_copper_pour: bool,
    pub no_footprints: bool,
}

pub struct DrcFootprint {
    pub id: String,
    pub side: Side,
    /// `(x0, y0, x1, y1)`.
    pub courtyard: (Um, Um, Um, Um),
    /// The real courtyard outlines in board space (`FOOTPRINT::GetCourtyard`),
    /// when the footprint has them (`Footprint::courtyard_outlines`); empty
    /// means only the `courtyard` box is known.
    pub outlines: Vec<Vec<Point>>,
}

pub struct DrcBoard {
    pub layers: Vec<String>,
    pub outline: Vec<Point>,
    pub pads: Vec<DrcPad>,
    pub tracks: Vec<DrcTrackSeg>,
    pub vias: Vec<DrcVia>,
    pub zones: Vec<DrcZone>,
    pub keepouts: Vec<DrcKeepout>,
    pub footprints: Vec<DrcFootprint>,
    pub shapes: Vec<IrShape>,
    pub texts: Vec<Text>,
    pub silk_items: Vec<SilkItem>,
}

/// A local-frame `(w, h)` box's axis-aligned board-space extent under a
/// footprint's rotation composed with the pad's own -- the same formula as
/// `eda_model::footprint`'s private `rotated_extent_by`, duplicated here
/// rather than exposed from `eda_model` to avoid widening that crate's
/// public surface for a five-line rotation helper.
fn rotated_extent_by(rot_millideg: i64, size: (Um, Um)) -> (Um, Um) {
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0 as f64, size.1 as f64);
    ((w * cos.abs() + h * sin.abs()).round() as Um, (w * sin.abs() + h * cos.abs()).round() as Um)
}

/// The copper shape of a `size`-sized pad of `shape` centred at `center` --
/// exact for `Rect`/`RoundRect`/`Circle`, and for `Oval` (an axis-aligned
/// stadium: KiCad's `SHAPE_SEGMENT` oval construction) since `size` is
/// already axis-aligned (see the module doc and the crate-level fidelity
/// notes on non-90-degree rotation).
fn shape_of(center: Point, size: (Um, Um), shape: PadShape, roundrect_ratio: Option<f64>) -> Shape {
    let (w, h) = size;
    match shape {
        PadShape::Circle => Shape::Circle { c: center, r: w.max(h) / 2 },
        PadShape::Rect => Shape::Rect { x0: center.x - w / 2, y0: center.y - h / 2, x1: center.x + w / 2, y1: center.y + h / 2 },
        PadShape::RoundRect => {
            let r = (roundrect_ratio.unwrap_or(0.25) * w.min(h) as f64).round() as Um;
            Shape::RoundRect { x0: center.x - w / 2, y0: center.y - h / 2, x1: center.x + w / 2, y1: center.y + h / 2, r }
        }
        PadShape::Oval => {
            let r = w.min(h) / 2;
            if w >= h {
                let foc = (w - h) / 2;
                Shape::Stadium { a: Point { x: center.x - foc, y: center.y }, b: Point { x: center.x + foc, y: center.y }, r }
            } else {
                let foc = (h - w) / 2;
                Shape::Stadium { a: Point { x: center.x, y: center.y - foc }, b: Point { x: center.x, y: center.y + foc }, r }
            }
        }
    }
}

/// Board-space centre of a pad's local-frame `at`, mirroring
/// `eda_model::footprint::to_board` (duplicated for the same reason as
/// `rotated_extent_by`: it is `pub` already, so this one just forwards).
fn pad_center(fp: &eda_model::ir::FootprintInstance, local: (Um, Um)) -> Point {
    eda_model::footprint::to_board(fp, local)
}

/// The reference-designator silk label's board-space shape, mirroring
/// `eda_kicad`'s exporter formula exactly (`crates/kicad/src/pcb.rs`'s
/// footprint writer) rather than placement's own `placed_refdes_box`
/// keepout approximation -- see this module's doc comment. The exporter
/// writes the label as a child of the footprint's own `(at .. rot)`, so its
/// local-frame offset from the footprint origin is rotated and translated
/// by [`pad_center`]/`to_board` exactly like a pad's; and it is written
/// with no extra local rotation, so it turns with the footprint on the
/// board (KiCad applies no keep-upright correction here), hence the same
/// `rotated_extent_by` conservative axis-aligned bbox pads use for a
/// rotated rectangle -- exact at 0/90/180/270, which is effectively every
/// placement this workspace produces.
fn exported_refdes_shape(model: &ConstraintModel, part: &eda_model::Part, fp: &eda_model::ir::FootprintInstance) -> Option<Shape> {
    let footprint = model.footprint_of(part)?;
    let (hw, hh) = footprint.courtyard_half();
    // `eda_kicad::pcb`'s own constants, kept exactly for the *anchor
    // position* (so this item sits where the real label really is): 300 µm
    // per character of half-width for the Left/Right offset, a 700 µm gap
    // above/below the courtyard (200 µm beside it).
    let half_w_offset = 300 * fp.id.chars().count() as Um;
    let local = match fp.label {
        LabelSide::Above => (0, -(hh + 700)),
        LabelSide::Below => (0, hh + 700),
        LabelSide::Left => (-(hw + 200 + half_w_offset), 0),
        LabelSide::Right => (hw + 200 + half_w_offset, 0),
    };
    let center = pad_center(fp, local);
    // The label's own *rendered* extent, for collision purposes, is a
    // separate (smaller) approximation of KiCad's actual stroke-font glyph
    // ink at 1 mm nominal size -- not exactly `half_w_offset`/1000 (this
    // model has no vector glyph outlines to collide against exactly, the
    // way KiCad's own `DRC_TEST_PROVIDER_SILK_CLEARANCE` does). Calibrated
    // against the oracle across the example boards (see the task report);
    // still an approximation, so a residual mismatch against kicad-cli
    // remains on some boards.
    let half_w_box = 220 * fp.id.chars().count() as Um;
    let (w, h) = rotated_extent_by(fp.rot as i64, (half_w_box * 2, 500));
    Some(Shape::Rect { x0: center.x - w / 2, y0: center.y - h / 2, x1: center.x + w / 2, y1: center.y + h / 2 })
}

/// A pad-frame offset `(x, y)` to board space: the pad's own rotation,
/// then the footprint's mirror (bottom side) and rotation -- the same
/// composition `to_board` applies to the pad's position.
fn pad_frame_to_board(fp: &eda_model::ir::FootprintInstance, pad_rot_mdeg: i64, v: (f64, f64)) -> (f64, f64) {
    let rot = |a_mdeg: i64, (x, y): (f64, f64)| {
        let (s, c) = ((a_mdeg as f64) / 1000.0 * std::f64::consts::PI / 180.0).sin_cos();
        (x * c - y * s, x * s + y * c)
    };
    let (x, y) = rot(pad_rot_mdeg, v);
    let x = if fp.side == Side::Bottom { -x } else { x };
    rot(fp.rot as i64, (x, y))
}

/// `PAD::GetEffectiveShape` for a pad whose board rotation is *not* a
/// multiple of 90 degrees: the true rotated outline (a rotated rectangle or
/// rounded rectangle as a polygon, an oval as a rotated stadium) instead of
/// the axis-aligned box around it, which over-reports clearance and
/// shorts between neighbouring pads of a 45-degree footprint.
fn rotated_pad_shape(fp: &eda_model::ir::FootprintInstance, pad: &Pad, center: Point) -> Shape {
    let (w, h) = (pad.size.0 as f64, pad.size.1 as f64);
    let to_pt = |v: (f64, f64)| {
        let (dx, dy) = pad_frame_to_board(fp, pad.rot as i64, v);
        Point { x: center.x + dx.round() as Um, y: center.y + dy.round() as Um }
    };
    match pad.shape {
        PadShape::Circle => Shape::Circle { c: center, r: pad.size.0.max(pad.size.1) / 2 },
        PadShape::Oval => {
            let r = (w.min(h) / 2.0).round() as Um;
            let f = (w.max(h) - w.min(h)) / 2.0;
            let axis = if w >= h { (f, 0.0) } else { (0.0, f) };
            Shape::Stadium { a: to_pt((-axis.0, -axis.1)), b: to_pt(axis), r }
        }
        PadShape::Rect => Shape::Polygon { pts: [(-w / 2.0, -h / 2.0), (w / 2.0, -h / 2.0), (w / 2.0, h / 2.0), (-w / 2.0, h / 2.0)].into_iter().map(to_pt).collect() },
        PadShape::RoundRect => {
            let r = pad.roundrect_ratio.unwrap_or(0.25) * w.min(h);
            let (ix, iy) = (w / 2.0 - r, h / 2.0 - r);
            let mut pts = Vec::new();
            for (cx, cy, a0) in [(ix, iy, 0.0f64), (-ix, iy, 90.0), (-ix, -iy, 180.0), (ix, -iy, 270.0)] {
                for k in 0..=8 {
                    let a = (a0 + 90.0 * k as f64 / 8.0).to_radians();
                    pts.push(to_pt((cx + r * a.cos(), cy + r * a.sin())));
                }
            }
            Shape::Polygon { pts }
        }
    }
}

fn pad_hole_shape(center: Point, pad: &Pad, board_rot: i64) -> Option<Shape> {
    if let Some(d) = pad.drill {
        return Some(Shape::Circle { c: center, r: d / 2 });
    }
    if let Some(slot) = pad.drill_slot {
        let size = rotated_extent_by(board_rot, slot);
        return Some(shape_of(center, size, PadShape::Oval, None));
    }
    None
}

pub fn build(design: &Design, model: &ConstraintModel) -> DrcBoard {
    let outline = design.placement.as_ref().map(|p| p.outline.clone()).unwrap_or_default();
    let layers = model.board.layers.clone();

    // "REF.PIN" -> net name, exactly `eda_kicad::pcb`'s own `pin_net` lookup.
    let pin_net: HashMap<String, String> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.clone(), n.name.clone()))).collect();

    let mut pads = Vec::new();
    let mut footprints = Vec::new();
    if let Some(pl) = &design.placement {
        for fp in &pl.footprints {
            let Some(part) = model.part(&fp.id) else { continue };
            let Some(footprint) = model.footprint_of(part) else { continue };
            if let Some(c) = placed_courtyard(model, part, fp) {
                let outlines = footprint.courtyard_outlines.iter().map(|o| o.iter().map(|&p| pad_center(fp, p)).collect()).collect();
                footprints.push(DrcFootprint { id: fp.id.clone(), side: fp.side, courtyard: c, outlines });
            }
            let board_rot = fp.rot as i64;
            for pad in &footprint.pads {
                let center = pad_center(fp, pad.at);
                let pad_rot = board_rot + pad.rot as i64;
                let size = rotated_extent_by(pad_rot, pad.size);
                let copper = if pad_rot.rem_euclid(90_000) == 0 { shape_of(center, size, pad.shape, pad.roundrect_ratio) } else { rotated_pad_shape(fp, pad, center) };
                let hole = pad_hole_shape(center, pad, pad_rot);
                let drill_slot = pad.drill_slot.map(|s| rotated_extent_by(pad_rot, s));
                let net = pin_net.get(&format!("{}.{}", fp.id, pad.number)).cloned();
                let pad_layers = match pad.kind {
                    PadKind::Smd => vec![if fp.side == Side::Bottom { "B.Cu".to_string() } else { "F.Cu".to_string() }],
                    PadKind::ThroughHole | PadKind::NonPlatedHole => layers.clone(),
                };
                pads.push(DrcPad {
                    id: format!("{}.{}", fp.id, pad.number),
                    footprint_ref: fp.id.clone(),
                    number: pad.number.clone(),
                    net,
                    center,
                    side: fp.side,
                    kind: pad.kind,
                    layers: pad_layers,
                    copper,
                    hole,
                    drill_round: pad.drill,
                    drill_slot,
                });
            }
        }
    }

    let mut tracks = Vec::new();
    let mut vias = Vec::new();
    let mut zones = Vec::new();
    let mut keepouts = Vec::new();
    if let Some(rt) = &design.routing {
        for t in &rt.tracks {
            let net = if t.net.is_empty() { None } else { Some(t.net.clone()) };
            if let Some((start, mid, end)) = t.arc() {
                tracks.push(DrcTrackSeg { id: format!("{}#0", t.id), net, layer: t.layer.clone(), width: t.width, a: start, b: end, arc_mid: Some(mid) });
                continue;
            }
            for (i, w) in t.pts.windows(2).enumerate() {
                tracks.push(DrcTrackSeg { id: format!("{}#{i}", t.id), net: net.clone(), layer: t.layer.clone(), width: t.width, a: w[0], b: w[1], arc_mid: None });
            }
        }
        for v in &rt.vias {
            let net = if v.net.is_empty() { None } else { Some(v.net.clone()) };
            vias.push(DrcVia { id: v.id.clone(), net, at: v.at, drill: v.drill, diameter: v.diameter, from_layer: v.from_layer.clone(), to_layer: v.to_layer.clone() });
        }
        for z in &rt.zones {
            if z.outline.len() < 3 {
                continue;
            }
            // A rule area is not copper -- it never becomes a `DrcZone`
            // (every provider that already iterates `board.zones` assumes
            // "this is a real copper pour"), only a `DrcKeepout`. See
            // task item 3 / `DrcKeepout`'s own doc.
            if z.is_rule_area {
                keepouts.push(DrcKeepout {
                    id: z.id.clone(),
                    layer: z.layer.clone(),
                    outline: z.outline.clone(),
                    no_tracks: z.keepout_tracks,
                    no_vias: z.keepout_vias,
                    no_pads: z.keepout_pads,
                    no_copper_pour: z.keepout_copper_pour,
                    no_footprints: z.keepout_footprints,
                });
                continue;
            }
            let net = if z.net.is_empty() { None } else { Some(z.net.clone()) };
            zones.push(DrcZone {
                id: z.id.clone(),
                net,
                layer: z.layer.clone(),
                outline: z.outline.clone(),
                priority: z.priority,
                clearance: z.clearance,
                min_thickness: z.min_thickness,
                thermal_gap: z.thermal_gap,
                thermal_spoke_width: z.thermal_spoke_width,
                pad_connection: z.pad_connection,
                island_removal_mode: z.island_removal_mode,
                min_island_area: z.min_island_area,
                teardrop: z.teardrop,
            });
        }
    }

    let (shapes, texts) = design.drawings.as_ref().map(|d| (d.shapes.clone(), d.texts.clone())).unwrap_or_default();

    let mut silk_items = Vec::new();
    for s in &shapes {
        if s.layer() == "F.SilkS" || s.layer() == "B.SilkS" {
            silk_items.push(SilkItem { id: s.id().to_string(), desc: format!("Graphic on {}", s.layer()), layer: s.layer().to_string(), shape: from_ir_shape(s) });
        }
    }
    for t in &texts {
        if t.layer == "F.SilkS" || t.layer == "B.SilkS" {
            // A text's true extent is a horizontal (or rotated) box; approximated
            // here as a circle sized to the font height, centred on its anchor --
            // adequate for a clearance check, not for exact overlap geometry.
            silk_items.push(SilkItem { id: t.id.clone(), desc: format!("Text on {}", t.layer), layer: t.layer.clone(), shape: Shape::Circle { c: t.at, r: t.size_um / 2 } });
        }
    }
    if let Some(pl) = &design.placement {
        for fp in &pl.footprints {
            let Some(part) = model.part(&fp.id) else { continue };
            if let Some(shape) = exported_refdes_shape(model, part, fp) {
                let layer = if fp.side == Side::Bottom { "B.SilkS" } else { "F.SilkS" };
                silk_items.push(SilkItem { id: format!("{}.ref", fp.id), desc: format!("Reference of {}", fp.id), layer: layer.to_string(), shape });
            }
        }
    }

    DrcBoard { layers, outline, pads, tracks, vias, zones, keepouts, footprints, shapes, texts, silk_items }
}

impl DrcVia {
    /// Copper shape at `layer` -- a via is a plain circle on every layer it
    /// spans; `from_layer`/`to_layer` in our model are always the two outer
    /// layers (no blind/buried vias), so it flashes on all board layers.
    pub fn shape(&self) -> Shape {
        Shape::Circle { c: self.at, r: self.diameter / 2 }
    }
    pub fn hole(&self) -> Shape {
        Shape::Circle { c: self.at, r: self.drill / 2 }
    }
}

/// `ARC_HIGH_DEF` (0.005 mm), in µm -- the chord error a DRC arc's
/// polyline is held to (`SHAPE_ARC::ConvertToPolyline`'s default).
pub const ARC_HIGH_DEF: f64 = 5.0;

/// An arc's polyline at most `max_error` µm from the true curve:
/// `GetArcToSegmentCount` (360 / (2 * acos(1 - err/r)) segments per turn),
/// over the arc's own sweep. A near-straight or degenerate arc gives its
/// chord.
pub fn arc_polyline(start: Point, mid: Point, end: Point, max_error: f64) -> Vec<Point> {
    let probe = eda_model::ir::tessellate_arc(start, mid, end, 2);
    if probe.len() < 3 {
        return probe;
    }
    let (sx, sy, mx, my, ex, ey) = (start.x as f64, start.y as f64, mid.x as f64, mid.y as f64, end.x as f64, end.y as f64);
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    let ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    let uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    let r = (sx - ux).hypot(sy - uy);
    // Sweep from the two-segment probe's own direction choice.
    let ang = |p: Point| (p.y as f64 - uy).atan2(p.x as f64 - ux);
    let half = (ang(probe[1]) - ang(start)).rem_euclid(std::f64::consts::TAU);
    let half = if half > std::f64::consts::PI { std::f64::consts::TAU - half } else { half };
    let sweep = 2.0 * half;
    let per_rad = if r > max_error { 1.0 / (2.0 * (1.0 - max_error / r).acos()) } else { 1.0 };
    let n = ((sweep * per_rad).ceil() as usize).clamp(2, 4096);
    eda_model::ir::tessellate_arc(start, mid, end, n)
}

impl DrcTrackSeg {
    pub fn shape(&self) -> Shape {
        match self.arc_mid {
            Some(mid) => {
                let pts = arc_polyline(self.a, mid, self.b, ARC_HIGH_DEF);
                Shape::Strokes { segs: pts.windows(2).map(|w| crate::kimath::Seg::new(w[0], w[1])).collect(), r: self.width / 2 }
            }
            None => Shape::Stadium { a: self.a, b: self.b, r: self.width / 2 },
        }
    }

    /// `true` for a `PCB_ARC`.
    pub fn is_arc(&self) -> bool {
        self.arc_mid.is_some()
    }
}

impl DrcZone {
    pub fn shape(&self) -> Shape {
        Shape::Polygon { pts: self.outline.clone() }
    }
}
