//! The Extruded 3D Body of a part, for the 3D view: what `RENDER_3D_OPENGL::renderExtrudedBodies` (`3d-viewer/3d_rendering/opengl/create_scene.cpp` at 8303b2ad) extrudes,
//! sent in `/api/state` as `parts[].extruded_body` ([`body_json`], from `model3d_api::part_json`). The body itself is the design's ([`eda_model::extruded_body`],
//! `Design::extruded_body`); this module makes its outline the way `GetExtrusionOutline` (`3d-viewer/3d_rendering/3d_placeholder_utils.cpp`) makes it, in board
//! coordinates, and the pins `GetExtrusionPinOutline` makes. The page applies the body's own placement (`ApplyExtrusionTransform`), works out the heights and the
//! material and draws it (`web/studio/src/kicad-port/extrudedBody3d.ts`, `components/viewer3d/extrudedBody.ts`): the outline is the backend's because it needs KiCad's
//! polygon code (Clipper2) and the outline chaining (`eda_drc::outline::convert_outline_to_polygon`), the rest is per frame.
//!
//! Differences from KiCad, all in what the studio's footprint holds:
//!   - the courtyard is the footprint's `courtyard_outlines` (what `FOOTPRINT::BuildCourtyardCaches` makes of the courtyard graphics), else the box the board file is
//!     written with (`Footprint::courtyard_half`, which kicad-cli reads as the courtyard) unless the footprint has none (`FootprintExtra::no_courtyard`);
//!   - the footprint is held in its library's frame and placed with `to_board`, so its `F.*` drawings stand for KiCad's flipped `B.*` ones: a back footprint takes its
//!     own side's layer as `GetExtrusionOutline` does (`if( isBack && ( extLayer == F_CrtYd .. ) ) extLayer = FlipLayer( extLayer )`) by construction;
//!   - `CalcPlaceholderLocalBox`'s last fallback, the footprint's bounding box made square, is no box here: a footprint with no pad and no drawing on its fab,
//!     courtyard or silkscreen layers has no body drawn.

use eda_model::extruded_body::{ExtrudedBody, LAYER_AUTO, LAYER_PAD_BBOX};
use eda_model::footprint::to_board;
use eda_model::ir::{Design, FootprintInstance, Point, Shape};
use eda_model::units::Nm;
use eda_model::{ConstraintModel, Footprint, Part};
use eda_shape_poly_set::ShapePolySet;
use serde_json::{json, Value};

/// `ARC_HIGH_DEF` (pcbnew: `pcbIUScale.mmToIU( 0.005 )`), the error of a circle made polygon.
const ARC_HIGH_DEF: Nm = Nm::new(5_000);
/// `buildFilledPolygonFromShapes`' `maxError` (0.005 mm) and `chainingEpsilon` (0.02 mm).
const MAX_ERROR: f64 = 5_000.0;
const CHAINING_EPSILON: Nm = Nm::new(20_000);

/// What the outline is made of, as `GetExtrusionOutline` sorts `m_layer` (the layer of the footprint on the top side, see [`ExtrudedBody::layer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// `UNDEFINED_LAYER`: the courtyard, else the fab drawings, else the silkscreen, else the pads' box.
    Auto,
    /// `UNSELECTED_LAYER`: the pads' box.
    PadBox,
    Courtyard,
    /// The drawings on this layer (`F.Fab` or `F.SilkS`, the footprint's own side).
    Drawings(&'static str),
    /// A layer no outline is made of ("Extrusion: no shapes found on explicit layer").
    Nothing,
}

fn source_of(layer: &str) -> Source {
    match layer {
        LAYER_AUTO => Source::Auto,
        LAYER_PAD_BBOX => Source::PadBox,
        "F.CrtYd" | "B.CrtYd" => Source::Courtyard,
        "F.Fab" | "B.Fab" => Source::Drawings("F.Fab"),
        "F.SilkS" | "B.SilkS" => Source::Drawings("F.SilkS"),
        _ => Source::Nothing,
    }
}

/// `FOOTPRINT::GetCourtyard` for the footprint's own side, placed: see the module doc for where it comes from.
fn courtyard(design: &Design, footprint: &Footprint, fp: &FootprintInstance) -> ShapePolySet {
    let mut set = ShapePolySet::new();
    let none = design.drawings.as_ref().is_some_and(|d| d.footprint_extras.iter().any(|e| e.id == fp.id && e.no_courtyard));
    let outlines: Vec<Vec<(Nm, Nm)>> = if footprint.courtyard_outlines.iter().any(|o| o.len() >= 3) {
        footprint.courtyard_outlines.clone()
    } else if !none && (footprint.courtyard.is_some() || !footprint.pads.is_empty()) {
        let (hw, hh) = footprint.courtyard_half();
        vec![vec![(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]]
    } else {
        Vec::new()
    };
    for o in outlines.iter().filter(|o| o.len() >= 3) {
        set.add_outline(o.iter().map(|&p| p64(to_board(fp, p))).collect());
    }
    set.simplify();
    set
}

fn p64(p: Point) -> eda_clipper2::Point64 {
    eda_clipper2::Point64::new(p.x.get(), p.y.get())
}

/// A copy of `shape` that stands for its area alone: filled, no stroke (`TransformCircleToPolygon` of the circle, `GetRectCorners`, `GetPolyShape`).
fn area_of(shape: &Shape) -> Shape {
    let mut s = shape.clone();
    s.set_filled(true);
    s.set_stroke_width(Nm::ZERO);
    s
}

/// `buildFilledPolygonFromShapes`: the footprint's drawings on `layer` (its own side's, in the library's frame), placed, as one filled outline -- the open ones (lines, arcs,
/// curves) chained into closed contours (`ConvertOutlineToPolygon` with disjoint contours allowed), else, when they do not close, their strokes made polygons and merged, the
/// outlines of that kept; the closed ones (circles, rectangles, polygons) as their areas; the whole merged (`Simplify`). Empty when there is nothing on the layer.
fn filled_from_drawings(footprint: &Footprint, fp: &FootprintInstance, layer: &str) -> ShapePolySet {
    let placed: Vec<Shape> = footprint.art.shapes_on(layer).map(|s| s.placed(fp).shape).collect();
    let (closed, open): (Vec<&Shape>, Vec<&Shape>) = placed.iter().partition(|s| matches!(s, Shape::Circle { .. } | Shape::Rect { .. } | Shape::Polygon { .. }));
    let mut outline = ShapePolySet::new();
    if closed.is_empty() && open.is_empty() {
        return outline;
    }
    if !open.is_empty() {
        let shapes: Vec<Shape> = open.iter().map(|s| (*s).clone()).collect();
        let (chained, ok, _) = eda_drc::outline::convert_outline_to_polygon(&shapes, MAX_ERROR, CHAINING_EPSILON, true);
        if ok && chained.outline_count() > 0 {
            outline = chained;
            outline.simplify();
        } else {
            outline.remove_all_contours();
            let mut strokes = ShapePolySet::new();
            for s in &open {
                strokes.boolean_add(&eda_drc::pad_shape::shape_polygons(s, Nm::new(MAX_ERROR as i64)));
            }
            if strokes.outline_count() > 0 {
                strokes.simplify();
                for i in 0..strokes.outline_count() {
                    outline.add_outline(strokes.outline(i).clone());
                }
            }
        }
    }
    for s in closed {
        outline.boolean_add(&eda_drc::pad_shape::shape_polygons(&area_of(s), ARC_HIGH_DEF));
    }
    outline.simplify();
    outline
}

/// `CalcPlaceholderLocalBox` in the footprint's frame: the box of the pads (each at its place, its size not turned: `GetFPRelativePosition`, `GetSize( ALL_LAYERS )`), else of
/// the drawings on the fab, courtyard and silkscreen layers; `None` when there are neither (see the module doc).
fn placeholder_local_box(footprint: &Footprint) -> Option<(Nm, Nm, Nm, Nm)> {
    let pads = footprint.pads.iter().map(|p| (p.at.0 - p.size.0 / 2, p.at.1 - p.size.1 / 2, p.at.0 + p.size.0 - p.size.0 / 2, p.at.1 + p.size.1 - p.size.1 / 2));
    let merge = |a: (Nm, Nm, Nm, Nm), b: (Nm, Nm, Nm, Nm)| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
    if let Some(b) = pads.reduce(merge) {
        return Some(b);
    }
    ["F.Fab", "F.CrtYd", "F.SilkS"].iter().filter_map(|l| footprint.art.bbox_on(l)).chain(footprint.courtyard_outlines.iter().flatten().map(|&(x, y)| (x, y, x, y))).reduce(merge)
}

/// `buildPadBoundingBox`: [`placeholder_local_box`]'s corners turned and moved with the footprint (`RotatePoint( corners[i], fpAngle ); corners[i] += fpPos`).
fn pad_box(footprint: &Footprint, fp: &FootprintInstance) -> ShapePolySet {
    let mut set = ShapePolySet::new();
    if let Some((x0, y0, x1, y1)) = placeholder_local_box(footprint).filter(|b| b.2 > b.0 && b.3 > b.1) {
        set.add_outline([(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&c| p64(to_board(fp, c))).collect());
    }
    set
}

/// `GetExtrusionOutline`: the outline of the body of the footprint at `fp`, board coordinates; empty when none can be made.
pub(crate) fn extrusion_outline(design: &Design, footprint: &Footprint, fp: &FootprintInstance, body: &ExtrudedBody) -> ShapePolySet {
    match source_of(&body.layer) {
        Source::PadBox => pad_box(footprint, fp),
        Source::Courtyard => courtyard(design, footprint, fp),
        Source::Drawings(layer) => filled_from_drawings(footprint, fp, layer),
        Source::Nothing => ShapePolySet::new(),
        // "Auto mode is courtyard -> fab -> pad bbox" (the silkscreen between the two)
        Source::Auto => {
            let c = courtyard(design, footprint, fp);
            if c.outline_count() > 0 {
                return c;
            }
            for layer in ["F.Fab", "F.SilkS"] {
                let d = filled_from_drawings(footprint, fp, layer);
                if d.outline_count() > 0 {
                    return d;
                }
            }
            pad_box(footprint, fp)
        }
    }
}

/// `GetExtrusionPinOutline`: every pad with a hole, as its hole (`TransformHoleToPolygon`: the slot `GetEffectiveHoleShape` makes, a round hole a slot of no length) shrunk by a
/// twentieth of the drill's x size ("~90% of hole diameter", `shrink = -GetDrillSize().x / 20` on each side), placed and merged.
pub(crate) fn pin_outline(footprint: &Footprint, fp: &FootprintInstance) -> ShapePolySet {
    use eda_shape_poly_set::basic_shapes::{oval_polygon, ErrorLoc};
    let mut set = ShapePolySet::new();
    for pad in &footprint.pads {
        let (dx, dy) = match (pad.drill_slot, pad.drill) {
            (Some(slot), _) => slot,
            (None, Some(d)) => (d, d),
            _ => continue,
        };
        if dx <= Nm::ZERO || dy <= Nm::ZERO {
            continue;
        }
        let shrink = -dx.get() / 20;
        // the slot's axis: along its long side, turned with the pad
        let (half, width) = if dx >= dy { ((dx - dy).get() / 2, dy.get()) } else { ((dy - dx).get() / 2, dx.get()) };
        let along_x = dx >= dy;
        let a = (pad.rot as f64 / 1000.0).to_radians();
        let (s, c) = a.sin_cos();
        let (ux, uy) = if along_x { (c, -s) } else { (s, c) };
        let ends = [1.0, -1.0].map(|k: f64| {
            let local = (pad.at.0 + Nm::kiround(k * half as f64 * ux), pad.at.1 + Nm::kiround(k * half as f64 * uy));
            p64(to_board(fp, local))
        });
        let w = width + 2 * shrink;
        if w > 0 {
            set.add_outline(oval_polygon(ends[0], ends[1], w, ARC_HIGH_DEF.get(), ErrorLoc::Inside, 0));
        }
    }
    set.simplify();
    set
}

/// The contours of `set` for the page: each polygon as `[outline, hole, ..]`, each a list of `[x, y]` in nanometres.
fn contours(set: &ShapePolySet) -> Value {
    json!(set.polys.iter().map(|poly| poly.iter().map(|chain| chain.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>()).collect::<Vec<_>>())
}

/// `parts[].extruded_body` of `part`, whose footprint resolved to `footprint`: the body (`height`, `standoff` in nm, `layer`, `material`, `color` -- the one it is drawn in,
/// `GetDefaultColor` for none of its own -- and `color_set`, `offset`, `scale`, `rotate`, `show`), the footprint's position (`at`, nm: what `ApplyExtrusionTransform` turns and
/// scales about), the `outline` KiCad extrudes before that transform and the `pins` (both [`contours`], board coordinates). `None` for a part with no body, a body with no
/// height, or a part not on the board.
pub fn body_json(design: &Design, _model: &ConstraintModel, part: &Part, footprint: &Footprint) -> Option<Value> {
    let body = design.extruded_body(&part.reference).filter(|b| b.height > Nm::ZERO)?;
    let fp = design.placement.as_ref()?.footprints.iter().find(|f| f.id == part.reference)?;
    let outline = extrusion_outline(design, footprint, fp, &body);
    let pins = pin_outline(footprint, fp);
    Some(json!({
        "height": body.height,
        "standoff": body.standoff,
        "layer": body.layer,
        "material": body.material.token(),
        "color": body.drawn_color(),
        "color_set": body.color.is_some(),
        "offset": body.offset,
        "scale": body.scale,
        "rotate": body.rotate,
        "show": body.show,
        "at": [fp.at.x, fp.at.y],
        "outline": contours(&outline),
        "pins": contours(&pins),
    }))
}

/// What the 3D Models tab's preview needs to draw the box's body as it is being set (`PANEL_FP_PROPERTIES_3D_MODEL::updateExtrusionPreview`: the dummy footprint's body takes
/// the box's heights, source, colour and material, and `UpdateDummyFootprint` draws it): the outline `GetExtrusionOutline` makes of the footprint at `fp` for each of the box's
/// sources (`m_extrusionLayers`: "auto", `F.CrtYd`, `F.Fab`, `F.SilkS`, "pad_bbox"; an empty list where it makes none) and the pins (`GetExtrusionPinOutline`), board nm.
pub fn preview_outlines(design: &Design, footprint: &Footprint, fp: &FootprintInstance) -> Value {
    let mut sources = serde_json::Map::new();
    for layer in [LAYER_AUTO, "F.CrtYd", "F.Fab", "F.SilkS", LAYER_PAD_BBOX] {
        let body = ExtrudedBody { layer: layer.into(), ..Default::default() };
        sources.insert(layer.into(), contours(&extrusion_outline(design, footprint, fp, &body)));
    }
    json!({ "sources": sources, "pins": contours(&pin_outline(footprint, fp)), "at": [fp.at.x, fp.at.y] })
}

/// The formats of the box's "Export..." file dialog (`OnExportExtrudedModel`: "STEP files (*.step)", "GLB files (*.glb)", "STL files (*.stl)", "BREP files
/// (*.brep)"), by their kicad-cli command.
pub const EXPORT_FORMATS: [&str; 4] = ["step", "glb", "stl", "brep"];

/// The kicad-cli arguments of the export of footprint `reference`'s body alone, its position the origin (`STEP_PCB_MODEL( .. )` with `origin =
/// m_footprint->GetPosition()` and `CreatePCB( outline, origin, false )`: no board body): `--no-board-body`, the one footprint (`--component-filter`), the origin
/// at the footprint (`--user-origin`, mm).
pub fn export_args(reference: &str, at: Point) -> Vec<String> {
    vec![
        "--no-board-body".into(),
        "--component-filter".into(),
        reference.into(),
        format!("--user-origin={:.6}x{:.6}mm", at.x.as_f64() / 1e6, at.y.as_f64() / 1e6),
        "-f".into(),
    ]
}

/// RFC 4648 base64 of `bytes`, padded: the file sent back to the page, which saves it (the browser's download stands for KiCad's `wxFileDialog`).
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn fail(message: &str) -> Value {
    json!({ "ok": false, "message": message })
}

/// `POST /api/extruded_body/export` -- the Extruded 3D Body box's "Export..." (`PANEL_FP_PROPERTIES_3D_MODEL::OnExportExtrudedModel`, pcbnew/dialogs/
/// panel_fp_properties_3d_model.cpp at KiCad 25ec2335): `{ part, format: "step"|"glb"|"stl"|"brep", body }` on the board, `{ footprint: "Lib:Name", reference, format,
/// body }` in the Footprint Editor (a scratch board of that footprint alone at the origin, [`library_scratch`]), `body` the box as it stands (the page checked the
/// heights as `readExtrusionHeights` does, "Export Extruded Body"). The outline is `GetExtrusionOutline( m_footprint, .., layer )` of the footprint on the board:
/// none is KiCad's "No extrusion outline could be generated for this footprint.". KiCad then builds the body and its pins in a `STEP_PCB_MODEL` (OpenCASCADE) and
/// writes it; the studio has kicad-cli do the same through `EXPORTER_STEP::buildFootprint3DShapes` (the same `AddExtrudedBody` and `AddExtrudedPins` with the same
/// heights) on a copy of the board where the footprint has the box's body and no model files, with the board body left out, that footprint alone and its
/// position the origin ([`export_args`]). The reply is `{ ok, name: "<REF>_extruded.<ext>", data: base64 }` (the file dialog's default name, `GetReference() +
/// "_extruded"`), or `{ ok: false, message }` -- "Failed to create extruded body geometry." for a body the board's check refuses (a standoff not under the height),
/// "Failed to write file." when kicad-cli writes nothing.
pub fn export(dir: &std::path::Path, req_body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(req_body).unwrap_or(Value::Null);
    let format = EXPORT_FORMATS.iter().copied().find(|f| Some(*f) == req.get("format").and_then(Value::as_str)).unwrap_or("step");
    let Some(mut body) = req.get("body").cloned().and_then(|b| serde_json::from_value::<ExtrudedBody>(b).ok()) else { return fail("no body") };
    let (meta, design, model) = match crate::board::load(dir) {
        Ok(x) => x,
        Err(e) => return fail(&crate::board::reasons(&e)),
    };
    // The Footprint Editor's box (`{ footprint: "Lib:Name", reference }`): its `m_footprint` is the library footprint at the editor's origin, `GetPosition()` (0, 0) the
    // export's origin and `GetReference()` the file's name -- here a scratch board of that footprint alone ([`library_scratch`]).
    let (design, model, reference, shown) = match req.get("footprint").and_then(Value::as_str) {
        Some(name) => match library_scratch(dir, name) {
            Ok((d, m)) => (d, m, SCRATCH_REFERENCE.to_string(), req.get("reference").and_then(Value::as_str).filter(|r| !r.is_empty()).unwrap_or("REF**").to_string()),
            Err(e) => return fail(&e),
        },
        None => {
            let Some(reference) = req.get("part").and_then(Value::as_str) else { return fail("no part") };
            (design, model, reference.to_string(), reference.to_string())
        }
    };
    let reference = reference.as_str();
    let Some(fp) = design.placement.as_ref().and_then(|p| p.footprints.iter().find(|f| f.id == reference)).cloned() else { return fail("the part is not on the board") };
    let Some(footprint) = model.parts.iter().find(|p| p.reference == reference).and_then(|p| model.footprint_of(p)) else { return fail("the part has no footprint") };
    if extrusion_outline(&design, &footprint, &fp, &body).outline_count() == 0 {
        return fail("No extrusion outline could be generated for this footprint.");
    }
    // The exporter draws a body only while it is shown (`GetExtruded()->m_show`); the dialog's export does not ask.
    body.show = true;
    let mut board = eda_ops::Board::new(design.clone(), &model, meta.snap_nm, meta.spacing_nm);
    if let Err(e) = board.apply(&eda_ops::Cmd::SetExtrudedBody { part: reference.into(), body: Some(body) }) {
        if !e.iter().any(|c| c.check == "ops_nothing_to_change") {
            return fail("Failed to create extruded body geometry.");
        }
    }
    // The body alone: the footprint's model files are not the body's (an empty list may already be what it has).
    let _ = board.apply(&eda_ops::Cmd::SetBoardModels { part: reference.into(), models: vec![] });
    let scratch = dir.join(".kicad").join("extruded-export");
    let out = scratch.join("out");
    let _ = std::fs::remove_dir_all(&scratch);
    let name = crate::kicad_engine::project_name_of(dir);
    let reply = eda_kicad_engine::export_pcb_into(board.design(), &model, &scratch.join("work"), dir, &name, format, &export_args(reference, fp.at), &out);
    let file = std::fs::read_dir(&out).ok().and_then(|d| d.filter_map(Result::ok).map(|e| e.path()).find(|p| p.extension().and_then(|e| e.to_str()) == Some(format)));
    let result = match (reply, file.and_then(|f| std::fs::read(f).ok())) {
        (Ok(_), Some(bytes)) if !bytes.is_empty() => json!({ "ok": true, "name": format!("{shown}_extruded.{format}"), "data": base64(&bytes) }),
        (Err(e), _) => json!({ "ok": false, "message": format!("Failed to write file.\n{}", crate::board::reasons(&e)) }),
        _ => fail("Failed to write file."),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// The reference the scratch board's one footprint has (kicad-cli's `--component-filter` names it; the file is named by the reference the dialog shows).
const SCRATCH_REFERENCE: &str = "REF1";

/// A board of library footprint `name` alone, at the origin on the top side, as the Footprint Editor holds it (`FOOTPRINT_EDIT_FRAME`'s footprint at (0, 0), not flipped):
/// what kicad-cli exports the body of, the board's outline never drawn (`--no-board-body`).
fn library_scratch(dir: &std::path::Path, name: &str) -> Result<(Design, ConstraintModel), String> {
    let (library, _) = crate::library_api::footprint_any(dir, name).map_err(|e| crate::board::reasons(&e))?;
    let footprint = library.to_engine_footprint();
    let part = Part { reference: SCRATCH_REFERENCE.into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(footprint.name.clone()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
    let model = ConstraintModel { parts: vec![part], footprints: vec![footprint], ..Default::default() };
    let mut design = crate::import_kicad::empty_board();
    design.placement.as_mut().expect("an empty board has a placement").footprints.push(FootprintInstance {
        id: SCRATCH_REFERENCE.into(),
        at: Point { x: Nm::ZERO, y: Nm::ZERO },
        rot: 0,
        side: eda_model::ir::Side::Top,
        label: Default::default(),
    });
    Ok((design, model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::extruded_body::ExtrusionMaterial;
    use eda_model::fp_art::{FpArt, FpShape};
    use eda_model::ir::{LabelSide, Side};
    use eda_model::units::mm;
    use eda_model::{PadKind, PadShape};

    fn pad(n: &str, x: f64, y: f64, drill: Option<f64>, slot: Option<(f64, f64)>) -> eda_model::Pad {
        let kind = if drill.is_some() || slot.is_some() { PadKind::ThroughHole } else { PadKind::Smd };
        eda_model::Pad { number: n.into(), at: (mm(x), mm(y)), size: (mm(1.0), mm(0.6)), shape: PadShape::Rect, kind, drill: drill.map(mm), drill_slot: slot.map(|(a, b)| (mm(a), mm(b))), rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
    }

    fn seg(layer: &str, a: (f64, f64), b: (f64, f64)) -> FpShape {
        FpShape::new(Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: mm(0.1), filled: false, start: Point { x: mm(a.0), y: mm(a.1) }, end: Point { x: mm(b.0), y: mm(b.1) } })
    }

    fn at(x: f64, y: f64, rot_deg: u32, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x: mm(x), y: mm(y) }, rot: rot_deg * 1000, side, label: LabelSide::Above }
    }

    /// A design with nothing in it but what a test sets.
    fn empty() -> Design {
        serde_json::from_value(json!({ "provenance": { "engine_version": "t", "intent_hash": "", "seed": 0 } })).unwrap()
    }

    /// The box of a set's outlines, millimetres.
    fn bbox(set: &ShapePolySet) -> (f64, f64, f64, f64) {
        let pts: Vec<_> = (0..set.outline_count()).flat_map(|i| set.outline(i).clone()).collect();
        let f = |v: i64| (v as f64 / 1e6 * 1000.0).round() / 1000.0;
        (f(pts.iter().map(|p| p.x).min().unwrap()), f(pts.iter().map(|p| p.y).min().unwrap()), f(pts.iter().map(|p| p.x).max().unwrap()), f(pts.iter().map(|p| p.y).max().unwrap()))
    }

    fn body(layer: &str) -> ExtrudedBody {
        ExtrudedBody { height: mm(1.0), layer: layer.into(), ..Default::default() }
    }

    /// A footprint with an off-centre courtyard, a fab rectangle drawn as four lines, a silkscreen line and two pads.
    fn footprint() -> Footprint {
        let mut art = FpArt::default();
        for (a, b) in [((-1.5, -1.0), (1.5, -1.0)), ((1.5, -1.0), (1.5, 1.0)), ((1.5, 1.0), (-1.5, 1.0)), ((-1.5, 1.0), (-1.5, -1.0))] {
            art.shapes.push(seg("F.Fab", a, b));
        }
        art.shapes.push(seg("F.SilkS", (-1.0, -1.4), (1.0, -1.4)));
        Footprint {
            name: "T:Box".into(),
            pads: vec![pad("1", -1.0, 0.0, None, None), pad("2", 1.0, 0.0, None, None)],
            courtyard_outlines: vec![vec![(mm(-2.0), mm(-1.5)), (mm(3.0), mm(-1.5)), (mm(3.0), mm(1.5)), (mm(-2.0), mm(1.5))]],
            art,
            ..Default::default()
        }
    }

    #[test]
    fn the_export_is_the_footprint_alone_at_its_position_and_its_file_is_base64() {
        assert_eq!(export_args("U1", Point { x: mm(10.5), y: mm(-2.0) }), ["--no-board-body", "--component-filter", "U1", "--user-origin=10.500000x-2.000000mm", "-f"]);
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(crate::foreign_api::base64_decode(&base64(&[0, 255, 7, 128, 1])).unwrap(), vec![0, 255, 7, 128, 1]);
    }

    #[test]
    fn the_outline_is_the_courtyard_then_the_fab_then_the_silkscreen_then_the_pads_box_as_get_extrusion_outline_takes_it() {
        let d = empty();
        let f = footprint();
        let top = at(10.0, 20.0, 0, Side::Top);
        assert_eq!(bbox(&extrusion_outline(&d, &f, &top, &body("auto"))), (8.0, 18.5, 13.0, 21.5), "auto: the courtyard, off-centre as it is");
        assert_eq!(bbox(&extrusion_outline(&d, &f, &top, &body("F.CrtYd"))), (8.0, 18.5, 13.0, 21.5));
        // the fab lines chain into a closed contour: its area is the rectangle's (`ConvertOutlineToPolygon`), not the strokes'
        let fab = extrusion_outline(&d, &f, &top, &body("F.Fab"));
        assert_eq!(bbox(&fab), (8.5, 19.0, 11.5, 21.0));
        assert!((fab.area() / 1e12 - 6.0).abs() < 1e-6, "the filled rectangle, 3 x 2 mm: {}", fab.area() / 1e12);
        // a single silkscreen line does not close: its stroke (0.1 mm wide, round ends) is the outline
        let silk = extrusion_outline(&d, &f, &top, &body("F.SilkS"));
        assert_eq!(bbox(&silk), (8.95, 18.55, 11.05, 18.65));
        // the pads' box: each pad's size unturned about its place (`CalcPlaceholderLocalBox`)
        assert_eq!(bbox(&extrusion_outline(&d, &f, &top, &body("pad_bbox"))), (8.5, 19.7, 11.5, 20.3));
        assert_eq!(extrusion_outline(&d, &f, &top, &body("F.Cu")).outline_count(), 0, "a layer no outline is made of");
        // the fallbacks in order: no courtyard -> the fab; no fab either -> the silkscreen; nothing drawn -> the pads' box
        let mut g = f.clone();
        g.courtyard_outlines.clear();
        let mut none = empty();
        none.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![eda_model::ir::FootprintExtra { id: "U1".into(), no_courtyard: true, ..Default::default() }], ..Default::default() });
        assert_eq!(bbox(&extrusion_outline(&none, &g, &top, &body("auto"))), (8.5, 19.0, 11.5, 21.0));
        g.art.shapes.retain(|s| s.shape.layer() != "F.Fab");
        assert_eq!(bbox(&extrusion_outline(&none, &g, &top, &body("auto"))), (8.95, 18.55, 11.05, 18.65));
        g.art.shapes.clear();
        assert_eq!(bbox(&extrusion_outline(&none, &g, &top, &body("auto"))), (8.5, 19.7, 11.5, 20.3));
        // with no courtyard of its own but not marked as having none, the courtyard is the box the board file is written with
        assert!(extrusion_outline(&d, &g, &top, &body("F.CrtYd")).outline_count() > 0);
    }

    #[test]
    fn the_outline_turns_and_flips_with_the_footprint() {
        let d = empty();
        let f = footprint();
        // KiCad's orientation 90 (counter-clockwise on the screen, y down; the studio's `rot` is the other way round, 270): the courtyard's +x side (3 mm) goes to -y
        assert_eq!(bbox(&extrusion_outline(&d, &f, &at(0.0, 0.0, 270, Side::Top), &body("auto"))), (-1.5, -3.0, 1.5, 2.0));
        // on the back, x mirrored (`FOOTPRINT::Flip` and the back courtyard `GetExtrusionOutline` takes for a back footprint)
        assert_eq!(bbox(&extrusion_outline(&d, &f, &at(0.0, 0.0, 0, Side::Bottom), &body("F.CrtYd"))), (-3.0, -1.5, 2.0, 1.5));
        assert_eq!(bbox(&extrusion_outline(&d, &f, &at(0.0, 0.0, 0, Side::Bottom), &body("B.CrtYd"))), (-3.0, -1.5, 2.0, 1.5), "either side's name is the footprint's own side");
    }

    #[test]
    fn the_pins_are_the_holes_at_nine_tenths_of_the_drill() {
        let mut f = footprint();
        f.pads = vec![pad("1", 0.0, 0.0, Some(1.0), None), pad("2", 3.0, 0.0, None, Some((2.0, 1.0))), pad("3", 6.0, 0.0, None, None)];
        let pins = pin_outline(&f, &at(0.0, 0.0, 0, Side::Top));
        assert_eq!(pins.outline_count(), 2, "the SMD pad has no pin");
        let mut boxes: Vec<_> = (0..pins.outline_count()).map(|i| {
            let mut one = ShapePolySet::new();
            one.add_outline(pins.outline(i).clone());
            bbox(&one)
        }).collect();
        boxes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // the round hole: 0.9 mm across (within the arc error, inside); the slot: its width 1 - 2 x 2/20 = 0.8 mm, its length 2 - 0.2 = 1.8 mm
        let near = |a: f64, b: f64| (a - b).abs() < 0.006;
        let r = boxes[0];
        assert!(near(r.0, -0.45) && near(r.2, 0.45) && near(r.1, -0.45) && near(r.3, 0.45), "{r:?}");
        let s = boxes[1];
        assert!(near(s.0, 3.0 - 0.9) && near(s.2, 3.0 + 0.9) && near(s.1, -0.4) && near(s.3, 0.4), "{s:?}");
    }

    #[test]
    fn a_part_has_its_body_in_the_state_only_when_it_has_one_with_a_height() {
        let (part, f): (Part, Footprint) = (serde_json::from_value(json!({ "reference": "U1" })).unwrap(), footprint());
        let mut d = empty();
        d.placement = Some(eda_model::ir::PlacementSection { outline: Vec::new(), footprints: vec![at(10.0, 20.0, 0, Side::Top)], modules: Vec::new() });
        let m = ConstraintModel::default();
        assert!(body_json(&d, &m, &part, &f).is_none());
        let b = ExtrudedBody { height: mm(2.0), standoff: mm(0.5), material: ExtrusionMaterial::Copper, ..Default::default() };
        d.drawings = Some(eda_model::ir::DrawingsSection { footprint_extras: vec![eda_model::ir::FootprintExtra { id: "U1".into(), extruded_body: Some(b.clone()), ..Default::default() }], ..Default::default() });
        let v = body_json(&d, &m, &part, &f).unwrap();
        assert_eq!((v["height"].as_i64(), v["standoff"].as_i64(), v["material"].as_str(), v["color_set"].as_bool()), (Some(2_000_000), Some(500_000), Some("copper"), Some(false)));
        assert_eq!(v["color"], json!([0.72, 0.45, 0.2, 1.0]), "the material's colour, `GetDefaultColor`");
        assert_eq!(v["at"], json!([10_000_000, 20_000_000]));
        assert_eq!(v["outline"].as_array().unwrap().len(), 1, "one polygon: the courtyard");
        assert_eq!(v["outline"][0][0].as_array().unwrap().len(), 4);
        d.drawings.as_mut().unwrap().footprint_extras[0].extruded_body = Some(ExtrudedBody { height: Nm::ZERO, ..b });
        assert!(body_json(&d, &m, &part, &f).is_none(), "no height, no body (the file writes none)");
    }
}
