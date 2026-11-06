//! Edit Pad as Graphic Shapes and Finish Pad Edit in the Footprint Editor (`pcbnew.PadTool.explodePad`, `pcbnew.PadTool.recombinePad`).
//!
//! Ports `PAD_TOOL::EditPad` and `PAD_TOOL::explodePad` (`pcbnew/tools/pad_tool.cpp`) and `PAD::Recombine` (`pcbnew/pad.cpp`) at KiCad 8303b2ad.
//! The tool is the Footprint Editor's alone (`if( !m_isFootprintEditor ) return 0;`): the board has no pad edit mode.
//!
//! * **Explode.** A custom pad's basic shapes become free graphics of the footprint, on the layer the pad is on (`F.Cu`, else `B.Cu`, else its first
//!   layer), each turned by the pad's orientation and moved to the pad's shape position (`Duplicate`, `Rotate`, `Move`, `SetLayer`); the pad is
//!   left as its anchor, a circle or a rectangle, with no shapes (`SetShape( GetAnchorPadShape )`, `DeletePrimitivesList`), and flagged `ENTERED`
//!   ([`PadEditMode`]). A pad that is not custom has nothing to explode and is only flagged. One undo step.
//! * **Recombine.** Every graphic on the pad's layer that touches the pad's copper (`BooleanIntersection` of the two polygons, the stroke counted),
//!   and every proxy item, is merged into the pad as a basic shape (hatched fills made solid, the shape moved and turned back into the pad's frame)
//!   and leaves the footprint; a pad that was not custom becomes one, its old outline a polygon round a new circular anchor (a circle or a
//!   rectangle is the anchor itself). A graphic on any layer of the pad that `EDA_SHAPE::Compare`s equal to a merged one (its points within 2 nm)
//!   goes too, and is not added twice. One undo step.
//!
//! The proxy items of a custom pad (the number box and the thermal spoke template) are not copper; this editor has no graphic that is a proxy, so
//! the explode sets them aside in the mode ([`PadEditMode::proxies`]) and the recombine gives them back.

use super::pcb_transform::{transform_shape, Xform};
use super::Board;
use eda_model::footprint::{CustomZoneMode, PadAnchor, PadCustom, PadPrimitive};
use eda_model::ir::{LibraryFootprint, LibraryPad, LibraryPadShape, PadEditMode, Point, Shape, Nm};
use eda_model::CheckResult;

/// `ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM` (`zones.h`): the width a thermal spoke template gets when the pad sets none.
const THERMAL_SPOKE_WIDTH_NM: Nm = Nm::new(500_000);

/// The error `PAD::Recombine` converts curves to polygons with (`BOARD_DESIGN_SETTINGS::m_MaxError`, 5 micrometres by default).
const MAX_ERROR_NM: Nm = Nm::new(5_000);

/// The layer the pad's shapes are on, and the one a recombine reads: `F.Cu` when the pad is on it, else `B.Cu`, else the first of its layers in
/// the order the layer lists show (`GetLayerSet().UIOrder().front()`: front copper, inner, back copper, then the technical layers).
pub fn pad_edit_layer(pad: &LibraryPad) -> String {
    let layers: Vec<String> = if pad.layers.is_empty() {
        match pad.kind {
            eda_model::PadKind::Smd => vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
            _ => vec!["F.Cu".into(), "B.Cu".into(), "F.Mask".into(), "B.Mask".into()],
        }
    } else {
        // The wildcards the pad's layer list may hold.
        pad.layers
            .iter()
            .flat_map(|l| match l.as_str() {
                "*.Cu" => vec!["F.Cu".to_string(), "B.Cu".to_string()],
                "F&B.Cu" => vec!["F.Cu".to_string(), "B.Cu".to_string()],
                w if w.starts_with("*.") => vec![format!("F.{}", &w[2..]), format!("B.{}", &w[2..])],
                other => vec![other.to_string()],
            })
            .collect()
    };
    if layers.iter().any(|l| l == "F.Cu") {
        return "F.Cu".into();
    }
    if layers.iter().any(|l| l == "B.Cu") {
        return "B.Cu".into();
    }
    layers.into_iter().min_by_key(|l| ui_order_rank(l)).unwrap_or_else(|| "F.Cu".into())
}

/// A layer's place in `LSET::UIOrder()`: the copper stack (front, inner layers by number, back), then `TechAndUserUIOrder()` (adhesive, paste, silkscreen,
/// mask, the user layers, the board edge, courtyard, fab) and the numbered user layers.
pub fn ui_order_rank(layer: &str) -> (u8, u32) {
    const TECH: [&str; 18] = [
        "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab",
    ];
    if layer == "F.Cu" {
        return (0, 0);
    }
    if layer == "B.Cu" {
        return (0, 10_000);
    }
    if let Some(n) = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|d| d.parse::<u32>().ok()) {
        return (0, n);
    }
    if let Some(i) = TECH.iter().position(|t| *t == layer) {
        return (1, i as u32);
    }
    if let Some(n) = layer.strip_prefix("User.").and_then(|d| d.parse::<u32>().ok()) {
        return (2, n);
    }
    (3, 0)
}

/// A point of the pad's own frame in the footprint's: turned by the pad's rotation about its position (`Rotate`), then moved by the pad's shape
/// position (`ShapePos`: the position plus the copper's offset turned with the pad), all of it as `Pad::rot` runs, clockwise.
struct PadFrame {
    pos: Point,
    rot: i64,
}

impl PadFrame {
    fn of(pad: &LibraryPad) -> PadFrame {
        let offset = rotate(pad.offset, pad.rot as i64);
        PadFrame { pos: Point { x: pad.at.x + offset.x, y: pad.at.y + offset.y }, rot: pad.rot as i64 }
    }

    /// `Rotate( 0, orientation )` then `Move( ShapePos )` on a shape of the pad's frame.
    fn out(&self, shape: &mut Shape, copper: usize) {
        transform_shape(shape, Xform::Rotate { pivot: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: self.rot }, copper);
        transform_shape(shape, Xform::Move { dx: self.pos.x, dy: self.pos.y }, copper);
    }

    /// `Move( - ShapePos )` then `Rotate( 0, - orientation )`: the way back into the pad's frame.
    fn back(&self, shape: &mut Shape, copper: usize) {
        transform_shape(shape, Xform::Move { dx: -self.pos.x, dy: -self.pos.y }, copper);
        transform_shape(shape, Xform::Rotate { pivot: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: -self.rot }, copper);
    }
}

fn rotate(p: Point, angle_mdeg: i64) -> Point {
    let (s, c) = (angle_mdeg as f64 / 1000.0).to_radians().sin_cos();
    Point { x: Nm::kiround(p.x.as_f64() * c - p.y.as_f64() * s), y: Nm::kiround(p.x.as_f64() * s + p.y.as_f64() * c) }
}

/// `EPSILON` of `EDA_SHAPE::Compare` (`common/eda_shape.cpp`): "Should be enough for rounding errors on calculated items", in nanometres.
const COMPARE_EPSILON: i64 = 2;

/// How a graphic of the footprint is filled, as `EDA_SHAPE::m_fill` (`FILL_T`) holds it: the shape's own `filled`, else its kept hatch
/// ([`eda_model::lib_keep::LibraryFootprintKeep::hatches`]), else `NO_FILL`.
fn fill_of(shape: &Shape, keep: &eda_model::lib_keep::LibraryFootprintKeep) -> (bool, Option<eda_model::fp_art::Hatch>) {
    if shape.is_filled() {
        (true, None)
    } else {
        (false, keep.hatches.get(shape.id()).copied())
    }
}

/// The line style of a graphic (`STROKE_PARAMS::GetLineStyle`): the kept `(stroke (type ..))` by id ([`eda_model::lib_keep::LibraryFootprintKeep::stroke_types`]), `solid`
/// when none is kept.
fn line_style_of<'k>(shape: &Shape, keep: &'k eda_model::lib_keep::LibraryFootprintKeep) -> &'k str {
    keep.stroke_types.get(shape.id()).map(String::as_str).unwrap_or("solid")
}

/// `EDA_SHAPE::Compare( other ) == 0` (`common/eda_shape.cpp`, KiCad 8303b2ad), as `PAD::Recombine`'s `findMatching` calls it on two graphics of the footprint:
/// the start and the end within [`COMPARE_EPSILON`] (`TEST_PT`); the same kind (`TEST( m_shape )`); a rectangle's corner radius equal (the editor's graphics have
/// none: 0 and 0); an arc's middle point (`GetArcMid`), a curve's two control points and the points it is drawn with (`m_bezierPoints`), within the epsilon; a
/// polygon's vertex count equal and each vertex within the epsilon; the stroke width within the epsilon; the line style and the fill (`FILL_T`, a hatch counted)
/// equal. The layer is not compared (`findMatching` tests it apart). A circle's `m_start` is its centre and `m_end` the point on it; a polygon's `m_start` and
/// `m_end` are unused (both zero). The curve's point lists are compared up to the shorter one after their lengths are found equal, as KiCad 25ec2335's guard
/// does (8303b2ad indexes the other list by this one's length).
fn shapes_compare_equal(a: &Shape, b: &Shape, keep: &eda_model::lib_keep::LibraryFootprintKeep) -> bool {
    let near = |p: Nm, q: Nm| (p.get() - q.get()).abs() <= COMPARE_EPSILON;
    let near_pt = |p: &Point, q: &Point| near(p.x, q.x) && near(p.y, q.y);
    let geometry = match (a, b) {
        (Shape::Segment { start: s1, end: e1, .. }, Shape::Segment { start: s2, end: e2, .. }) | (Shape::Rect { start: s1, end: e1, .. }, Shape::Rect { start: s2, end: e2, .. }) => near_pt(s1, s2) && near_pt(e1, e2),
        (Shape::Circle { center: c1, end: e1, .. }, Shape::Circle { center: c2, end: e2, .. }) => near_pt(c1, c2) && near_pt(e1, e2),
        (Shape::Arc { start: s1, mid: m1, end: e1, .. }, Shape::Arc { start: s2, mid: m2, end: e2, .. }) => near_pt(s1, s2) && near_pt(e1, e2) && near_pt(m1, m2),
        (Shape::Bezier { start: s1, c1: a1, c2: b1, end: e1, .. }, Shape::Bezier { start: s2, c1: a2, c2: b2, end: e2, .. }) => {
            let (p1, p2) = (a.bezier_points().unwrap_or_default(), b.bezier_points().unwrap_or_default());
            near_pt(s1, s2) && near_pt(e1, e2) && near_pt(a1, a2) && near_pt(b1, b2) && p1.len() == p2.len() && p1.iter().zip(&p2).all(|(p, q)| near_pt(p, q))
        }
        (Shape::Polygon { pts: p1, .. }, Shape::Polygon { pts: p2, .. }) => p1.len() == p2.len() && p1.iter().zip(p2).all(|(p, q)| near_pt(p, q)),
        _ => false,
    };
    geometry && near(a.stroke_width(), b.stroke_width()) && line_style_of(a, keep) == line_style_of(b, keep) && fill_of(a, keep) == fill_of(b, keep)
}

/// `PAD::GetLayerSet().test( aLayer )`: whether the pad is on the layer, its layer list read as KiCad's reader reads one (`m_layerMasks`: `*.Cu` every copper
/// layer, `*In.Cu` the inner ones, `F&B.Cu` the outer two, `*.Mask` / `*.Paste` / `*.SilkS` / `*.Adhes` / `*.Fab` / `*.CrtYd` the front and back of the kind). An
/// empty list is the one the library writer puts in (`F.Cu F.Paste F.Mask` for an SMD pad, `*.Cu *.Mask` for a hole).
pub fn pad_is_on_layer(pad: &LibraryPad, layer: &str) -> bool {
    let defaults: Vec<String>;
    let layers: &[String] = if pad.layers.is_empty() {
        defaults = match pad.kind {
            eda_model::PadKind::Smd => vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
            _ => vec!["*.Cu".into(), "*.Mask".into()],
        };
        &defaults
    } else {
        &pad.layers
    };
    let inner = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).is_some_and(|d| d.parse::<u32>().is_ok());
    layers.iter().any(|l| match l.as_str() {
        "*.Cu" => layer == "F.Cu" || layer == "B.Cu" || inner,
        "*In.Cu" => inner,
        "F&B.Cu" => layer == "F.Cu" || layer == "B.Cu",
        "*.Adhes" | "*.Paste" | "*.Mask" | "*.SilkS" | "*.Fab" | "*.CrtYd" => layer.len() > 2 && (layer.starts_with("F.") || layer.starts_with("B.")) && layer[2..] == l[2..],
        other => other == layer,
    })
}

impl Board<'_> {
    /// `PAD_TOOL::EditPad` with a pad selected and none being edited: explode it. One undo step.
    pub(crate) fn explode_pad(&mut self, footprint: &str, id: &str) -> Result<(), Vec<CheckResult>> {
        let copper = self.copper_layer_count();
        let fp = self.library_footprint_mut(footprint)?;
        if let Some(mode) = &fp.pad_edit {
            return Err(vec![CheckResult::fail("ops_pad_edit_mode", &mode.pad, "a pad is already being edited as graphic shapes; finish that edit first")]);
        }
        let index = fp.pads.iter().position(|p| p.id == id).ok_or_else(|| vec![CheckResult::fail("ops_unknown_pad", id, "no pad with this id")])?;
        let layer = pad_edit_layer(&fp.pads[index]);
        let frame = PadFrame::of(&fp.pads[index]);
        let spoke_width = fp.pads[index].thermal_spoke_width_override.unwrap_or(THERMAL_SPOKE_WIDTH_NM);

        let mut mode = PadEditMode { pad: id.to_string(), zone: CustomZoneMode::Outline, proxies: Vec::new() };
        let mut shapes: Vec<Shape> = Vec::new();
        // `if( aPad->GetShape( PADSTACK::ALL_LAYERS ) == PAD_SHAPE::CUSTOM )`
        if fp.pads[index].shape == LibraryPadShape::Custom {
            let custom = fp.pads[index].custom.clone().unwrap_or_default();
            mode.zone = custom.zone;
            for primitive in custom.primitives {
                let mut shape = primitive.shape;
                shape.set_id(String::new());
                // A proxy segment (a thermal spoke template) is drawn as wide as the pad's spoke.
                if primitive.proxy && matches!(shape, Shape::Segment { .. }) {
                    shape.set_stroke_width(spoke_width);
                }
                frame.out(&mut shape, copper);
                shape.set_layer(layer.clone());
                if primitive.proxy {
                    mode.proxies.push(PadPrimitive { shape, proxy: true });
                } else {
                    shapes.push(shape);
                }
            }
            // `SetShape( GetAnchorPadShape )`, `DeletePrimitivesList`
            let pad = &mut fp.pads[index];
            pad.shape = if custom.anchor == PadAnchor::Rect { LibraryPadShape::Rect } else { LibraryPadShape::Circle };
            pad.custom = None;
        }
        fp.graphics.extend(shapes);
        fp.pad_edit = Some(mode);
        fp.assign_missing_ids();
        Ok(())
    }

    /// `PAD_TOOL::EditPad` with a pad being edited: recombine it (`PAD::Recombine`), whatever the action that asked. One undo step.
    pub(crate) fn recombine_pad(&mut self, footprint: &str) -> Result<(), Vec<CheckResult>> {
        let copper = self.copper_layer_count();
        let fp = self.library_footprint_mut(footprint)?;
        let Some(mode) = fp.pad_edit.take() else {
            return Err(vec![CheckResult::fail("ops_pad_edit_mode", footprint, "no pad is being edited as graphic shapes")]);
        };
        // A pad deleted while it was being edited leaves the mode and nothing to merge into (`PAD_TOOL::Reset`).
        let Some(index) = fp.pads.iter().position(|p| p.id == mode.pad) else { return Ok(()) };
        recombine(fp, index, mode, copper);
        Ok(())
    }

    /// How many copper layers the board has, for the flip of a layer name; two when it does not say.
    fn copper_layer_count(&self) -> usize {
        self.model.board.layers.len().max(2)
    }
}

/// `PAD::Recombine( aIsDryRun = false )` of the pad `index` of `fp`, whose mode was `mode`.
fn recombine(fp: &mut LibraryFootprint, index: usize, mode: PadEditMode, copper: usize) {
    let layer = pad_edit_layer(&fp.pads[index]);
    let frame = PadFrame::of(&fp.pads[index]);

    // The pad's copper in the footprint's frame, for `findNext`'s test. `findNext` takes the pad's polygon as it is *now*
    // (`TransformShapeToPolygon( padPoly, ... )` on every call, and each merge's `AddPrimitive` marks the pad dirty), so a
    // graphic that touches the pad only through an earlier merged one is merged too: each merged graphic's polygon (the
    // primitive's own, in the footprint's frame) is added to `pad_polys`.
    let mut pad_polys = eda_drc::pad_shape::pad_polygons_in_footprint(&fp.pads[index].to_engine_pad(), MAX_ERROR_NM);
    // `PCB_SHAPE::TransformShapeToPolygon` passes `includeFill = false`, so a hatched graphic counts as solid (`IsHatchedFill() && !includeFill`).
    let polygons_of = |shape: &Shape, keep: &eda_model::lib_keep::LibraryFootprintKeep| {
        if keep.hatches.contains_key(shape.id()) {
            let mut solid = shape.clone();
            solid.set_filled(true);
            eda_drc::pad_shape::shape_polygons(&solid, MAX_ERROR_NM)
        } else {
            eda_drc::pad_shape::shape_polygons(shape, MAX_ERROR_NM)
        }
    };
    let touches = |shape: &Shape, pad_polys: &eda_shape_poly_set::ShapePolySet, keep: &eda_model::lib_keep::LibraryFootprintKeep| -> bool {
        let mut draw = polygons_of(shape, keep);
        draw.boolean_intersection(pad_polys);
        !draw.is_empty()
    };

    // `findNext`, until it finds nothing: the first graphic on the layer, scanning from the start of the list every time,
    // that touches the pad as it has grown; the matching ones follow it out. `found` holds what `findNext` returned (each
    // becomes a primitive), `taken` every graphic that leaves the footprint (`SKIP_STRUCT`, `mergedShapes`).
    let mut found: Vec<usize> = Vec::new();
    let mut taken = vec![false; fp.graphics.len()];
    while let Some(i) = (0..fp.graphics.len()).find(|&i| !taken[i] && fp.graphics[i].layer() == layer && touches(&fp.graphics[i], &pad_polys, &fp.kicad)) {
        taken[i] = true;
        found.push(i);
        pad_polys.boolean_add(&polygons_of(&fp.graphics[i], &fp.kicad));
        // `findMatching`: "KiCad won't produce these, but old footprints from other vendors have them" -- a graphic on any layer of the pad
        // (`GetLayerSet().test( other->GetLayer() )`) that `Compare`s equal to the one just merged, within its two nanometres, is deleted with it.
        for j in 0..fp.graphics.len() {
            if !taken[j] && pad_is_on_layer(&fp.pads[index], fp.graphics[j].layer()) && shapes_compare_equal(&fp.graphics[i], &fp.graphics[j], &fp.kicad) {
                taken[j] = true;
            }
        }
    }
    // The proxy items are on the layer too, and `findNext` returns one whether it touches the pad or not.
    let have_something = !found.is_empty() || !mode.proxies.is_empty();

    if have_something {
        // The primitives to add, in the pad's frame: what `findNext` returned (the matching ones are only deleted), then the proxies.
        let mut primitives: Vec<PadPrimitive> = Vec::new();
        for &i in &found {
            let mut shape = fp.graphics[i].clone();
            // "Convert any hatched fills to solid": `if( primitive->IsAnyFill() ) primitive->SetFillMode( FILL_T::FILLED_SHAPE )`.
            if fp.kicad.hatches.contains_key(shape.id()) {
                shape.set_filled(true);
            }
            shape.set_id(String::new());
            frame.back(&mut shape, copper);
            shape.set_layer(String::new());
            primitives.push(PadPrimitive { shape, proxy: false });
        }
        for proxy in &mode.proxies {
            let mut shape = proxy.shape.clone();
            frame.back(&mut shape, copper);
            shape.set_layer(String::new());
            primitives.push(PadPrimitive { shape, proxy: true });
        }

        let pad = &mut fp.pads[index];
        let mut custom = pad.custom.clone().unwrap_or_default();
        if pad.shape != LibraryPadShape::Custom {
            match pad.shape {
                // The pad is its own anchor.
                LibraryPadShape::Circle => custom.anchor = PadAnchor::Circle,
                LibraryPadShape::Rect => custom.anchor = PadAnchor::Rect,
                // A new circular anchor the size of the pad's narrower side, and the old outline a polygon primitive.
                _ => {
                    let outline = eda_drc::pad_shape::pad_polygons_in_footprint(&pad.to_engine_pad(), MAX_ERROR_NM);
                    let pts: Vec<Point> = if outline.outline_count() > 0 { outline.outline(0).iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect() } else { Vec::new() };
                    let min_extent = pad.size.0.min(pad.size.1);
                    custom.anchor = PadAnchor::Circle;
                    pad.size = (min_extent, min_extent);
                    if pts.len() >= 3 {
                        let mut poly = Shape::Polygon { id: String::new(), layer: String::new(), stroke_width: Nm::ZERO, filled: true, pts };
                        frame.back(&mut poly, copper);
                        poly.set_layer(String::new());
                        primitives.insert(0, PadPrimitive { shape: poly, proxy: false });
                    }
                }
            }
            pad.shape = LibraryPadShape::Custom;
            // The pad was not custom: nothing it had besides the size is a custom pad's.
            pad.roundrect_ratio = None;
            pad.trapezoid_delta = None;
            pad.chamfer_ratio = None;
            pad.chamfer_corners = Default::default();
        }
        custom.zone = mode.zone;
        custom.primitives.extend(primitives);
        pad.custom = Some(PadCustom { anchor: custom.anchor, zone: custom.zone, primitives: custom.primitives });

        // The merged graphics leave the footprint, and what was kept by their ids with them.
        let mut keep = Vec::with_capacity(fp.graphics.len());
        for (i, g) in std::mem::take(&mut fp.graphics).into_iter().enumerate() {
            if !taken[i] {
                keep.push(g);
            } else {
                fp.kicad.forget(g.id());
            }
        }
        fp.graphics = keep;
    }
}
