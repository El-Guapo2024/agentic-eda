//! A footprint's graphics and texts ([`eda_model::fp_art`]) in a `.kicad_mod` and in a `.kicad_pcb`: read the way
//! `PCB_IO_KICAD_SEXPR_PARSER` reads them (`parseFOOTPRINT_unchecked`, `parsePCB_SHAPE`, `parsePCB_TEXT`, `parsePCB_TEXT_effects`,
//! `parseEDA_TEXT`) and written the way `PCB_IO_KICAD_SEXPR` writes them (`format( FOOTPRINT )`, `format( PCB_SHAPE )`, `format( PCB_TEXT )`,
//! `EDA_TEXT::Format`, `STROKE_PARAMS::Format`) at KiCad 8303b2ad.
//!
//! # The footprint's frame
//!
//! A footprint's items are stored in its own frame -- the board position minus the footprint's, turned back by its orientation
//! (`formatInternalUnits( pos, parentFP )`) -- and a flipped footprint's frame is whatever `FOOTPRINT::Flip` left it: its graphics mirrored,
//! on the other side's layers. This studio keeps a definition as seen from the top and mirrors x when it places a bottom-side instance, so a
//! bottom-side footprint's file x is negated on the way in and on the way out, exactly as a pad's is
//! ([`crate::import`]'s `parse_pad_geometry`, `write_footprint`); its layers are swapped ([`eda_model::fp_art::layer_on_side`], an
//! involution) and a text's angle and mirror flag follow `PCB_TEXT::Flip` ([`eda_model::fp_edit::FieldLayout`] has the angle arithmetic).
//!
//! What is read: `fp_line`, `fp_arc` (also the pre-KiCad-7 `(start CENTRE) (end START) (angle A)` form), `fp_circle`, `fp_rect`, `fp_poly`
//! (an `(arc ..)` among its points is flattened at 5 micrometres), `fp_curve` and `fp_text user`, on every layer but the courtyard and
//! the board edge (see [`eda_model::fp_art`]). A hidden text is left out: KiCad turns it into a hidden field.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use eda_model::fp_art::{is_art_layer, is_side_specific, layer_on_side, FpArt, FpShape, FpText, Hatch, LineStyle};
use eda_model::ir::{FootprintInstance, Millideg, Point, Shape, Side, Um};
use eda_model::outline::Arc as GeoArc;
use eda_model::{ConstraintModel, Footprint};

use crate::fp_fields::{effects_with, parse_layout};
use crate::import::{legacy_arc, mm_to_um, xy_point};
use crate::sexpr::{self, Sexpr};
use crate::{duid_for, fmt_mm_f, mm, sexpr_str};

/// `DEFAULT_LINE_WIDTH` (`board_design_settings.h`), 0.1 mm: what a stroke of no width that is not filled is given (`parsePCB_SHAPE`).
const DEFAULT_LINE_WIDTH_UM: Um = 100;

const SHAPE_TAGS: [&str; 6] = ["fp_line", "fp_arc", "fp_circle", "fp_rect", "fp_poly", "fp_curve"];

// ---------------------------------------------------------------------------------------------------------------- reading

/// The art of a `(footprint ..)`: `side` and `fp_rot` are the footprint's own (`Side::Top` and 0 for a `.kicad_mod`; `fp_rot` is the studio's
/// clockwise millidegrees, the file angle negated).
pub(crate) fn read_art(fp: &[Sexpr], side: Side, fp_rot: Millideg) -> FpArt {
    let mut art = FpArt::default();
    for item in fp.iter().filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some(tag) if SHAPE_TAGS.contains(&tag) => art.shapes.extend(read_shape(item, tag, side)),
            Some("fp_text") if sexpr::txt(item, 1) == Some("user") => art.texts.extend(read_text(item, side, fp_rot)),
            _ => {}
        }
    }
    art
}

/// Layer words the file uses for a set (`m_layerMasks`): the wildcards stand for both sides' layer.
fn expand_layer_word(word: &str) -> Vec<String> {
    match word {
        "*.Cu" | "F&B.Cu" => vec!["F.Cu".into(), "B.Cu".into()],
        w if w.starts_with("*.") => vec![format!("F.{}", &w[2..]), format!("B.{}", &w[2..])],
        w => vec![w.to_string()],
    }
}

/// `(layer "L")`, else `(layers "A" "B" ..)`, as the layer names of the file.
fn file_layers(item: &[Sexpr]) -> Vec<String> {
    if let Some(l) = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)) {
        return vec![l.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for w in sexpr::find(item, "layers").map(|l| l.iter().skip(1).filter_map(Sexpr::text).collect::<Vec<_>>()).unwrap_or_default() {
        for l in expand_layer_word(w) {
            if !out.contains(&l) {
                out.push(l);
            }
        }
    }
    out
}

/// The points of a `(pts ..)` list: an `(xy x y)` is itself, an `(arc (start ..) (mid ..) (end ..))` its flattening
/// (`SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`; the arc's start is the point before it, which is not repeated).
fn pts_of(item: &[Sexpr]) -> Vec<Point> {
    let Some(pts) = sexpr::find(item, "pts") else { return Vec::new() };
    let mut out: Vec<Point> = Vec::new();
    for e in pts.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(e) {
            Some("xy") => out.extend(xy_point(e)),
            Some("arc") => {
                let get = |name: &str| sexpr::find(e, name).and_then(xy_point);
                if let (Some(s), Some(m), Some(en)) = (get("start"), get("mid"), get("end")) {
                    let line = GeoArc::through(s, m, en).polyline(eda_model::outline::MAX_ERROR_UM as f64);
                    for p in line {
                        if out.last() != Some(&p) {
                            out.push(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// One `fp_line`, `fp_arc`, `fp_circle`, `fp_rect`, `fp_poly` or `fp_curve` (`parsePCB_SHAPE`). `None` for an item that lacks what it needs, one
/// that is only on the courtyard or the board edge, or a polygon of fewer than three points.
fn read_shape(item: &[Sexpr], tag: &str, side: Side) -> Option<FpShape> {
    let mut file_layers = file_layers(item);
    file_layers.retain(|l| is_art_layer(l));
    if file_layers.is_empty() {
        return None;
    }
    // The definition is seen from the top: a bottom-side footprint's x is un-mirrored and its layers are turned back over.
    let mirror = |p: Point| if side == Side::Bottom { Point { x: -p.x, y: p.y } } else { p };
    let raw = |name: &str| sexpr::find(item, name).and_then(xy_point);
    let pt = |name: &str| raw(name).map(mirror);
    let layers: Vec<String> = file_layers.iter().map(|l| layer_on_side(l, side)).collect();
    let layer = layers[0].clone();

    // `(stroke (width w) (type t))`, or the older `(width w)` on the item itself.
    let stroke = sexpr::find(item, "stroke");
    let mut width = stroke.and_then(|s| sexpr::find(s, "width")).or_else(|| sexpr::find(item, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(0);
    let style = stroke.and_then(|s| sexpr::find(s, "type")).and_then(|t| sexpr::txt(t, 1)).and_then(LineStyle::from_token).unwrap_or(LineStyle::Solid);

    // `(fill yes|solid|no|none|hatch|reverse_hatch|cross_hatch)`; `yes` is what a solid fill was first called.
    let mut filled = false;
    let mut hatch = None;
    let fill = sexpr::find(item, "fill");
    for w in fill.map(|f| f.iter().skip(1).filter_map(Sexpr::text).collect::<Vec<_>>()).unwrap_or_default() {
        match w {
            "yes" | "solid" => filled = true,
            "none" | "no" => filled = false,
            "hatch" => hatch = Some(Hatch::Hatch),
            "reverse_hatch" => hatch = Some(Hatch::ReverseHatch),
            "cross_hatch" => hatch = Some(Hatch::CrossHatch),
            _ => {}
        }
    }
    // Files from before the `fill` token: a rectangle or circle of no width was filled, a polygon off the board edge always was.
    if fill.is_none() && ((width == 0 && matches!(tag, "fp_rect" | "fp_circle")) || tag == "fp_poly") {
        filled = true;
    }
    // Only a filled shape may have no width.
    if width <= 0 && !(filled || hatch.is_some()) {
        width = DEFAULT_LINE_WIDTH_UM;
    }
    let width = width.max(0);

    let shape = match tag {
        "fp_line" => Shape::Segment { id: String::new(), layer, stroke_width: width, filled: false, start: pt("start")?, end: pt("end")? },
        "fp_arc" => {
            let (start, mid, end) = match raw("mid") {
                Some(mid) => (pt("start")?, mirror(mid), pt("end")?),
                // `(start CENTRE) (end ARC_START) (angle A)`: before KiCad 7.
                None => {
                    let angle = sexpr::find(item, "angle").and_then(|a| sexpr::num(a, 1))?;
                    let (s, m, e) = legacy_arc(raw("start")?, raw("end")?, angle);
                    (mirror(s), mirror(m), mirror(e))
                }
            };
            Shape::Arc { id: String::new(), layer, stroke_width: width, filled: false, start, mid, end }
        }
        "fp_circle" => Shape::Circle { id: String::new(), layer, stroke_width: width, filled, center: pt("center")?, end: pt("end")? },
        "fp_rect" => Shape::Rect { id: String::new(), layer, stroke_width: width, filled, start: pt("start")?, end: pt("end")? },
        "fp_poly" => {
            let pts: Vec<Point> = pts_of(item).into_iter().map(mirror).collect();
            if pts.len() < 3 {
                return None;
            }
            Shape::Polygon { id: String::new(), layer, stroke_width: width, filled, pts }
        }
        _ => {
            let pts: Vec<Point> = pts_of(item).into_iter().map(mirror).collect();
            let [start, c1, c2, end] = pts[..] else { return None };
            Shape::Bezier { id: String::new(), layer, stroke_width: width, filled: false, start, c1, c2, end }
        }
    };
    let radius = if tag == "fp_rect" { sexpr::find(item, "radius").and_then(|r| sexpr::num(r, 1)).map(mm_to_um).unwrap_or(0).max(0) } else { 0 };
    Some(FpShape {
        shape,
        style,
        hatch: hatch.filter(|_| matches!(tag, "fp_rect" | "fp_circle" | "fp_poly")),
        radius,
        layers: if layers.len() > 1 { layers } else { Vec::new() },
        mask_margin: sexpr::find(item, "solder_mask_margin").and_then(|m| sexpr::num(m, 1)).map(mm_to_um),
    })
}

/// One `(fp_text user "text" (at x y [angle]) (layer "L") (effects ..))` (`parsePCB_TEXT`). The layout is read by the fields' reader (it is the
/// same `EDA_TEXT`), then turned from the file's frame to the definition's: layer and mirror flag as `PCB_TEXT::Flip` leaves them on a bottom-side
/// footprint. `None` for a hidden text.
fn read_text(item: &[Sexpr], side: Side, fp_rot: Millideg) -> Option<FpText> {
    // `(fp_text user locked "text" ..)`: the bare `locked` of KiCad 5.99 sits before the text.
    let at = if sexpr::txt(item, 2) == Some("locked") { 3 } else { 2 };
    let raw = sexpr::txt(item, at)?;
    let mut l = parse_layout(item, side, fp_rot);
    if !l.visible {
        return None;
    }
    let file_layer = l.layer.clone();
    l.layer = layer_on_side(&file_layer, side);
    if side == Side::Bottom && is_side_specific(&file_layer) {
        l.mirror = !l.mirror;
    }
    let font = sexpr::find(item, "effects").and_then(|e| sexpr::find(e, "font"));
    let face = font.and_then(|f| sexpr::find(f, "face")).and_then(|f| sexpr::txt(f, 1)).filter(|f| !f.is_empty()).map(str::to_string);
    let line_spacing_permille = font.and_then(|f| sexpr::find(f, "line_spacing")).and_then(|s| sexpr::num(s, 1)).map(|v| (v * 1000.0).round() as i64).filter(|v| *v != 1000).unwrap_or(0);
    // `%V` and `%R` are the old spellings of `${VALUE}` and `${REFERENCE}` (the lexer has already made the `\n` of the file a newline).
    let text = raw.replace("%V", "${VALUE}").replace("%R", "${REFERENCE}");
    Some(FpText { text, layout: l, face, line_spacing_permille })
}

// ------------------------------------------------------------------------------------------ the installed KiCad footprint of a built-in one

/// Installed footprints read once and kept, against the modification time of their file (a board is loaded on every request).
fn installed_cache() -> &'static Mutex<HashMap<PathBuf, (Option<std::time::SystemTime>, Option<Arc<Footprint>>)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (Option<std::time::SystemTime>, Option<Arc<Footprint>>)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The footprint a `.kicad_mod` file holds (`id` its name), read once for as long as the file stays as it is.
fn installed_file(path: &Path, id: &str) -> Option<Arc<Footprint>> {
    let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some((s, hit)) = installed_cache().lock().unwrap_or_else(PoisonError::into_inner).get(path) {
        if *s == stamp {
            return hit.clone();
        }
    }
    let found = std::fs::read_to_string(path).ok().and_then(|text| crate::footprint_lib::parse_footprint_file(&text, id).ok()).map(Arc::new);
    installed_cache().lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_path_buf(), (stamp, found.clone()));
    found
}

/// The art of an installed library footprint put on the pads of the built-in footprint that stands for it: the turn and shift that bring the library's pads
/// onto the built-in ones ([`crate::fit_pads`], as the 3D model of such a part is placed) carry the art along.
fn fitted_art(lib: &Footprint, builtin: &Footprint) -> eda_model::fp_art::FpArt {
    let pads = |f: &Footprint| -> Vec<(String, (f64, f64))> { f.pads.iter().map(|p| (p.number.clone(), (p.at.0 as f64, p.at.1 as f64))).collect() };
    match crate::fit_pads(&pads(lib), &pads(builtin)) {
        None => lib.art.clone(),
        Some(fit) => {
            let by = FootprintInstance { id: String::new(), at: Point { x: fit.dx_um.round() as Um, y: fit.dy_um.round() as Um }, rot: (fit.rot_deg * 1000.0).round().rem_euclid(360_000.0) as Millideg, side: Side::Top, label: Default::default() };
            lib.art.moved(&by)
        }
    }
}

/// For every part whose footprint is one of the studio's built-in packages (no footprint of the intent or the libraries under its name), the art of the installed
/// KiCad footprint the package stands for (`eda_model::footprint::kicad_footprint_of_part`: `0603` on an `R` is `Resistor_SMD:R_0603_1608Metric`), found
/// through `find` as any library footprint is: the part's silkscreen, its fab body and its texts, which the built-in pads have none of. Filled into
/// [`ConstraintModel::installed_art`]. Best-effort: a package whose twin is not installed is left as it was.
pub(crate) fn attach_installed_art(model: &mut ConstraintModel, find: &mut dyn FnMut(&str) -> Option<PathBuf>) {
    let mut found: BTreeMap<String, eda_model::fp_art::FpArt> = BTreeMap::new();
    for part in &model.parts {
        let keys = [part.footprint.as_deref(), part.package.as_deref()];
        let explicit = keys.into_iter().flatten().any(|k| model.footprints.iter().any(|f| f.name == k || eda_model::footprint::normalize_name(&f.name) == eda_model::footprint::normalize_name(k)));
        if explicit {
            continue;
        }
        let Some(builtin) = model.library_footprint_of(part) else { continue };
        let Some(twin) = eda_model::footprint::kicad_footprint_of_part(part) else { continue };
        let id = twin.id();
        let Some(lib) = find(&id).and_then(|path| installed_file(&path, &id)) else { continue };
        if !lib.art.is_empty() {
            found.insert(part.reference.clone(), fitted_art(&lib, &builtin));
        }
    }
    model.installed_art = found;
}

// ---------------------------------------------------------------------------------------------------------------- writing

/// The order `PCB_LAYER_ID` numbers the layers in: what `formatLayers` lists the layers it has no wildcard for in.
fn layer_rank(layer: &str) -> usize {
    const ORDER: [&str; 12] = ["B.Adhes", "F.Adhes", "B.Paste", "F.Paste", "B.SilkS", "F.SilkS", "B.Mask", "F.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User"];
    const AFTER_EDGE: [&str; 5] = ["Margin", "B.CrtYd", "F.CrtYd", "B.Fab", "F.Fab"];
    if layer == "F.Cu" {
        return 0;
    }
    if let Some(n) = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|n| n.parse::<usize>().ok()) {
        return n;
    }
    if layer == "B.Cu" {
        return 31;
    }
    if let Some(i) = ORDER.iter().position(|l| *l == layer) {
        return 32 + i;
    }
    if layer == "Edge.Cuts" {
        return 44;
    }
    if let Some(i) = AFTER_EDGE.iter().position(|l| *l == layer) {
        return 45 + i;
    }
    if let Some(n) = layer.strip_prefix("User.").and_then(|n| n.parse::<usize>().ok()) {
        return 49 + n;
    }
    1000
}

/// `formatLayers( mask, false )`: both outer copper layers are `"*.Cu"`, a front and back pair of any other side-specific layer `"*.X"`, and the rest
/// one by one in layer order.
fn layers_token(layers: &[String]) -> String {
    let mut rest: BTreeSet<&str> = layers.iter().map(String::as_str).collect();
    let mut words: Vec<String> = Vec::new();
    if rest.contains("F.Cu") && rest.contains("B.Cu") {
        words.push("*.Cu".into());
        rest.remove("F.Cu");
        rest.remove("B.Cu");
    }
    for kind in ["Adhes", "Paste", "SilkS", "Mask", "CrtYd", "Fab"] {
        let (f, b) = (format!("F.{kind}"), format!("B.{kind}"));
        if rest.contains(f.as_str()) && rest.contains(b.as_str()) {
            words.push(format!("*.{kind}"));
            rest.remove(f.as_str());
            rest.remove(b.as_str());
        }
    }
    let mut singles: Vec<&str> = rest.into_iter().collect();
    singles.sort_by_key(|l| (layer_rank(l), *l));
    words.extend(singles.into_iter().map(str::to_string));
    format!("(layers {})", words.iter().map(|w| sexpr_str(w)).collect::<Vec<_>>().join(" "))
}

/// A pad's `(layers ..)`: the layers it is on by its type and side (`base`), and those its import found it on besides copper, paste and mask (`PadMaskInfo::art_layers`, named as
/// for a top-side footprint, so they go to `side`'s).
pub(crate) fn pad_layers_token(base: &[&str], art: &[String], side: Side) -> String {
    let mut all: Vec<String> = base.iter().map(|l| l.to_string()).collect();
    all.extend(art.iter().map(|l| eda_model::fp_art::layer_on_side(l, side)));
    layers_token(&all)
}

/// A string as KiCad's `Quotew` writes it (a newline is `\n`: [`sexpr_str`]).
fn quote(s: &str) -> String {
    sexpr_str(s)
}

/// The art of the footprint `fp`, in the file's frame: shapes, then texts (`format( FOOTPRINT )` writes the drawings sorted by type, shapes before
/// text), each with a uuid made from the footprint and the item's place in the art.
pub(crate) fn write_art(out: &mut String, art: &FpArt, fp: &FootprintInstance) {
    for (i, s) in art.shapes.iter().enumerate() {
        write_shape(out, s, fp, &duid_for(&format!("footprint:{}:art:{i}", fp.id), &fp.id));
    }
    for (i, t) in art.texts.iter().enumerate() {
        write_text(out, t, fp, &duid_for(&format!("footprint:{}:arttext:{i}", fp.id), &fp.id));
    }
}

fn write_shape(out: &mut String, s: &FpShape, fp: &FootprintInstance, uuid: &str) {
    let bottom = fp.side == Side::Bottom;
    let pt = |p: Point| format!("{} {}", mm(if bottom { -p.x } else { p.x }), mm(p.y));
    out.push_str("\t\t");
    match &s.shape {
        Shape::Segment { start, end, .. } => write!(out, "(fp_line (start {}) (end {})", pt(*start), pt(*end)).unwrap(),
        Shape::Arc { start, mid, end, .. } => write!(out, "(fp_arc (start {}) (mid {}) (end {})", pt(*start), pt(*mid), pt(*end)).unwrap(),
        Shape::Circle { center, end, .. } => write!(out, "(fp_circle (center {}) (end {})", pt(*center), pt(*end)).unwrap(),
        Shape::Rect { start, end, .. } => {
            write!(out, "(fp_rect (start {}) (end {})", pt(*start), pt(*end)).unwrap();
            if s.radius > 0 {
                write!(out, " (radius {})", mm(s.radius)).unwrap();
            }
        }
        Shape::Polygon { pts, .. } => {
            out.push_str("(fp_poly (pts");
            for p in pts {
                write!(out, " (xy {})", pt(*p)).unwrap();
            }
            out.push(')');
        }
        Shape::Bezier { start, c1, c2, end, .. } => write!(out, "(fp_curve (pts (xy {}) (xy {}) (xy {}) (xy {}))", pt(*start), pt(*c1), pt(*c2), pt(*end)).unwrap(),
    }
    write!(out, " (stroke (width {}) (type {}))", mm(s.shape.stroke_width().max(0)), s.style.token()).unwrap();
    // The fill is written for the three shapes that have an inside only (`format( PCB_SHAPE )`).
    if matches!(s.shape, Shape::Rect { .. } | Shape::Circle { .. } | Shape::Polygon { .. }) {
        match s.hatch {
            Some(h) => write!(out, " (fill {})", h.token()).unwrap(),
            None => write!(out, " (fill {})", if s.shape.is_filled() { "yes" } else { "no" }).unwrap(),
        }
    }
    let on_board: Vec<String> = s.all_layers().iter().map(|l| layer_on_side(l, fp.side)).collect();
    if on_board.len() > 1 {
        write!(out, " {}", layers_token(&on_board)).unwrap();
    } else {
        write!(out, " (layer {})", sexpr_str(&on_board[0])).unwrap();
    }
    if let Some(m) = s.mask_margin {
        if on_board[0] == "F.Cu" || on_board[0] == "B.Cu" {
            write!(out, " (solder_mask_margin {})", mm(m)).unwrap();
        }
    }
    writeln!(out, " (uuid \"{uuid}\"))").unwrap();
}

fn write_text(out: &mut String, t: &FpText, fp: &FootprintInstance, uuid: &str) {
    // The layout back into the file's frame: the layer on the footprint's side and the mirror flag as `PCB_TEXT::Flip` leaves it, the anchor and the
    // angle as `FieldLayout` has them across.
    let mut l = t.layout.clone();
    if fp.side == Side::Bottom && is_side_specific(&l.layer) {
        l.mirror = !l.mirror;
    }
    l.layer = layer_on_side(&l.layer, fp.side);
    let (x, y) = l.file_position(fp.side);
    let angle = fmt_mm_f(l.file_angle_deg(fp.side, fp.rot));
    let unlocked = if l.keep_upright { "" } else { " (unlocked yes)" };
    let knockout = if l.knockout { " knockout" } else { "" };
    writeln!(
        out,
        "\t\t(fp_text user {} (at {} {} {angle}){unlocked} (layer {}{knockout}) (uuid \"{uuid}\") {})",
        quote(&t.text),
        mm(x),
        mm(y),
        sexpr_str(&l.layer),
        effects_with(&l, t.face.as_deref(), t.line_spacing_permille)
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    fn inst(side: Side, rot: Millideg) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: p(0, 0), rot, side, label: Default::default() }
    }

    /// Every kind of shape, a multi-layer one and two texts, as a library file writes them.
    const FIXTURE: &str = r#"(footprint "Test:All" (version 20240108) (generator "pcbnew") (layer "F.Cu")
        (property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
        (property "Value" "All" (at 0 3 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
        (fp_line (start -2 -1.5) (end 2 -1.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "a1"))
        (fp_arc (start 1 0) (mid 0 1) (end -1 0) (stroke (width 0.12) (type dash)) (layer "F.SilkS") (uuid "a2"))
        (fp_circle (center 0 0) (end 0.5 0) (stroke (width 0.1) (type solid)) (fill yes) (layer "F.Fab") (uuid "a3"))
        (fp_rect (start -1.5 -1) (end 1.5 1) (radius 0.2) (stroke (width 0.1) (type default)) (fill no) (layer "F.Fab") (uuid "a4"))
        (fp_poly (pts (xy 0 0) (xy 0.5 0) (xy 0.5 0.5)) (stroke (width 0) (type solid)) (fill yes) (layer "F.SilkS") (uuid "a5"))
        (fp_curve (pts (xy 0 0) (xy 0.1 0.2) (xy 0.3 0.2) (xy 0.4 0)) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "a6"))
        (fp_poly (pts (xy 3 3) (xy 4 3) (xy 4 4)) (stroke (width 0.1) (type solid)) (fill cross_hatch) (layer "Dwgs.User") (uuid "a7"))
        (fp_rect (start 5 5) (end 6 6) (stroke (width 0.1) (type solid)) (fill yes) (layers "F.Cu" "F.Mask") (solder_mask_margin 0.05) (uuid "a8"))
        (fp_rect (start -3 -2) (end 3 2) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd") (uuid "a9"))
        (fp_line (start -4 -3) (end 4 -3) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "b0"))
        (fp_text user "${REFERENCE}" (at 0 0.5 90) (layer "F.Fab") (uuid "b1") (effects (font (size 0.5 0.8) (thickness 0.075) bold italic) (justify left bottom)))
        (fp_text user "two\nlines" (at 1 2) (unlocked yes) (layer "B.SilkS" knockout) (uuid "b2") (effects (font (face "Arial") (size 1 1) (line_spacing 1.2)) (justify mirror)))
        (fp_text user "hidden" (at 0 0) (layer "F.SilkS") (hide yes) (uuid "b3") (effects (font (size 1 1))))
        (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste")))"#;

    #[test]
    fn every_shape_kind_is_read_with_its_layer_width_style_and_fill() {
        let art = read_art(&parse(FIXTURE), Side::Top, 0);
        // The courtyard rectangle and the Edge.Cuts line have homes of their own; the hidden text is a field in KiCad's eyes.
        assert_eq!(art.shapes.len(), 8, "{:#?}", art.shapes);
        assert_eq!(art.texts.len(), 2);

        let by = |i: usize| &art.shapes[i];
        assert!(matches!(&by(0).shape, Shape::Segment { start, end, stroke_width: 120, layer, .. } if *start == p(-2_000, -1_500) && *end == p(2_000, -1_500) && layer == "F.SilkS"));
        assert_eq!((by(1).style, matches!(by(1).shape, Shape::Arc { .. })), (LineStyle::Dash, true));
        assert!(matches!(&by(2).shape, Shape::Circle { filled: true, center, end, layer, .. } if *center == p(0, 0) && *end == p(500, 0) && layer == "F.Fab"));
        assert!(matches!(&by(3).shape, Shape::Rect { filled: false, .. }));
        assert_eq!((by(3).radius, by(3).style), (200, LineStyle::Unset), "a rectangle's corner radius and the file's own `default` stroke type");
        // A polygon of no width is allowed because it is filled.
        assert!(matches!(&by(4).shape, Shape::Polygon { filled: true, stroke_width: 0, pts, .. } if pts.len() == 3));
        assert!(matches!(&by(5).shape, Shape::Bezier { c1, c2, .. } if *c1 == p(100, 200) && *c2 == p(300, 200)));
        assert_eq!((by(6).hatch, by(6).shape.is_filled(), by(6).layer()), (Some(Hatch::CrossHatch), false, "Dwgs.User"));
        assert_eq!((by(7).layers.clone(), by(7).mask_margin), (vec!["F.Cu".to_string(), "F.Mask".to_string()], Some(50)));
    }

    #[test]
    fn a_text_keeps_its_effects() {
        let art = read_art(&parse(FIXTURE), Side::Top, 0);
        let t = &art.texts[0];
        assert_eq!(t.text, "${REFERENCE}");
        let l = &t.layout;
        assert_eq!((l.at, l.angle, l.layer.as_str()), (p(0, 500), 90_000, "F.Fab"));
        // `(size HEIGHT WIDTH)` is read as (width, height)
        assert_eq!((l.size, l.thickness, l.bold, l.italic, l.halign, l.valign, l.mirror, l.keep_upright), ((800, 500), 75, true, true, -1, 1, false, true));
        let u = &art.texts[1];
        assert_eq!(u.text, "two\nlines");
        assert_eq!((u.layout.layer.as_str(), u.layout.knockout, u.layout.keep_upright, u.layout.mirror), ("B.SilkS", true, false, true));
        assert_eq!((u.face.as_deref(), u.line_spacing_permille), (Some("Arial"), 1200));
    }

    #[test]
    fn what_is_written_reads_back_the_same_on_either_side() {
        for side in [Side::Top, Side::Bottom] {
            for rot in [0, 90_000, 45_000] {
                // Read from a top-side definition, placed on `side`, written, parsed as a footprint of that side, read again: the same definition.
                let art = read_art(&parse(FIXTURE), Side::Top, 0);
                let fp = inst(side, rot);
                let mut text = String::new();
                write_art(&mut text, &art, &fp);
                let again = read_art(&parse(&format!("(footprint \"x\" {text})")), side, rot);
                assert_eq!(again, art, "{side:?} {rot}\n{text}");
            }
        }
    }

    #[test]
    fn a_bottom_side_footprint_is_written_mirrored_on_the_other_layers() {
        let art = read_art(&parse(FIXTURE), Side::Top, 0);
        let mut text = String::new();
        write_art(&mut text, &art, &inst(Side::Bottom, 0));
        // x negated, the silkscreen on the back, the fab on the back
        assert!(text.contains("(fp_line (start 2 -1.5) (end -2 -1.5) (stroke (width 0.12) (type solid)) (layer \"B.SilkS\")"), "{text}");
        assert!(text.contains("(fp_circle (center 0 0) (end -0.5 0) (stroke (width 0.1) (type solid)) (fill yes) (layer \"B.Fab\")"), "{text}");
        // a user layer stays where it is
        assert!(text.contains("(fill cross_hatch) (layer \"Dwgs.User\")"), "{text}");
        // copper and mask go to the back together
        assert!(text.contains("(layers \"B.Cu\" \"B.Mask\") (solder_mask_margin 0.05)"), "{text}");
        // the text: the front fab text is now a back one, its angle turned over, mirrored
        assert!(text.contains("(fp_text user \"${REFERENCE}\" (at 0 0.5 270) (layer \"B.Fab\")"), "{text}");
        assert!(text.contains("(justify left bottom mirror)"), "{text}");
        // and the footprint's turn is the file's, negated, in its angles
        let mut turned = String::new();
        write_art(&mut turned, &art, &inst(Side::Top, 90_000));
        assert!(turned.contains("(fp_text user \"${REFERENCE}\" (at 0 0.5 0) (layer \"F.Fab\")"), "angle 90 on a part turned 90 clockwise on the screen reads 0 in the file: {turned}");
    }

    #[test]
    fn a_board_footprint_on_the_bottom_is_read_back_to_the_top_view() {
        // The file of a bottom-side footprint, as `write_art` writes it, is a top-side definition mirrored: reading it with the side gives the original.
        let source = parse(
            r#"(footprint "x" (layer "B.Cu")
                (fp_line (start 2 -1.5) (end -2 -1.5) (stroke (width 0.12) (type solid)) (layer "B.SilkS"))
                (fp_text user "T" (at -1 2 270) (layer "B.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify mirror))))"#,
        );
        let art = read_art(&source, Side::Bottom, 0);
        assert!(matches!(&art.shapes[0].shape, Shape::Segment { start, end, layer, .. } if *start == p(-2_000, -1_500) && *end == p(2_000, -1_500) && layer == "F.SilkS"));
        let t = &art.texts[0].layout;
        assert_eq!((t.at, t.layer.as_str(), t.mirror), (p(1_000, 2_000), "F.Fab", false));
        assert_eq!(t.angle, 90_000);
    }

    #[test]
    fn the_old_spellings_are_read() {
        let art = read_art(
            &parse(
                r#"(footprint "x" (layer "F.Cu")
                    (fp_arc (start 15 5) (end 20 5) (angle 90) (layer "F.SilkS") (width 0.15))
                    (fp_circle (center 0 0) (end 1 0) (layer "F.Fab") (width 0))
                    (fp_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (layer "F.SilkS") (width 0.1))
                    (fp_text user "%R %V" (at 0 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)))))"#,
            ),
            Side::Top,
            0,
        );
        let Shape::Arc { start, mid, end, stroke_width, .. } = &art.shapes[0].shape else { panic!("an arc") };
        assert_eq!((*start, *mid, *end, *stroke_width), (p(20_000, 5_000), p(18_536, 8_536), p(15_000, 10_000), 150));
        // a circle of no width and a polygon with no `fill` token are filled
        assert!(art.shapes[1].shape.is_filled() && art.shapes[2].shape.is_filled());
        assert_eq!(art.texts[0].text, "${REFERENCE} ${VALUE}");
    }

    #[test]
    fn a_polygon_with_an_arc_is_flattened_and_a_stroke_of_no_width_gets_the_default() {
        let art = read_art(
            &parse(
                r#"(footprint "x" (layer "F.Cu")
                    (fp_poly (pts (xy 0 0) (arc (start 0 0) (mid 1 1) (end 2 0)) (xy 0 2)) (stroke (width 0) (type solid)) (fill no) (layer "F.SilkS"))
                    (fp_line (start 0 0) (end 1 0) (stroke (width 0) (type solid)) (layer "F.SilkS")))"#,
            ),
            Side::Top,
            0,
        );
        let Shape::Polygon { pts, stroke_width, .. } = &art.shapes[0].shape else { panic!("a polygon") };
        assert!(pts.len() > 4, "the arc has become several points: {pts:?}");
        assert_eq!(pts.first(), Some(&p(0, 0)));
        assert_eq!(pts.last(), Some(&p(0, 2_000)));
        assert_eq!(*stroke_width, 100, "not filled, so the width is the default");
        assert_eq!(art.shapes[1].shape.stroke_width(), 100);
    }

    #[test]
    fn layers_are_written_the_way_format_layers_does() {
        let l = |v: &[&str]| layers_token(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(l(&["F.Cu", "F.Mask"]), "(layers \"F.Cu\" \"F.Mask\")");
        assert_eq!(l(&["B.Cu", "F.Cu", "F.Mask", "B.Mask"]), "(layers \"*.Cu\" \"*.Mask\")");
        assert_eq!(l(&["F.Fab", "B.Fab", "Dwgs.User"]), "(layers \"*.Fab\" \"Dwgs.User\")");
        assert_eq!(l(&["F.SilkS", "B.Paste"]), "(layers \"B.Paste\" \"F.SilkS\")");
    }
}
