//! Does a footprint on the board differ from the library's copy of it? `FOOTPRINT::FootprintNeedsUpdate` (`pcbnew/drc/drc_test_provider_library_parity.cpp` at KiCad
//! 8303b2ad), the test behind Update Footprint from Library ("(no changes)") and Compare Footprint with Library (`BOARD_INSPECTION_TOOL::DiffFootprint`, whose
//! summary page is the reporter's lines).
//!
//! KiCad's DRC runs the same test as the "footprint does not match copy in library" violation; that stays kicad-cli's. This is the interactive tool's list, over the
//! studio's model: the footprint as the board has it ([`InstanceFacts`]: its pads with the edits laid on, its graphics, its attributes and its overrides) against the library's
//! [`Footprint`], with what the library's file says of its own and its pads' margins and zone settings ([`LibraryFacts`]: the engine footprint has no
//! place for them). What the model keeps of a library footprint is its pads, graphics, courtyard and models; the file's attribute flags, net-tie groups and rule areas it
//! does not keep, so those are not compared (the footprint type is the one the pads give).
//!
//! The order of the lines is KiCad's: footprint type, fabrication attributes, overrides, graphics, pads.

use crate::footprint::{Footprint, Pad, PadShape};
use crate::fp_art::FpShape;
use crate::fp_edit::{FootprintAttrs, FootprintKind};
use crate::ir::{LibraryFootprint, Millideg, PadConnection, Point, Shape, Nm};
use std::cmp::Ordering;

/// The footprint-level overrides the board -- or the library copy -- has on a footprint (`GetLocalClearance`, `GetLocalSolderMaskMargin`,
/// `GetLocalSolderPasteMargin`, `GetLocalSolderPasteMarginRatio`, `GetLocalZoneConnection`), `None` for one it does not set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FootprintOverrides {
    pub clearance: Option<Nm>,
    pub solder_mask_margin: Option<Nm>,
    /// `GetLocalSolderPasteMargin` (`(solder_paste_margin ..)`).
    pub solder_paste_margin: Option<Nm>,
    /// `GetLocalSolderPasteMarginRatio` (`(solder_paste_margin_ratio ..)`; the legacy `(solder_paste_ratio ..)`).
    pub solder_paste_margin_ratio: Option<f64>,
    pub zone_connection: Option<PadConnection>,
}

/// The overrides of one pad (`padHasOverrides`), by its number.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadOverrides {
    pub number: String,
    pub clearance: Option<Nm>,
    pub solder_mask_margin: Option<Nm>,
    pub solder_paste_margin: Option<Nm>,
    pub solder_paste_margin_ratio: Option<f64>,
    pub zone_connection: Option<PadConnection>,
    pub thermal_gap: Option<Nm>,
    pub thermal_spoke_width: Option<Nm>,
    /// `GetThermalSpokeAngle` as the file has it; `None` is the shape's default (see [`default_spoke_angle`]).
    pub thermal_spoke_angle_mdeg: Option<Millideg>,
}

/// What the library's copy sets on itself and its pads (`aLibFP`): the values the board's overrides are compared with. The library's margins of
/// a footprint file are kept as written ([`crate::lib_keep::LibraryFootprintKeep::tokens`]), since the editor does not edit them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryFacts {
    pub overrides: FootprintOverrides,
    pub pad_overrides: Vec<PadOverrides>,
}

impl LibraryFacts {
    /// The library footprint's own values, as `FootprintNeedsUpdate` reads them off `aLibFP`.
    pub fn of_library_footprint(lib: &LibraryFootprint) -> Self {
        let tokens = &lib.kicad.tokens;
        let overrides = FootprintOverrides {
            clearance: crate::lib_keep::kept_mm(tokens, "clearance"),
            solder_mask_margin: crate::lib_keep::kept_mm(tokens, "solder_mask_margin"),
            solder_paste_margin: crate::lib_keep::kept_mm(tokens, "solder_paste_margin"),
            solder_paste_margin_ratio: crate::lib_keep::kept_number(tokens, "solder_paste_margin_ratio").or_else(|| crate::lib_keep::kept_number(tokens, "solder_paste_ratio")),
            zone_connection: lib.zone_connection,
        };
        let pad_overrides = lib
            .pads
            .iter()
            .map(|p| PadOverrides {
                number: p.number.clone(),
                clearance: p.clearance_override,
                solder_mask_margin: p.kicad.solder_mask_margin,
                solder_paste_margin: p.kicad.solder_paste_margin,
                solder_paste_margin_ratio: p.kicad.solder_paste_margin_ratio,
                zone_connection: p.zone_connection,
                thermal_gap: p.thermal_gap_override,
                thermal_spoke_width: p.thermal_spoke_width_override,
                thermal_spoke_angle_mdeg: p.thermal_spoke_angle_mdeg,
            })
            .collect();
        LibraryFacts { overrides, pad_overrides }
    }
}

/// What the board has of a footprint, to be compared with the library's.
#[derive(Debug, Clone)]
pub struct InstanceFacts<'a> {
    /// The footprint as the board has it: the pads with their edits laid on, the graphics, the courtyard.
    pub footprint: &'a Footprint,
    /// The attributes in effect (`FOOTPRINT::GetAttributes`).
    pub attrs: FootprintAttrs,
    pub overrides: FootprintOverrides,
    pub pad_overrides: Vec<PadOverrides>,
}

/// `StrNumCmp`: strings compared with digit runs as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (da, db) = (run(&mut a), run(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                match ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                match x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()).then(x.cmp(&y)) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a.next();
                b.next();
            }
        }
    }
}

fn kind_name(shape: &Shape) -> &'static str {
    match shape {
        Shape::Segment { .. } => "Line",
        Shape::Arc { .. } => "Arc",
        Shape::Rect { .. } => "Rectangle",
        Shape::Circle { .. } => "Circle",
        Shape::Polygon { .. } => "Polygon",
        Shape::Bezier { .. } => "Curve",
    }
}

fn kind_rank(shape: &Shape) -> u8 {
    match shape {
        Shape::Segment { .. } => 0,
        Shape::Rect { .. } => 1,
        Shape::Arc { .. } => 2,
        Shape::Circle { .. } => 3,
        Shape::Polygon { .. } => 4,
        Shape::Bezier { .. } => 5,
    }
}

fn is_copper(layer: &str) -> bool {
    layer.ends_with(".Cu")
}

/// `FOOTPRINT::cmp_drawings` for shapes: by layer, kind, start and end (not for a polygon), centre or control points, vertices, width.
fn shape_key(s: &FpShape) -> (String, u8, Vec<(Nm, Nm)>, Nm) {
    let pts: Vec<Point> = match &s.shape {
        Shape::Segment { start, end, .. } | Shape::Rect { start, end, .. } => vec![*start, *end],
        Shape::Arc { start, mid, end, .. } => vec![*start, *end, *mid],
        Shape::Circle { center, end, .. } => vec![*center, *end],
        Shape::Polygon { pts, .. } => {
            let mut v = vec![Point { x: Nm::new(pts.len() as i64), y: Nm::ZERO }];
            v.extend(pts.iter().copied());
            v
        }
        Shape::Bezier { start, c1, c2, end, .. } => vec![*start, *end, *c1, *c2],
    };
    (s.layer().to_string(), kind_rank(&s.shape), pts.into_iter().map(|p| (p.x, p.y)).collect(), s.shape.stroke_width())
}

/// `EPSILON` of `drc_test_provider_library_parity.cpp`: two points closer than this on both axes are one (`TEST_PT`), nanometres.
const EPSILON: Nm = Nm::new(10);

/// `TEST_PT( a, b )`: the points are more than [`EPSILON`] apart on an axis.
fn pt_differs(a: (Nm, Nm), b: (Nm, Nm)) -> bool {
    (a.0 - b.0).abs() > EPSILON || (a.1 - b.1).abs() > EPSILON
}

fn point_differs(a: &Point, b: &Point) -> bool {
    pt_differs((a.x, a.y), (b.x, b.y))
}

/// Two outlines are the same when one is the other started at another vertex, each within [`EPSILON`] (`SHAPE_LINE_CHAIN::CompareGeometry( other, true,
/// EPSILON )`, cyclical).
fn same_outline(a: &[Point], b: &[Point]) -> bool {
    a.len() == b.len() && (a.is_empty() || (0..a.len()).any(|shift| (0..a.len()).all(|i| !point_differs(&a[i], &b[(i + shift) % a.len()]))))
}

/// `shapeNeedsUpdate`: the shapes (already in the footprint's frame) differ in kind, geometry, fill, layer, or -- on copper -- stroke.
fn shape_differs(a: &FpShape, b: &FpShape) -> bool {
    if kind_rank(&a.shape) != kind_rank(&b.shape) || a.layer() != b.layer() || a.shape.is_filled() != b.shape.is_filled() || a.hatch != b.hatch {
        return true;
    }
    if is_copper(a.layer()) && a.shape.stroke_width() != b.shape.stroke_width() {
        return true;
    }
    match (&a.shape, &b.shape) {
        (Shape::Rect { start: s1, end: e1, .. }, Shape::Rect { start: s2, end: e2, .. }) => {
            let norm = |s: &Point, e: &Point| ((s.x.min(e.x), s.y.min(e.y)), (s.x.max(e.x), s.y.max(e.y)));
            let ((o1, f1), (o2, f2)) = (norm(s1, e1), norm(s2, e2));
            pt_differs(o1, o2) || pt_differs(f1, f2)
        }
        (Shape::Segment { start: s1, end: e1, .. }, Shape::Segment { start: s2, end: e2, .. }) => point_differs(s1, s2) || point_differs(e1, e2),
        (Shape::Circle { center: c1, end: e1, .. }, Shape::Circle { center: c2, end: e2, .. }) => point_differs(c1, c2) || point_differs(e1, e2),
        // "Arc center is calculated and so may have round-off errors": the middle may be `pcbIUScale.mmToIU( 0.0005 )` apart
        (Shape::Arc { start: s1, mid: m1, end: e1, .. }, Shape::Arc { start: s2, mid: m2, end: e2, .. }) => point_differs(s1, s2) || point_differs(e1, e2) || ((m1.x - m2.x).as_f64()).hypot((m1.y - m2.y).as_f64()) > 500.0,
        (Shape::Bezier { start: s1, c1: a1, c2: b1, end: e1, .. }, Shape::Bezier { start: s2, c1: a2, c2: b2, end: e2, .. }) => point_differs(s1, s2) || point_differs(e1, e2) || point_differs(a1, a2) || point_differs(b1, b2),
        (Shape::Polygon { pts: p1, .. }, Shape::Polygon { pts: p2, .. }) => !same_outline(p1, p2),
        _ => true,
    }
}

fn describe(s: &FpShape) -> String {
    format!("{} on {}", kind_name(&s.shape), s.layer())
}

fn shape_rank(shape: PadShape) -> u8 {
    match shape {
        PadShape::Circle => 0,
        PadShape::Rect => 1,
        PadShape::Oval => 2,
        PadShape::Trapezoid => 3,
        PadShape::RoundRect => 4,
        PadShape::ChamferedRect => 5,
        PadShape::Custom => 6,
    }
}

/// `FOOTPRINT::cmp_pads`: by number (as `StrNumCmp`), position, size and shape.
fn pad_order(a: &Pad, b: &Pad) -> Ordering {
    natural_cmp(&a.number, &b.number)
        .then(a.at.0.cmp(&b.at.0))
        .then(a.at.1.cmp(&b.at.1))
        .then(a.size.0.cmp(&b.size.0))
        .then(a.size.1.cmp(&b.size.1))
        .then(shape_rank(a.shape).cmp(&shape_rank(b.shape)))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.000_010
}

/// The copper layers of the pad's padstack, as `PADSTACK::UniqueLayers` has them: the front, and for a front/inner/back stack the inner layers (named `In1.Cu`, as
/// `PADSTACK::INNER_LAYERS` is) and the back, for a custom one every layer it names.
fn stack_roles(a: &Pad) -> Vec<String> {
    use crate::padstack::PadStackMode;
    let mut roles = vec!["F.Cu".to_string()];
    match a.ext.stack.mode {
        PadStackMode::Normal => {}
        PadStackMode::FrontInnerBack => roles.extend(["In1.Cu".to_string(), "B.Cu".to_string()]),
        PadStackMode::Custom => roles.extend(a.ext.stack.layers.keys().cloned()),
    }
    roles
}

/// `padNeedsUpdate` with a reporter: the lines for the pad `a` (the board's) against `lib` (the library's).
fn pad_lines(a: &Pad, lib: &Pad, out: &mut Vec<String>) {
    let name = format!("Pad {}", a.number);
    if pt_differs(a.at, lib.at) {
        out.push(format!("{name} position differs."));
    }
    // `layerSettingsDiffer`: whether the unconnected layers are removed, and, when they are, whether the outside ones are kept.
    let (ua, ul) = (a.ext.stack.unconnected, lib.ext.stack.unconnected);
    if a.opposite_side != lib.opposite_side || ua.removes() != ul.removes() || (ua.removes() && ua.keeps_outer() != ul.keeps_outer()) {
        out.push(format!("{name} layers differ."));
    }
    if a.kind != lib.kind {
        out.push(format!("{name} pad type differs."));
    }
    if a.rot != lib.rot {
        out.push(format!("{name} orientation differs."));
    }
    // Each layer of the board pad's padstack (`a->Padstack().UniqueLayers()`) against what the library's pad has on it.
    let roles = stack_roles(a);
    let layer_name = |role: &str| if role == "F.Cu" && a.opposite_side { "B.Cu".to_string() } else { role.to_string() };
    for role in &roles {
        let (sa, sl) = (a.shape_for_role(role), lib.shape_for_role(role));
        let layer = layer_name(role);
        if sa.shape != sl.shape {
            out.push(format!("{name} pad shape type differs on layer {layer}."));
        }
        if sa.size != sl.size {
            out.push(format!("{name} size differs on layer {layer}."));
        }
        if sa.trapezoid_delta.unwrap_or((Nm::ZERO, Nm::ZERO)) != sl.trapezoid_delta.unwrap_or((Nm::ZERO, Nm::ZERO)) {
            out.push(format!("{name} trapezoid delta differs on layer {layer}."));
        }
        if matches!(sa.shape, PadShape::RoundRect | PadShape::ChamferedRect) && !close(sa.roundrect_ratio.unwrap_or(0.25), sl.roundrect_ratio.unwrap_or(0.25)) {
            out.push(format!("{name} rounded corners differ on layer {layer}."));
        }
        if sa.shape == PadShape::ChamferedRect {
            if !close(sa.chamfer_ratio.unwrap_or(0.2), sl.chamfer_ratio.unwrap_or(0.2)) {
                out.push(format!("{name} chamfered corner sizes differ on layer {layer}."));
            }
            if sa.chamfer_corners != sl.chamfer_corners {
                out.push(format!("{name} chamfered corners differ on layer {layer}."));
            }
        }
        if pt_differs(sa.offset.unwrap_or((Nm::ZERO, Nm::ZERO)), sl.offset.unwrap_or((Nm::ZERO, Nm::ZERO))) {
            out.push(format!("{name} shape offset from hole differs on layer {layer}."));
        }
    }
    if a.drill.is_some() != lib.drill.is_some() || a.drill_slot.is_some() != lib.drill_slot.is_some() {
        out.push(format!("{name} drill shape differs."));
    }
    if a.drill != lib.drill || a.drill_slot != lib.drill_slot {
        out.push(format!("{name} drill size differs."));
    }
    for role in &roles {
        if a.shape_for_role(role).custom != lib.shape_for_role(role).custom {
            out.push(format!("{name} shape primitives differ on layer {}.", layer_name(role)));
        }
    }
}

/// `PADSTACK::DefaultThermalSpokeAngleForShape`: 90 degrees for an oval or a (rounded, chamfered) rectangle, 45 for the other shapes.
fn default_spoke_angle(shape: PadShape) -> Millideg {
    match shape {
        PadShape::Oval | PadShape::Rect | PadShape::RoundRect | PadShape::ChamferedRect => 90_000,
        _ => 45_000,
    }
}

/// `PAD::GetThermalSpokeAngle()`: the pad's own angle, else [`default_spoke_angle`]'s for its shape.
fn spoke_angle(p: &Pad, local: Option<Millideg>) -> Millideg {
    local.unwrap_or_else(|| default_spoke_angle(p.shape))
}

/// `PAD::GetCustomShapeInZoneOpt()`: a custom pad's `(options (clearance outline|convexhull))`, `OUTLINE` for every pad that is not custom.
fn zone_mode(p: &Pad) -> crate::footprint::CustomZoneMode {
    p.ext.custom.as_ref().map(|c| c.zone).unwrap_or_default()
}

/// `padHasOverrides`: the board's overrides on a pad that the library's pad does not share. Each is told only when the board sets it and the
/// library's value -- its `LibraryFootprint`'s, which `b` alone cannot say -- is not the same; a spoke angle and a custom pad's zone knockout
/// are compared always, defaults included.
fn pad_override_lines(a: &Pad, b: &Pad, o: &PadOverrides, l: &PadOverrides, out: &mut Vec<String>) {
    let name = format!("Pad {}", a.number);
    if o.clearance.is_some() && o.clearance != l.clearance {
        out.push(format!("{name} has clearance override."));
    }
    if o.solder_mask_margin.is_some() && o.solder_mask_margin != l.solder_mask_margin {
        out.push(format!("{name} has solder mask expansion override."));
    }
    // KiCad has one `if` for the absolute margin and one for the relative, both saying the same words.
    if o.solder_paste_margin.is_some() && o.solder_paste_margin != l.solder_paste_margin {
        out.push(format!("{name} has solder paste clearance override."));
    }
    if o.solder_paste_margin_ratio.is_some() && o.solder_paste_margin_ratio != l.solder_paste_margin_ratio {
        out.push(format!("{name} has solder paste clearance override."));
    }
    if o.zone_connection.is_some() && o.zone_connection != l.zone_connection {
        out.push(format!("{name} has zone connection override."));
    }
    // `GetLocalThermalGapOverride().has_value() && GetThermalGap() != GetThermalGap()`: the gate is the board's local value, the values
    // compared are the effective ones (`GetThermalGap()` is 0 for a pad that sets none).
    if o.thermal_gap.is_some() && o.thermal_gap.unwrap_or(Nm::ZERO) != l.thermal_gap.unwrap_or(Nm::ZERO) {
        out.push(format!("{name} has thermal relief gap override."));
    }
    if o.thermal_spoke_width.is_some() && o.thermal_spoke_width != l.thermal_spoke_width {
        out.push(format!("{name} has thermal relief spoke width override."));
    }
    if spoke_angle(a, o.thermal_spoke_angle_mdeg) != spoke_angle(b, l.thermal_spoke_angle_mdeg) {
        out.push(format!("{name} has thermal relief spoke angle override."));
    }
    if zone_mode(a) != zone_mode(b) {
        out.push(format!("{name} has zone knockout setting override."));
    }
}

/// The type a footprint's pads give it (`GetLikelyAttribute`): through-hole when any pad has a hole, else SMD when any pad is.
fn likely_kind(fp: &Footprint) -> FootprintKind {
    FootprintAttrs::derived(fp, false, false).kind
}

/// The courtyard of a footprint as outlines: its real outlines, else the box the placer uses.
fn courtyard_of(fp: &Footprint) -> Vec<Vec<Point>> {
    if !fp.courtyard_outlines.is_empty() {
        return fp.courtyard_outlines.iter().map(|o| o.iter().map(|&(x, y)| Point { x, y }).collect()).collect();
    }
    match fp.courtyard {
        Some((hw, hh)) => vec![vec![Point { x: -hw, y: -hh }, Point { x: hw, y: -hh }, Point { x: hw, y: hh }, Point { x: -hw, y: hh }]],
        None => Vec::new(),
    }
}

/// `FootprintNeedsUpdate( aLibFP, 0, reporter )`: every way the footprint the board has differs from the library's copy (`library`, with the values
/// `library_facts` reads off it) -- empty when it needs no update. Texts are not compared ("Currently we punt and ignore all the text items").
pub fn footprint_differences(instance: &InstanceFacts, library: &Footprint, library_facts: &LibraryFacts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let board = instance.footprint;

    if instance.attrs.kind != likely_kind(library) {
        out.push("Footprint types differ.".to_string());
    }
    // The attributes of a library footprint are none of these: a footprint the schematic marks, or the board edits, differs
    let a = &instance.attrs;
    for (set, what) in [(a.board_only, "Not in schematic"), (a.exclude_from_pos_files, "Exclude from position files"), (a.exclude_from_bom, "Exclude from bill of materials"), (a.dnp, "Do not populate")] {
        if set {
            out.push(format!("'{what}' settings differ."));
        }
    }
    // "Clearance and zone connection overrides are as likely to be set at the board level as in the library ... we just do it when we have a
    // reporter": each is told only when the board sets it and the library's differs.
    let (mine, theirs) = (&instance.overrides, &library_facts.overrides);
    if mine.clearance.is_some() && mine.clearance != theirs.clearance {
        out.push("Pad clearance overridden.".to_string());
    }
    if mine.solder_mask_margin.is_some() && mine.solder_mask_margin != theirs.solder_mask_margin {
        out.push("Solder mask expansion overridden.".to_string());
    }
    if mine.solder_paste_margin.is_some() && mine.solder_paste_margin != theirs.solder_paste_margin {
        out.push("Solder paste absolute clearance overridden.".to_string());
    }
    if mine.solder_paste_margin_ratio.is_some() && mine.solder_paste_margin_ratio != theirs.solder_paste_margin_ratio {
        out.push("Solder paste relative clearance overridden.".to_string());
    }
    if mine.zone_connection.is_some() && mine.zone_connection != theirs.zone_connection {
        out.push("Zone connection overridden.".to_string());
    }

    // graphics (and the courtyard, which KiCad keeps among them)
    let mut mine: Vec<&FpShape> = board.art.shapes.iter().collect();
    let mut theirs: Vec<&FpShape> = library.art.shapes.iter().collect();
    mine.sort_by_key(|s| shape_key(s));
    theirs.sort_by_key(|s| shape_key(s));
    if mine.len() != theirs.len() {
        out.push("Graphic item count differs.".to_string());
    } else {
        for (m, t) in mine.iter().zip(&theirs) {
            if shape_differs(m, t) {
                out.push(format!("{} differs.", describe(m)));
            }
        }
    }
    let (mc, lc) = (courtyard_of(board), courtyard_of(library));
    if mc.len() != lc.len() {
        out.push("Courtyard differs.".to_string());
    } else if !mc.iter().all(|m| lc.iter().any(|l| same_outline(m, l))) {
        out.push("Courtyard differs.".to_string());
    }

    // pads
    let mut mine: Vec<&Pad> = board.pads.iter().collect();
    let mut theirs: Vec<&Pad> = library.pads.iter().collect();
    mine.sort_by(|a, b| pad_order(a, b));
    theirs.sort_by(|a, b| pad_order(a, b));
    if mine.len() != theirs.len() {
        out.push("Pad count differs.".to_string());
    } else {
        for (m, t) in mine.iter().zip(&theirs) {
            pad_lines(m, t, &mut out);
            // with a reporter the board's overrides on the pad are told too, against the library pad's own values
            let none = PadOverrides::default();
            let o = instance.pad_overrides.iter().find(|o| o.number == m.number).unwrap_or(&none);
            let l = library_facts.pad_overrides.iter().find(|o| o.number == t.number).unwrap_or(&none);
            pad_override_lines(m, t, o, l, &mut out);
        }
    }
    // the 3D models are not part of the test (`FootprintNeedsUpdate` leaves them to "Update 3D models"), but a board that has none of the library's does not match it
    out
}

/// Does the footprint the board has need an update from the library (`FootprintNeedsUpdate( aLibFP )`)?
pub fn needs_update(instance: &InstanceFacts, library: &Footprint, library_facts: &LibraryFacts) -> bool {
    !footprint_differences(instance, library, library_facts).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::PadKind;
    use crate::fp_art::FpArt;

    fn pad(number: &str, x: Nm, y: Nm) -> Pad {
        Pad { number: number.into(), at: (x, y), size: (Nm::new(900_000), Nm::new(900_000)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
    }

    fn line(layer: &str, a: (Nm, Nm), b: (Nm, Nm)) -> FpShape {
        FpShape::new(Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: Nm::new(120_000), filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } })
    }

    fn fp(pads: Vec<Pad>, shapes: Vec<FpShape>) -> Footprint {
        Footprint { name: "X".into(), pads, art: FpArt { shapes, texts: vec![], courtyard: vec![] }, courtyard: Some((Nm::new(1_700_000), Nm::new(800_000))), ..Default::default() }
    }

    fn facts(f: &Footprint) -> InstanceFacts<'_> {
        InstanceFacts { footprint: f, attrs: FootprintAttrs::derived(f, false, false), overrides: Default::default(), pad_overrides: vec![] }
    }

    #[test]
    fn a_footprint_equal_to_its_library_copy_needs_no_update() {
        let lib = fp(vec![pad("1", Nm::new(-1_000_000), Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO)], vec![line("F.SilkS", (Nm::new(-500_000), Nm::new(-700_000)), (Nm::new(500_000), Nm::new(-700_000)))]);
        assert!(footprint_differences(&facts(&lib.clone()), &lib, &LibraryFacts::default()).is_empty());
        assert!(!needs_update(&facts(&lib.clone()), &lib, &LibraryFacts::default()));
    }

    /// `FootprintNeedsUpdate` reports an override only when the board's value is set *and* is not the library's (`GetLocalClearance().has_value()
    /// && GetLocalClearance() != aLibFP->GetLocalClearance()`, `GetLocalZoneConnection() != INHERITED && a != b`). The board's values come from
    /// the footprint's own facts (`Design::pad_zone_facts`, `FootprintExtra`): for a footprint placed from a library they are the library's, so an
    /// unedited footprint must list nothing. The library's values are its `LibraryFootprint`'s -- the engine `Footprint` has no margins.
    #[test]
    fn an_override_equal_to_the_librarys_is_not_reported() {
        let engine = fp(vec![pad("1", Nm::ZERO, Nm::ZERO)], vec![]);
        let mut library = LibraryFootprint::from_engine_footprint(&engine);
        library.pads[0].zone_connection = Some(PadConnection::Full);
        library.kicad.tokens.push("(clearance 0.25)".to_string());
        let library_facts = LibraryFacts::of_library_footprint(&library);
        let mut board = facts(&engine);
        board.overrides.clearance = Some(Nm::new(250_000));
        board.pad_overrides.push(PadOverrides { number: "1".into(), zone_connection: Some(PadConnection::Full), ..Default::default() });
        assert!(footprint_differences(&board, &engine, &library_facts).is_empty(), "the board's overrides are the library's own values: an unedited footprint needs no update");
        // A value other than the library's, and one the library does not set, are still told.
        board.overrides.clearance = Some(Nm::new(300_000));
        board.pad_overrides[0].zone_connection = Some(PadConnection::Thermal);
        assert_eq!(footprint_differences(&board, &engine, &library_facts), ["Pad clearance overridden.", "Pad 1 has zone connection override."]);
    }

    /// The footprint-level solder paste checks `FootprintNeedsUpdate` has and this port lacked, and the pad's spoke angle, which `padHasOverrides`
    /// compares always (`a->GetThermalSpokeAngle() != b->GetThermalSpokeAngle()`, the shape's default where the file says none).
    #[test]
    fn the_paste_margins_and_the_spoke_angle_are_compared_too() {
        let engine = fp(vec![pad("1", Nm::ZERO, Nm::ZERO)], vec![]);
        let mut board = facts(&engine);
        board.overrides.solder_paste_margin = Some(Nm::new(-10_000));
        board.overrides.solder_paste_margin_ratio = Some(0.05);
        let library = LibraryFacts::default();
        assert_eq!(footprint_differences(&board, &engine, &library), ["Solder paste absolute clearance overridden.", "Solder paste relative clearance overridden."]);
        // A rectangle pad's default is 90 degrees: a library pad whose file says 45 differs from the board's, which says nothing.
        let mut lib = LibraryFootprint::from_engine_footprint(&engine);
        lib.pads[0].thermal_spoke_angle_mdeg = Some(45_000);
        let library = LibraryFacts::of_library_footprint(&lib);
        assert_eq!(footprint_differences(&facts(&engine), &engine, &library), ["Pad 1 has thermal relief spoke angle override."]);
        // The library's own 90 degrees is the rectangle's default: nothing differs.
        lib.pads[0].thermal_spoke_angle_mdeg = Some(90_000);
        assert!(footprint_differences(&facts(&engine), &engine, &LibraryFacts::of_library_footprint(&lib)).is_empty());
    }

    #[test]
    fn the_lines_follow_kicads_order_and_wording() {
        let lib = fp(vec![pad("1", Nm::new(-1_000_000), Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO)], vec![line("F.SilkS", (Nm::new(-500_000), Nm::new(-700_000)), (Nm::new(500_000), Nm::new(-700_000)))]);
        let mut board = lib.clone();
        board.pads[1].size = (Nm::new(1_100_000), Nm::new(900_000));
        board.pads[0].at = (Nm::new(-1_200_000), Nm::ZERO);
        board.art.shapes[0] = line("F.SilkS", (Nm::new(-500_000), Nm::new(-700_000)), (Nm::new(600_000), Nm::new(-700_000)));
        let mut f = facts(&board);
        f.attrs.dnp = true;
        f.overrides.clearance = Some(Nm::new(250_000));
        f.pad_overrides.push(PadOverrides { number: "2".into(), clearance: Some(Nm::new(300_000)), ..Default::default() });
        let lines = footprint_differences(&f, &lib, &LibraryFacts::default());
        assert_eq!(
            lines,
            [
                "'Do not populate' settings differ.",
                "Pad clearance overridden.",
                "Line on F.SilkS differs.",
                "Pad 1 position differs.",
                "Pad 2 size differs on layer F.Cu.",
                "Pad 2 has clearance override.",
            ]
        );
    }

    #[test]
    fn a_count_that_differs_is_one_line_and_the_items_are_not_paired() {
        let lib = fp(vec![pad("1", Nm::new(-1_000_000), Nm::ZERO), pad("2", Nm::new(1_000_000), Nm::ZERO)], vec![]);
        let mut board = lib.clone();
        board.pads.push(pad("3", Nm::ZERO, Nm::new(1_000_000)));
        board.art.shapes.push(line("F.Fab", (Nm::ZERO, Nm::ZERO), (Nm::new(1000), Nm::new(1000))));
        assert_eq!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()), ["Graphic item count differs.", "Pad count differs."]);
    }

    #[test]
    fn the_footprint_type_is_the_one_the_pads_give_and_a_hole_makes_it_through_hole() {
        let lib = fp(vec![pad("1", Nm::ZERO, Nm::ZERO)], vec![]);
        let mut th = lib.clone();
        th.pads[0].kind = PadKind::ThroughHole;
        th.pads[0].drill = Some(Nm::new(800_000));
        let lines = footprint_differences(&facts(&th), &lib, &LibraryFacts::default());
        assert!(lines.contains(&"Footprint types differ.".to_string()), "{lines:?}");
        assert!(lines.contains(&"Pad 1 pad type differs.".to_string()) && lines.contains(&"Pad 1 drill shape differs.".to_string()) && lines.contains(&"Pad 1 drill size differs.".to_string()), "{lines:?}");
    }

    #[test]
    fn a_polygon_started_at_another_vertex_and_a_rectangle_drawn_the_other_way_are_the_same() {
        let square = |pts: &[(Nm, Nm)]| FpShape::new(Shape::Polygon { id: String::new(), layer: "F.SilkS".into(), stroke_width: Nm::new(100_000), filled: false, pts: pts.iter().map(|&(x, y)| Point { x, y }).collect() });
        let rect = |a: (Nm, Nm), b: (Nm, Nm)| FpShape::new(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: Nm::new(100_000), filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } });
        let lib = fp(vec![pad("1", Nm::ZERO, Nm::ZERO)], vec![square(&[(Nm::ZERO, Nm::ZERO), (Nm::new(10_000), Nm::ZERO), (Nm::new(10_000), Nm::new(10_000)), (Nm::ZERO, Nm::new(10_000))]), rect((Nm::ZERO, Nm::ZERO), (Nm::new(10_000), Nm::new(10_000)))]);
        let mut board = lib.clone();
        board.art.shapes = vec![square(&[(Nm::new(10_000), Nm::new(10_000)), (Nm::ZERO, Nm::new(10_000)), (Nm::ZERO, Nm::ZERO), (Nm::new(10_000), Nm::ZERO)]), rect((Nm::new(10_000), Nm::new(10_000)), (Nm::ZERO, Nm::ZERO))];
        assert!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()).is_empty());
        board.art.shapes[0] = square(&[(Nm::ZERO, Nm::ZERO), (Nm::new(10_000), Nm::ZERO), (Nm::new(10_000), Nm::new(11_000)), (Nm::ZERO, Nm::new(10_000))]);
        assert_eq!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()), ["Polygon on F.SilkS differs."]);
    }

    #[test]
    fn a_stroke_only_counts_on_copper() {
        let lib = fp(vec![pad("1", Nm::ZERO, Nm::ZERO)], vec![line("F.SilkS", (Nm::ZERO, Nm::ZERO), (Nm::new(1000), Nm::new(1000))), line("F.Cu", (Nm::ZERO, Nm::ZERO), (Nm::new(5000), Nm::new(5000)))]);
        let mut board = lib.clone();
        board.art.shapes[0].shape.set_stroke_width(Nm::new(300_000));
        assert!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()).is_empty(), "a silkscreen line may be thicker");
        board.art.shapes[1].shape.set_stroke_width(Nm::new(300_000));
        assert_eq!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()), ["Line on F.Cu differs."]);
    }

    #[test]
    fn pads_are_paired_by_number_the_way_a_person_counts() {
        let lib = fp(vec![pad("2", Nm::ZERO, Nm::ZERO), pad("10", Nm::new(1_000_000), Nm::ZERO), pad("1", Nm::new(-1_000_000), Nm::ZERO)], vec![]);
        let mut board = lib.clone();
        board.pads.reverse();
        assert!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()).is_empty());
        assert_eq!(natural_cmp("2", "10"), Ordering::Less);
        assert_eq!(natural_cmp("A1", "A10"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "A1"), Ordering::Greater, "lower case after upper on a tie");
    }

    #[test]
    fn the_courtyard_is_compared_too() {
        let lib = fp(vec![pad("1", Nm::ZERO, Nm::ZERO)], vec![]);
        let mut board = lib.clone();
        board.courtyard = Some((Nm::new(1_800_000), Nm::new(800_000)));
        assert_eq!(footprint_differences(&facts(&board), &lib, &LibraryFacts::default()), ["Courtyard differs."]);
    }
}
