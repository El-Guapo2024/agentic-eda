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
//!   rectangle is the anchor itself). Items identical to a merged one (`Compare() == 0`) go too. One undo step.
//!
//! The proxy items of a custom pad (the number box and the thermal spoke template) are not copper; this editor has no graphic that is a proxy, so
//! the explode sets them aside in the mode ([`PadEditMode::proxies`]) and the recombine gives them back.

use super::pcb_transform::{transform_shape, Xform};
use super::Board;
use eda_model::footprint::{CustomZoneMode, PadAnchor, PadCustom, PadPrimitive};
use eda_model::ir::{LibraryFootprint, LibraryPad, LibraryPadShape, PadEditMode, Point, Shape, Nm};
use eda_model::CheckResult;

/// `ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM` (`zones.h`): the width a thermal spoke template gets when the pad sets none.
const THERMAL_SPOKE_WIDTH_NM: Nm = Nm::from_raw_shim(500_000);

/// The error `PAD::Recombine` converts curves to polygons with (`BOARD_DESIGN_SETTINGS::m_MaxError`, 5 micrometres by default).
const MAX_ERROR_NM: Nm = Nm::from_raw_shim(5_000);

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
    Point { x: Nm::from_raw_shim((p.x.raw_shim() as f64 * c - p.y.raw_shim() as f64 * s).round() as i64), y: Nm::from_raw_shim((p.x.raw_shim() as f64 * s + p.y.raw_shim() as f64 * c).round() as i64) }
}

/// `PCB_SHAPE::Compare( other ) == 0` for the shapes the editor has: the same kind on the same layer with the same geometry, stroke width and fill.
fn same_shape(a: &Shape, b: &Shape) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    a.set_id(String::new());
    b.set_id(String::new());
    a == b
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
    let touches = |shape: &Shape, pad_polys: &eda_shape_poly_set::ShapePolySet| -> bool {
        let mut draw = eda_drc::pad_shape::shape_polygons(shape, MAX_ERROR_NM);
        draw.boolean_intersection(pad_polys);
        !draw.is_empty()
    };

    // `findNext`, until it finds nothing: the first graphic on the layer, scanning from the start of the list every time,
    // that touches the pad as it has grown; the matching ones follow it out.
    let mut merged: Vec<usize> = Vec::new();
    let mut taken = vec![false; fp.graphics.len()];
    while let Some(i) = (0..fp.graphics.len()).find(|&i| !taken[i] && fp.graphics[i].layer() == layer && touches(&fp.graphics[i], &pad_polys)) {
        taken[i] = true;
        merged.push(i);
        pad_polys.boolean_add(&eda_drc::pad_shape::shape_polygons(&fp.graphics[i], MAX_ERROR_NM));
        // `findMatching`: "KiCad won't produce these, but old footprints from other vendors have them".
        for j in 0..fp.graphics.len() {
            if !taken[j] && fp.graphics[j].layer() == layer && same_shape(&fp.graphics[i], &fp.graphics[j]) {
                taken[j] = true;
                merged.push(j);
            }
        }
    }
    // The proxy items are on the layer too, and `findNext` returns one whether it touches the pad or not.
    let have_something = !merged.is_empty() || !mode.proxies.is_empty();

    if have_something {
        // The primitives to add, in the pad's frame: the merged graphics (the ones identical to another are not added twice), then the proxies.
        let mut primitives: Vec<PadPrimitive> = Vec::new();
        for (n, &i) in merged.iter().enumerate() {
            // Only the first of a group of identical shapes becomes a primitive (`mergedShapes` has the rest only to delete them): the group's
            // later members are exactly the ones `findMatching` collected.
            let duplicate = merged[..n].iter().any(|&k| same_shape(&fp.graphics[k], &fp.graphics[i]));
            if duplicate {
                continue;
            }
            let mut shape = fp.graphics[i].clone();
            // Hatched fills become solid is implicit: the editor's graphics have no hatch; a filled shape stays filled.
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
                    let pts: Vec<Point> = if outline.outline_count() > 0 { outline.outline(0).iter().map(|p| Point { x: Nm::from_raw_shim(p.x), y: Nm::from_raw_shim(p.y) }).collect() } else { Vec::new() };
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

        // The merged graphics leave the footprint.
        let mut keep = Vec::with_capacity(fp.graphics.len());
        for (i, g) in std::mem::take(&mut fp.graphics).into_iter().enumerate() {
            if !taken[i] {
                keep.push(g);
            }
        }
        fp.graphics = keep;
    }
}
