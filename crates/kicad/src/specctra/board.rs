//! The `BOARD` that `SPECCTRA_DB::FromBOARD` reads: the board file this studio derives (`export_kicad_pcb`) read the way `PCB_IO_KICAD_SEXPR_PARSER` reads it, down to the
//! integer arithmetic that puts a footprint's pads where the board has them. Only what the export looks at is kept: the copper layers, the nets, the footprints with their
//! pads, graphics and rule areas, the tracks and vias, and the zones.
//!
//! Coordinates are KiCad's: nanometres, `y` down. A footprint's children are in board coordinates once read (`SetFPRelativePosition`, `shape->Rotate( .., orientation )`,
//! `shape->Move( position )`), as in KiCad, and [`Fp::flip_to_top`] is `FOOTPRINT::Flip( position, TOP_BOTTOM )`, the turn over `FlipFOOTPRINTs` gives each footprint of the
//! back before the export.

use eda_model::units::Nm;
use super::geom::*;
use crate::sexpr::{self, Sexpr};
use eda_model::ir::{Point, Shape};

/// `KiROUND( mm * IU_PER_MM )`.
pub fn nm(mm: f64) -> i64 {
    kiround(mm * 1_000_000.0)
}

fn xy_of(list: &[Sexpr]) -> Option<V> {
    Some(V::new(nm(sexpr::num(list, 1)?), nm(sexpr::num(list, 2)?)))
}

fn point(list: &[Sexpr], name: &str) -> Option<V> {
    sexpr::find(list, name).and_then(xy_of)
}

/// A copper layer of the board, outermost first.
#[derive(Debug, Clone, PartialEq)]
pub struct CopperLayer {
    /// The name the board file writes (`F.Cu`, `In1.Cu`).
    pub name: String,
    /// `BOARD::GetLayerName`: the name the user gave it, else the standard one.
    pub shown: String,
    /// `signal`, `power`, `mixed` or `jumper`.
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadAttrib {
    Pth,
    Smd,
    Conn,
    Npth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadShapeKind {
    Circle,
    Rect,
    Oval,
    Trapezoid,
    RoundRect,
    ChamferedRect,
    Custom,
}

/// A `PAD`.
#[derive(Debug, Clone)]
pub struct Pad {
    pub number: String,
    pub attrib: PadAttrib,
    pub shape: PadShapeKind,
    /// `PAD::GetPosition`: the board's, not the footprint's.
    pub pos: V,
    /// `PAD::GetOrientation`: the board's, normalized to 0..360.
    pub orient: Angle,
    pub size: V,
    /// `PAD::GetDelta`.
    pub delta: V,
    /// `PAD::GetOffset`: of the copper shape, in the pad's own frame.
    pub offset: V,
    /// `PAD::GetDrillSize`.
    pub drill: V,
    pub rratio: f64,
    pub chamfer_ratio: f64,
    /// `RECT_CHAMFER_POSITIONS`.
    pub chamfer: i32,
    /// The layers the pad is on, wildcards expanded.
    pub layers: Vec<String>,
    pub net_name: String,
    pub net_code: i32,
    /// A custom pad's basic shapes, in the pad's frame.
    pub prims: Vec<Gfx>,
    /// A custom pad's anchor is a rectangle (`(options (anchor rect))`); a circle otherwise.
    pub anchor_rect: bool,
}

impl Pad {
    pub fn on_layer(&self, layer: &str) -> bool {
        self.layers.iter().any(|l| l == layer)
    }

    pub fn on_copper(&self) -> bool {
        self.layers.iter().any(|l| is_copper(l))
    }
}

/// What a footprint's graphic is, in the frame it is in.
#[derive(Debug, Clone, PartialEq)]
pub enum GfxShape {
    Segment(V, V),
    /// The centre and a point of the circle.
    Circle(V, V),
    Rect(V, V),
    /// Start, a point on the arc, end.
    Arc(V, V, V),
    Poly(Vec<V>),
    Bezier([V; 4]),
}

/// A `PCB_SHAPE`.
#[derive(Debug, Clone, PartialEq)]
pub struct Gfx {
    pub shape: GfxShape,
    pub layer: String,
    pub width: i64,
    pub filled: bool,
}

impl Gfx {
    /// The shape of the item in micrometre-free `ir` terms: the coordinates are nanometres (the unit does not matter to the code that chains them).
    pub fn to_ir(&self) -> Shape {
        let p = |v: &V| Point { x: Nm::from_raw_shim(v.x), y: Nm::from_raw_shim(v.y) };
        let (layer, stroke_width, filled) = (self.layer.clone(), self.width, self.filled);
        match &self.shape {
            GfxShape::Segment(a, b) => Shape::Segment { id: String::new(), layer, stroke_width: Nm::from_raw_shim(stroke_width), filled, start: p(a), end: p(b) },
            GfxShape::Circle(c, e) => Shape::Circle { id: String::new(), layer, stroke_width: Nm::from_raw_shim(stroke_width), filled, center: p(c), end: p(e) },
            GfxShape::Rect(a, b) => Shape::Rect { id: String::new(), layer, stroke_width: Nm::from_raw_shim(stroke_width), filled, start: p(a), end: p(b) },
            GfxShape::Arc(a, m, b) => Shape::Arc { id: String::new(), layer, stroke_width: Nm::from_raw_shim(stroke_width), filled, start: p(a), mid: p(m), end: p(b) },
            GfxShape::Poly(pts) => Shape::Polygon { id: String::new(), layer, stroke_width: Nm::from_raw_shim(stroke_width), filled, pts: pts.iter().map(p).collect() },
            GfxShape::Bezier([a, c1, c2, b]) => Shape::Bezier { id: String::new(), layer, stroke_width: Nm::from_raw_shim(stroke_width), filled, start: p(a), c1: p(c1), c2: p(c2), end: p(b) },
        }
    }

    fn map_points(&mut self, f: &dyn Fn(V) -> V) {
        match &mut self.shape {
            GfxShape::Segment(a, b) | GfxShape::Rect(a, b) | GfxShape::Circle(a, b) => {
                *a = f(*a);
                *b = f(*b);
            }
            GfxShape::Arc(a, m, b) => {
                *a = f(*a);
                *m = f(*m);
                *b = f(*b);
            }
            GfxShape::Poly(pts) => pts.iter_mut().for_each(|p| *p = f(*p)),
            GfxShape::Bezier(pts) => pts.iter_mut().for_each(|p| *p = f(*p)),
        }
    }

    /// `EDA_SHAPE::rotate`: the points turn about `centre`; a rectangle a non-cardinal turn leaves tilted is a polygon.
    pub fn rotate(&mut self, centre: V, angle: Angle) {
        if let GfxShape::Rect(a, b) = &self.shape {
            let a360 = angle.normalize().0;
            if !(a360 == 0.0 || a360 == 90.0 || a360 == 180.0 || a360 == 270.0) {
                let (a, b) = (*a, *b);
                let corners = vec![a, V::new(b.x, a.y), b, V::new(a.x, b.y)];
                self.shape = GfxShape::Poly(corners);
            }
        }
        self.map_points(&|p| rotate_point_about(p, centre, angle));
    }

    pub fn translate(&mut self, d: V) {
        self.map_points(&|p| p + d);
    }

    /// `EDA_SHAPE::flip( centre, TOP_BOTTOM )`: mirrored in the horizontal line through `ref_y`; an arc swaps its ends.
    pub fn flip_top_bottom(&mut self, ref_y: i64) {
        self.map_points(&|p| V::new(p.x, mirror_y(p.y, ref_y)));
        if let GfxShape::Arc(a, _, b) = &mut self.shape {
            std::mem::swap(a, b);
        }
    }
}

/// `::FlipLayer( layer, copperCount )`.
pub fn flip_layer(layer: &str, copper_layers: usize) -> String {
    const PAIRS: [(&str, &str); 7] = [("F.Cu", "B.Cu"), ("F.SilkS", "B.SilkS"), ("F.Adhes", "B.Adhes"), ("F.Mask", "B.Mask"), ("F.Paste", "B.Paste"), ("F.CrtYd", "B.CrtYd"), ("F.Fab", "B.Fab")];
    for (front, back) in PAIRS {
        if layer == front {
            return back.to_string();
        }
        if layer == back {
            return front.to_string();
        }
    }
    if copper_layers >= 4 {
        if let Some(n) = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|d| d.parse::<i64>().ok()) {
            let inner_index = n - 1;
            let max_index = copper_layers as i64 - 3;
            let flipped = (max_index - inner_index).clamp(0, max_index);
            return format!("In{}.Cu", flipped + 1);
        }
    }
    layer.to_string()
}

pub fn is_copper(layer: &str) -> bool {
    eda_model::layers::is_copper_layer_name(layer)
}

/// `FOOTPRINT`'s `Zones()` entry and a board's: a zone or a rule area.
#[derive(Debug, Clone)]
pub struct ZoneItem {
    /// The outline first, then its holes (`ZONE::AddPolygon`).
    pub polys: Vec<Vec<V>>,
    /// `ZONE::GetLayerSet()`, in `PCB_LAYER_ID` order.
    pub layers: Vec<String>,
    pub net_name: String,
    pub rule_area: bool,
    pub no_vias: bool,
    pub no_tracks: bool,
}

impl ZoneItem {
    /// `ZONE::IsOnCopperLayer`.
    pub fn on_copper(&self) -> bool {
        self.layers.iter().any(|l| is_copper(l))
    }
}

/// A `FOOTPRINT`.
#[derive(Debug, Clone)]
pub struct Fp {
    /// `GetFPID().Format()`.
    pub fpid: String,
    pub reference: String,
    pub value: String,
    pub back: bool,
    pub pos: V,
    /// `FOOTPRINT::GetOrientation`, normalized to (-180, 180].
    pub orient: Angle,
    pub pads: Vec<Pad>,
    pub gfx: Vec<Gfx>,
    pub zones: Vec<ZoneItem>,
    /// `FOOTPRINT::GetFlag`: set by `FlipFOOTPRINTs` on the footprints it turned over.
    pub flag: bool,
}

impl Fp {
    /// `PAD::GetFPRelativePosition`.
    pub fn relative_position(&self, abs: V) -> V {
        rotate_point(abs - self.pos, -self.orient)
    }

    /// `FOOTPRINT::Flip( position, TOP_BOTTOM )`, for a board with `copper` copper layers.
    pub fn flip_to_top(&mut self, copper: usize) {
        let ref_y = self.pos.y;
        let new_orientation = (-self.orient).normalize180();
        self.back = !self.back;
        // Pads are mirrored with the footprint's orientation taken as zero: a pad's own orientation is then the one relative to the footprint.
        for pad in &mut self.pads {
            pad.pos = V::new(pad.pos.x, mirror_y(pad.pos.y, ref_y));
            pad.offset = V::new(pad.offset.x, -pad.offset.y);
            pad.delta = V::new(pad.delta.x, -pad.delta.y);
            pad.orient = (-pad.orient).normalize();
            // `mirrorBitFlags` for a flip about the X axis: top and bottom corners trade places.
            let c = pad.chamfer;
            let mut out = c & !(RECT_CHAMFER_TOP_LEFT | RECT_CHAMFER_BOTTOM_LEFT | RECT_CHAMFER_TOP_RIGHT | RECT_CHAMFER_BOTTOM_RIGHT);
            if c & RECT_CHAMFER_TOP_LEFT != 0 {
                out |= RECT_CHAMFER_BOTTOM_LEFT;
            }
            if c & RECT_CHAMFER_BOTTOM_LEFT != 0 {
                out |= RECT_CHAMFER_TOP_LEFT;
            }
            if c & RECT_CHAMFER_TOP_RIGHT != 0 {
                out |= RECT_CHAMFER_BOTTOM_RIGHT;
            }
            if c & RECT_CHAMFER_BOTTOM_RIGHT != 0 {
                out |= RECT_CHAMFER_TOP_RIGHT;
            }
            pad.chamfer = out;
            pad.layers = pad.layers.iter().map(|l| flip_layer(l, copper)).collect();
            for prim in &mut pad.prims {
                prim.flip_top_bottom(0);
            }
        }
        // The new orientation is set once the pads are done with.
        self.orient = new_orientation;
        for zone in &mut self.zones {
            for poly in &mut zone.polys {
                for p in poly.iter_mut() {
                    p.y = mirror_y(p.y, ref_y);
                }
            }
            let flipped: Vec<String> = zone.layers.iter().map(|l| flip_layer(l, copper)).collect();
            zone.layers = flipped;
            zone.layers.sort_by_key(|l| eda_model::layers::pcb_layer_id(l).unwrap_or(u32::MAX));
        }
        for g in &mut self.gfx {
            g.flip_top_bottom(ref_y);
            g.layer = flip_layer(&g.layer, copper);
        }
    }
}

/// A `PCB_TRACK`, `PCB_ARC` or `PCB_VIA`.
#[derive(Debug, Clone)]
pub enum TrackItem {
    Segment { start: V, end: V, width: i64, layer: String, net_code: i32, locked: bool },
    Arc { start: V, end: V, width: i64, layer: String, net_code: i32, locked: bool },
    Via { pos: V, size: i64, drill: i64, top: String, bottom: String, net_code: i32, locked: bool },
}

/// The `BOARD`.
#[derive(Debug, Clone, Default)]
pub struct BoardView {
    pub copper: Vec<CopperLayer>,
    /// `(net N "name")`, code 0 (no net) included.
    pub nets: Vec<(i32, String)>,
    pub footprints: Vec<Fp>,
    pub tracks: Vec<TrackItem>,
    pub zones: Vec<ZoneItem>,
}

impl BoardView {
    /// `BOARD::GetCopperLayerCount`.
    pub fn copper_count(&self) -> usize {
        self.copper.len()
    }

    /// `BOARD::GetLayerName` of a copper layer.
    pub fn layer_name(&self, kicad_layer: &str) -> String {
        self.copper.iter().find(|l| l.name == kicad_layer).map(|l| l.shown.clone()).unwrap_or_else(|| kicad_layer.to_string())
    }

    pub fn net_name(&self, code: i32) -> &str {
        self.nets.iter().find(|(c, _)| *c == code).map(|(_, n)| n.as_str()).unwrap_or("")
    }

    pub fn net_code(&self, name: &str) -> i32 {
        self.nets.iter().find(|(_, n)| n == name).map(|(c, _)| *c).unwrap_or(0)
    }

    /// Read the text of a `.kicad_pcb`.
    pub fn parse(text: &str) -> Result<BoardView, String> {
        let tree = sexpr::parse(text)?;
        let root = tree.as_list().ok_or("not a board file")?;
        let mut view = BoardView::default();

        // The copper layers: `(layers (0 "F.Cu" signal) (4 "In1.Cu" power "Power") (2 "B.Cu" signal))`.
        if let Some(layers) = sexpr::find(root, "layers") {
            for entry in layers.iter().skip(1).filter_map(Sexpr::as_list) {
                let Some(name) = sexpr::txt(entry, 1) else { continue };
                if !is_copper(name) {
                    continue;
                }
                let kind = sexpr::txt(entry, 2).unwrap_or("signal").to_string();
                let shown = sexpr::txt(entry, 3).filter(|s| !s.is_empty()).unwrap_or(name).to_string();
                view.copper.push(CopperLayer { name: name.to_string(), shown, kind });
            }
        }
        if view.copper.is_empty() {
            view.copper = vec![
                CopperLayer { name: "F.Cu".into(), shown: "F.Cu".into(), kind: "signal".into() },
                CopperLayer { name: "B.Cu".into(), shown: "B.Cu".into(), kind: "signal".into() },
            ];
        }
        let copper_names: Vec<String> = view.copper.iter().map(|l| l.name.clone()).collect();

        for net in sexpr::find_all(root, "net") {
            if let (Some(code), Some(name)) = (sexpr::num(net, 1), sexpr::txt(net, 2)) {
                view.nets.push((code as i32, name.to_string()));
            }
        }
        let code_of = |view: &BoardView, name: &str| view.net_code(name);

        for fp in sexpr::find_all(root, "footprint").chain(sexpr::find_all(root, "module")) {
            let parsed = parse_footprint(fp, &copper_names, &view);
            view.footprints.push(parsed);
        }

        for item in root.iter().filter_map(Sexpr::as_list) {
            match sexpr::tag(item) {
                Some(kind @ ("segment" | "arc")) => {
                    let (Some(start), Some(end)) = (point(item, "start"), point(item, "end")) else { continue };
                    let width = sexpr::find(item, "width").and_then(|w| sexpr::num(w, 1)).map(nm).unwrap_or(0);
                    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
                    let net_code = sexpr::find(item, "net").and_then(|n| sexpr::num(n, 1)).map(|c| c as i32).unwrap_or(0);
                    let locked = is_locked(item);
                    if kind == "segment" {
                        view.tracks.push(TrackItem::Segment { start, end, width, layer, net_code, locked });
                    } else {
                        view.tracks.push(TrackItem::Arc { start, end, width, layer, net_code, locked });
                    }
                }
                Some("via") => {
                    let Some(pos) = point(item, "at") else { continue };
                    let size = sexpr::find(item, "size").and_then(|w| sexpr::num(w, 1)).map(nm).unwrap_or(0);
                    let drill = sexpr::find(item, "drill").and_then(|w| sexpr::num(w, 1)).map(nm).unwrap_or(0);
                    let layers = sexpr::find(item, "layers");
                    let top = layers.and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
                    let bottom = layers.and_then(|l| sexpr::txt(l, 2)).unwrap_or("B.Cu").to_string();
                    let net_code = sexpr::find(item, "net").and_then(|n| sexpr::num(n, 1)).map(|c| c as i32).unwrap_or(0);
                    view.tracks.push(TrackItem::Via { pos, size, drill, top, bottom, net_code, locked: is_locked(item) });
                }
                Some("zone") => {
                    if let Some(z) = parse_zone(item, &copper_names, &view) {
                        view.zones.push(z);
                    }
                }
                _ => {}
            }
        }
        let _ = code_of;
        Ok(view)
    }

    /// `SPECCTRA_DB::FlipFOOTPRINTs`: the footprints of the back turned over to the front; their flag says which.
    pub fn flip_footprints_to_top(&mut self) {
        let copper = self.copper_count();
        for fp in &mut self.footprints {
            fp.flag = false;
            if fp.back {
                fp.flip_to_top(copper);
                fp.flag = true;
            }
        }
    }
}

fn is_locked(item: &[Sexpr]) -> bool {
    sexpr::find(item, "locked").is_some_and(|l| sexpr::txt(l, 1).map_or(true, |t| t == "yes")) || item.iter().any(|c| c.text() == Some("locked"))
}

/// A `(layers ..)` / `(layer ..)` list with wildcards expanded against the board's copper layers (`parseBoardItemLayersAsMask`).
fn expand_layers(names: &[&str], copper: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        match *n {
            "*.Cu" => out.extend(copper.iter().cloned()),
            "F&B.Cu" => out.extend(["F.Cu".to_string(), "B.Cu".to_string()]),
            w if w.starts_with("*.") => {
                out.push(format!("F.{}", &w[2..]));
                out.push(format!("B.{}", &w[2..]));
            }
            other => out.push(other.to_string()),
        }
    }
    out.sort_by_key(|l| (eda_model::layers::pcb_layer_id(l).unwrap_or(u32::MAX), l.clone()));
    out.dedup();
    out
}

fn parse_zone(z: &[Sexpr], copper: &[String], view: &BoardView) -> Option<ZoneItem> {
    let mut names: Vec<&str> = Vec::new();
    if let Some(l) = sexpr::find(z, "layer") {
        names.extend(sexpr::txt(l, 1));
    }
    if let Some(l) = sexpr::find(z, "layers") {
        names.extend((1..l.len()).filter_map(|i| sexpr::txt(l, i)));
    }
    let layers = expand_layers(&names, copper);
    let net_name = match sexpr::find(z, "net_name").and_then(|n| sexpr::txt(n, 1)) {
        Some(n) => n.to_string(),
        None => sexpr::find(z, "net").and_then(|n| sexpr::num(n, 1)).map(|c| view.net_name(c as i32).to_string()).unwrap_or_default(),
    };
    let keepout = sexpr::find(z, "keepout");
    let rule_area = keepout.is_some();
    let not_allowed = |what: &str| keepout.and_then(|k| sexpr::find(k, what)).and_then(|t| sexpr::txt(t, 1)) == Some("not_allowed");
    let mut polys: Vec<Vec<V>> = Vec::new();
    for polygon in sexpr::find_all(z, "polygon") {
        let Some(pts) = sexpr::find(polygon, "pts") else { continue };
        let ring: Vec<V> = pts.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| sexpr::tag(l) == Some("xy")).filter_map(xy_of).collect();
        polys.push(ring);
    }
    if polys.is_empty() {
        return None;
    }
    Some(ZoneItem { polys, layers, net_name, rule_area, no_vias: not_allowed("vias"), no_tracks: not_allowed("tracks") })
}

fn parse_footprint(fp: &[Sexpr], copper: &[String], view: &BoardView) -> Fp {
    let back = sexpr::find(fp, "layer").and_then(|l| sexpr::txt(l, 1)) == Some("B.Cu");
    let at = sexpr::find(fp, "at");
    let pos = at.and_then(xy_of).unwrap_or_default();
    // `footprint->SetOrientation( angle )`: normalized to (-180, 180].
    let orient = Angle(at.and_then(|a| sexpr::num(a, 3)).unwrap_or(0.0)).normalize180();
    let mut out = Fp { fpid: sexpr::txt(fp, 1).unwrap_or("").to_string(), reference: String::new(), value: String::new(), back, pos, orient, pads: Vec::new(), gfx: Vec::new(), zones: Vec::new(), flag: false };

    for child in fp.iter().skip(2).filter_map(Sexpr::as_list) {
        match sexpr::tag(child) {
            Some("property") => match sexpr::txt(child, 1) {
                Some("Reference") => out.reference = sexpr::txt(child, 2).unwrap_or("").to_string(),
                Some("Value") => out.value = sexpr::txt(child, 2).unwrap_or("").to_string(),
                _ => {}
            },
            Some("fp_text") => match sexpr::txt(child, 1) {
                Some("reference") => out.reference = sexpr::txt(child, 2).unwrap_or("").to_string(),
                Some("value") => out.value = sexpr::txt(child, 2).unwrap_or("").to_string(),
                _ => {}
            },
            Some("pad") => {
                if let Some(p) = parse_pad(child, &out, copper, view) {
                    out.pads.push(p);
                }
            }
            Some(tag @ ("fp_line" | "fp_arc" | "fp_circle" | "fp_rect" | "fp_poly" | "fp_curve")) => {
                if let Some(mut g) = gfx_nm(child, &tag[3..]) {
                    // `shape->Rotate( VECTOR2I( 0, 0 ), footprint->GetOrientation() ); shape->Move( footprint->GetPosition() )`.
                    g.rotate(V::new(0, 0), out.orient);
                    g.translate(out.pos);
                    out.gfx.push(g);
                }
            }
            Some("zone") => {
                if let Some(z) = parse_zone(child, copper, view) {
                    out.zones.push(z);
                }
            }
            _ => {}
        }
    }
    out
}

/// A `PCB_SHAPE` of the file (`fp_line`, `gr_arc`, ..: `kind` is the name after the prefix) in nanometres, `parsePCB_SHAPE` reading every coordinate with
/// `parseBoardUnits` -- a library footprint's graphics are often at fractions of a micrometre. The points of a polygon are its `(xy ..)`; a repeated one is dropped.
fn gfx_nm(item: &[Sexpr], kind: &str) -> Option<Gfx> {
    let pt = |name: &str| point(item, name);
    let width = sexpr::find(item, "stroke").and_then(|s| sexpr::find(s, "width")).or_else(|| sexpr::find(item, "width")).and_then(|w| sexpr::num(w, 1)).map(nm).unwrap_or(0);
    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("Cmts.User").to_string();
    let filled = sexpr::find(item, "fill").and_then(|f| sexpr::txt(f, 1)).is_some_and(|t| t == "yes" || t == "solid");
    let poly = || -> Vec<V> {
        let mut out: Vec<V> = Vec::new();
        if let Some(pts) = sexpr::find(item, "pts") {
            for p in pts.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| sexpr::tag(l) == Some("xy")).filter_map(xy_of) {
                if out.last() != Some(&p) {
                    out.push(p);
                }
            }
        }
        out
    };
    let shape = match kind {
        "line" => GfxShape::Segment(pt("start")?, pt("end")?),
        "arc" => GfxShape::Arc(pt("start")?, pt("mid")?, pt("end")?),
        "rect" => GfxShape::Rect(pt("start")?, pt("end")?),
        "circle" => GfxShape::Circle(pt("center")?, pt("end")?),
        "poly" => GfxShape::Poly(poly()),
        "curve" => {
            let pts = poly();
            let [a, c1, c2, b] = pts[..] else { return None };
            GfxShape::Bezier([a, c1, c2, b])
        }
        _ => return None,
    };
    Some(Gfx { shape, layer, width, filled: filled && kind != "curve" })
}

fn parse_pad(pad: &[Sexpr], parent: &Fp, copper: &[String], view: &BoardView) -> Option<Pad> {
    let number = sexpr::txt(pad, 1)?.to_string();
    let attrib = match sexpr::txt(pad, 2)? {
        "thru_hole" => PadAttrib::Pth,
        "smd" => PadAttrib::Smd,
        "connect" => PadAttrib::Conn,
        "np_thru_hole" => PadAttrib::Npth,
        _ => return None,
    };
    let shape_word = sexpr::txt(pad, 3).unwrap_or("rect");
    let mut shape = match shape_word {
        "circle" => PadShapeKind::Circle,
        "oval" => PadShapeKind::Oval,
        "trapezoid" => PadShapeKind::Trapezoid,
        "roundrect" => PadShapeKind::RoundRect,
        "custom" => PadShapeKind::Custom,
        _ => PadShapeKind::Rect,
    };
    let rel = sexpr::find(pad, "at").and_then(xy_of).unwrap_or_default();
    // `SetFPRelativePosition`: turned by the footprint's orientation and moved to its position.
    let pos = rotate_point(rel, parent.orient) + parent.pos;
    // The angle in the file is the pad's own on the board, and none is zero.
    let orient = Angle(sexpr::find(pad, "at").and_then(|a| sexpr::num(a, 3)).unwrap_or(0.0)).normalize();
    let size = sexpr::find(pad, "size").and_then(xy_of).unwrap_or_default();
    let delta = sexpr::find(pad, "rect_delta").and_then(xy_of).unwrap_or_default();
    let drill_node = sexpr::find(pad, "drill");
    let mut drill = V::default();
    let mut offset = V::default();
    if let Some(d) = drill_node {
        let values: Vec<f64> = d.iter().skip(1).filter_map(|c| c.text().and_then(|t| t.parse::<f64>().ok())).collect();
        match values.as_slice() {
            [w] => drill = V::new(nm(*w), nm(*w)),
            [w, h, ..] => drill = V::new(nm(*w), nm(*h)),
            [] => {}
        }
        if let Some(o) = sexpr::find(d, "offset").and_then(xy_of) {
            offset = o;
        }
    } else if matches!(attrib, PadAttrib::Pth) {
        // A thru-hole pad with no `drill` token has a drill of 1 nm.
        drill = V::new(1, 1);
    }
    let rratio = sexpr::find(pad, "roundrect_rratio").and_then(|r| sexpr::num(r, 1)).unwrap_or(0.25);
    let chamfer_ratio = sexpr::find(pad, "chamfer_ratio").and_then(|r| sexpr::num(r, 1)).unwrap_or(0.0);
    let mut chamfer = 0;
    if let Some(c) = sexpr::find(pad, "chamfer") {
        for tok in c.iter().skip(1).filter_map(Sexpr::text) {
            chamfer |= match tok {
                "top_left" => RECT_CHAMFER_TOP_LEFT,
                "top_right" => RECT_CHAMFER_TOP_RIGHT,
                "bottom_left" => RECT_CHAMFER_BOTTOM_LEFT,
                "bottom_right" => RECT_CHAMFER_BOTTOM_RIGHT,
                _ => 0,
            };
        }
    }
    if chamfer != 0 || chamfer_ratio > 0.0 {
        if shape == PadShapeKind::RoundRect {
            shape = PadShapeKind::ChamferedRect;
        }
    }
    let names: Vec<&str> = sexpr::find(pad, "layers").map(|l| l.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
    let layers = expand_layers(&names, copper);
    let net_name = sexpr::find(pad, "net").and_then(|n| sexpr::txt(n, 2)).unwrap_or("").to_string();
    let net_code = if net_name.is_empty() { 0 } else { view.net_code(&net_name) };
    let mut prims = Vec::new();
    let anchor_rect = sexpr::find(pad, "options").and_then(|o| sexpr::find(o, "anchor")).and_then(|a| sexpr::txt(a, 1)) == Some("rect");
    if shape == PadShapeKind::Custom {
        if let Some(p) = sexpr::find(pad, "primitives") {
            for child in p.iter().skip(1).filter_map(Sexpr::as_list) {
                let Some(tag) = sexpr::tag(child) else { continue };
                if !matches!(tag, "gr_line" | "gr_arc" | "gr_circle" | "gr_rect" | "gr_poly" | "gr_curve") {
                    continue;
                }
                if let Some(g) = gfx_nm(child, &tag[3..]) {
                    prims.push(g);
                }
            }
        }
    }
    Some(Pad { number, attrib, shape, pos, orient, size, delta, offset, drill, rratio, chamfer_ratio, chamfer, layers, net_name, net_code, prims, anchor_rect })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(extra: &str) -> String {
        format!(
            r#"(kicad_pcb (version 20241229)
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user))
  (net 0 "") (net 1 "GND") (net 2 "VCC")
  {extra}
)"#
        )
    }

    #[test]
    fn a_footprint_is_read_with_its_pads_at_the_board() {
        let b = BoardView::parse(&board(
            r#"(footprint "lib:R" (layer "F.Cu") (at 10 20 90)
                 (property "Reference" "R1") (property "Value" "10k")
                 (pad "1" smd rect (at -1 0 90) (size 1 0.5) (layers "F.Cu" "F.Mask") (net 1 "GND"))
                 (fp_line (start -1 -1) (end 1 -1) (layer "F.SilkS") (stroke (width 0.1) (type default))))"#,
        ))
        .unwrap();
        let fp = &b.footprints[0];
        assert_eq!((fp.reference.as_str(), fp.value.as_str(), fp.fpid.as_str()), ("R1", "10k", "lib:R"));
        assert_eq!(fp.orient, Angle(90.0));
        // (-1, 0) turned by 90 degrees (RotatePoint: (y, -x)) and moved to (10, 20).
        assert_eq!(fp.pads[0].pos, V::new(10_000_000, 20_000_000 + 1_000_000));
        assert_eq!(fp.pads[0].net_code, 1);
        assert_eq!(fp.pads[0].layers, vec!["F.Cu".to_string(), "F.Mask".to_string()]);
        // (-1, -1) and (1, -1) turned by 90 degrees are (-1, 1) and (-1, -1), then moved.
        assert_eq!(fp.gfx[0].shape, GfxShape::Segment(V::new(9_000_000, 21_000_000), V::new(9_000_000, 19_000_000)));
    }

    /// A footprint's graphics and a custom pad's basic shapes are read to the nanometre, as `parseBoardUnits` reads them: a library footprint's courtyard at
    /// -1.100577 mm stays there.
    #[test]
    fn graphics_are_read_to_the_nanometre() {
        let b = BoardView::parse(&board(
            r#"(footprint "lib:R" (layer "F.Cu") (at 10 20)
                 (property "Reference" "R1") (property "Value" "10k")
                 (pad "1" smd custom (at 0 0) (size 0.3 0.3) (layers "F.Cu") (options (anchor rect))
                   (primitives (gr_poly (pts (xy -0.500001 -0.75) (xy 0.5 -0.750003) (xy 1 0)) (width 0.012345) (fill yes))))
                 (fp_line (start -1.100577 -0.899717) (end 1.099423 -0.899717) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
                 (fp_arc (start -1 -1) (mid 0 -1.414214) (end 1 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS")))"#,
        ))
        .unwrap();
        let fp = &b.footprints[0];
        assert_eq!(fp.gfx[0].shape, GfxShape::Segment(V::new(10_000_000 - 1_100_577, 20_000_000 - 899_717), V::new(10_000_000 + 1_099_423, 20_000_000 - 899_717)));
        assert_eq!(fp.gfx[0].width, 50_000);
        assert_eq!(fp.gfx[1].shape, GfxShape::Arc(V::new(9_000_000, 19_000_000), V::new(10_000_000, 20_000_000 - 1_414_214), V::new(11_000_000, 19_000_000)));
        let prim = &fp.pads[0].prims[0];
        assert_eq!(prim.shape, GfxShape::Poly(vec![V::new(-500_001, -750_000), V::new(500_000, -750_003), V::new(1_000_000, 0)]));
        assert_eq!((prim.width, prim.filled), (12_345, true));
    }

    #[test]
    fn flipping_a_back_footprint_to_the_top_negates_its_orientation_and_mirrors_its_pads() {
        let mut b = BoardView::parse(&board(
            r#"(footprint "lib:R" (layer "B.Cu") (at 10 20 30)
                 (property "Reference" "R1") (property "Value" "10k")
                 (pad "1" smd rect (at -1 0 30) (size 1 0.5) (layers "B.Cu" "B.Mask")))"#,
        ))
        .unwrap();
        let before = b.footprints[0].pads[0].pos;
        b.flip_footprints_to_top();
        let fp = &b.footprints[0];
        assert!(fp.flag && !fp.back);
        assert_eq!(fp.orient, Angle(-30.0));
        assert_eq!(fp.pads[0].pos, V::new(before.x, mirror_y(before.y, 20_000_000)));
        assert_eq!(fp.pads[0].layers, vec!["F.Cu".to_string(), "F.Mask".to_string()]);
        assert_eq!(fp.pads[0].orient, Angle(330.0));
    }

    #[test]
    fn tracks_vias_and_zones_are_read_in_file_order() {
        let b = BoardView::parse(&board(
            r#"(segment (start 0 0) (end 5 0) (width 0.2) (layer "F.Cu") (net 1) (uuid "a"))
               (arc (start 5 0) (mid 6 1) (end 7 0) (width 0.2) (layer "F.Cu") (net 1) (locked yes) (uuid "b"))
               (via (at 7 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "c"))
               (zone (net 2) (net_name "VCC") (layer "F.Cu") (uuid "z") (polygon (pts (xy 0 0) (xy 10 0) (xy 10 10))))
               (zone (net 0) (net_name "") (layers "F.Cu" "B.Cu") (keepout (tracks not_allowed) (vias allowed) (pads allowed) (copperpour allowed) (footprints allowed)) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))"#,
        ))
        .unwrap();
        assert_eq!(b.tracks.len(), 3);
        assert!(matches!(&b.tracks[1], TrackItem::Arc { locked: true, .. }));
        assert!(matches!(&b.tracks[2], TrackItem::Via { size: 600_000, drill: 300_000, .. }));
        assert_eq!(b.zones.len(), 2);
        assert_eq!(b.zones[0].net_name, "VCC");
        assert!(b.zones[1].rule_area && b.zones[1].no_tracks && !b.zones[1].no_vias);
        assert_eq!(b.zones[1].layers, vec!["F.Cu".to_string(), "B.Cu".to_string()]);
    }
}
