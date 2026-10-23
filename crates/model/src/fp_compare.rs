//! Does a footprint on the board differ from the library's copy of it? `FOOTPRINT::FootprintNeedsUpdate` (`pcbnew/drc/drc_test_provider_library_parity.cpp` at KiCad
//! 8303b2ad), the test behind Update Footprint from Library ("(no changes)") and Compare Footprint with Library (`BOARD_INSPECTION_TOOL::DiffFootprint`, whose
//! summary page is the reporter's lines).
//!
//! KiCad's DRC runs the same test as the "footprint does not match copy in library" violation; that stays kicad-cli's. This is the interactive tool's list, over the
//! studio's model: the footprint as the board has it ([`InstanceFacts`]: its pads with the edits laid on, its graphics, its attributes and its overrides) against the library's
//! [`Footprint`]. What the model keeps of a library footprint is its pads, graphics, courtyard and models; the file's attribute flags, net-tie groups and rule areas it does
//! not keep, so those are not compared (the footprint type is the one the pads give).
//!
//! The order of the lines is KiCad's: footprint type, fabrication attributes, overrides, graphics, pads.

use crate::footprint::{Footprint, Pad, PadShape};
use crate::fp_art::FpShape;
use crate::fp_edit::{FootprintAttrs, FootprintKind};
use crate::ir::{PadConnection, Point, Shape, Um};
use std::cmp::Ordering;

/// The footprint-level overrides the board has on a footprint (`GetLocalClearance`, `GetLocalSolderMaskMargin`, `GetLocalZoneConnection`), `None` for one it does not set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FootprintOverrides {
    pub clearance: Option<Um>,
    pub solder_mask_margin: Option<Um>,
    pub zone_connection: Option<PadConnection>,
}

/// The overrides of one pad (`padHasOverrides`), by its number.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadOverrides {
    pub number: String,
    pub clearance: Option<Um>,
    pub solder_mask_margin: Option<Um>,
    pub solder_paste_margin: Option<Um>,
    pub solder_paste_margin_ratio: Option<f64>,
    pub zone_connection: Option<PadConnection>,
    pub thermal_gap: Option<Um>,
    pub thermal_spoke_width: Option<Um>,
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
fn shape_key(s: &FpShape) -> (String, u8, Vec<(Um, Um)>, Um) {
    let pts: Vec<Point> = match &s.shape {
        Shape::Segment { start, end, .. } | Shape::Rect { start, end, .. } => vec![*start, *end],
        Shape::Arc { start, mid, end, .. } => vec![*start, *end, *mid],
        Shape::Circle { center, end, .. } => vec![*center, *end],
        Shape::Polygon { pts, .. } => {
            let mut v = vec![Point { x: pts.len() as Um, y: 0 }];
            v.extend(pts.iter().copied());
            v
        }
        Shape::Bezier { start, c1, c2, end, .. } => vec![*start, *end, *c1, *c2],
    };
    (s.layer().to_string(), kind_rank(&s.shape), pts.into_iter().map(|p| (p.x, p.y)).collect(), s.shape.stroke_width())
}

/// Two outlines are the same when one is the other started at another vertex (`SHAPE_LINE_CHAIN::CompareGeometry`, cyclical).
fn same_outline(a: &[Point], b: &[Point]) -> bool {
    a.len() == b.len() && (a.is_empty() || (0..a.len()).any(|shift| (0..a.len()).all(|i| a[i] == b[(i + shift) % a.len()])))
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
            let norm = |s: &Point, e: &Point| (s.x.min(e.x), s.y.min(e.y), s.x.max(e.x), s.y.max(e.y));
            norm(s1, e1) != norm(s2, e2)
        }
        (Shape::Segment { start: s1, end: e1, .. }, Shape::Segment { start: s2, end: e2, .. }) => s1 != s2 || e1 != e2,
        (Shape::Circle { center: c1, end: e1, .. }, Shape::Circle { center: c2, end: e2, .. }) => c1 != c2 || e1 != e2,
        // "Arc center is calculated and so may have round-off errors": the middle may be 0.5 um apart
        (Shape::Arc { start: s1, mid: m1, end: e1, .. }, Shape::Arc { start: s2, mid: m2, end: e2, .. }) => s1 != s2 || e1 != e2 || ((m1.x - m2.x) as f64).hypot((m1.y - m2.y) as f64) > 0.5,
        (Shape::Bezier { start: s1, c1: a1, c2: b1, end: e1, .. }, Shape::Bezier { start: s2, c1: a2, c2: b2, end: e2, .. }) => s1 != s2 || e1 != e2 || a1 != a2 || b1 != b2,
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
    if a.at != lib.at {
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
        if sa.trapezoid_delta.unwrap_or((0, 0)) != sl.trapezoid_delta.unwrap_or((0, 0)) {
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
        if sa.offset.unwrap_or((0, 0)) != sl.offset.unwrap_or((0, 0)) {
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

/// `padHasOverrides`: what the board sets on a pad that the library's pad does not.
fn pad_override_lines(number: &str, o: &PadOverrides, out: &mut Vec<String>) {
    let name = format!("Pad {number}");
    if o.clearance.is_some() {
        out.push(format!("{name} has clearance override."));
    }
    if o.solder_mask_margin.is_some() {
        out.push(format!("{name} has solder mask expansion override."));
    }
    if o.solder_paste_margin.is_some() || o.solder_paste_margin_ratio.is_some() {
        out.push(format!("{name} has solder paste clearance override."));
    }
    if o.zone_connection.is_some() {
        out.push(format!("{name} has zone connection override."));
    }
    if o.thermal_gap.is_some() {
        out.push(format!("{name} has thermal relief gap override."));
    }
    if o.thermal_spoke_width.is_some() {
        out.push(format!("{name} has thermal relief spoke width override."));
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

/// `FootprintNeedsUpdate( aLibFP, 0, reporter )`: every way the footprint the board has differs from the library's copy -- empty when it needs no update. Texts are not
/// compared ("Currently we punt and ignore all the text items").
pub fn footprint_differences(instance: &InstanceFacts, library: &Footprint) -> Vec<String> {
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
    // "Clearance and zone connection overrides are as likely to be set at the board level as in the library"
    if instance.overrides.clearance.is_some() {
        out.push("Pad clearance overridden.".to_string());
    }
    if instance.overrides.solder_mask_margin.is_some() {
        out.push("Solder mask expansion overridden.".to_string());
    }
    if instance.overrides.zone_connection.is_some() {
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
            // with a reporter the board's overrides on the pad are told too
            if let Some(o) = instance.pad_overrides.iter().find(|o| o.number == m.number) {
                pad_override_lines(&m.number, o, &mut out);
            }
        }
    }
    // the 3D models are not part of the test (`FootprintNeedsUpdate` leaves them to "Update 3D models"), but a board that has none of the library's does not match it
    out
}

/// Does the footprint the board has need an update from the library (`FootprintNeedsUpdate( aLibFP )`)?
pub fn needs_update(instance: &InstanceFacts, library: &Footprint) -> bool {
    !footprint_differences(instance, library).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::PadKind;
    use crate::fp_art::FpArt;

    fn pad(number: &str, x: Um, y: Um) -> Pad {
        Pad { number: number.into(), at: (x, y), size: (900, 900), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
    }

    fn line(layer: &str, a: (Um, Um), b: (Um, Um)) -> FpShape {
        FpShape::new(Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: 120, filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } })
    }

    fn fp(pads: Vec<Pad>, shapes: Vec<FpShape>) -> Footprint {
        Footprint { name: "X".into(), pads, art: FpArt { shapes, texts: vec![] }, courtyard: Some((1_700, 800)), ..Default::default() }
    }

    fn facts(f: &Footprint) -> InstanceFacts<'_> {
        InstanceFacts { footprint: f, attrs: FootprintAttrs::derived(f, false, false), overrides: Default::default(), pad_overrides: vec![] }
    }

    #[test]
    fn a_footprint_equal_to_its_library_copy_needs_no_update() {
        let lib = fp(vec![pad("1", -1_000, 0), pad("2", 1_000, 0)], vec![line("F.SilkS", (-500, -700), (500, -700))]);
        assert!(footprint_differences(&facts(&lib.clone()), &lib).is_empty());
        assert!(!needs_update(&facts(&lib.clone()), &lib));
    }

    #[test]
    fn the_lines_follow_kicads_order_and_wording() {
        let lib = fp(vec![pad("1", -1_000, 0), pad("2", 1_000, 0)], vec![line("F.SilkS", (-500, -700), (500, -700))]);
        let mut board = lib.clone();
        board.pads[1].size = (1_100, 900);
        board.pads[0].at = (-1_200, 0);
        board.art.shapes[0] = line("F.SilkS", (-500, -700), (600, -700));
        let mut f = facts(&board);
        f.attrs.dnp = true;
        f.overrides.clearance = Some(250);
        f.pad_overrides.push(PadOverrides { number: "2".into(), clearance: Some(300), ..Default::default() });
        let lines = footprint_differences(&f, &lib);
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
        let lib = fp(vec![pad("1", -1_000, 0), pad("2", 1_000, 0)], vec![]);
        let mut board = lib.clone();
        board.pads.push(pad("3", 0, 1_000));
        board.art.shapes.push(line("F.Fab", (0, 0), (1, 1)));
        assert_eq!(footprint_differences(&facts(&board), &lib), ["Graphic item count differs.", "Pad count differs."]);
    }

    #[test]
    fn the_footprint_type_is_the_one_the_pads_give_and_a_hole_makes_it_through_hole() {
        let lib = fp(vec![pad("1", 0, 0)], vec![]);
        let mut th = lib.clone();
        th.pads[0].kind = PadKind::ThroughHole;
        th.pads[0].drill = Some(800);
        let lines = footprint_differences(&facts(&th), &lib);
        assert!(lines.contains(&"Footprint types differ.".to_string()), "{lines:?}");
        assert!(lines.contains(&"Pad 1 pad type differs.".to_string()) && lines.contains(&"Pad 1 drill shape differs.".to_string()) && lines.contains(&"Pad 1 drill size differs.".to_string()), "{lines:?}");
    }

    #[test]
    fn a_polygon_started_at_another_vertex_and_a_rectangle_drawn_the_other_way_are_the_same() {
        let square = |pts: &[(Um, Um)]| FpShape::new(Shape::Polygon { id: String::new(), layer: "F.SilkS".into(), stroke_width: 100, filled: false, pts: pts.iter().map(|&(x, y)| Point { x, y }).collect() });
        let rect = |a: (Um, Um), b: (Um, Um)| FpShape::new(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } });
        let lib = fp(vec![pad("1", 0, 0)], vec![square(&[(0, 0), (10, 0), (10, 10), (0, 10)]), rect((0, 0), (10, 10))]);
        let mut board = lib.clone();
        board.art.shapes = vec![square(&[(10, 10), (0, 10), (0, 0), (10, 0)]), rect((10, 10), (0, 0))];
        assert!(footprint_differences(&facts(&board), &lib).is_empty());
        board.art.shapes[0] = square(&[(0, 0), (10, 0), (10, 11), (0, 10)]);
        assert_eq!(footprint_differences(&facts(&board), &lib), ["Polygon on F.SilkS differs."]);
    }

    #[test]
    fn a_stroke_only_counts_on_copper() {
        let lib = fp(vec![pad("1", 0, 0)], vec![line("F.SilkS", (0, 0), (1, 1)), line("F.Cu", (0, 0), (5, 5))]);
        let mut board = lib.clone();
        board.art.shapes[0].shape.set_stroke_width(300);
        assert!(footprint_differences(&facts(&board), &lib).is_empty(), "a silkscreen line may be thicker");
        board.art.shapes[1].shape.set_stroke_width(300);
        assert_eq!(footprint_differences(&facts(&board), &lib), ["Line on F.Cu differs."]);
    }

    #[test]
    fn pads_are_paired_by_number_the_way_a_person_counts() {
        let lib = fp(vec![pad("2", 0, 0), pad("10", 1_000, 0), pad("1", -1_000, 0)], vec![]);
        let mut board = lib.clone();
        board.pads.reverse();
        assert!(footprint_differences(&facts(&board), &lib).is_empty());
        assert_eq!(natural_cmp("2", "10"), Ordering::Less);
        assert_eq!(natural_cmp("A1", "A10"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "A1"), Ordering::Greater, "lower case after upper on a tie");
    }

    #[test]
    fn the_courtyard_is_compared_too() {
        let lib = fp(vec![pad("1", 0, 0)], vec![]);
        let mut board = lib.clone();
        board.courtyard = Some((1_800, 800));
        assert_eq!(footprint_differences(&facts(&board), &lib), ["Courtyard differs."]);
    }
}
