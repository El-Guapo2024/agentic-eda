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

use eda_model::fp_edit::{FieldLayout, LIBRARY_REFERENCE_TEXT};
use eda_model::footprint::{PadKind, PadShape};
use eda_model::ir::{FootprintAttributes, FootprintField, LibraryFootprint, LibraryPad, LibraryPadShape, Point, Shape, Side, Text, TextJustify};

use crate::import::mm_to_nm;
use crate::sexpr::{self, Sexpr};

/// What [`parse_library_footprint`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedFootprint {
    pub footprint: LibraryFootprint,
    /// The files the footprint embeds (`(embedded_files ..)`, `(embedded_fonts ..)`), data as read -- see [`eda_model::embedded_files`]. They are not part of the
    /// [`LibraryFootprint`]: the design keeps them beside it (`FootprintLibrarySection::embedded`).
    pub embedded: eda_model::embedded_files::EmbeddedFiles,
    /// One line per kind of item that could not be represented and was approximated or left out.
    pub warnings: Vec<String>,
}

fn point(list: &[Sexpr], tag: &str) -> Option<Point> {
    let p = sexpr::find(list, tag)?;
    Some(Point { x: mm_to_nm(sexpr::num(p, 1)?), y: mm_to_nm(sexpr::num(p, 2)?) })
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
    mm_to_nm(w.unwrap_or(0.12))
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

    // From KiCad 8 a `(property ..)` is a field with a place and a layer of its own; before it a property held a value and nothing more (`PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT_unchecked`).
    let version = sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).map(|v| v as i64).unwrap_or(i64::MAX);
    let legacy = version < crate::fp_fields::FIELD_PROPERTIES_VERSION;
    let mut fp = LibraryFootprint::new_empty(name);
    fp.description = sexpr::find(root, "descr").and_then(|d| sexpr::txt(d, 1)).unwrap_or("").to_string();
    fp.keywords = sexpr::find(root, "tags").and_then(|d| sexpr::txt(d, 1)).unwrap_or("").to_string();
    fp.attributes = attributes(root);
    fp.zone_connection = sexpr::find(root, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64));
    // Every (model ...) with its placement; `model` is the first path, as `Footprint::model` is.
    fp.models3d = crate::footprint_lib::models_from(root);
    fp.model = fp.models3d.first().map(|m| m.path.clone());
    fp.courtyard = crate::footprint_lib::courtyard_from(root);

    // in step with `fp.graphics`: each graphic's stroke type when it is not `solid`; with `fp.texts`: each text's looks beyond `Text` (see `LibraryFootprintKeep`)
    let mut strokes: Vec<Option<String>> = Vec::new();
    let mut text_looks: Vec<(eda_model::board_text::TextExtra, bool)> = Vec::new();
    // and each pad's, graphic's and text's uuid
    let (mut pad_uuids, mut graphic_uuids, mut text_uuids): (Vec<Option<String>>, Vec<Option<String>>, Vec<Option<String>>) = (Vec::new(), Vec::new(), Vec::new());
    let uuid_of = |item: &[Sexpr]| sexpr::find(item, "uuid").or_else(|| sexpr::find(item, "tstamp")).and_then(|u| sexpr::txt(u, 1)).filter(|u| !u.is_empty()).map(str::to_string);
    for child in root.iter().skip(2).filter_map(Sexpr::as_list) {
        let (drawn, written, padded) = (fp.graphics.len(), fp.texts.len(), fp.pads.len());
        match sexpr::tag(child) {
            Some("pad") => {
                if let Some(pad) = parse_pad(child, version) {
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
                Some("user") => {
                    if let Some(t) = parse_text(child, sexpr::txt(child, 2).unwrap_or("")) {
                        fp.texts.push(t);
                        let look = crate::board_text_io::read_style(child).extra;
                        text_looks.push((look, crate::fp_fields::parse_layout(child, Side::Top, 0).keep_upright));
                    }
                }
                // the pre-KiCad-8 form: `(fp_text reference "REF**" (at ..) (layer ..) (effects ..))`
                Some("reference") => {
                    let l = field_layout(child, sexpr::txt(child, 2).unwrap_or(""), LIBRARY_REFERENCE_TEXT);
                    fp.reference_visible = l.visible;
                    fp.reference = Some(l);
                }
                Some("value") => {
                    let l = field_layout(child, sexpr::txt(child, 2).unwrap_or(""), &fp.name);
                    fp.value_visible = l.visible;
                    fp.value = Some(l);
                }
                _ => {}
            },
            Some("property") => {
                let (key, value) = (sexpr::txt(child, 1).unwrap_or(""), sexpr::txt(child, 2).unwrap_or(""));
                if let Some(u) = uuid_of(child) {
                    fp.kicad.uuids.insert(format!("field:{key}"), u);
                }
                // a field of KiCad 8 has its layout; an older property only the hide flag
                let layout = (!legacy).then(|| crate::fp_fields::parse_layout(child, Side::Top, 0));
                let visible = layout.as_ref().map_or(!hidden(child), |l| l.visible);
                match key {
                    "Reference" => {
                        fp.reference_visible = visible;
                        if let Some(l) = layout {
                            fp.reference = Some(with_text(l, value, LIBRARY_REFERENCE_TEXT));
                        }
                    }
                    "Value" => {
                        fp.value_visible = visible;
                        if let Some(l) = layout {
                            fp.value = Some(with_text(l, value, &fp.name));
                        }
                    }
                    // The Datasheet and Description fields (`FIELD_T::DATASHEET`, `DESCRIPTION`): KiCad 8's are their own fields beside the library description
                    // `(descr ..)`; an older file's Description property was the description.
                    "Description" => match layout {
                        Some(mut l) => {
                            l.text = Some(value.to_string());
                            fp.description_field = Some(l);
                        }
                        None => {
                            if fp.description.is_empty() {
                                fp.description = value.to_string();
                            }
                        }
                    },
                    "Datasheet" => {
                        if let Some(mut l) = layout {
                            l.text = Some(value.to_string());
                            fp.datasheet = Some(l);
                        }
                    }
                    "" => {}
                    _ => fp.fields.push(FootprintField { name: key.to_string(), value: value.to_string(), visible, layout }),
                }
            }
            Some("fp_text_private") => warnings.push(format!("{} items are not supported by the library footprint and were left out", sexpr::tag(child).unwrap_or("?"))),
            // KiCad 10's typed model, the Extruded 3D Body (`(model (type extruded) (overall_height ..) ..)`, `parseFOOTPRINT`'s `case T_model:` "Typed model"): no
            // path for `models_from` to take, so kept as the file wrote it until the body is ported.
            Some("model") if sexpr::txt(child, 1).is_none() => fp.kicad.tokens.push(sexpr::to_text(&Sexpr::List(child.to_vec()))),
            Some(tag) if !OWN_TOKENS.contains(&tag) => {
                // In a file before KiCad 9 a margin or clearance of 0 meant "inherit" (`parseFOOTPRINT_unchecked`): it says nothing, and is not kept.
                let inherit = version <= 20240201 && KEPT_NUMBERS.contains(&tag) && sexpr::num(child, 1) == Some(0.0);
                if !inherit {
                    fp.kicad.tokens.push(sexpr::to_text(&Sexpr::List(child.to_vec())));
                }
            }
            _ => {}
        }
        if fp.graphics.len() > drawn {
            strokes.push(sexpr::find(child, "stroke").and_then(|st| sexpr::find(st, "type")).and_then(|t| sexpr::txt(t, 1)).filter(|t| *t != "solid").map(str::to_string));
            graphic_uuids.push(uuid_of(child));
        }
        if fp.texts.len() > written {
            text_uuids.push(uuid_of(child));
        }
        if fp.pads.len() > padded {
            pad_uuids.push(uuid_of(child));
        }
    }
    if fp.pads.is_empty() && fp.graphics.is_empty() && fp.texts.is_empty() {
        warnings.push("the footprint holds no pad, graphic or text".to_string());
    }
    fp.assign_missing_ids();
    let ids = fp.pads.iter().map(|p| p.id.clone()).zip(pad_uuids).chain(fp.graphics.iter().map(|g| g.id().to_string()).zip(graphic_uuids)).chain(fp.texts.iter().map(|t| t.id.clone()).zip(text_uuids));
    let uuids: Vec<(String, String)> = ids.filter_map(|(id, u)| Some((id, u?))).collect();
    fp.kicad.uuids.extend(uuids);
    for (g, stroke) in fp.graphics.iter().zip(strokes) {
        if let Some(t) = stroke {
            fp.kicad.stroke_types.insert(g.id().to_string(), t);
        }
    }
    for (t, (mut look, keep_upright)) in fp.texts.iter().zip(text_looks) {
        if !keep_upright {
            fp.kicad.unlocked_texts.insert(t.id.clone());
        }
        if !look.is_default() {
            look.id = t.id.clone();
            fp.kicad.text_extras.push(look);
        }
    }
    let embedded = crate::embedded::read_owner(root, &mut warnings);
    warnings.sort();
    warnings.dedup();
    Ok(ParsedFootprint { footprint: fp, embedded, warnings })
}

/// The forms of a footprint this reader takes into the [`LibraryFootprint`] (or knows to leave out: the board-only ones); any other is kept as the file wrote it
/// (`LibraryFootprintKeep::tokens`).
const OWN_TOKENS: &[&str] = &[
    "version", "generator", "generator_version", "layer", "descr", "tags", "property", "attr", "zone_connect", "fp_line", "fp_arc", "fp_rect", "fp_circle", "fp_poly", "fp_curve",
    "fp_text", "pad", "model", "embedded_fonts", "embedded_files", "uuid", "tstamp", "tedit", "at", "path", "sheetname", "sheetfile", "units", "component_classes",
    // read and dropped by KiCad itself ("never exposed in the GUI")
    "thermal_width", "thermal_gap",
];

/// The kept footprint numbers that a file before KiCad 9 wrote as 0 for "inherit".
const KEPT_NUMBERS: &[&str] = &["clearance", "solder_mask_margin", "solder_paste_margin", "solder_paste_margin_ratio", "solder_paste_ratio"];

/// A `(property ..)` or `(fp_text reference|value ..)`'s layout, with the text it shows when that is not `own` (the footprint's default text for the field).
fn field_layout(item: &[Sexpr], text: &str, own: &str) -> FieldLayout {
    with_text(crate::fp_fields::parse_layout(item, Side::Top, 0), text, own)
}

fn with_text(mut l: FieldLayout, text: &str, own: &str) -> FieldLayout {
    l.text = (text != own).then(|| text.to_string());
    l
}

fn xy_points(item: &[Sexpr]) -> Vec<Point> {
    sexpr::find(item, "pts").map(|pts| sexpr::find_all(pts, "xy").filter_map(|xy| Some(Point { x: mm_to_nm(sexpr::num(xy, 1)?), y: mm_to_nm(sexpr::num(xy, 2)?) })).collect()).unwrap_or_default()
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
        at: Point { x: mm_to_nm(x), y: mm_to_nm(y) },
        angle: ((sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round() as i64).rem_euclid(360_000) as u32,
        layer: layer_of(item),
        size_nm: mm_to_nm(size_mm).max(1),
        stroke_width: mm_to_nm(thickness_mm),
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

fn parse_pad(pad: &[Sexpr], version: i64) -> Option<LibraryPad> {
    let number = sexpr::txt(pad, 1).unwrap_or("").to_string();
    let kind = match sexpr::txt(pad, 2).unwrap_or("") {
        "np_thru_hole" => PadKind::NonPlatedHole,
        "smd" | "connect" => PadKind::Smd,
        _ => PadKind::ThroughHole,
    };
    let at = sexpr::find(pad, "at")?;
    let size = sexpr::find(pad, "size")?;
    // The shape with its delta, chamfer, primitives and offset (`parsePAD`); a `.kicad_mod` is always seen from the top.
    let read = crate::pad_shape::read_pad_shape(pad, Side::Top);
    let (drill, drill_slot) = match sexpr::find(pad, "drill") {
        Some(d) => {
            let toks: Vec<&str> = d.iter().skip(1).filter_map(Sexpr::text).collect();
            if toks.first() == Some(&"oval") {
                let nums: Vec<f64> = toks[1..].iter().filter_map(|s| s.parse::<f64>().ok()).collect();
                let dw = nums.first().copied().unwrap_or(0.0);
                (None, Some((mm_to_nm(dw), mm_to_nm(nums.get(1).copied().unwrap_or(dw)))))
            } else {
                (toks.iter().find_map(|s| s.parse::<f64>().ok()).map(mm_to_nm), None)
            }
        }
        None => (None, None),
    };
    let um = |tag: &str| sexpr::find(pad, tag).and_then(|c| sexpr::num(c, 1)).map(mm_to_nm);
    let size_nm = (mm_to_nm(sexpr::num(size, 1)?), mm_to_nm(sexpr::num(size, 2)?));
    // The padstack (`(padstack ..)`, `(remove_unused_layers ..)`, `(keep_end_layers ..)`): a `.kicad_mod` is a footprint on the top, and the library has no board, so a custom
    // stack has every copper layer KiCad's file can name.
    let main = eda_model::padstack::PadLayerShape {
        shape: read.shape,
        size: size_nm,
        roundrect_ratio: read.roundrect_ratio,
        offset: read.ext.offset,
        trapezoid_delta: read.ext.trapezoid_delta,
        chamfer_ratio: read.ext.chamfer_ratio,
        chamfer_corners: read.ext.chamfer_corners,
        custom: read.ext.custom.clone(),
        tokens: Vec::new(),
    };
    let read_stack = crate::padstack_io::read_padstack(pad, &main, Side::Top, &crate::padstack_io::library_copper_layers());
    let stack = read_stack.stack.clone();
    // A `(layer "F.Cu" ..)` node of the stack says the front shape again: it is the pad's own.
    let main = read_stack.own.unwrap_or(main);
    let shape = match main.shape {
        PadShape::Circle => LibraryPadShape::Circle,
        PadShape::Oval => LibraryPadShape::Oval,
        PadShape::RoundRect => LibraryPadShape::RoundRect,
        PadShape::ChamferedRect => LibraryPadShape::ChamferedRect,
        PadShape::Trapezoid => LibraryPadShape::Trapezoid,
        PadShape::Custom => LibraryPadShape::Custom,
        PadShape::Rect => LibraryPadShape::Rect,
    };
    // What the editor does not edit (`LibraryPadKeep`): an edge connector, the fabrication property, the margins (a 0 of a file before KiCad 9 is "inherit"), the rest as written.
    let mut keep = eda_model::lib_keep::LibraryPadKeep { connector: sexpr::txt(pad, 2) == Some("connect"), ..Default::default() };
    let margin = |tag: &str| sexpr::find(pad, tag).and_then(|c| sexpr::num(c, 1)).filter(|v| !(version <= 20240201 && *v == 0.0));
    keep.solder_mask_margin = margin("solder_mask_margin").map(mm_to_nm);
    keep.solder_paste_margin = margin("solder_paste_margin").map(mm_to_nm);
    keep.solder_paste_margin_ratio = margin("solder_paste_margin_ratio").or_else(|| margin("solder_paste_ratio"));
    for item in pad.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("property") => keep.property = sexpr::txt(item, 1).map(str::to_string),
            Some(tag) if !OWN_PAD_TOKENS.contains(&tag) => keep.tokens.push(sexpr::to_text(&Sexpr::List(item.to_vec()))),
            _ => {}
        }
    }
    Some(LibraryPad {
        id: String::new(),
        number,
        at: Point { x: mm_to_nm(sexpr::num(at, 1)?), y: mm_to_nm(sexpr::num(at, 2)?) },
        size: main.size,
        // The offset lives inside `(drill ..)`; an older export of this app wrote it as a token of the pad.
        offset: main.offset.map(|(x, y)| Point { x, y }).or_else(|| point(pad, "offset")).unwrap_or_default(),
        shape,
        kind,
        drill,
        drill_slot,
        rot: crate::pad_rot_from_file(Side::Top, 0, sexpr::num(at, 3).unwrap_or(0.0)),
        roundrect_ratio: match shape {
            LibraryPadShape::RoundRect | LibraryPadShape::ChamferedRect => Some(main.roundrect_ratio.unwrap_or(0.25)),
            _ => None,
        },
        trapezoid_delta: main.trapezoid_delta,
        chamfer_ratio: if shape == LibraryPadShape::ChamferedRect { Some(main.chamfer_ratio.unwrap_or(0.2)) } else { None },
        chamfer_corners: main.chamfer_corners,
        custom: main.custom,
        layers: sexpr::find(pad, "layers").map(|l| l.iter().skip(1).filter_map(Sexpr::text).map(str::to_string).collect()).unwrap_or_default(),
        clearance_override: um("clearance"),
        thermal_gap_override: um("thermal_gap"),
        thermal_spoke_width_override: um("thermal_bridge_width"),
        zone_connection: sexpr::find(pad, "zone_connect").and_then(|z| sexpr::num(z, 1)).and_then(|v| crate::zone_connection_from_file(v as i64)),
        thermal_spoke_angle_mdeg: sexpr::find(pad, "thermal_bridge_angle").and_then(|a| sexpr::num(a, 1)).map(|d| ((d * 1000.0).round() as i64).rem_euclid(360_000) as eda_model::ir::Millideg),
        stack,
        kicad: keep,
    })
}

/// The forms of a pad that [`parse_pad`] reads into the [`LibraryPad`] (or that a library pad cannot have: `net`); the others are kept as written.
const OWN_PAD_TOKENS: &[&str] = &[
    "at", "size", "offset", "drill", "layers", "roundrect_rratio", "chamfer_ratio", "chamfer", "rect_delta", "clearance", "zone_connect", "thermal_bridge_width", "thermal_width",
    "thermal_bridge_angle", "thermal_gap", "options", "primitives", "uuid", "tstamp", "padstack", "remove_unused_layers", "keep_end_layers", "net", "solder_mask_margin",
    "solder_paste_margin", "solder_paste_margin_ratio", "solder_paste_ratio",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export_kicad_mod;
    use eda_model::ir::ChamferCorners;

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
            custom: None,
            layers: vec![],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
            stack: Default::default(),
            kicad: Default::default(),
        }
    }

    fn sample() -> LibraryFootprint {
        let mut fp = LibraryFootprint::new_empty("Test:Sample");
        fp.description = "a sample".into();
        fp.keywords = "one two".into();
        fp.attributes = FootprintAttributes { smd: true, exclude_from_bom: true, exclude_from_position_files: true, dnp: false, ..FootprintAttributes::default() };
        fp.model = Some("${KICAD9_3DMODEL_DIR}/x.step".into());
        let mut p1 = pad("1", PadKind::Smd, LibraryPadShape::RoundRect, (-1_000_000, 0), (800_000, 1_200_000));
        p1.roundrect_ratio = Some(0.3);
        p1.rot = 90_000;
        p1.layers = vec!["F.Cu".into(), "F.Mask".into()];
        p1.clearance_override = Some(200_000);
        let mut p2 = pad("2", PadKind::ThroughHole, LibraryPadShape::Oval, (1_000_000, 0), (1_600_000, 2_000_000));
        p2.drill = Some(800_000);
        p2.offset = Point { x: 100_000, y: -50_000 };
        p2.layers = vec!["*.Cu".into(), "*.Mask".into()];
        let mut p3 = pad("3", PadKind::ThroughHole, LibraryPadShape::Circle, (0, 2_000_000), (1_700_000, 1_700_000));
        p3.drill_slot = Some((600_000, 1_700_000));
        let p4 = pad("", PadKind::NonPlatedHole, LibraryPadShape::Circle, (0, -2_000_000), (900_000, 900_000));
        fp.pads = vec![p1, p2, p3, p4];
        fp.graphics = vec![
            Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120_000, filled: false, start: Point { x: -2_000_000, y: -1_500_000 }, end: Point { x: 2_000_000, y: -1_500_000 } },
            Shape::Arc { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120_000, filled: false, start: Point { x: 1_000_000, y: 0 }, mid: Point { x: 0, y: 1_000_000 }, end: Point { x: -1_000_000, y: 0 } },
            Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100_000, filled: true, start: Point { x: -1_500_000, y: -1_000_000 }, end: Point { x: 1_500_000, y: 1_000_000 } },
            Shape::Circle { id: String::new(), layer: "F.CrtYd".into(), stroke_width: 50_000, filled: false, center: Point { x: 0, y: 0 }, end: Point { x: 3_000_000, y: 0 } },
            Shape::Polygon { id: String::new(), layer: "F.Fab".into(), stroke_width: 100_000, filled: false, pts: vec![Point { x: 0, y: 0 }, Point { x: 500_000, y: 0 }, Point { x: 500_000, y: 500_000 }] },
            Shape::Bezier { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120_000, filled: false, start: Point { x: 0, y: 0 }, c1: Point { x: 100_000, y: 200_000 }, c2: Point { x: 300_000, y: 200_000 }, end: Point { x: 400_000, y: 0 } },
        ];
        fp.texts = vec![Text { id: String::new(), content: "hello".into(), at: Point { x: 100_000, y: 3_000_000 }, angle: 90_000, layer: "F.SilkS".into(), size_nm: 1_000_000, stroke_width: 150_000, justify: TextJustify::Left, mirror: true }];
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
        assert_eq!((p1.shape, p1.kind, p1.rot, p1.at, p1.size), (LibraryPadShape::RoundRect, PadKind::Smd, 90_000, Point { x: -1_000_000, y: 0 }, (800_000, 1_200_000)));
        assert_eq!(p1.roundrect_ratio, Some(0.3));
        assert_eq!(p1.layers, vec!["F.Cu".to_string(), "F.Mask".to_string()]);
        assert_eq!(p1.clearance_override, Some(200_000));
        let p2 = by("2");
        assert_eq!((p2.shape, p2.kind, p2.drill, p2.offset), (LibraryPadShape::Oval, PadKind::ThroughHole, Some(800_000), Point { x: 100_000, y: -50_000 }));
        assert_eq!(by("3").drill_slot, Some((600_000, 1_700_000)));
        let npth = back.pads.iter().find(|p| p.kind == PadKind::NonPlatedHole).unwrap();
        assert_eq!(npth.number, "");
        assert_eq!(back.graphics.len(), 6);
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Rect { filled: true, layer, .. } if layer == "F.Fab")));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Arc { mid, .. } if *mid == Point { x: 0, y: 1_000_000 })));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Bezier { c2, .. } if *c2 == Point { x: 300_000, y: 200_000 })));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Polygon { pts, .. } if pts.len() == 3)));
        assert_eq!(back.texts.len(), 1);
        let t = &back.texts[0];
        assert_eq!((t.content.as_str(), t.angle, t.size_nm, t.stroke_width, t.justify, t.mirror), ("hello", 90_000, 1_000_000, 150_000, TextJustify::Left, true));
        assert!(back.courtyard.is_some(), "the F.CrtYd circle gives a courtyard box");
        assert!(back.pads.iter().all(|p| !p.id.is_empty()) && back.graphics.iter().all(|g| !g.id().is_empty()));
    }

    #[test]
    fn the_zone_connection_fields_of_a_footprint_and_its_pads_survive_a_kicad_mod_round_trip() {
        // `parsePAD` / `format( const PAD* )`: zone_connect, thermal_bridge_width, thermal_bridge_angle, thermal_gap, clearance;
        // `(footprint (zone_connect N))` for the footprint itself.
        let mut fp = sample();
        fp.zone_connection = Some(eda_model::ir::PadConnection::Thermal);
        fp.pads[0].zone_connection = Some(eda_model::ir::PadConnection::Full);
        fp.pads[0].thermal_gap_override = Some(250_000);
        fp.pads[0].thermal_spoke_width_override = Some(300_000);
        fp.pads[0].thermal_spoke_angle_mdeg = Some(30_500);
        fp.pads[1].zone_connection = Some(eda_model::ir::PadConnection::ThtThermal);
        // 90 degrees is an oval's default angle: KiCad does not write it, and the reader gives back "not set".
        fp.pads[1].thermal_spoke_angle_mdeg = Some(90_000);
        fp.pads[2].zone_connection = Some(eda_model::ir::PadConnection::None);
        // 45 degrees is a circle's default and is not written either; any other angle is.
        fp.pads[2].thermal_spoke_angle_mdeg = Some(45_000);

        let text = export_kicad_mod(&fp);
        assert!(text.contains("(zone_connect 1)"), "the footprint's own connection is written: {text}");
        assert_eq!(text.matches("(thermal_bridge_angle").count(), 1, "only the angle that is not the shape's default is written");
        let back = parse_library_footprint(&text).expect("parses").footprint;

        assert_eq!(back.zone_connection, Some(eda_model::ir::PadConnection::Thermal));
        let by = |n: &str| back.pads.iter().find(|p| p.number == n).unwrap();
        let p1 = by("1");
        assert_eq!(p1.zone_connection, Some(eda_model::ir::PadConnection::Full));
        assert_eq!((p1.thermal_gap_override, p1.thermal_spoke_width_override, p1.thermal_spoke_angle_mdeg, p1.clearance_override), (Some(250_000), Some(300_000), Some(30_500), Some(200_000)));
        assert_eq!((by("2").zone_connection, by("2").thermal_spoke_angle_mdeg), (Some(eda_model::ir::PadConnection::ThtThermal), None));
        assert_eq!((by("3").zone_connection, by("3").thermal_spoke_angle_mdeg), (Some(eda_model::ir::PadConnection::None), None));
        // a footprint that sets nothing writes nothing
        let plain = export_kicad_mod(&sample());
        assert!(!plain.contains("zone_connect") && !plain.contains("thermal_"), "{plain}");
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
        assert_eq!(fp.fields.len(), 1);
        let vendor = &fp.fields[0];
        assert_eq!((vendor.name.as_str(), vendor.value.as_str(), vendor.visible), ("Vendor", "ACME", false));
        let l = vendor.layout.as_ref().expect("a property of KiCad 8 has its layout");
        assert_eq!((l.layer.as_str(), l.at, l.size, l.visible), ("F.Fab", Point { x: 0, y: 0 }, (1_000_000, 1_000_000), false));
        // the Reference and the Value keep their places; the Value's text is the footprint's name, so it stores none of its own
        let reference = fp.reference.as_ref().expect("the Reference layout");
        assert_eq!((reference.layer.as_str(), reference.at, reference.text.as_deref(), reference.visible), ("F.SilkS", Point { x: 0, y: -1_430_000 }, None, true));
        let value = fp.value.as_ref().expect("the Value layout");
        assert_eq!((value.layer.as_str(), value.at.y, value.visible), ("F.Fab", 1_430_000, false));
        assert!(fp.attributes.smd && fp.attributes.allow_soldermask_bridges);
        let by = |n: &str| fp.pads.iter().find(|p| p.number == n).unwrap();
        assert_eq!((by("1").shape, by("1").roundrect_ratio), (LibraryPadShape::RoundRect, Some(0.25)));
        let chamfered = by("2");
        assert_eq!(chamfered.shape, LibraryPadShape::ChamferedRect);
        assert_eq!(chamfered.chamfer_ratio, Some(0.2));
        assert!(chamfered.chamfer_corners.top_left && chamfered.chamfer_corners.bottom_right && !chamfered.chamfer_corners.top_right);
        assert_eq!((by("3").shape, by("3").trapezoid_delta), (LibraryPadShape::Trapezoid, Some((0, 200_000))));
        assert_eq!(by("4").shape, LibraryPadShape::Custom, "a custom pad keeps its anchor and its basic shapes");
        let custom = by("4").custom.as_ref().expect("the custom pad's primitives");
        assert_eq!((custom.anchor, custom.primitives.len()), (eda_model::footprint::PadAnchor::Rect, 1));
        // the zone is kept as the file has it (`LibraryFootprintKeep::tokens`), for the writer
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(fp.kicad.tokens.len(), 1);
        assert!(fp.kicad.tokens[0].starts_with("(zone"), "{:?}", fp.kicad.tokens);
        assert!(export_kicad_mod(&fp).contains("(zone"), "and written back");
    }

    #[test]
    fn refuses_a_file_that_is_not_a_footprint() {
        assert!(parse_library_footprint("(kicad_symbol_lib (version 1))").is_err());
        assert!(parse_library_footprint("garbage").is_err());
    }
}
