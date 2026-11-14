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
//!
//! The footprint's dimensions, zones, text boxes, points, reference images, barcodes, tables and groups are its items
//! ([`eda_model::fp_items::FootprintItems`]), read as `parseFOOTPRINT_unchecked` reads them: each with the board's own reader of the item
//! (`parseDIMENSION`, `parseZONE`, `parsePCB_TEXTBOX`, `parsePCB_POINT`, `parsePCB_REFERENCE_IMAGE`, `parsePCB_BARCODE`, `parsePCB_TABLE`,
//! `parseGROUP`), a group's members resolved to the ids of the items their uuids name once every item has its id (`resolveGroups`). A form those
//! items cannot hold is kept as the file wrote it (`LibraryFootprintKeep::tokens`), as every one of them was before (see `ItemsRead::read`).

use eda_model::units::Nm;
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
fn stroke_width(item: &[Sexpr]) -> Nm {
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

/// A graphic's hatched fill, `(fill hatch|reverse_hatch|cross_hatch)` (`parsePCB_SHAPE`'s `T_fill`); `None` for a solid fill or none.
fn hatch_of(item: &[Sexpr]) -> Option<eda_model::fp_art::Hatch> {
    use eda_model::fp_art::Hatch;
    let fill = sexpr::find(item, "fill")?;
    fill.iter().skip(1).filter_map(Sexpr::text).find_map(|w| match w {
        "hatch" => Some(Hatch::Hatch),
        "reverse_hatch" => Some(Hatch::ReverseHatch),
        "cross_hatch" => Some(Hatch::CrossHatch),
        _ => None,
    })
}

/// `(uuid "..")`, or the `(tstamp ..)` of a file before KiCad 7; `None` for none or an empty one.
fn uuid_of(item: &[Sexpr]) -> Option<String> {
    sexpr::find(item, "uuid").or_else(|| sexpr::find(item, "tstamp")).and_then(|u| sexpr::txt(u, 1)).filter(|u| !u.is_empty()).map(str::to_string)
}

fn layer_of(item: &[Sexpr]) -> String {
    sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.SilkS").to_string()
}

/// One footprint of a clipboard text as `CLIPBOARD_IO::Parse` gives it to `PCB_CONTROL::Paste` (UI-LEFT-3 F3-10): the footprint read as a library footprint is (what
/// a file holds in the footprint's own frame -- its graphics, text boxes, texts' and fields' places, pads' places -- stays in that frame; the angles of its pads, texts
/// and fields, which the file writes as the board has them, and its dimensions, tables, barcodes, images and zones, which the file writes in the board's frame, stay
/// as the file has them), with where the board had it: `at` and `angle`, the footprint's `(at x y angle)` (counter-clockwise millidegrees, as KiCad's angles are). A
/// footprint's own clipboard (`(footprint ..)`, what the Footprint Editor copies) is at the origin, angle 0; so are a board clipboard's drawings ([`parse_footprint_clipboard`]).
pub struct ClipboardFootprint {
    pub parsed: ParsedFootprint,
    pub at: Point,
    pub angle: i64,
}

/// The board items `PCB_CONTROL::Paste` takes from a board's clipboard into the Footprint Editor beside the items of its footprints (`clipBoard->Drawings()`'s
/// `PCB_TEXT_T`, `PCB_TEXTBOX_T`, `PCB_TABLE_T`, `PCB_SHAPE_T`, `PCB_BARCODE_T` and the five dimensions; an image, a point, a zone, a track or a via is not pasted) and
/// its groups (`clipBoard->Groups()`), each in its footprint form (`gr_line` is a footprint's `fp_line` ..; `gr_text "x"` is `fp_text user "x"`).
fn board_drawing_as_footprint_item(item: &[Sexpr]) -> Option<Sexpr> {
    let tag = sexpr::tag(item)?;
    let renamed = match tag {
        "gr_line" => "fp_line",
        "gr_arc" => "fp_arc",
        "gr_circle" => "fp_circle",
        "gr_rect" => "fp_rect",
        "gr_poly" => "fp_poly",
        "gr_curve" => "fp_curve",
        "gr_text" => "fp_text",
        "gr_text_box" | "dimension" | "table" | "barcode" | "group" => tag,
        _ => return None,
    };
    let mut out: Vec<Sexpr> = vec![Sexpr::Atom(renamed.to_string())];
    if tag == "gr_text" {
        out.push(Sexpr::Atom("user".to_string()));
    }
    out.extend(item.iter().skip(1).cloned());
    Some(Sexpr::List(out))
}

/// The footprints a clipboard text holds for a paste into the Footprint Editor, with where the board had each (`CLIPBOARD_IO::Parse` in `PCB_CONTROL::Paste`; UI-LEFT-2
/// FF1, UI-LEFT-3 F3-10): a footprint's text (`(footprint ..)` or `(module ..)`) is its one footprint at the origin; a board's (`(kicad_pcb ..)`) is each `(footprint ..)`
/// at its `(at x y angle)`, then -- when the board has any -- its drawings and its own groups as one footprint at the origin (`PCB_CONTROL::Paste` takes
/// `clipBoard->Drawings()` and `clipBoard->Groups()` too; a group's member that is not pasted, a footprint, leaves it). `Ok(None)` for a text that is neither (the paste
/// then makes a text of it).
pub fn parse_footprint_clipboard(text: &str) -> Result<Option<Vec<ClipboardFootprint>>, String> {
    let Ok(tree) = sexpr::parse(text) else { return Ok(None) };
    let Some(root) = tree.as_list() else { return Ok(None) };
    let at_origin = |parsed| ClipboardFootprint { parsed, at: Point::default(), angle: 0 };
    match sexpr::tag(root) {
        Some("footprint") | Some("module") => Ok(Some(vec![at_origin(parse_library_footprint(text)?)])),
        Some("kicad_pcb") => {
            let mut out = Vec::new();
            for l in root.iter().filter_map(|n| n.as_list().filter(|l| sexpr::tag(l) == Some("footprint"))) {
                let parsed = parse_library_footprint(&sexpr::to_text(&Sexpr::List(l.to_vec())))?;
                let place = sexpr::find(l, "at");
                let n = |i| place.and_then(|a| sexpr::num(a, i)).unwrap_or(0.0);
                out.push(ClipboardFootprint { parsed, at: Point { x: mm_to_nm(n(1)), y: mm_to_nm(n(2)) }, angle: (n(3) * 1000.0).round() as i64 });
            }
            let drawings: Vec<Sexpr> = root.iter().filter_map(Sexpr::as_list).filter_map(board_drawing_as_footprint_item).collect();
            if !drawings.is_empty() {
                let mut fp = vec![Sexpr::Atom("footprint".into()), Sexpr::Atom("clipboard".into())];
                fp.extend(sexpr::find(root, "version").map(|v| Sexpr::List(v.to_vec())));
                fp.extend(drawings);
                out.push(at_origin(parse_library_footprint(&sexpr::to_text(&Sexpr::List(fp)))?));
            }
            Ok(Some(out))
        }
        _ => Ok(None),
    }
}

/// Parse a `.kicad_mod` file (`(footprint "Name" ...)`, or the KiCad 6 `(module ...)`).
pub fn parse_library_footprint(text: &str) -> Result<ParsedFootprint, String> {
    let mut tree = sexpr::parse(text).map_err(|e| format!("not a valid s-expression file: {e}"))?;
    // KiCad 10's bold text, whose thickness is the width before the bold factor: `parseEDA_TEXT` widens it back for a footprint file of version
    // 20260826 or later, the footprint's own `(version ..)` (`parseFOOTPRINT_unchecked`'s `m_requiredVersion`). See `bold_text`.
    crate::bold_text::normalize(&mut tree);
    // A file older than 20210606: its `~X~` overbars in KiCad's new `~{X}` notation, net names and texts, as KiCad's parser converts them (`overbar`).
    crate::overbar::normalize(&mut tree);
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

    // in step with `fp.graphics`: each graphic's stroke type when it is not `solid`, its hatch and its lock; with `fp.texts`: each text's looks beyond `Text`, whether it
    // keeps upright, is shown and is locked (see `LibraryFootprintKeep`)
    let mut strokes: Vec<(Option<String>, Option<eda_model::fp_art::Hatch>, bool, eda_model::line_ending::LineEndings)> = Vec::new();
    let mut text_looks: Vec<(eda_model::board_text::TextExtra, bool, bool, bool)> = Vec::new();
    // and each pad's, graphic's and text's uuid
    let (mut pad_uuids, mut graphic_uuids, mut text_uuids): (Vec<Option<String>>, Vec<Option<String>>, Vec<Option<String>>) = (Vec::new(), Vec::new(), Vec::new());
    // the items of `fp.items` with their uuids, the groups as the file names their members, and the uuids of the forms kept as the file wrote them
    let mut items = ItemsRead::default();
    let mut raw_groups: Vec<(crate::import_items::RawGroup, String)> = Vec::new();
    let mut kept_uuids: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                    // `(fp_text user locked "text" ..)`: the bare `locked` of KiCad 5.99 sits before the text
                    let at = if sexpr::txt(child, 2) == Some("locked") { 3 } else { 2 };
                    if let Some(t) = parse_text(child, sexpr::txt(child, at).unwrap_or("")) {
                        fp.texts.push(t);
                        let look = crate::board_text_io::read_style(child).extra;
                        let layout = crate::fp_fields::parse_layout(child, Side::Top, 0);
                        // `parsePCB_TEXT`'s `T_hide` (a text of a footprint) and the older bare `hide` of its effects
                        let shown = layout.visible && !sexpr::find(child, "effects").is_some_and(|e| hidden(e));
                        text_looks.push((look, layout.keep_upright, shown, crate::import_items::is_locked(child)));
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
            // The items of `FootprintItems`, read by the board's readers; a form they cannot hold is kept as the file wrote it.
            Some(tag @ ("dimension" | "zone" | "fp_text_box" | "gr_text_box" | "point" | "image" | "barcode" | "table")) => {
                if !items.read(child, tag, version, &mut fp.items) {
                    kept_uuids.extend(uuid_of(child));
                    fp.kicad.tokens.push(sexpr::to_text(&Sexpr::List(child.to_vec())));
                }
            }
            // `parseGROUP`: the members are resolved once every item has its id (`resolveGroups`), below.
            Some("group") => raw_groups.push((crate::import_items::read_group(child), sexpr::to_text(&Sexpr::List(child.to_vec())))),
            // KiCad 10's typed model, the Extruded 3D Body (`(model (type extruded) (overall_height ..) ..)`, `parseFOOTPRINT`'s `case T_model:` "Typed model"): the footprint's
            // body (`crate::extruded_body_io`; a second one goes on setting it, `EnsureExtrudedBody()`); a typed model of another type is kept as the file wrote it.
            Some("model") if sexpr::txt(child, 1).is_none() => match crate::extruded_body_io::read(child, fp.extruded_body.clone()) {
                Some(body) => fp.extruded_body = Some(body),
                None => fp.kicad.tokens.push(sexpr::to_text(&Sexpr::List(child.to_vec()))),
            },
            Some(tag) if !OWN_TOKENS.contains(&tag) => {
                // In a file before KiCad 9 a margin or clearance of 0 meant "inherit" (`parseFOOTPRINT_unchecked`): it says nothing, and is not kept.
                let inherit = version <= 20240201 && KEPT_NUMBERS.contains(&tag) && sexpr::num(child, 1) == Some(0.0);
                if !inherit {
                    kept_uuids.extend(uuid_of(child));
                    fp.kicad.tokens.push(sexpr::to_text(&Sexpr::List(child.to_vec())));
                }
            }
            _ => {}
        }
        if fp.graphics.len() > drawn {
            let stroke = sexpr::find(child, "stroke").and_then(|st| sexpr::find(st, "type")).and_then(|t| sexpr::txt(t, 1)).filter(|t| *t != "solid").map(str::to_string);
            // KiCad 10's line endings (`parsePCB_SHAPE`'s `T_start_shape` / `T_end_shape`, KiCad 25ec2335)
            strokes.push((stroke, hatch_of(child), crate::import_items::is_locked(child), crate::line_ending_io::read(child)));
            graphic_uuids.push(uuid_of(child));
        }
        if fp.texts.len() > written {
            text_uuids.push(uuid_of(child));
        }
        if fp.pads.len() > padded {
            pad_uuids.push(uuid_of(child));
        }
    }
    if fp.pads.is_empty() && fp.graphics.is_empty() && fp.texts.is_empty() && fp.items.is_empty() {
        warnings.push("the footprint holds no pad, graphic or text".to_string());
    }
    // The ids, the groups' excepted: a group's id is made from its members' (`Group::id_seed`), which are named once the items have theirs.
    fp.assign_missing_ids();
    let ids = fp.pads.iter().map(|p| p.id.clone()).zip(pad_uuids).chain(fp.graphics.iter().map(|g| g.id().to_string()).zip(graphic_uuids)).chain(fp.texts.iter().map(|t| t.id.clone()).zip(text_uuids));
    let uuids: Vec<(String, String)> = ids.filter_map(|(id, u)| Some((id, u?))).collect();
    fp.kicad.uuids.extend(uuids);
    items.keep_uuids(&mut fp);
    resolve_groups(&mut fp, raw_groups, kept_uuids);
    for (g, (stroke, hatch, locked, endings)) in fp.graphics.iter().zip(strokes) {
        if !endings.is_none() {
            fp.kicad.endings.insert(g.id().to_string(), endings);
        }
        if let Some(t) = stroke {
            fp.kicad.stroke_types.insert(g.id().to_string(), t);
        }
        if let Some(h) = hatch {
            fp.kicad.hatches.insert(g.id().to_string(), h);
        }
        if locked {
            fp.kicad.locked.insert(g.id().to_string());
        }
    }
    for (t, (mut look, keep_upright, shown, locked)) in fp.texts.iter().zip(text_looks) {
        if !keep_upright {
            fp.kicad.unlocked_texts.insert(t.id.clone());
        }
        if !shown {
            fp.kicad.hidden_texts.insert(t.id.clone());
        }
        if locked {
            fp.kicad.locked.insert(t.id.clone());
        }
        if !look.is_default() {
            look.id = t.id.clone();
            fp.kicad.text_extras.push(look);
        }
    }
    // The footprint's clearance, margins, private layers, net ties and jumpers (Footprint Properties' Clearance Overrides and Pad Connections, UI-LEFT F4): out of
    // the kept forms into their fields.
    fp.settings.absorb_kept(&mut fp.kicad.tokens);
    let embedded = crate::embedded::read_owner(root, &mut warnings);
    warnings.sort();
    warnings.dedup();
    Ok(ParsedFootprint { footprint: fp, embedded, warnings })
}

/// The file uuid of each item of [`eda_model::fp_items::FootprintItems`] the reader took, in step with its list, until the items have their ids.
#[derive(Default)]
struct ItemsRead {
    dimensions: Vec<Option<String>>,
    zones: Vec<Option<String>>,
    text_boxes: Vec<Option<String>>,
    points: Vec<Option<String>>,
    images: Vec<Option<String>>,
    barcodes: Vec<Option<String>>,
    tables: Vec<Option<String>>,
    /// The items that are `(locked yes)`, by kind and place in their list (UI-LEFT-2 FF6): `LibraryFootprintKeep::locked` once they have their ids.
    locked: Vec<(&'static str, usize)>,
    /// What a zone's form says beyond [`eda_model::ir::Zone`] (its fill, thieving, per-layer settings, arcs), by its place in the list.
    zone_extras: Vec<(usize, ZoneExtras)>,
}

/// What a footprint's zone keeps beside it (`LibraryFootprintKeep::zone_fills`, `zone_thieving`, `zone_layer_props`, `zone_arcs`).
struct ZoneExtras {
    fill: Option<eda_model::lib_keep::KeptZoneFill>,
    thieving: Option<String>,
    layer_props: Option<String>,
    arc_rings: Option<Vec<String>>,
}

/// `ARC_HIGH_DEF` (`pcbIUScale.mmToIU( 0.005 )`): the error a legacy segment fill's ovals are made with.
const ARC_HIGH_DEF_NM: i64 = 5_000;

/// `parseZONE`'s legacy segment fill (`T_fill_segments`: "Legacy zones only had one layer", each `(pts (xy a) (xy b))` a segment on `GetFirstLayer()`), as it makes
/// the fill when the zone stores no `(filled_polygon ..)`: each segment `TransformOvalToPolygon( segPolygon, A, B, GetMinThickness(), ARC_HIGH_DEF, ERROR_OUTSIDE )`,
/// `BooleanAdd`ed into the layer's fill (`SetFilledPolysList`). The fill is kept as `format( const ZONE* )` writes it: each outline of the set, as a `(filled_polygon
/// ..)`, not an island. (KiCad's parse warning, "The legacy segment zone fill mode is no longer supported.", is a board load's; a footprint's has none to show.)
fn legacy_segment_fill(child: &[Sexpr], zone: &eda_model::ir::Zone) -> Option<(String, Vec<(bool, Vec<[i64; 2]>)>)> {
    use eda_shape_poly_set::basic_shapes::{oval_polygon, ErrorLoc};
    use eda_shape_poly_set::ShapePolySet;
    let xy = |l: &[Sexpr]| Some(eda_clipper2::Point64::new(crate::mm_to_nm(sexpr::num(l, 1)?).get(), crate::mm_to_nm(sexpr::num(l, 2)?).get()));
    let mut fill = ShapePolySet::new();
    let mut any = false;
    for segs in sexpr::find_all(child, "fill_segments") {
        for pts in segs[1..].iter().filter_map(Sexpr::as_list).filter(|l| sexpr::tag(l) == Some("pts")) {
            let ends: Vec<eda_clipper2::Point64> = sexpr::find_all(pts, "xy").filter_map(xy).collect();
            let [a, b] = ends.as_slice() else { continue };
            let mut seg = ShapePolySet::new();
            seg.add_outline(oval_polygon(*a, *b, zone.min_thickness.get(), ARC_HIGH_DEF_NM, ErrorLoc::Outside, 0));
            fill.boolean_add(&seg);
            any = true;
        }
    }
    if !any {
        return None;
    }
    let rings = (0..fill.outline_count()).map(|i| (false, fill.outline(i).iter().map(|p| [p.x, p.y]).collect())).collect();
    Some((zone.layer_set().first().map(|l| l.to_string()).unwrap_or_else(|| zone.layer.clone()), rings))
}

/// A zone's `(filled_polygon (layer ..) [(island yes)] (pts ..))`s and its `(fill yes ..)` flag (`parseZONE`'s `T_filled_polygon`, `SetIsFilled`), in the footprint's
/// frame -- or, with none, the fill its legacy `(fill_segments ..)` make ([`legacy_segment_fill`]); `None` when it stores none.
fn zone_fill_of(child: &[Sexpr], zone: &eda_model::ir::Zone) -> Option<eda_model::lib_keep::KeptZoneFill> {
    let mut layers: std::collections::BTreeMap<String, Vec<(bool, Vec<[i64; 2]>)>> = std::collections::BTreeMap::new();
    for fp in sexpr::find_all(child, "filled_polygon") {
        let layer = sexpr::find(fp, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or(zone.layer.as_str()).to_string();
        let island = sexpr::find(fp, "island").is_some_and(|i| sexpr::txt(i, 1).is_none_or(|v| v == "yes"));
        let pts: Vec<[i64; 2]> = sexpr::find(fp, "pts")
            .map(|p| sexpr::find_all(p, "xy").filter_map(|xy| Some([crate::mm_to_nm(sexpr::num(xy, 1)?).get(), crate::mm_to_nm(sexpr::num(xy, 2)?).get()])).collect())
            .unwrap_or_default();
        layers.entry(layer).or_default().push((island, pts));
    }
    // `else if( legacySegs.size() > 0 )`: no polygons, just a segment fill (a rule area's too: the legacy branch does not ask)
    if layers.is_empty() {
        let (layer, rings) = legacy_segment_fill(child, zone)?;
        layers.insert(layer, rings);
    }
    let filled = sexpr::find(child, "fill").is_some_and(|f| f.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == "yes")));
    Some(eda_model::lib_keep::KeptZoneFill { sig: crate::zone_fills::zone_signature(zone), unfilled: !filled, layers, outline: zone.outline.clone(), holes: zone.holes.clone(), zone_layers: zone.layer_set().into_iter().map(str::to_string).collect() })
}

impl ItemsRead {
    /// One `(dimension ..)`, `(zone ..)`, `(fp_text_box ..)`, `(point ..)`, `(image ..)`, `(barcode ..)` or `(table ..)` of the footprint into `items`, read as
    /// `parseFOOTPRINT_unchecked` reads it -- with the board's own parser of the item (`parseDIMENSION`, `parseZONE`, `parsePCB_TEXTBOX`, `parsePCB_POINT`,
    /// `parsePCB_REFERENCE_IMAGE`, `parsePCB_BARCODE`, `parsePCB_TABLE`), in the footprint's frame, which a library footprint's file is written in. `version` is the
    /// footprint file's (`m_requiredVersion`). False when the form says what the item cannot hold, and the caller keeps it as the file wrote it:
    /// - the board's reader gives none (`parseDIMENSION`'s pre-6.0 form, a zone with no outline, a table whose cells are off its grid, an image that is no image).
    /// A teardrop zone (`(attr (teardrop (type ..)))`, a board's) is a zone that says so (`Zone::teardrop`, `teardrop_track_end`: `SetTeardropAreaType`), and a
    /// legacy segment fill (`(fill_segments (pts (xy ..) (xy ..)) ..)`, a file before 6.0) is the fill KiCad makes of it (UI-LEFT-3 F3-12; they were kept as text).
    /// A lock (`(locked yes)`) is kept by the item's id (`LibraryFootprintKeep::locked`), and a zone's fill (`(filled_polygon ..)`), copper thieving, per-layer settings
    /// and the arcs of its outline beside it (`LibraryFootprintKeep::zone_fills` ..), as the board keeps them (UI-LEFT-2 FF6). The hatch pitch and the layers' hatch
    /// offsets (`(property (layer ..) (hatch_position ..))`) are the zone's own (`Zone::border_hatch_pitch`, `Zone::hatch_offsets`).
    ///
    /// A zone of a library footprint has no net (whatever `(net ..)` says: a footprint's copper zone takes the net of its pad once placed, which this editor does
    /// not model), and its uuid is kept with the others' (`LibraryFootprintKeep::uuids`), not in `Zone::uuid`; a table's too, its cells keeping theirs.
    fn read(&mut self, child: &[Sexpr], tag: &str, version: i64, items: &mut eda_model::fp_items::FootprintItems) -> bool {
        // `parsePCB_POINT` has no `locked` (the board's reader, `pcb_point`, reads none either); a lock is kept by the item's id (`LibraryFootprintKeep::locked`)
        let locked = tag != "point" && crate::import_items::is_locked(child);
        let uuid = uuid_of(child);
        let kind: &'static str = match tag {
            "dimension" => "dimension",
            "zone" => "zone",
            "fp_text_box" | "gr_text_box" => "text_box",
            "image" => "image",
            "barcode" => "barcode",
            "table" => "table",
            _ => "point",
        };
        let place = match kind {
            "dimension" => self.dimensions.len(),
            "zone" => self.zones.len(),
            "text_box" => self.text_boxes.len(),
            "image" => self.images.len(),
            "barcode" => self.barcodes.len(),
            "table" => self.tables.len(),
            _ => self.points.len(),
        };
        match tag {
            "dimension" => {
                let Some(d) = crate::import_items::read_dimension(child) else { return false };
                items.dimensions.push(d);
                self.dimensions.push(uuid);
            }
            "zone" => {
                // (`parseZONE`'s `T_attr` / `T_teardrop` is `read_zone`'s: the zone says it is a teardrop and of which type)
                let Some(read) = crate::import::read_zone(child, &Default::default(), &crate::padstack_io::library_copper_layers()) else { return false };
                let zone = eda_model::ir::Zone { net: String::new(), uuid: String::new(), ..read.zone };
                let extras = ZoneExtras { fill: zone_fill_of(child, &zone), thieving: read.thieving, layer_props: read.layer_props, arc_rings: read.arc_rings };
                if extras.fill.is_some() || extras.thieving.is_some() || extras.layer_props.is_some() || extras.arc_rings.is_some() {
                    self.zone_extras.push((self.zones.len(), extras));
                }
                items.zones.push(zone);
                self.zones.push(uuid);
            }
            "fp_text_box" | "gr_text_box" => {
                let Some(t) = crate::pcb_textbox::read(child, version) else { return false };
                items.text_boxes.push(t);
                self.text_boxes.push(uuid);
            }
            "point" => {
                items.points.push(crate::pcb_point::read(child));
                self.points.push(uuid);
            }
            "image" => {
                let Some(mut img) = crate::pcb_image::read(child) else { return false };
                img.scale_ppm = crate::pcb_image::scale_from_file(img.scale_ppm, &img.data, version);
                items.images.push(img);
                self.images.push(uuid);
            }
            "barcode" => {
                let Some(b) = crate::barcode_io::read(child) else { return false };
                items.barcodes.push(b);
                self.barcodes.push(uuid);
            }
            "table" => {
                let Some(t) = crate::pcb_table::read(child) else { return false };
                items.tables.push(eda_model::table::Table { uuid: String::new(), ..t });
                self.tables.push(uuid);
            }
            _ => return false,
        }
        if locked {
            self.locked.push((kind, place));
        }
        true
    }

    /// Once the items have their ids: each one's uuid into `fp.kicad.uuids`, its lock into `fp.kicad.locked`, a zone's fill, thieving, per-layer settings and arcs
    /// into theirs.
    fn keep_uuids(self, fp: &mut LibraryFootprint) {
        let it = &fp.items;
        for &(kind, i) in &self.locked {
            let id = match kind {
                "dimension" => it.dimensions.get(i).map(|x| x.id.clone()),
                "zone" => it.zones.get(i).map(|x| x.id.clone()),
                "text_box" => it.text_boxes.get(i).map(|x| x.id.clone()),
                "image" => it.images.get(i).map(|x| x.id.clone()),
                "barcode" => it.barcodes.get(i).map(|x| x.id.clone()),
                "table" => it.tables.get(i).map(|x| x.id.clone()),
                _ => None,
            };
            if let Some(id) = id {
                fp.kicad.locked.insert(id);
            }
        }
        for (i, extras) in self.zone_extras {
            let Some(z) = it.zones.get(i) else { continue };
            if let Some(f) = extras.fill {
                fp.kicad.zone_fills.insert(z.id.clone(), f);
            }
            if let Some(t) = extras.thieving {
                fp.kicad.zone_thieving.insert(z.id.clone(), t);
            }
            if let Some(p) = extras.layer_props {
                fp.kicad.zone_layer_props.insert(z.id.clone(), p);
            }
            if let Some(rings) = extras.arc_rings {
                fp.kicad.zone_arcs.push(eda_model::ir::ZoneArcs { id: z.id.clone(), rings, shape: eda_model::ir::ZoneArcs::shape_of(z) });
            }
        }
        let ids = (it.dimensions.iter().map(|d| d.id.clone()).zip(self.dimensions))
            .chain(it.zones.iter().map(|z| z.id.clone()).zip(self.zones))
            .chain(it.text_boxes.iter().map(|t| t.id.clone()).zip(self.text_boxes))
            .chain(it.points.iter().map(|p| p.id.clone()).zip(self.points))
            .chain(it.images.iter().map(|i| i.id.clone()).zip(self.images))
            .chain(it.barcodes.iter().map(|b| b.id.clone()).zip(self.barcodes))
            .chain(it.tables.iter().map(|t| t.id.clone()).zip(self.tables));
        let uuids: Vec<(String, String)> = ids.filter_map(|(id, u)| Some((id, u?))).collect();
        fp.kicad.uuids.extend(uuids);
    }
}

/// `PCB_IO_KICAD_SEXPR_PARSER::resolveGroups` for a footprint's groups (`raw`, each with the text of its form): a member is the item of the footprint the uuid names
/// -- a pad, a graphic, a text, a field (`field:<name>`), an item of `fp.items`, or another group -- as KiCad looks it up among the footprint's children
/// (`footprint->RunOnChildren( .., NO_RECURSE )`), and a uuid that names none is dropped. The groups are made with [`crate::import_items::build_groups_with_uuids`],
/// the board's: an inner group before the group that holds it, each group's id from its members'; each keeps its uuid in `fp.kicad.uuids`.
///
/// A locked group keeps its lock by its id (`LibraryFootprintKeep::locked`). A group that names a form kept as the file wrote it (`kept`: the uuids of
/// `fp.kicad.tokens`; a group holding such a group too) is kept as the file wrote it, as before `FootprintItems`: its members are named by uuid there, and every
/// item keeps its uuid, so the group still holds them.
fn resolve_groups(fp: &mut LibraryFootprint, raw: Vec<(crate::import_items::RawGroup, String)>, mut kept: std::collections::HashSet<String>) {
    let mut as_text: Vec<bool> = vec![false; raw.len()];
    // a locked group keeps its lock by its id (`LibraryFootprintKeep::locked`, UI-LEFT-2 FF6), as a graphic does
    let locked_uuids: std::collections::HashSet<String> = raw.iter().filter(|(g, _)| g.locked).filter_map(|(g, _)| g.uuid.clone()).collect();
    loop {
        let mut more = false;
        for (i, (g, _)) in raw.iter().enumerate() {
            if !as_text[i] && g.members.iter().any(|m| kept.contains(m)) {
                as_text[i] = true;
                kept.extend(g.uuid.clone());
                more = true;
            }
        }
        if !more {
            break;
        }
    }
    let mut by_uuid: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (id, uuid) in &fp.kicad.uuids {
        by_uuid.entry(uuid.clone()).or_default().push(id.clone());
    }
    let mut groups = Vec::new();
    for ((g, text), text_form) in raw.into_iter().zip(as_text) {
        if text_form {
            fp.kicad.tokens.push(text);
        } else {
            groups.push(g);
        }
    }
    let names: std::collections::BTreeSet<String> = fp.pads.iter().map(|p| p.id.clone()).chain(fp.graphics.iter().map(|g| g.id().to_string())).chain(fp.texts.iter().map(|t| t.id.clone())).chain(fp.items.ids().map(str::to_string)).collect();
    let (built, _, uuids) = crate::import_items::build_groups_with_uuids(&by_uuid, &groups, names);
    for (g, uuid) in built.iter().zip(uuids) {
        if let Some(u) = uuid.filter(|u| !u.is_empty()) {
            if locked_uuids.contains(&u) {
                fp.kicad.locked.insert(g.id.clone());
            }
            fp.kicad.uuids.insert(g.id.clone(), u);
        }
    }
    fp.items.groups = built;
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
        size_nm: mm_to_nm(size_mm).max(Nm::new(1)),
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
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;
    use crate::export_kicad_mod;
    use eda_model::ir::ChamferCorners;

    fn pad(number: &str, kind: PadKind, shape: LibraryPadShape, at: (Nm, Nm), size: (Nm, Nm)) -> LibraryPad {
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
        let mut p1 = pad("1", PadKind::Smd, LibraryPadShape::RoundRect, (mm(-1.0), Nm::ZERO), (mm(0.8), mm(1.2)));
        p1.roundrect_ratio = Some(0.3);
        p1.rot = 90_000;
        p1.layers = vec!["F.Cu".into(), "F.Mask".into()];
        p1.clearance_override = Some(mm(0.2));
        let mut p2 = pad("2", PadKind::ThroughHole, LibraryPadShape::Oval, (mm(1.0), Nm::ZERO), (mm(1.6), mm(2.0)));
        p2.drill = Some(mm(0.8));
        p2.offset = Point { x: mm(0.1), y: mm(-0.05) };
        p2.layers = vec!["*.Cu".into(), "*.Mask".into()];
        let mut p3 = pad("3", PadKind::ThroughHole, LibraryPadShape::Circle, (Nm::ZERO, mm(2.0)), (mm(1.7), mm(1.7)));
        p3.drill_slot = Some((mm(0.6), mm(1.7)));
        let p4 = pad("", PadKind::NonPlatedHole, LibraryPadShape::Circle, (Nm::ZERO, mm(-2.0)), (mm(0.9), mm(0.9)));
        fp.pads = vec![p1, p2, p3, p4];
        fp.graphics = vec![
            Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: Point { x: mm(-2.0), y: mm(-1.5) }, end: Point { x: mm(2.0), y: mm(-1.5) } },
            Shape::Arc { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: Point { x: mm(1.0), y: Nm::ZERO }, mid: Point { x: Nm::ZERO, y: mm(1.0) }, end: Point { x: mm(-1.0), y: Nm::ZERO } },
            Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: true, start: Point { x: mm(-1.5), y: mm(-1.0) }, end: Point { x: mm(1.5), y: mm(1.0) } },
            Shape::Circle { id: String::new(), layer: "F.CrtYd".into(), stroke_width: mm(0.05), filled: false, center: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(3.0), y: Nm::ZERO } },
            Shape::Polygon { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(0.5), y: Nm::ZERO }, Point { x: mm(0.5), y: mm(0.5) }] },
            Shape::Bezier { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, c1: Point { x: mm(0.1), y: mm(0.2) }, c2: Point { x: mm(0.3), y: mm(0.2) }, end: Point { x: mm(0.4), y: Nm::ZERO } },
        ];
        fp.texts = vec![Text { id: String::new(), content: "hello".into(), at: Point { x: mm(0.1), y: mm(3.0) }, angle: 90_000, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Left, mirror: true }];
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
        assert_eq!((p1.shape, p1.kind, p1.rot, p1.at, p1.size), (LibraryPadShape::RoundRect, PadKind::Smd, 90_000, Point { x: mm(-1.0), y: Nm::ZERO }, (mm(0.8), mm(1.2))));
        assert_eq!(p1.roundrect_ratio, Some(0.3));
        assert_eq!(p1.layers, vec!["F.Cu".to_string(), "F.Mask".to_string()]);
        assert_eq!(p1.clearance_override, Some(mm(0.2)));
        let p2 = by("2");
        assert_eq!((p2.shape, p2.kind, p2.drill, p2.offset), (LibraryPadShape::Oval, PadKind::ThroughHole, Some(mm(0.8)), Point { x: mm(0.1), y: mm(-0.05) }));
        assert_eq!(by("3").drill_slot, Some((mm(0.6), mm(1.7))));
        let npth = back.pads.iter().find(|p| p.kind == PadKind::NonPlatedHole).unwrap();
        assert_eq!(npth.number, "");
        assert_eq!(back.graphics.len(), 6);
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Rect { filled: true, layer, .. } if layer == "F.Fab")));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Arc { mid, .. } if *mid == Point { x: Nm::ZERO, y: mm(1.0) })));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Bezier { c2, .. } if *c2 == Point { x: mm(0.3), y: mm(0.2) })));
        assert!(back.graphics.iter().any(|g| matches!(g, Shape::Polygon { pts, .. } if pts.len() == 3)));
        assert_eq!(back.texts.len(), 1);
        let t = &back.texts[0];
        assert_eq!((t.content.as_str(), t.angle, t.size_nm, t.stroke_width, t.justify, t.mirror), ("hello", 90_000, mm(1.0), mm(0.15), TextJustify::Left, true));
        assert!(back.courtyard.is_some(), "the F.CrtYd circle gives a courtyard box");
        assert!(back.pads.iter().all(|p| !p.id.is_empty()) && back.graphics.iter().all(|g| !g.id().is_empty()));
    }

    /// As [`crate::footprint_lib::parse_footprint_file`] (the test there): the Footprint Editor's reader is gated by the footprint file's own
    /// `(version ..)` too (`parseEDA_TEXT`'s `m_requiredVersion < 20260826`), so a 20261017 bold `(thickness 0.125)` is the older 0.2 mm.
    #[test]
    fn a_kicad_11_footprint_files_bold_text_keeps_its_width() {
        let text = r#"(footprint "X" (version 20261017) (generator "pcbnew") (layer "F.Cu")
            (fp_text user "X" (at 0 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.125) (bold yes))))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))"#;
        let fp = parse_library_footprint(text).expect("parses").footprint;
        assert_eq!(fp.texts.len(), 1);
        assert_eq!(fp.texts[0].stroke_width, mm(0.2), "the width KiCad 10 drew, which the writer's older file version means");
    }

    #[test]
    fn the_zone_connection_fields_of_a_footprint_and_its_pads_survive_a_kicad_mod_round_trip() {
        // `parsePAD` / `format( const PAD* )`: zone_connect, thermal_bridge_width, thermal_bridge_angle, thermal_gap, clearance;
        // `(footprint (zone_connect N))` for the footprint itself.
        let mut fp = sample();
        fp.zone_connection = Some(eda_model::ir::PadConnection::Thermal);
        fp.pads[0].zone_connection = Some(eda_model::ir::PadConnection::Full);
        fp.pads[0].thermal_gap_override = Some(mm(0.25));
        fp.pads[0].thermal_spoke_width_override = Some(mm(0.3));
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
        assert_eq!((p1.thermal_gap_override, p1.thermal_spoke_width_override, p1.thermal_spoke_angle_mdeg, p1.clearance_override), (Some(mm(0.25)), Some(mm(0.3)), Some(30_500), Some(mm(0.2))));
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
        assert_eq!((l.layer.as_str(), l.at, l.size, l.visible), ("F.Fab", Point { x: Nm::ZERO, y: Nm::ZERO }, (mm(1.0), mm(1.0)), false));
        // the Reference and the Value keep their places; the Value's text is the footprint's name, so it stores none of its own
        let reference = fp.reference.as_ref().expect("the Reference layout");
        assert_eq!((reference.layer.as_str(), reference.at, reference.text.as_deref(), reference.visible), ("F.SilkS", Point { x: Nm::ZERO, y: mm(-1.43) }, None, true));
        let value = fp.value.as_ref().expect("the Value layout");
        assert_eq!((value.layer.as_str(), value.at.y, value.visible), ("F.Fab", mm(1.43), false));
        assert!(fp.attributes.smd && fp.attributes.allow_soldermask_bridges);
        let by = |n: &str| fp.pads.iter().find(|p| p.number == n).unwrap();
        assert_eq!((by("1").shape, by("1").roundrect_ratio), (LibraryPadShape::RoundRect, Some(0.25)));
        let chamfered = by("2");
        assert_eq!(chamfered.shape, LibraryPadShape::ChamferedRect);
        assert_eq!(chamfered.chamfer_ratio, Some(0.2));
        assert!(chamfered.chamfer_corners.top_left && chamfered.chamfer_corners.bottom_right && !chamfered.chamfer_corners.top_right);
        assert_eq!((by("3").shape, by("3").trapezoid_delta), (LibraryPadShape::Trapezoid, Some((Nm::ZERO, mm(0.2)))));
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
