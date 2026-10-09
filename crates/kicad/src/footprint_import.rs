//! `.kicad_mod` text -> an editable [`LibraryFootprint`]: the reader behind the Footprint Editor's
//! Import Footprint (`FOOTPRINT_EDIT_FRAME::ImportFootprint`, pcbnew/footprint_libraries_utils.cpp).
//!
//! `footprint_lib::parse_footprint_file` already reads these files, but into the engine's `Footprint`,
//! which keeps only the pads (without their layers or offsets) and a courtyard box -- no silkscreen, no
//! text, no attributes, no description. This reader is the inverse of
//! [`export_kicad_mod`](crate::export_kicad_mod): everything that writer emits comes back, so an
//! exported footprint imports unchanged; a real KiCad file keeps its graphics, text, attributes, tags and
//! pad layers, and loses only what the editable type cannot hold -- a custom pad's primitives, which the
//! reader reports in `warnings` rather than dropping silently.
//!
//! The pad grammar is `PCB_IO_KICAD_SEXPR_PARSER::parsePAD`'s: a pad is `(pad "N" <type> <shape> (at x y
//! [angle]) (size w h) [(offset x y)] [(drill ..)] (layers ..) [(roundrect_rratio r)] [(chamfer_ratio r)
//! (chamfer <corners>)] [(rect_delta dx dy)] [(clearance c)] ..)`; a chamfered rectangle is written by KiCad
//! as `roundrect` plus the chamfer tokens.

use eda_model::footprint::PadKind;
use eda_model::ir::{ChamferCorners, FootprintAttributes, FootprintField, LibraryFootprint, LibraryPad, LibraryPadShape, Point, Shape, Side, Text, TextJustify};

use crate::import::mm_to_um;
use crate::sexpr::{self, Sexpr};

/// What [`parse_library_footprint`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedFootprint {
    pub footprint: LibraryFootprint,
    /// One line per kind of item that could not be represented and was approximated or left out.
    pub warnings: Vec<String>,
}

fn point(list: &[Sexpr], tag: &str) -> Option<Point> {
    let p = sexpr::find(list, tag)?;
    Some(Point { x: mm_to_um(sexpr::num(p, 1)?), y: mm_to_um(sexpr::num(p, 2)?) })
}

/// `(hide yes)` or a bare `hide`.
fn hidden(list: &[Sexpr]) -> bool {
    list.iter().skip(1).any(|c| match c {
        Sexpr::Atom(a) => a == "hide",
        Sexpr::List(l) => sexpr::tag(l) == Some("hide") && sexpr::txt(l, 1).map(|v| v == "yes").unwrap_or(true),
    })
}

/// A stroke width in um: `(stroke (width w))`, else the older `(width w)`, else KiCad's 0.12 mm default.
fn stroke_width(item: &[Sexpr]) -> i64 {
    let w = sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).or_else(|| sexpr::find(item, "width")).and_then(|w| sexpr::num(w, 1));
    mm_to_um(w.unwrap_or(0.12))
}

/// `(fill yes|solid)` / `(fill (type solid|outline|...))`.
fn filled(item: &[Sexpr]) -> bool {
    match sexpr::find(item, "fill") {
        Some(f) => match sexpr::txt(f, 1) {
            Some(t) => matches!(t, "yes" | "solid" | "outline" | "background" | "color"),
            None => sexpr::find(f, "type").and_then(|t| sexpr::txt(t, 1)).map(|t| t != "none").unwrap_or(false),
        },
        None => false,
    }
}

fn layer_of(item: &[Sexpr]) -> String {
    sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.SilkS").to_string()
}

/// Parse a `.kicad_mod` file (`(footprint "Name" ...)`, or the KiCad 6 `(module ...)`).
pub fn parse_library_footprint(text: &str) -> Result<ParsedFootprint, String> {
    let tree = sexpr::parse(text).map_err(|e| format!("not a valid s-expression file: {e}"))?;
    let root = tree.as_list().filter(|l| matches!(sexpr::tag(l), Some("footprint") | Some("module"))).ok_or("the top-level form is not (footprint ...); not a KiCad footprint file")?;
    let name = sexpr::txt(root, 1).ok_or("the footprint has no name")?;
    let mut warnings = Vec::new();

    let mut fp = LibraryFootprint::new_empty(name);
    fp.description = sexpr::find(root, "descr").and_then(|d| sexpr::txt(d, 1)).unwrap_or("").to_string();
    fp.keywords = sexpr::find(root, "tags").and_then(|d| sexpr::txt(d, 1)).unwrap_or("").to_string();
    fp.attributes = attributes(root);
    fp.zone_connection = sexpr::find(root, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64));
    fp.model = crate::footprint_lib::model_from(root);
    fp.courtyard = crate::footprint_lib::courtyard_from(root);

    for child in root.iter().skip(2).filter_map(Sexpr::as_list) {
        match sexpr::tag(child) {
            Some("pad") => {
                if let Some(pad) = parse_pad(child, &mut warnings) {
                    fp.pads.push(pad);
                }
            }
            Some("fp_line") => {
                if let (Some(start), Some(end)) = (point(child, "start"), point(child, "end")) {
                    fp.graphics.push(Shape::Segment { id: String::new(), layer: layer_of(child), stroke_width: stroke_width(child), filled: false, start, end });
                }
            }
            Some("fp_arc") => match (point(child, "start"), point(child, "mid"), point(child, "end")) {
                (Some(start), Some(mid), Some(end)) => fp.graphics.push(Shape::Arc { id: String::new(), layer: layer_of(child), stroke_width: stroke_width(child), filled: false, start, mid, end }),
                _ => warnings.push("arcs in the pre-KiCad-7 (centre + angle) format were left out".to_string()),
            },
            Some("fp_rect") => {
                if let (Some(start), Some(end)) = (point(child, "start"), point(child, "end")) {
                    fp.graphics.push(Shape::Rect { id: String::new(), layer: layer_of(child), stroke_width: stroke_width(child), filled: filled(child), start, end });
                }
            }
            Some("fp_circle") => {
                if let (Some(center), Some(end)) = (point(child, "center"), point(child, "end")) {
                    fp.graphics.push(Shape::Circle { id: String::new(), layer: layer_of(child), stroke_width: stroke_width(child), filled: filled(child), center, end });
                }
            }
            Some("fp_poly") => {
                let pts = xy_points(child);
                if pts.len() >= 3 {
                    fp.graphics.push(Shape::Polygon { id: String::new(), layer: layer_of(child), stroke_width: stroke_width(child), filled: filled(child), pts });
                }
            }
            Some("fp_curve") => {
                let pts = xy_points(child);
                if pts.len() == 4 {
                    fp.graphics.push(Shape::Bezier { id: String::new(), layer: layer_of(child), stroke_width: stroke_width(child), filled: false, start: pts[0], c1: pts[1], c2: pts[2], end: pts[3] });
                }
            }
            Some("fp_text") => match sexpr::txt(child, 1) {
                Some("user") => fp.texts.extend(parse_text(child, sexpr::txt(child, 2).unwrap_or(""))),
                Some("reference") => fp.reference_visible = !hidden(child),
                Some("value") => fp.value_visible = !hidden(child),
                _ => {}
            },
            Some("property") => {
                let (key, value) = (sexpr::txt(child, 1).unwrap_or(""), sexpr::txt(child, 2).unwrap_or(""));
                match key {
                    "Reference" => fp.reference_visible = !hidden(child),
                    "Value" => fp.value_visible = !hidden(child),
                    "Description" => {
                        if fp.description.is_empty() {
                            fp.description = value.to_string();
                        }
                    }
                    "Datasheet" | "" => {}
                    _ => fp.fields.push(FootprintField { name: key.to_string(), value: value.to_string(), visible: !hidden(child) }),
                }
            }
            Some("zone") | Some("group") | Some("image") | Some("dimension") | Some("fp_text_box") | Some("fp_text_private") => warnings.push(format!("{} items are not supported by the library footprint and were left out", sexpr::tag(child).unwrap_or("?"))),
            _ => {}
        }
    }
    if fp.pads.is_empty() && fp.graphics.is_empty() && fp.texts.is_empty() {
        warnings.push("the footprint holds no pad, graphic or text".to_string());
    }
    fp.assign_missing_ids();
    warnings.sort();
    warnings.dedup();
    Ok(ParsedFootprint { footprint: fp, warnings })
}

fn xy_points(item: &[Sexpr]) -> Vec<Point> {
    sexpr::find(item, "pts").map(|pts| sexpr::find_all(pts, "xy").filter_map(|xy| Some(Point { x: mm_to_um(sexpr::num(xy, 1)?), y: mm_to_um(sexpr::num(xy, 2)?) })).collect()).unwrap_or_default()
}

/// `(attr smd through_hole board_only exclude_from_pos_files exclude_from_bom allow_missing_courtyard
/// allow_solder_mask_bridges dnp)`.
fn attributes(root: &[Sexpr]) -> FootprintAttributes {
    let mut a = FootprintAttributes::default();
    if let Some(attr) = sexpr::find(root, "attr") {
        for tok in attr.iter().skip(1).filter_map(Sexpr::text) {
            match tok {
                "smd" => a.smd = true,
                "through_hole" => a.through_hole = true,
                "board_only" => a.board_only = true,
                "exclude_from_pos_files" => a.exclude_from_position_files = true,
                "exclude_from_bom" => a.exclude_from_bom = true,
                "allow_missing_courtyard" => a.allow_missing_courtyard = true,
                "allow_solder_mask_bridges" | "allow_soldermask_bridges" => a.allow_soldermask_bridges = true,
                "dnp" => a.dnp = true,
                _ => {}
            }
        }
    }
    a
}

fn parse_text(item: &[Sexpr], content: &str) -> Option<Text> {
    let at = sexpr::find(item, "at")?;
    let (x, y) = (sexpr::num(at, 1)?, sexpr::num(at, 2)?);
    let effects = sexpr::find(item, "effects");
    let font = effects.and_then(|e| sexpr::find(e, "font"));
    let size_mm = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).unwrap_or(1.0);
    let thickness_mm = font.and_then(|f| sexpr::find(f, "thickness")).and_then(|t| sexpr::num(t, 1)).unwrap_or(size_mm / 8.0);
    let justify = effects.and_then(|e| sexpr::find(e, "justify"));
    let has = |tok: &str| justify.is_some_and(|j| j.iter().skip(1).filter_map(Sexpr::text).any(|t| t == tok));
    Some(Text {
        id: String::new(),
        content: content.to_string(),
        at: Point { x: mm_to_um(x), y: mm_to_um(y) },
        angle: ((sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round() as i64).rem_euclid(360_000) as u32,
        layer: layer_of(item),
        size_um: mm_to_um(size_mm).max(1),
        stroke_width: mm_to_um(thickness_mm),
        justify: if has("left") {
            TextJustify::Left
        } else if has("right") {
            TextJustify::Right
        } else {
            TextJustify::Center
        },
        mirror: has("mirror"),
    })
}

fn parse_pad(pad: &[Sexpr], warnings: &mut Vec<String>) -> Option<LibraryPad> {
    let number = sexpr::txt(pad, 1).unwrap_or("").to_string();
    let kind = match sexpr::txt(pad, 2).unwrap_or("") {
        "np_thru_hole" => PadKind::NonPlatedHole,
        "smd" | "connect" => PadKind::Smd,
        _ => PadKind::ThroughHole,
    };
    let at = sexpr::find(pad, "at")?;
    let size = sexpr::find(pad, "size")?;
    let rratio = sexpr::find(pad, "roundrect_rratio").and_then(|r| sexpr::num(r, 1));
    let chamfer_ratio = sexpr::find(pad, "chamfer_ratio").and_then(|r| sexpr::num(r, 1));
    let corners = sexpr::find(pad, "chamfer").map(|c| {
        let toks: Vec<&str> = c.iter().skip(1).filter_map(Sexpr::text).collect();
        ChamferCorners { top_left: toks.contains(&"top_left"), top_right: toks.contains(&"top_right"), bottom_left: toks.contains(&"bottom_left"), bottom_right: toks.contains(&"bottom_right") }
    });
    let shape = match sexpr::txt(pad, 3).unwrap_or("") {
        "circle" => LibraryPadShape::Circle,
        "oval" => LibraryPadShape::Oval,
        "roundrect" if corners.is_some() || chamfer_ratio.is_some() => LibraryPadShape::ChamferedRect,
        "roundrect" => LibraryPadShape::RoundRect,
        "trapezoid" => LibraryPadShape::Trapezoid,
        "custom" => {
            warnings.push("custom-shaped pads were read as their rectangular anchor (the primitives were left out)".to_string());
            LibraryPadShape::Rect
        }
        _ => LibraryPadShape::Rect,
    };
    let (drill, drill_slot) = match sexpr::find(pad, "drill") {
        Some(d) => {
            let toks: Vec<&str> = d.iter().skip(1).filter_map(Sexpr::text).collect();
            if toks.first() == Some(&"oval") {
                let nums: Vec<f64> = toks[1..].iter().filter_map(|s| s.parse::<f64>().ok()).collect();
                let dw = nums.first().copied().unwrap_or(0.0);
                (None, Some((mm_to_um(dw), mm_to_um(nums.get(1).copied().unwrap_or(dw)))))
            } else {
                (toks.iter().find_map(|s| s.parse::<f64>().ok()).map(mm_to_um), None)
            }
        }
        None => (None, None),
    };
    let um = |tag: &str| sexpr::find(pad, tag).and_then(|c| sexpr::num(c, 1)).map(mm_to_um);
    Some(LibraryPad {
        id: String::new(),
        number,
        at: Point { x: mm_to_um(sexpr::num(at, 1)?), y: mm_to_um(sexpr::num(at, 2)?) },
        size: (mm_to_um(sexpr::num(size, 1)?), mm_to_um(sexpr::num(size, 2)?)),
        offset: point(pad, "offset").unwrap_or_default(),
        shape,
        kind,
        drill,
        drill_slot,
        rot: crate::pad_rot_from_file(Side::Top, 0, sexpr::num(at, 3).unwrap_or(0.0)),
        roundrect_ratio: if shape == LibraryPadShape::RoundRect { Some(rratio.unwrap_or(0.25)) } else { None },
        trapezoid_delta: if shape == LibraryPadShape::Trapezoid { sexpr::find(pad, "rect_delta").map(|d| (mm_to_um(sexpr::num(d, 1).unwrap_or(0.0)), mm_to_um(sexpr::num(d, 2).unwrap_or(0.0)))) } else { None },
        chamfer_ratio: if shape == LibraryPadShape::ChamferedRect { Some(chamfer_ratio.or(rratio).unwrap_or(0.2)) } else { None },
        chamfer_corners: if shape == LibraryPadShape::ChamferedRect { corners.unwrap_or_default() } else { ChamferCorners::default() },
        layers: sexpr::find(pad, "layers").map(|l| l.iter().skip(1).filter_map(Sexpr::text).map(str::to_string).collect()).unwrap_or_default(),
        clearance_override: um("clearance"),
        thermal_gap_override: um("thermal_gap"),
        thermal_spoke_width_override: um("thermal_bridge_width"),
        zone_connection: sexpr::find(pad, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64)),
        thermal_spoke_angle_mdeg: sexpr::find(pad, "thermal_bridge_angle").and_then(|a| sexpr::num(a, 1)).map(|d| ((d * 1000.0).round() as i64).rem_euclid(360_000) as eda_model::ir::Millideg),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export_kicad_mod;

    fn pad(number: &str, kind: PadKind, shape: LibraryPadShape, at: (i64, i64), size: (i64, i64)) -> LibraryPad {
        LibraryPad {
            id: String::new(),
            number: number.into(),
            at: Point { x: at.0, y: at.1 },
            size,
            offset: Point::default(),
            shape,
            kind,
            drill: None,
            drill_slot: None,
            rot: 0,
            roundrect_ratio: None,
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: ChamferCorners::default(),
            layers: vec![],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
        }
    }

    fn sample() -> LibraryFootprint {
        let mut fp = LibraryFootprint::new_empty("Test:Sample");
        fp.description = "a sample".into();
        fp.keywords = "one two".into();
        fp.attributes = FootprintAttributes { smd: true, exclude_from_bom: true, exclude_from_position_files: true, dnp: false, ..FootprintAttributes::default() };
        fp.model = Some("${KICAD9_3DMODEL_DIR}/x.step".into());
        let mut p1 = pad("1", PadKind::Smd, LibraryPadShape::RoundRect, (-1000, 0), (800, 1200));
        p1.roundrect_ratio = Some(0.3);
        p1.rot = 90_000;
        p1.layers = vec!["F.Cu".into(), "F.Mask".into()];
        p1.clearance_override = Some(200);
        let mut p2 = pad("2", PadKind::ThroughHole, LibraryPadShape::Oval, (1000, 0), (1600, 2000));
        p2.drill = Some(800);
        p2.offset = Point { x: 100, y: -50 };
        p2.layers = vec!["*.Cu".into(), "*.Mask".into()];
        let mut p3 = pad("3", PadKind::ThroughHole, LibraryPadShape::Circle, (0, 2000), (1700, 1700));
        p3.drill_slot = Some((600, 1700));
        let p4 = pad("", PadKind::NonPlatedHole, LibraryPadShape::Circle, (0, -2000), (900, 900));
        fp.pads = vec![p1, p2, p3, p4];
        fp.graphics = vec![
            Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: Point { x: -2000, y: -1500 }, end: Point { x: 2000, y: -1500 } },
            Shape::Arc { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: Point { x: 1000, y: 0 }, mid: Point { x: 0, y: 1000 }, end: Point { x: -1000, y: 0 } },
            Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: true, start: Point { x: -1500, y: -1000 }, end: Point { x: 1500, y: 1000 } },
            Shape::Circle { id: String::new(), layer: "F.CrtYd".into(), stroke_width: 50, filled: false, center: Point { x: 0, y: 0 }, end: Point { x: 3000, y: 0 } },
            Shape::Polygon { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: false, pts: vec![Point { x: 0, y: 0 }, Point { x: 500, y: 0 }, Point { x: 500, y: 500 }] },
            Shape::Bezier { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: Point { x: 0, y: 0 }, c1: Point { x: 100, y: 200 }, c2: Point { x: 300, y: 200 }, end: Point { x: 400, y: 0 } },
        ];
        fp.texts = vec![Text { id: String::new(), content: "hello".into(), at: Point { x: 100, y: 3000 }, angle: 90_000, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Left, mirror: true }];
        fp.assign_missing_ids();
        fp
    }

    #[test]
    fn export_then_import_gives_back_everything_the_writer_emits() {
        let fp = sample();
        let parsed = parse_library_footprint(&export_kicad_mod(&fp)).expect("parses");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        let back = parsed.footprint;
        assert_eq!(back.name, "Test:Sample");
        assert_eq!((back.description.as_str(), back.keywords.as_str()), ("a sample", "one two"));
        assert_eq!(back.attributes, fp.attributes);
        assert_eq!(back.model, fp.model);
        assert_eq!(back.pads.len(), 4);
        let by = |n: &str| back.pads.iter().find(|p| p.number == n).unwrap();
        let p1 = by("1");
        assert_eq!((p1.shape, p1.kind, p1.rot, p1.at, p1.size), (LibraryPadShape::RoundRect, PadKind::Smd, 90_000, Point { x: -1000, y: 0 }, (800, 1200)));
        assert_eq!(p1.roundrect_ratio, Some(0.3));
        assert_eq!(p1.layers, vec!["F.Cu".to_string(), "F.Mask".to_string()]);
        assert_eq!(p1.clearance_override, Some(200));
        let p2 = by("2");
        assert_eq!((p2.shape, p2.kind, p2.drill, p2.offset), (LibraryPadShape::Oval, PadKind::ThroughHole, Some(800), Point { x: 100, y: -50 }));
        assert_eq!(by("3").drill_slot, Some((600, 1700)));
        let npth = back.pads.iter().find(|p| p.kind == PadKind::NonPlatedHole).unwrap();
        assert_eq!(npth.number, "");
        assert_eq!(back.graphics.len(), 6);
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Rect { filled: true, layer, .. } if layer == "F.Fab")));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Arc { mid, .. } if *mid == Point { x: 0, y: 1000 })));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Bezier { c2, .. } if *c2 == Point { x: 300, y: 200 })));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Polygon { pts, .. } if pts.len() == 3)));
        assert_eq!(back.texts.len(), 1);
        let t = &back.texts[0];
        assert_eq!((t.content.as_str(), t.angle, t.size_um, t.stroke_width, t.justify, t.mirror), ("hello", 90_000, 1000, 150, TextJustify::Left, true));
        assert!(back.courtyard.is_some(), "the F.CrtYd circle gives a courtyard box");
        assert!(back.pads.iter().all(|p| !p.id.is_empty()) && back.graphics.iter().all(|g| !g.id().is_empty()));
    }

    #[test]
    fn a_real_kicad_9_file_keeps_its_properties_attributes_and_chamfered_pads() {
        let text = r#"(footprint "R_0603" (version 20240108) (generator "pcbnew") (layer "F.Cu")
            (descr "Resistor SMD 0603")
            (tags "resistor")
            (property "Reference" "REF**" (at 0 -1.43 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
            (property "Value" "R_0603" (at 0 1.43 0) (layer "F.Fab") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
            (property "Vendor" "ACME" (at 0 0 0) (layer "F.Fab") (hide yes) (effects (font (size 1 1))))
            (attr smd allow_solder_mask_bridges)
            (fp_line (start -0.8 -0.4) (end 0.8 -0.4) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "a"))
            (pad "1" smd roundrect (at -0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25) (uuid "b"))
            (pad "2" smd roundrect (at 0.8 0) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste") (chamfer_ratio 0.2) (chamfer top_left bottom_right) (uuid "c"))
            (pad "3" smd trapezoid (at 0 2) (size 1 1) (rect_delta 0 0.2) (layers "F.Cu") (uuid "d"))
            (pad "4" smd custom (at 0 -2) (size 1 1) (layers "F.Cu") (zone_connect 2) (options (clearance outline) (anchor rect)) (primitives (gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (width 0) (fill yes))) (uuid "e"))
            (zone (net 0) (net_name "") (layer "F.Cu") (uuid "z"))
            )"#;
        let parsed = parse_library_footprint(text).unwrap();
        let fp = parsed.footprint;
        assert_eq!((fp.description.as_str(), fp.keywords.as_str()), ("Resistor SMD 0603", "resistor"));
        assert!(fp.reference_visible && !fp.value_visible, "the Value property is hidden");
        assert_eq!(fp.fields, vec![FootprintField { name: "Vendor".into(), value: "ACME".into(), visible: false }]);
        assert!(fp.attributes.smd && fp.attributes.allow_soldermask_bridges);
        let by = |n: &str| fp.pads.iter().find(|p| p.number == n).unwrap();
        assert_eq!((by("1").shape, by("1").roundrect_ratio), (LibraryPadShape::RoundRect, Some(0.25)));
        let chamfered = by("2");
        assert_eq!(chamfered.shape, LibraryPadShape::ChamferedRect);
        assert_eq!(chamfered.chamfer_ratio, Some(0.2));
        assert!(chamfered.chamfer_corners.top_left && chamfered.chamfer_corners.bottom_right && !chamfered.chamfer_corners.top_right);
        assert_eq!((by("3").shape, by("3").trapezoid_delta), (LibraryPadShape::Trapezoid, Some((0, 200))));
        assert_eq!(by("4").shape, LibraryPadShape::Rect, "a custom pad keeps its rectangular anchor");
        assert_eq!(parsed.warnings.len(), 2, "{:?}", parsed.warnings);
        assert!(parsed.warnings.iter().any(|w| w.contains("custom")) && parsed.warnings.iter().any(|w| w.contains("zone")));
    }

    #[test]
    fn refuses_a_file_that_is_not_a_footprint() {
        assert!(parse_library_footprint("(kicad_symbol_lib (version 1))").is_err());
        assert!(parse_library_footprint("garbage").is_err());
    }
}
