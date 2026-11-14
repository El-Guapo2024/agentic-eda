//! `GET /api/board3d`: what the 3D viewer needs of the board that is polygon arithmetic, done here with the Clipper2 the DRC and the outline use, rather than in
//! the page (`docs/HANDOFF.md`'s port-vs-reuse rule: fast public C++ on the backend).
//!
//! The board body: `BOARD_ADAPTER::createBoardPolygon` (`3d-viewer/3d_canvas/board_adapter.cpp`, `GetBoardPolygonOutlines( m_board_poly, true, nullptr, false,
//! true )`) less the holes `RENDER_3D_OPENGL::reload` (`3d_rendering/opengl/create_scene.cpp`) takes out for `m_boardWithHoles`:
//!
//! ```text
//! SHAPE_POLY_SET board_poly_with_holes = m_boardAdapter.GetBoardPoly().CloneDropTriangulation();
//! board_poly_with_holes.BooleanSubtract( m_boardAdapter.GetTH_ODPolys() );
//! board_poly_with_holes.BooleanSubtract( m_boardAdapter.GetNPTH_ODPolys() );
//! ```
//!
//! `m_TH_ODPolys` and `m_NPTH_ODPolys` are `BOARD_ADAPTER::createLayers`' (`create_layer_items.cpp`): a through via's hole grown by the plating
//! (`TransformCircleToPolygon( m_TH_ODPolys, via->GetStart(), holediameter / 2 + GetHolePlatingThickness(), via->GetMaxError(), ERROR_INSIDE )`; a blind,
//! buried or micro via goes to its layers' hole maps instead, so it does not cut the body), a plated pad's hole the same
//! (`pad->TransformHoleToPolygon( m_TH_ODPolys, GetHolePlatingThickness(), pad->GetMaxError(), ERROR_INSIDE )`) and a non-plated one as drilled
//! (`TransformHoleToPolygon( m_NPTH_ODPolys, 0, .. )`). `GetHolePlatingThickness()` is `ADVANCED_CFG::m_HoleWallThickness`, 0.020 mm.
//!
//! The counterbores, countersinks and backdrills KiCad also subtracts are not in the model (a via's post-machining is kept as its tokens only).

use eda_model::footprint::{to_board, PadKind};
use eda_model::ir::{Design, FootprintInstance, Nm, Point};
use eda_model::ConstraintModel;
use eda_shape_poly_set::basic_shapes::{circle_polygon, oval_polygon, ErrorLoc};
use eda_shape_poly_set::{CornerStrategy, LineChain, ShapePolySet};
use eda_clipper2::Point64;
use serde_json::{json, Value};

/// `ADVANCED_CFG::m_HoleWallThickness` (`common/advanced_config.cpp`: `m_HoleWallThickness = 0.020;`), what `BOARD_DESIGN_SETTINGS::GetHolePlatingThickness` returns.
pub const HOLE_PLATING_THICKNESS: Nm = Nm::new(20_000);

/// `BOARD_ITEM::GetMaxError`: the board's `m_MaxError`, `ARC_HIGH_DEF` (0.005 mm) by default.
const MAX_ERROR: i64 = eda_model::outline::MAX_ERROR_NM.get();

fn p64(p: Point) -> Point64 {
    Point64::new(p.x.get(), p.y.get())
}

/// `PAD::GetEffectiveHoleShape` on the board, as `PAD::TransformHoleToPolygon` takes it: the segment of the hole and its width, `None` for a pad with no hole
/// (`if( !drillsize.x || !drillsize.y ) return false;`). The hole is at the pad's position (not where an offset moves the copper), its slot along the pad's
/// own axes turned with the pad (`RotatePoint( half_len, GetOrientation() )`), mirrored with a bottom-side footprint.
pub fn pad_hole_segment(fp: &FootprintInstance, pad: &eda_model::footprint::Pad) -> Option<(Point64, Point64, i64)> {
    let centre = to_board(fp, pad.at);
    let (sx, sy) = match (pad.drill_slot, pad.drill) {
        (Some((w, h)), _) => (w.get(), h.get()),
        (None, Some(d)) => (d.get(), d.get()),
        (None, None) => return None,
    };
    if sx == 0 || sy == 0 {
        return None;
    }
    // `half_size = drill.size / 2`; a round hole is `half_size.x` wide, an oblong one `min( half_size.x, half_size.y )` with `half_len` the rest.
    let (hx, hy) = (sx / 2, sy / 2);
    let round = pad.drill_slot.is_none();
    let half_width = if round { hx } else { hx.min(hy) };
    let (lx, ly) = if round { (0, 0) } else { (hx - half_width, hy - half_width) };
    let frame = eda_model::pad_geom::PadFrame::of(fp, pad, Point { x: Nm::ZERO, y: Nm::ZERO });
    let (dx, dy) = frame.to_board((lx as f64, ly as f64));
    let (dx, dy) = (dx.round() as i64, dy.round() as i64);
    let c = p64(centre);
    Some((Point64::new(c.x - dx, c.y - dy), Point64::new(c.x + dx, c.y + dy), half_width * 2))
}

/// `PAD::TransformHoleToPolygon( aBuffer, aClearance, aError, ERROR_INSIDE )`: the hole's segment as an oval `aClearance` wider on each side.
fn hole_polygon(seg: (Point64, Point64, i64), clearance: i64) -> LineChain {
    oval_polygon(seg.0, seg.1, seg.2 + clearance * 2, MAX_ERROR, ErrorLoc::Inside, 0)
}

/// The through holes (`m_TH_ODPolys`) and the non-plated ones (`m_NPTH_ODPolys`) of the board, as `BOARD_ADAPTER::createLayers` builds them.
pub fn board_holes(design: &Design, model: &ConstraintModel) -> (ShapePolySet, ShapePolySet) {
    let mut th = ShapePolySet::new();
    let mut npth = ShapePolySet::new();
    let plating = HOLE_PLATING_THICKNESS.get();
    if let Some(r) = design.routing.as_ref() {
        for v in &r.vias {
            // `if( viatype != VIATYPE::THROUGH )`: a blind, buried or micro via's hole is its layers', not the body's.
            let kind = design.drawings.as_ref().and_then(|d| d.via_extra(&v.id)).map(|e| e.kind).unwrap_or_default();
            if kind != eda_model::via_extra::ViaKind::Through {
                continue;
            }
            th.add_outline(circle_polygon(p64(v.at), v.drill.get() / 2 + plating, MAX_ERROR, ErrorLoc::Inside, 0));
        }
    }
    if let Some(pl) = design.placement.as_ref() {
        for fp in &pl.footprints {
            let Some(part) = model.parts.iter().find(|p| p.reference == fp.id) else { continue };
            if let Some(footprint) = model.footprint_of(part) {
                for pad in &footprint.pads {
                    let Some(seg) = pad_hole_segment(fp, pad) else { continue };
                    match pad.kind {
                        // "The hole in the body is inflated by copper thickness."
                        PadKind::ThroughHole => th.add_outline(hole_polygon(seg, plating)),
                        // "If not plated, no copper."
                        PadKind::NonPlatedHole => npth.add_outline(hole_polygon(seg, 0)),
                        PadKind::Smd => 0,
                    };
                }
            }
            // A non-plated hole on no copper layer (`FootprintExtra::holes`) is a pad of the footprint to KiCad, drilled like any other.
            if let Some(extra) = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp.id)) {
                for hole in &extra.holes {
                    if let Some(seg) = pad_hole_segment(fp, &hole.pad) {
                        npth.add_outline(hole_polygon(seg, 0));
                    }
                }
            }
        }
    }
    // `m_TH_ODPolys.Simplify(); m_NPTH_ODPolys.Simplify();` (the end of `createLayers`): the holes that overlap are one.
    th.simplify();
    npth.simplify();
    (th, npth)
}

/// The board body with its holes, `m_boardWithHoles`' polygon.
pub fn board_body(design: &Design, model: &ConstraintModel) -> ShapePolySet {
    let outline = eda_drc::outline::board_outline_with(design, Some(model), true);
    let mut body = outline.polys.clone();
    if body.is_empty() {
        return body;
    }
    let (th, npth) = board_holes(design, model);
    body.boolean_subtract(&th);
    body.boolean_subtract(&npth);
    body
}

fn polys_json(set: &ShapePolySet) -> Value {
    let ring = |c: &LineChain| c.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>();
    Value::Array((0..set.outline_count()).map(|i| json!({ "outer": ring(set.outline(i)), "holes": (0..set.hole_count(i)).map(|h| ring(set.hole(i, h))).collect::<Vec<_>>() })).collect())
}

/// The route's answer: `{ "body": [{ "outer": [[x, y], ..], "holes": [[[x, y], ..], ..] }, ..], "plating": nm }` (nanometres, board space). `body` is empty for
/// a board with no outline.
pub fn board3d_json(design: &Design, model: &ConstraintModel) -> Value {
    json!({ "body": polys_json(&board_body(design, model)), "plating": HOLE_PLATING_THICKNESS.get() })
}

/// One contour of a request (`[[x, y], ..]`, nm), closed.
fn chain_of(v: &Value) -> Option<LineChain> {
    let pts = v.as_array()?;
    let chain: LineChain = pts.iter().filter_map(|p| Some(Point64::new(p.get(0)?.as_f64()?.round() as i64, p.get(1)?.as_f64()?.round() as i64))).collect();
    (chain.len() >= 3).then_some(chain)
}

/// `[{ "outer": [[x, y], ..], "holes": [[[x, y], ..], ..] }, ..]` as a `SHAPE_POLY_SET`, unioned (`Simplify`).
fn polys_of(v: &Value) -> ShapePolySet {
    let mut set = ShapePolySet::new();
    for p in v.as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(outer) = p.get("outer").and_then(chain_of) else { continue };
        let i = set.add_outline(outer);
        for h in p.get("holes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            if let Some(h) = chain_of(h) {
                set.add_hole(h, Some(i));
            }
        }
    }
    set.simplify();
    set
}

/// `POST /api/board3d/polys`: the polygon arithmetic the 3D view's layers need and the page cannot do (`BOARD_ADAPTER::createPadWithMargin`'s `poly.Inflate( clearance.x,
/// CORNER_STRATEGY::ROUND_ALL_CORNERS, aPad->GetMaxError() )` of a custom pad's paste and mask; `addShape`'s `polyList.Inflate( margin, cornerStr, .. )` of a filled shape's
/// mask opening; `PCB_TEXT::TransformTextToPolySet`'s knockout, `finalPoly.BooleanSubtract( textShape )`; `RENDER_3D_OPENGL::bgWorker`'s `board_poly_with_holes.BooleanSubtract(
/// GetFrontCounterborePolys() )` ..). The body is `{ "jobs": [{ "key", "a": polys, "inflate"?: nm, "acute"?: bool, "b"?: polys }] }`; each job is `a` unioned, inflated by
/// `inflate` (`ROUND_ALL_CORNERS`, or `ALLOW_ACUTE_CORNERS` with `acute`) at the board's error, less `b` unioned. The answer is `{ "results": { key: polys } }`.
pub fn polys_report(body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let mut results = serde_json::Map::new();
    for job in req.get("jobs").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        let Some(key) = job.get("key").and_then(Value::as_str) else { continue };
        let mut a = polys_of(job.get("a").unwrap_or(&Value::Null));
        let inflate = job.get("inflate").and_then(Value::as_f64).map(|v| v.round() as i64).unwrap_or(0);
        if inflate != 0 {
            let strategy = if job.get("acute").and_then(Value::as_bool) == Some(true) { CornerStrategy::AllowAcuteCorners } else { CornerStrategy::RoundAllCorners };
            a.inflate(inflate, strategy, MAX_ERROR as i32, true);
        }
        if let Some(b) = job.get("b") {
            let b = polys_of(b);
            if !b.is_empty() {
                a.boolean_subtract(&b);
            }
        }
        results.insert(key.to_string(), polys_json(&a));
    }
    json!({ "results": results })
}

/// The answer for the board in `dir`: [`board3d_json`] and the custom rules' margins ([`rule_margins`]).
pub fn report(dir: &std::path::Path) -> Value {
    match crate::board::load(dir) {
        Ok((meta, design, model)) => {
            let mut out = board3d_json(&design, &model);
            out["rule_margins"] = rule_margins(dir, &meta.intent, &design, &model);
            out
        }
        Err(e) => json!({ "error": crate::board::reasons(&e) }),
    }
}

/// What the custom rules make of the solder mask expansions and paste margins the 3D view opens the masks and lays the paste with: `PAD::GetSolderMaskExpansion`,
/// `PAD::GetSolderPasteMargin`, `PCB_TRACK::GetSolderMaskExpansion` and `PCB_SHAPE::GetSolderMaskExpansion` (pcbnew/pad.cpp, pcb_track.cpp, pcb_shape.cpp) ask the DRC
/// engine -- `if( drcEngine->HasRulesForConstraintType( SOLDER_MASK_EXPANSION_CONSTRAINT ) )` .. `EvalRules( SOLDER_MASK_EXPANSION_CONSTRAINT, this, nullptr, aLayer )`,
/// its `m_Value.Opt()` -- whenever the board's rules have a constraint of the kind; the engine's own fallback is the item's, its footprint's and the board's values
/// (`eda_drc::constraints`). `null` for a board whose rules have none of the three kinds (the page then reads the items' own values); else
/// `{ "mask": bool, "paste_abs": bool, "paste_rel": bool, "pads": { "<REF.NUM[#k]>": { "mask": [front, back], "abs": [front, back], "rel": [front, back] } },
/// "tracks": { "<id>": [segment's, ..] }, "shapes": { "<id>": m }, "fp_shapes": { "<REF>": [m | null, ..] } }`: each the constraint's `opt` (nm; a relative margin in
/// thousandths, as the rule holds it) or `null` when it has none, the clamps left to the page as KiCad applies them after (a pad on F.Mask / B.Mask and F.Paste /
/// B.Paste, a track or shape on its own layer; a footprint's graphics in its art's order, those not on outer copper `null`).
pub fn rule_margins(dir: &std::path::Path, intent: &str, design: &Design, model: &ConstraintModel) -> Value {
    use eda_drc::constraints::{CType, Units};
    use eda_drc::rule_item::Class;
    let Some(text) = model.board.custom_rules_text.as_deref() else { return Value::Null };
    let dru = crate::inspect_api::expanded_rules(dir, intent, design, model, text);
    let drc_board = eda_drc::board::build(design, model);
    let rb = eda_drc::rule_board::RuleBoard::new(design, model, &drc_board, Units::Mm, Some(&dru));
    let e = rb.engine();
    let (mask, abs, rel) = (e.has_rules_for(CType::SolderMaskExpansion), e.has_rules_for(CType::SolderPasteAbsMargin), e.has_rules_for(CType::SolderPasteRelMargin));
    if !(mask || abs || rel) {
        return Value::Null;
    }
    let opt = |t: CType, item: &std::rc::Rc<eda_drc::rule_item::RuleItem>, layer: &str| e.eval_rules(t, Some(item), None, Some(layer), None).value.opt;
    let outer = |l: &str| l == "F.Cu" || l == "B.Cu";
    let mut pads = serde_json::Map::new();
    for (pid, idx) in rb.pad_ids() {
        let Some(item) = rb.pad_item_at(*idx) else { continue };
        let side = |t: CType, on: bool, f: &str, b: &str| if on { json!([opt(t, &item, f), opt(t, &item, b)]) } else { Value::Null };
        pads.insert(
            pid.clone(),
            json!({ "mask": side(CType::SolderMaskExpansion, mask, "F.Mask", "B.Mask"), "abs": side(CType::SolderPasteAbsMargin, abs, "F.Paste", "B.Paste"), "rel": side(CType::SolderPasteRelMargin, rel, "F.Paste", "B.Paste") }),
        );
    }
    let mut tracks = serde_json::Map::new();
    let mut shapes = serde_json::Map::new();
    let mut fp_shapes = serde_json::Map::new();
    if mask {
        // `copper_items`: the pads, then every track segment (its track's id) in the board's order, then the vias
        for (id, item) in rb.copper_items() {
            if !matches!(item.class, Class::Track | Class::Arc) || !item.layer.as_deref().is_some_and(outer) {
                continue;
            }
            let m = opt(CType::SolderMaskExpansion, &item, item.layer.as_deref().unwrap_or("F.Cu"));
            if let Value::Array(list) = tracks.entry(id).or_insert_with(|| Value::Array(Vec::new())) {
                list.push(json!(m));
            }
        }
        for s in design.drawings.as_ref().map(|d| d.shapes.as_slice()).unwrap_or_default() {
            if outer(s.layer()) {
                // the shape's own `(solder_mask_margin ..)` (`ShapeStyle`), the engine's local override
                let mut it = (*rb.shape_item_of(s)).clone();
                it.overrides.solder_mask_margin = design.drawings.as_ref().and_then(|d| d.board_items.style_of(s.id())).and_then(|st| st.solder_mask_margin);
                shapes.insert(s.id().to_string(), json!(opt(CType::SolderMaskExpansion, &std::rc::Rc::new(it), s.layer())));
            }
        }
        for fp in design.placement.as_ref().map(|p| p.footprints.as_slice()).unwrap_or_default() {
            let Some(footprint) = model.part(&fp.id).and_then(|p| model.footprint_of(p)) else { continue };
            let parent = rb.footprint_item(&fp.id);
            let list: Vec<Value> = footprint
                .art
                .shapes
                .iter()
                .map(|s| {
                    let placed = s.placed(fp).shape;
                    if !outer(placed.layer()) {
                        return Value::Null;
                    }
                    let mut it = (*rb.shape_item_of(&placed)).clone();
                    // the graphic's own `(solder_mask_margin ..)`, the engine's local override
                    it.overrides.solder_mask_margin = s.mask_margin;
                    it.footprint = parent.as_ref().and_then(|p| p.footprint.clone());
                    it.parent = parent.clone();
                    json!(opt(CType::SolderMaskExpansion, &std::rc::Rc::new(it), placed.layer()))
                })
                .collect();
            if list.iter().any(|v| !v.is_null()) {
                fp_shapes.insert(fp.id.clone(), Value::Array(list));
            }
        }
    }
    json!({ "mask": mask, "paste_abs": abs, "paste_rel": rel, "pads": pads, "tracks": tracks, "shapes": shapes, "fp_shapes": fp_shapes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::footprint::{Pad, PadShape};
    use eda_model::ir::{LabelSide, Side};
    use eda_model::units::mm;

    fn instance(at: (Nm, Nm), rot: u32, side: Side) -> FootprintInstance {
        FootprintInstance { id: "H1".into(), at: Point { x: at.0, y: at.1 }, rot, side, label: LabelSide::Above }
    }

    fn hole_pad(kind: PadKind, drill: Option<Nm>, slot: Option<(Nm, Nm)>, rot: u32) -> Pad {
        Pad { number: "1".into(), at: (mm(1.0), Nm::ZERO), size: (mm(3.0), mm(3.0)), shape: PadShape::Circle, kind, drill, drill_slot: slot, rot, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
    }

    #[test]
    fn a_round_hole_is_at_the_pads_position_and_as_wide_as_the_drill() {
        let fp = instance((mm(10.0), mm(5.0)), 0, Side::Top);
        let (a, b, w) = pad_hole_segment(&fp, &hole_pad(PadKind::NonPlatedHole, Some(mm(1.2)), None, 0)).unwrap();
        assert_eq!((a, b, w), (Point64::new(11_000_000, 5_000_000), Point64::new(11_000_000, 5_000_000), 1_200_000));
        // On the bottom the footprint is mirrored in x: the pad at +1 mm is at -1 mm.
        let (a, _, _) = pad_hole_segment(&instance((mm(10.0), mm(5.0)), 0, Side::Bottom), &hole_pad(PadKind::NonPlatedHole, Some(mm(1.2)), None, 0)).unwrap();
        assert_eq!(a, Point64::new(9_000_000, 5_000_000));
        assert!(pad_hole_segment(&fp, &hole_pad(PadKind::Smd, None, None, 0)).is_none(), "no drill, no hole");
    }

    #[test]
    fn a_slot_lies_along_the_pad_turned_with_it_and_the_footprint() {
        // A 2 x 0.8 mm slot: 0.8 wide, its ends 0.6 mm either side of the centre along the pad's x.
        let slot = hole_pad(PadKind::ThroughHole, None, Some((mm(2.0), mm(0.8))), 0);
        let (a, b, w) = pad_hole_segment(&instance((Nm::ZERO, Nm::ZERO), 0, Side::Top), &slot).unwrap();
        assert_eq!((a, b, w), (Point64::new(400_000, 0), Point64::new(1_600_000, 0), 800_000));
        // Turned a quarter by the footprint: along y, about the pad's position (0, 1 mm).
        let (a, b, _) = pad_hole_segment(&instance((Nm::ZERO, Nm::ZERO), 90_000, Side::Top), &slot).unwrap();
        assert_eq!((a.x, b.x), (0, 0));
        assert_eq!(((a.y - 1_000_000).abs(), (b.y - 1_000_000).abs()), (600_000, 600_000));
    }

    #[test]
    fn the_body_loses_every_through_hole_grown_by_the_plating_and_every_non_plated_one() {
        let mut d = Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: eda_model::ir::Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        // A 40 x 30 mm board (Edge.Cuts rectangle), a through via and a blind one, and a footprint with a plated and a non-plated hole.
        d.drawings = Some(Default::default());
        let edge = eda_model::ir::Shape::Rect { id: "e".into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.1), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(40.0), y: mm(30.0) } };
        d.drawings.as_mut().unwrap().shapes.push(edge);
        let model = ConstraintModel::default();
        let outline = eda_drc::outline::board_outline_with(&d, Some(&model), true);
        assert_eq!(outline.polys.outline_count(), 1, "the rectangle is the outline");
        let full = outline.polys.area();
        let mut r: eda_model::ir::RoutingSection = serde_json::from_value(json!({ "tracks": [], "vias": [] })).unwrap();
        r.vias.push(eda_model::ir::Via { id: "v1".into(), net: "N".into(), at: Point { x: mm(10.0), y: mm(10.0) }, drill: mm(0.4), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        d.routing = Some(r);
        let body = board_body(&d, &model);
        assert_eq!((body.outline_count(), body.hole_count(0)), (1, 1), "one hole: the via's");
        // The hole is the drill plus 20 um of plating all round: radius 0.22 mm, the polygon inside it (ERROR_INSIDE): 16 sides at the 5 um error, the area of a
        // regular 16-gon in that circle, `8 r^2 sin( 22.5 deg )`.
        let hole = 8.0 * 220_000.0f64.powi(2) * (22.5f64).to_radians().sin();
        let cut = full - body.area();
        assert!((cut - hole).abs() < hole * 0.002, "{cut} vs {hole}");
        // A blind via cuts nothing from the body.
        let blind = eda_model::ir::Via { id: "v2".into(), net: "N".into(), at: Point { x: mm(20.0), y: mm(10.0) }, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "In1.Cu".into() };
        d.routing.as_mut().unwrap().vias.push(blind);
        d.drawings.as_mut().unwrap().via_extras.push(eda_model::via_extra::ViaExtra { id: "v2".into(), kind: eda_model::via_extra::ViaKind::Blind, ..Default::default() });
        assert_eq!(board_body(&d, &model).hole_count(0), 1, "still one hole");
        let json = board3d_json(&d, &model);
        assert_eq!(json["body"][0]["holes"].as_array().map(Vec::len), Some(1));
        assert_eq!(json["plating"].as_i64(), Some(20_000));
    }

    #[test]
    fn a_job_inflates_with_round_corners_and_subtracts() {
        // A 1 mm square grown by 0.1 mm with round corners: 1.2 x 1.2 less the four corners' squares plus their quarter circles.
        let sq = json!([{ "outer": [[0, 0], [1_000_000, 0], [1_000_000, 1_000_000], [0, 1_000_000]], "holes": [] }]);
        let hole = json!([{ "outer": [[400_000, 400_000], [600_000, 400_000], [600_000, 600_000], [400_000, 600_000]] }]);
        let body = json!({ "jobs": [{ "key": "k", "a": sq, "inflate": 100_000 }, { "key": "cut", "a": sq, "b": hole }] }).to_string();
        let out = polys_report(body.as_bytes());
        let poly = &out["results"]["k"][0]["outer"];
        let pts: Vec<(f64, f64)> = poly.as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
        let area = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum::<f64>().abs() / 2.0;
        let expect = 1.0e12 + 4.0 * 1.0e11 + std::f64::consts::PI * 1.0e10;
        assert!((area - expect).abs() < expect * 0.002, "{area} vs {expect}");
        assert_eq!(out["results"]["cut"][0]["holes"].as_array().map(Vec::len), Some(1), "the square less the hole has it as a hole");
    }

    /// UI-LEFT-3 D3-3: the custom rules' solder mask expansion and paste margins reach the 3D view (`PAD::GetSolderMaskExpansion`'s `EvalRules`), none without such a rule.
    #[test]
    fn the_custom_rules_margins_are_the_rule_engines() {
        use crate::inspect_api::tests::{scratch, setup};
        let dir = scratch("board3d_rules");
        setup(&dir, mm(1.0), eda_model::BoardRules { clearance: mm(0.2), ..Default::default() });
        let (meta, design, model) = crate::board::load(&dir).unwrap();
        assert!(rule_margins(&dir, &meta.intent, &design, &model).is_null(), "no rules file");
        let rules = "(version 1)\n(rule \"one\" (condition \"A.Pad_Number == '1'\") (constraint solder_mask_expansion (opt 0.2mm)))\n(rule \"paste\" (condition \"A.Type == 'Pad'\") (constraint solder_paste_abs_margin (opt -0.03mm)))\n";
        let mut model = model;
        model.board.custom_rules_text = Some(rules.into());
        let m = rule_margins(&dir, &meta.intent, &design, &model);
        assert_eq!((m["mask"].as_bool(), m["paste_abs"].as_bool(), m["paste_rel"].as_bool()), (Some(true), Some(true), Some(false)), "{m}");
        assert_eq!(m["pads"]["U1.1"]["mask"], json!([200_000, 200_000]), "pad 1 takes the rule on both masks: {m}");
        // pad 2 matches no mask rule: the engine falls back to the board's expansion (0 here)
        assert_eq!(m["pads"]["U1.2"]["mask"], json!([0, 0]), "{m}");
        assert_eq!(m["pads"]["U1.2"]["abs"], json!([-30_000, -30_000]));
        assert!(m["pads"]["U1.2"]["rel"].is_null(), "no relative rule");
    }
}
