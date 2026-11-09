//! The board's Draw Via tool (`pcbnew/tools/drawing_tool.cpp::DRAWING_TOOL::DrawVia`, its `VIA_PLACER`), the part that changes the board: `PlaceItem` after the
//! net was chosen (the studio's `kicad-port/viaPlacer.ts` ports `SnapItem` and the net choice of `PlaceItem`, which need the canvas's view: what is under the
//! cursor, the zone fills, the visible layers, the "Select Net:" menu).
//!
//! - The net may be none (`NETINFO_LIST::ORPHANED`): a free via where nothing is under it. (`SetIsFree( GetNetCode() > 0 )` for a via that took a zone's net
//!   is not modelled: the IR's via has no `free` flag, so the file gets no `(free yes)`.)
//! - `checkDRCViolation`, refused with KiCad's "Via location violates DRC." unless the router allows DRC violations (`m_allowDRCViolations`), when it is placed
//!   and the message still said (the reply's `drc_violation`): [`via_violates_drc`].
//! - The track the via landed on is split there (`track->SetEnd( viaPos ); newTrack->SetStart( viaPos )`), unless the via is on one of its ends or off its line
//!   (`trackSeg.Contains( viaPos )`). The studio's track is a polyline, a `PCB_TRACK` per segment: the polyline is cut at the via inside the segment that
//!   contains it, the first part keeping the track's id and the second a new one (`Clone()`, `ResetUuidDirect()`), as locked as the first. An arc track is not
//!   cut: KiCad's `SetEnd` on a `PCB_ARC` keeps its mid and bends it, and the via sits on its chord, not on it.
//! - One undo step for the via and the split (`doInteractiveItemPlacement`'s `commit.Push( "Place via" )`).

use super::Board;
use eda_drc::board::DrcBoard;
use eda_drc::constraints::CType;
use eda_drc::kimath::Shape;
use eda_drc::rule_board::RuleBoard;
use eda_model::ir::{Design, Point, Track, Via};
use eda_model::units::Nm;
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeSet;
use std::rc::Rc;

/// `PlaceItem`'s infobar message, word for word.
pub const VIA_VIOLATES_DRC: &str = "Via location violates DRC.";

/// `GetDRCEpsilon()`: `ADVANCED_CFG::m_DRCEpsilon`, 0.5 um.
const DRC_EPSILON: Nm = Nm::new(500);

/// `sub_e( aClearance )`: the clearance less the DRC epsilon, never below zero.
fn sub_e(clearance: Nm) -> Nm {
    (clearance - DRC_EPSILON).max(Nm::ZERO)
}

/// `VIA_PLACER::checkDRCViolation` for `via`, as it would sit on the board: whether it breaks a clearance or a hole clearance to an item of another net, or
/// sits in a rule area that keeps vias out, or a `disallow` rule takes it (`DISALLOW_CONSTRAINT`, unless its rule's severity is `ignore`).
///
/// `hasDRCViolation` per item:
/// - a rule area with `GetDoNotAllowVias` hits when its outline (`SHAPE_POLY_SET::Collide`: inside it and not in a hole, or within the via's radius of
///   an edge, a hole's included) meets the via's circle; a copper zone is skipped ("stitching vias bind to zones");
/// - a connected item (`BOARD_CONNECTED_ITEM`: a pad, track, via, a graphic, and a text box, which is a `PCB_SHAPE`) of no net or of the via's net never
///   collides -- so a text box, whose net is always none, never refuses a via (UI-LEFT-2 PP12: the brief named copper text boxes among the items the check
///   misses; KiCad's `connectedItemNet == 0` leaves them out, and here too);
/// - otherwise, on each copper layer of the item's layer set, the via's copper there (`aVia->GetEffectiveShape( layer )`: its circle on a layer it spans,
///   its drill's off its span) against the item's (`aOther->GetEffectiveShape( layer )`) at `EvalRules( CLEARANCE_CONSTRAINT, .., layer )` less the
///   epsilon, then the via's widest copper against the item's hole at `HOLE_CLEARANCE_CONSTRAINT`.
///
/// The item's copper on a layer: a pad's shape there, or -- for a plated pad on a layer it does not flash (`PAD::FlashLayer`: its padstack's unconnected
/// layers, `IsConnectedOnLayer` for the conditional ones) -- its hole (`GetEffectiveHoleShape`, round 6 C6-4); a via's circle of that layer's own
/// diameter (`PCB_VIA::GetWidth( layer )`, a padstack that is not a normal one), or its drill's on a layer it does not flash; a track's, a graphic's;
/// a board text's and a footprint text's strokes (a knockout text: its bounding rectangle, R7-4); a table's outline and its borders
/// (`PCB_TABLE::GetEffectiveShape`); a dimension's label strokes and its lines and circles (`PCB_DIMENSION_BASE::GetEffectiveShape`: zero-width
/// segments, solid circles); a barcode's box and its printed text's box (round 6 C6-5). Not judged: a pad's own clearance override beyond what the rules
/// give; a backdrilled or post-machined layer's larger hole (`IsBackdrilledOrPostMachined`); a footprint's own tables and dimensions. **Differs:** a
/// barcode is its symbol's box (`m_poly`, the symbol's modules, is the zint library's: not modelled here).
///
/// `hidden_layers` are the layers the Appearance panel hides: `checkDRCViolation` takes its candidates from `view->Query`, which skips the view layers that are
/// not shown (round 6 C6-1), so an item none of whose layers is shown is not judged. A via (its hole layers) and a plated pad (the plated-hole layers) are
/// always shown; a track, an SMD or NPTH pad, a graphic, a text, a table, a dimension and a barcode by their copper layers.
pub fn via_violates_drc(design: &Design, model: &ConstraintModel, via: &Via, hidden_layers: &[String]) -> bool {
    let shown = |layers: &[&str]| layers.iter().any(|l| !hidden_layers.iter().any(|h| h == l));
    let board: DrcBoard = eda_drc::board::build(design, model);
    let rules = &model.board;
    let rb = RuleBoard::new(design, model, &board, eda_drc::constraints::Units::Mm, rules.custom_rules_text.as_deref());
    let probe = rb.probe_via(&via.net, via.at, via.diameter, via.drill, &via.from_layer, &via.to_layer);
    let eval = |t: CType, other: &Rc<eda_drc::rule_item::RuleItem>, layer: Option<&str>| -> Nm { Nm::new(rb.engine().eval_rules(t, Some(&probe), Some(other), layer, None).value.min()) };
    let copper: Vec<String> = rules.layers.iter().filter(|l| l.ends_with(".Cu")).cloned().collect();
    let via_span = layer_span(&copper, &via.from_layer, &via.to_layer);
    // `aVia->GetEffectiveShape( UNDEFINED_LAYER )`: the new via is a normal padstack (`CreateItem`: `SetWidth( PADSTACK::ALL_LAYERS, .. )`)
    let via_shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
    // `aVia->GetEffectiveShape( layer )`: `FlashLayer( layer )` is false off its span (`!IsOnLayer( layer )`), where it is its drill
    let via_shape_on = |layer: &str| if via_span.iter().any(|l| l == layer) { via_shape.clone() } else { Shape::Circle { c: via.at, r: via.drill / 2 } };
    let other_net = |net: &Option<String>| net.as_deref().is_some_and(|n| !n.is_empty() && n != via.net);
    let collides_on = |layer: &str, other: &Shape, clearance: Nm| clearance >= Nm::ZERO && via_shape_on(layer).collides(other, sub_e(clearance)).is_some();
    let collides = |other: &Shape, clearance: Nm| clearance >= Nm::ZERO && via_shape.collides(other, sub_e(clearance)).is_some();
    // `PAD::FlashLayer` / `PCB_VIA::FlashLayer` of the pads and vias that remove their unconnected layers (`eda_connectivity::flash`)
    let flashed = eda_connectivity::flash::removes_any_layer(design, model).then(|| eda_connectivity::flash::flashed_layers(design, model));
    let flashes = |kept: Option<&Vec<String>>, layer: &str| kept.is_none_or(|ls| ls.iter().any(|l| l == layer));

    // Rule areas that keep vias out (`zone->GetDoNotAllowVias()`: `zone->Outline()->Collide( aVia->GetPosition(), aVia->GetWidth( aLayer ) / 2 )`).
    for z in design.routing.iter().flat_map(|rt| rt.zones.iter()) {
        if z.is_rule_area && z.keepout_vias && z.outline.len() >= 3 && outline_collides(&z.outline, &z.holes, via.at, via.diameter / 2) {
            return true;
        }
    }

    // Pads: each copper layer the pad has copper on (its shape, or its hole where a plated pad does not flash), then its hole.
    let (first, last) = (copper.first().cloned().unwrap_or_default(), copper.last().cloned().unwrap_or_default());
    for (i, pad) in board.pads.iter().enumerate() {
        if !other_net(&pad.net) {
            continue;
        }
        let plated_hole = pad.hole.is_some() && pad.kind != eda_model::PadKind::NonPlatedHole;
        if !plated_hole && !shown(&copper.iter().filter(|l| pad.copper_on(l).is_some()).map(String::as_str).collect::<Vec<_>>()) {
            continue;
        }
        let Some(item) = rb.pad_item_at(i) else { continue };
        let kept = flashed.as_ref().and_then(|f| f.pads.get(&pad.id));
        for layer in &copper {
            let Some(shape) = pad.copper_on(layer) else { continue };
            let flash = match pad.flash(layer, &first, &last) {
                eda_model::padstack::Flash::Always => true,
                eda_model::padstack::Flash::Never => false,
                eda_model::padstack::Flash::IfConnected => flashes(kept, layer),
            };
            // `PAD::GetEffectiveShape( layer )`: "if( !flash ) { if( GetAttribute() == PAD_ATTRIB::PTH ) AddShape( GetEffectiveHoleShape() ) .. }"
            let shape = match (flash, plated_hole, &pad.hole) {
                (true, _, _) => shape,
                (false, true, Some(hole)) => hole,
                _ => continue,
            };
            if collides_on(layer, shape, eval(CType::Clearance, &item, Some(layer))) {
                return true;
            }
        }
        if let Some(hole) = &pad.hole {
            if collides(hole, eval(CType::HoleClearance, &item, None)) {
                return true;
            }
        }
    }
    // Tracks (a segment of a polyline, or an arc).
    for (i, t) in board.tracks.iter().enumerate() {
        if !other_net(&t.net) || !shown(&[t.layer.as_str()]) {
            continue;
        }
        let item = rb.segment_item_at(i);
        if collides_on(&t.layer, &t.shape(), eval(CType::Clearance, &item, Some(&t.layer))) {
            return true;
        }
    }
    // Vias: each copper layer of their span (the layer's own diameter, or the drill where they do not flash), then their hole.
    for (i, v) in board.vias.iter().enumerate() {
        if !other_net(&v.net) {
            continue;
        }
        let item = rb.via_item_at(i);
        let kept = flashed.as_ref().and_then(|f| f.vias.get(&v.id));
        for layer in &layer_span(&copper, &v.from_layer, &v.to_layer) {
            let shape = if flashes(kept, layer) { v.shape_on(layer) } else { v.hole() };
            if collides_on(layer, &shape, eval(CType::Clearance, &item, Some(layer))) {
                return true;
            }
        }
        if collides(&v.hole(), eval(CType::HoleClearance, &item, None)) {
            return true;
        }
    }
    // Graphics on copper with a net of their own.
    for s in design.drawings.iter().flat_map(|d| d.shapes.iter()) {
        if !copper.iter().any(|l| l == s.layer()) || !shown(&[s.layer()]) {
            continue;
        }
        let net = design.drawings.as_ref().and_then(|d| d.board_items.net_of(s.id())).map(str::to_string);
        if !other_net(&net) {
            continue;
        }
        let item = rb.shape_item_of(s);
        let clearance = eval(CType::Clearance, &item, Some(s.layer()));
        if eda_drc::board::effective_shapes(s).iter().any(|part| collides_on(s.layer(), part, clearance)) {
            return true;
        }
    }
    // Text on copper: the board's own and the footprints'.
    for t in design.drawings.iter().flat_map(|d| d.texts.iter()).filter(|t| copper.contains(&t.layer) && shown(&[t.layer.as_str()])) {
        let item = rb.board_text_item_of(t);
        let extra = design.drawings.as_ref().and_then(|d| d.text_extra(&t.id));
        if board_text_effective_shape(t, extra).is_some_and(|shape| collides_on(&t.layer, &shape, eval(CType::Clearance, &item, Some(&t.layer)))) {
            return true;
        }
    }
    for piece in eda_drc::fp_art::footprint_copper(design, model) {
        if let eda_drc::fp_art::FpCopper::Text(t) = &piece {
            if !shown(&[t.layer.as_str()]) {
                continue;
            }
            let item = rb.text_item_of("fp_text", t);
            if let Some(shape) = eda_drc::board::footprint_text_shape(t) {
                if collides_on(&t.layer, &shape, eval(CType::Clearance, &item, Some(&t.layer))) {
                    return true;
                }
            }
        }
    }
    // Tables, dimensions and barcodes on copper: items of no net (`PCB_TABLE`, `PCB_DIMENSION_BASE`, `PCB_BARCODE` are no `BOARD_CONNECTED_ITEM`).
    if let Some(d) = design.drawings.as_ref() {
        let judged = |id: &str, layer: &str, class: eda_drc::rule_item::Class, type_name: &str, shapes: &[Shape]| -> bool {
            if !copper.iter().any(|l| l == layer) || !shown(&[layer]) || shapes.is_empty() {
                return false;
            }
            let item = Rc::new(other_item(id, layer, class, type_name));
            let clearance = eval(CType::Clearance, &item, Some(layer));
            shapes.iter().any(|shape| collides_on(layer, shape, clearance))
        };
        for t in &d.tables {
            if judged(&t.id, &t.layer, eda_drc::rule_item::Class::Shape, "Table", &[table_effective_shape(t)]) {
                return true;
            }
        }
        for dim in &d.dimensions {
            let class = if matches!(dim.kind, eda_model::ir::DimensionKind::Leader) { eda_drc::rule_item::Class::Leader } else { eda_drc::rule_item::Class::Dimension };
            if judged(&dim.id, &dim.layer, class, class.type_name(), &dimension_effective_shapes(dim)) {
                return true;
            }
        }
        for b in &d.barcodes {
            if judged(&b.id, &b.layer, eda_drc::rule_item::Class::Shape, "Barcode", &barcode_effective_shapes(b)) {
                return true;
            }
        }
    }

    // `EvalRules( DISALLOW_CONSTRAINT, aVia, nullptr, UNDEFINED_LAYER )`: `m_DisallowFlags && constraint.GetSeverity() != RPT_SEVERITY_IGNORE` (the severity
    // is the rule's, `m_parentRule->m_Severity`).
    let disallow = rb.engine().eval_rules(CType::Disallow, Some(&probe), None, None, None);
    disallow.disallow != 0 && disallow.parent.and_then(|i| rb.engine().rules.get(i)).is_none_or(|r| r.severity != eda_drc::constraints::Severity::Ignore)
}

/// `SHAPE_POLY_SET::Collide( aPoint, aClearance )` of a zone's outline with its holes: the point is inside the outline and in no hole (`Contains`), or
/// within `r` of an edge of the outline or of a hole.
fn outline_collides(outline: &[Point], holes: &[Vec<Point>], at: Point, r: Nm) -> bool {
    let point = Shape::Circle { c: at, r: Nm::ZERO };
    let ring = |pts: &[Point]| Shape::Strokes { segs: (0..pts.len()).map(|k| eda_drc::kimath::Seg::new(pts[k], pts[(k + 1) % pts.len()])).collect(), r: Nm::ZERO };
    let near_edge = std::iter::once(outline).chain(holes.iter().map(Vec::as_slice)).filter(|c| c.len() >= 2).any(|c| ring(c).collides(&point, r).is_some());
    if near_edge {
        return true;
    }
    let inside = |pts: &[Point]| pts.len() >= 3 && Shape::Polygon { pts: pts.to_vec() }.collides(&point, Nm::ZERO).is_some();
    inside(outline) && !holes.iter().any(|h| inside(h))
}

/// A rule item of a table, dimension or barcode on copper, as the rules see it: its kind (`A.Type`), its layer, no net.
fn other_item(id: &str, layer: &str, class: eda_drc::rule_item::Class, type_name: &str) -> eda_drc::rule_item::RuleItem {
    let mut it = eda_drc::rule_item::RuleItem::new(id.to_string(), class);
    it.type_name = Some(type_name.to_string());
    it.layers = vec![layer.to_string()];
    it.layer = Some(layer.to_string());
    it.set("Layer", eda_drc::rule_item::Prop::Layer(layer.to_string()));
    it
}

/// `PCB_TABLE::GetEffectiveShape`: the polygon of the table's outer corners and, for the borders it draws, their strokes (`DrawBorders`'s segments of the
/// stroke's width). The outline's sides stand for every border here, at the widest stroke the table draws (an inner separator lies inside the polygon).
fn table_effective_shape(t: &eda_model::table::Table) -> Shape {
    let (w, h) = t.local_size();
    let corners: Vec<Point> = [(Nm::ZERO, Nm::ZERO), (w, Nm::ZERO), (w, h), (Nm::ZERO, h)].iter().map(|&c| t.to_world(c)).collect();
    let sep = if t.separators.rows || t.separators.cols { t.separators.stroke.width_nm / 2 } else { Nm::ZERO };
    let border = if t.border.external || t.border.header { t.border.stroke.width_nm / 2 } else { Nm::ZERO };
    let gap = sep.max(border);
    if gap <= Nm::ZERO {
        return Shape::Polygon { pts: corners };
    }
    let ring: Vec<eda_drc::kimath::Seg> = (0..4).map(|k| eda_drc::kimath::Seg::new(corners[k], corners[(k + 1) % 4])).collect();
    Shape::Compound { parts: vec![Shape::Polygon { pts: corners }, Shape::Strokes { segs: ring, r: gap }] }
}

/// `PCB_DIMENSION_BASE::GetEffectiveShape`: `GetEffectiveTextShape()` (the label's strokes, its pen wide) and every shape of `GetShapes()` -- the lines as
/// `SHAPE_SEGMENT`s of no width, a leader's circle frame as a solid `SHAPE_CIRCLE`.
fn dimension_effective_shapes(dim: &eda_model::ir::Dimension) -> Vec<Shape> {
    let g = eda_connectivity::dimension::compute_dimension_geometry(dim);
    let mut out: Vec<Shape> = g.lines.iter().map(|&(a, b)| Shape::Stadium { a, b, r: Nm::ZERO }).collect();
    out.extend(g.circles.iter().map(|&(c, r)| Shape::Circle { c, r }));
    if !g.text.is_empty() {
        let size = (dim.text_size_nm, dim.text_size_nm);
        let thickness = dim.text_thickness_nm.unwrap_or_else(|| Nm::kiround(dim.text_size_nm.as_f64() * 0.15));
        let pen = eda_drc::stroke_font::effective_pen_width(thickness, size, false);
        let rot = (g.text_angle as f64 / 1000.0).rem_euclid(360.0);
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &g.text, pos: g.text_at, size, thickness, angle_deg: rot, mirror: false, halign: 0, valign: 0 });
        if !segs.is_empty() {
            out.push(Shape::Strokes { segs, r: pen / 2 });
        }
    }
    out
}

/// `PCB_BARCODE::GetEffectiveShape`, as far as the model knows the barcode: the box of its symbol (the barcode's width and height about its centre, turned
/// by its angle) and, when the text is shown, the box of the printed text under it.
fn barcode_effective_shapes(b: &eda_model::barcode::Barcode) -> Vec<Shape> {
    let (hw, hh) = (b.width_nm.as_f64() / 2.0, b.height_nm.as_f64() / 2.0);
    // `RotatePoint( corner, m_pos, m_angle )`: counter-clockwise on the screen, y down
    let (sin, cos) = (b.angle as f64 / 1000.0).to_radians().sin_cos();
    let corner = |x: f64, y: f64| Point { x: b.at.x + Nm::kiround(x * cos + y * sin), y: b.at.y + Nm::kiround(-x * sin + y * cos) };
    let mut out = vec![Shape::Polygon { pts: vec![corner(-hw, -hh), corner(hw, -hh), corner(hw, hh), corner(-hw, hh)] }];
    if b.show_text && !b.text.is_empty() {
        let h = b.text_height_nm.max(Nm::new(1));
        let pen = Nm::kiround(h.as_f64() / 8.0).max(Nm::new(1));
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &b.text, pos: Point { x: Nm::ZERO, y: Nm::ZERO }, size: (h, h), thickness: pen, angle_deg: 0.0, mirror: false, halign: 0, valign: 0 });
        let r = pen / 2;
        if let (Some(x0), Some(x1), Some(y0), Some(y1)) = (segs.iter().flat_map(|s| [s.a.x, s.b.x]).min(), segs.iter().flat_map(|s| [s.a.x, s.b.x]).max(), segs.iter().flat_map(|s| [s.a.y, s.b.y]).min(), segs.iter().flat_map(|s| [s.a.y, s.b.y]).max()) {
            let (tw, th) = ((x1 - x0 + r * 2).as_f64(), (y1 - y0 + r * 2).as_f64());
            // centred under the symbol, its top 1 mm below the symbol's bottom (`ComputeTextPoly`)
            let top = hh + 1_000_000.0;
            out.push(Shape::Polygon { pts: vec![corner(-tw / 2.0, top), corner(tw / 2.0, top), corner(tw / 2.0, top + th), corner(-tw / 2.0, top + th)] });
        }
    }
    out
}

/// `PCB_TEXT::GetEffectiveShape` of a board text, with what its extras say (round 6 C6-3): the glyph width (`width_nm`, else square), the vertical
/// justification, bold (the automatic pen of a text with no thickness), and for a knockout text `TransformTextToPolySet`'s
/// `buildBoundingHull( &finalPoly, textShape, margin )` (R7-4): the strokes turned back by the draw rotation about the text's position
/// (`poly.Rotate( -GetDrawRotation(), GetDrawPos() )`), their box (`BBox( aClearance )`) grown by `GetKnockoutTextMargin( size, penWidth )` =
/// `max( KiROUND( penWidth / 2.0 ), KiROUND( size.y / 9.0 ) )`, its four corners turned forward again (`RotatePoint( corner, GetDrawPos(),
/// GetDrawRotation() )`). The glyphs it then subtracts are inside the rectangle and no via fits in one.
pub(crate) fn board_text_effective_shape(t: &eda_model::ir::Text, extra: Option<&eda_model::board_text::TextExtra>) -> Option<Shape> {
    let default = eda_model::board_text::TextExtra::default();
    let x = extra.unwrap_or(&default);
    let size = (x.width_nm.unwrap_or(t.size_nm), t.size_nm);
    let thickness = if x.auto_thickness { Nm::ZERO } else { t.stroke_width };
    let pen = eda_drc::stroke_font::effective_pen_width(thickness, size, x.bold);
    let halign = match t.justify {
        eda_model::ir::TextJustify::Left => -1,
        eda_model::ir::TextJustify::Center => 0,
        eda_model::ir::TextJustify::Right => 1,
    };
    let valign = match x.valign {
        eda_model::ir::TextVAlign::Top => -1,
        eda_model::ir::TextVAlign::Center => 0,
        eda_model::ir::TextVAlign::Bottom => 1,
    };
    let rot = (t.angle as f64 / 1000.0).rem_euclid(360.0);
    let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &t.content, pos: t.at, size, thickness, angle_deg: rot, mirror: t.mirror, halign, valign });
    if segs.is_empty() {
        return None;
    }
    if !x.knockout {
        return Some(Shape::Strokes { segs, r: pen / 2 });
    }
    let margin = Nm::kiround(pen.as_f64() / 2.0).max(Nm::kiround(size.1.as_f64() / 9.0));
    // `RotatePoint( p, centre, angle )` with y down: x' = x cos + y sin, y' = -x sin + y cos (relative to the centre)
    let (sin, cos) = rot.to_radians().sin_cos();
    let turn = |p: (f64, f64), s: f64| (p.0 * cos + p.1 * s, -p.0 * s + p.1 * cos);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in segs.iter().flat_map(|s| [s.a, s.b]) {
        let q = turn(((p.x - t.at.x).as_f64(), (p.y - t.at.y).as_f64()), -sin);
        (x0, y0, x1, y1) = (x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1));
    }
    // the rendered text's own box is its strokes' with half the pen, then the margin
    let grow = (pen.get() / 2 + margin.get()) as f64;
    let pts = [(x0 - grow, y0 - grow), (x1 + grow, y0 - grow), (x1 + grow, y1 + grow), (x0 - grow, y1 + grow)]
        .into_iter()
        .map(|c| {
            let q = turn(c, sin);
            Point { x: t.at.x + Nm::kiround(q.0), y: t.at.y + Nm::kiround(q.1) }
        })
        .collect();
    Some(Shape::Polygon { pts })
}

/// The copper layers from `a` to `b`, either way round (all of them when one is unknown).
fn layer_span(copper: &[String], a: &str, b: &str) -> Vec<String> {
    match (copper.iter().position(|l| l == a), copper.iter().position(|l| l == b)) {
        (Some(x), Some(y)) => copper[x.min(y)..=x.max(y)].to_vec(),
        _ => copper.to_vec(),
    }
}

/// `SEG::Contains( aP )`: `SquaredDistance( aP ) <= 3`.
fn seg_contains(a: Point, b: Point, p: Point) -> bool {
    let (ax, ay, bx, by, px, py) = (a.x.get() as f64, a.y.get() as f64, b.x.get() as f64, b.y.get() as f64, p.x.get() as f64, p.y.get() as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (ax + t * dx - px, ay + t * dy - py);
    qx * qx + qy * qy <= 3.0
}

/// Where `track`'s polyline is cut by a via at `at`: the index of the segment that contains it, unless the via is on an end of that segment (a joint).
fn split_index(track: &Track, at: Point) -> Option<usize> {
    if track.arc().is_some() {
        return None;
    }
    let k = track.pts.windows(2).position(|w| seg_contains(w[0], w[1], at))?;
    (track.pts[k] != at && track.pts[k + 1] != at).then_some(k)
}

impl Board<'_> {
    /// `Cmd::PlaceVia`: `VIA_PLACER::PlaceItem` with the net chosen ([`self`](crate::via_place) module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place_via(&mut self, net: &str, x: Nm, y: Nm, drill: Nm, diameter: Nm, from_layer: &str, to_layer: &str, split_track: Option<&str>, allow_drc_violations: bool, hidden_layers: &[String]) -> Result<(), Vec<CheckResult>> {
        if !net.is_empty() {
            self.known_net(net)?;
        }
        self.known_layer(from_layer)?;
        self.known_layer(to_layer)?;
        if from_layer == to_layer {
            return Err(vec![CheckResult::fail("ops_bad_via", net, "a via needs two different layers to span")]);
        }
        if drill <= Nm::ZERO || diameter <= Nm::ZERO || drill >= diameter {
            return Err(vec![CheckResult::fail("ops_bad_via", net, "via drill and diameter must be positive, the drill smaller")]);
        }
        let at = Point { x, y };
        let via = Via { id: String::new(), net: net.into(), at, drill, diameter, from_layer: from_layer.into(), to_layer: to_layer.into() };
        // `PlaceItem`: `if( checkDRCViolation( via ) ) { ShowInfoBarError( _( "Via location violates DRC." ), true, DRC_VIOLATION ); if( !m_allowDRCViolations )
        // return false; }` -- the message shows whether or not the via goes down (round 6 C6-6): refused, or placed with it said
        // ([`crate::notice::note_drc_violation`]); a clean placement dismisses a standing one (the studio's toast, `infobar->Dismiss()`).
        if via_violates_drc(&self.design, self.model, &via, hidden_layers) {
            if !allow_drc_violations {
                return Err(vec![CheckResult::fail("ops_via_drc", net, VIA_VIOLATES_DRC)]);
            }
            crate::notice::note_drc_violation(VIA_VIOLATES_DRC);
        }
        // The split, before the via goes in: the track must be the one under the via, of its net.
        let split = match split_track {
            Some(id) => {
                let t = self.design.routing.as_ref().and_then(|rt| rt.tracks.iter().find(|t| t.id == id)).ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "no track with this id")])?;
                split_index(t, at).map(|k| (t.clone(), k))
            }
            None => None,
        };
        let locked = split.as_ref().is_some_and(|(t, _)| self.design.drawings.as_ref().is_some_and(|d| d.locked_ids.iter().any(|l| *l == t.id)));
        let rt = self.routing_mut();
        rt.vias.push(via);
        let mut new_track_seed = None;
        if let Some((t, k)) = split {
            let mut first: Vec<Point> = t.pts[..=k].to_vec();
            first.push(at);
            let mut second = vec![at];
            second.extend_from_slice(&t.pts[k + 1..]);
            if let Some(orig) = rt.tracks.iter_mut().find(|x| x.id == t.id) {
                orig.pts = first;
                orig.pins.clear();
            }
            let tail = Track { id: String::new(), net: t.net.clone(), pins: Vec::new(), layer: t.layer.clone(), width: t.width, pts: second.clone(), arc_mid_offset: None };
            new_track_seed = Some(second);
            rt.tracks.push(tail);
        }
        rt.assign_missing_ids();
        let tail_id = new_track_seed.and_then(|pts| rt.tracks.iter().find(|x| x.pts == pts).map(|x| x.id.clone()));
        self.sort_routing();
        if locked {
            if let Some(id) = tail_id {
                let dr = self.drawings_mut();
                let mut set: BTreeSet<String> = dr.locked_ids.iter().cloned().collect();
                set.insert(id);
                dr.locked_ids = set.into_iter().collect();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cmd;
    use eda_model::ir::{PlacementSection, Provenance, Zone};
    use eda_model::units::mm;
    use eda_model::Net;

    fn model() -> ConstraintModel {
        ConstraintModel { nets: vec![Net { name: "GND".into(), pins: vec![] }, Net { name: "VCC".into(), pins: vec![] }], ..Default::default() }
    }

    fn design() -> Design {
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None, routing: None, drawings: None,
            placement: Some(PlacementSection { outline: vec![p(0.0, 0.0), p(50.0, 0.0), p(50.0, 50.0), p(0.0, 50.0)], footprints: Vec::new(), modules: Vec::new() }),
        }
    }

    fn place(net: &str, x: f64, y: f64, split: Option<String>, allow: bool) -> Cmd {
        Cmd::PlaceVia { net: net.into(), x: mm(x), y: mm(y), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), split_track: split, allow_drc_violations: allow, hidden_layers: Vec::new() }
    }

    fn track(net: &str, a: (f64, f64), b: (f64, f64)) -> Cmd {
        Cmd::AddTrack { net: net.into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(a.0), y: mm(a.1) }, Point { x: mm(b.0), y: mm(b.1) }] }
    }

    #[test]
    fn a_via_where_nothing_is_is_a_via_of_no_net() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&place("", 10.0, 10.0, None, false)).unwrap();
        let v = &b.design().routing.as_ref().unwrap().vias[0];
        assert_eq!((v.net.as_str(), v.at), ("", Point { x: mm(10.0), y: mm(10.0) }));
        assert_eq!(b.apply(&place("NOPE", 20.0, 10.0, None, false)).unwrap_err()[0].check, "ops_unknown_net");
    }

    #[test]
    fn a_via_on_the_middle_of_a_track_cuts_it_there() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&track("GND", (0.0, 5.0), (10.0, 5.0))).unwrap();
        let id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
        b.apply(&place("GND", 4.0, 5.0, Some(id.clone()), false)).unwrap();
        let rt = b.design().routing.as_ref().unwrap();
        assert_eq!(rt.vias.len(), 1);
        assert_eq!(rt.tracks.len(), 2, "{:?}", rt.tracks);
        let first = rt.tracks.iter().find(|t| t.id == id).expect("the first part keeps the track's id");
        assert_eq!(first.pts, vec![Point { x: mm(0.0), y: mm(5.0) }, Point { x: mm(4.0), y: mm(5.0) }]);
        let second = rt.tracks.iter().find(|t| t.id != id).unwrap();
        assert_eq!(second.pts, vec![Point { x: mm(4.0), y: mm(5.0) }, Point { x: mm(10.0), y: mm(5.0) }]);
        // On an end: no cut.
        b.apply(&place("GND", 10.0, 5.0, Some(second.id.clone()), false)).unwrap();
        assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 2);
    }

    #[test]
    fn a_via_too_near_another_nets_copper_is_refused_with_kicads_message() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&track("VCC", (0.0, 5.0), (10.0, 5.0))).unwrap();
        // 0.3 mm via radius + 0.1 mm half track: 0.45 mm from the track's axis leaves 0.05 mm, less than the default 0.2 mm clearance.
        let e = b.apply(&place("GND", 5.0, 5.45, None, false)).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        assert!(b.design().routing.as_ref().unwrap().vias.is_empty());
        // The router's "Allow DRC violations": placed anyway.
        b.apply(&place("GND", 5.0, 5.45, None, true)).unwrap();
        // A via of the track's own net, or of no net next to it, or far enough: no violation.
        b.apply(&place("VCC", 7.0, 5.45, None, false)).unwrap();
        b.apply(&place("GND", 2.0, 6.0, None, false)).unwrap();
    }

    /// Round 6 C6-1: `checkDRCViolation` takes its candidates from `view->Query`, which skips hidden layers.
    #[test]
    fn copper_on_a_hidden_layer_does_not_refuse_the_via() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&track("VCC", (0.0, 5.0), (10.0, 5.0))).unwrap();
        assert!(b.apply(&place("GND", 5.0, 5.45, None, false)).is_err());
        let Cmd::PlaceVia { net, x, y, drill, diameter, from_layer, to_layer, split_track, allow_drc_violations, .. } = place("GND", 5.0, 5.45, None, false) else { unreachable!() };
        b.apply(&Cmd::PlaceVia { net, x, y, drill, diameter, from_layer, to_layer, split_track, allow_drc_violations, hidden_layers: vec!["F.Cu".into()] }).unwrap();
    }

    /// Round 6 C6-3: a board text is judged by `GetEffectiveShape` with its own attributes: the Text tool's bottom-justified text sits above its anchor, and a
    /// knockout text is its bounding rectangle (R7-4: `buildBoundingHull`).
    #[test]
    fn a_bottom_justified_text_is_judged_above_its_anchor_and_a_knockout_by_its_rectangle() {
        use eda_model::board_text::TextExtra;
        use eda_model::ir::{Text, TextJustify, TextVAlign};
        let m = model();
        let text = Text { id: String::new(), content: "GND".into(), at: Point { x: mm(10.0), y: mm(10.0) }, angle: 0, layer: "F.Cu".into(), size_nm: mm(1.5), stroke_width: mm(0.3), justify: TextJustify::Left, mirror: false };
        let with_text = |text: &Text, extra: TextExtra| {
            let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
            b.apply(&Cmd::PlaceText { text: text.clone(), extra: Some(extra) }).unwrap();
            b
        };
        let with = |extra: TextExtra| with_text(&text, extra);
        let bottom = TextExtra { bold: true, valign: TextVAlign::Bottom, ..Default::default() };
        // The via's edge 0.7 mm below the anchor: clear of the glyphs, which end 0.2 mm above it (plus half the pen and the 0.2 mm clearance); the same
        // text centred on its anchor reaches 0.68 mm below it.
        with(bottom.clone()).apply(&place("GND", 10.5, 11.0, None, false)).unwrap();
        assert!(with(TextExtra::default()).apply(&place("GND", 10.5, 11.0, None, false)).is_err(), "the same text centred on its anchor is too near");
        // A 3 mm text: its strokes end 0.23 mm above the anchor (half the pen included), a knockout's hull, grown by max( pen / 2, 3 mm / 9 ), 0.11 mm
        // below it. The via's edge on the anchor: clear of the strokes by more than the clearance, inside the hull.
        let big = Text { size_nm: mm(3.0), ..text.clone() };
        with_text(&big, bottom.clone()).apply(&place("GND", 12.0, 10.3, None, false)).unwrap();
        assert!(with_text(&big, TextExtra { knockout: true, ..bottom }).apply(&place("GND", 12.0, 10.3, None, false)).is_err());
    }

    /// A board of four copper layers (`crate::pcb_transform_tests`: U1 and U2, `via_a` of GND at (20, 50) mm, a GND track ending on it).
    fn four_layer_model() -> ConstraintModel {
        crate::pcb_transform_tests::model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"])
    }

    fn place_vcc(x: f64, y: f64, allow: bool) -> Cmd {
        place("VCC", x, y, None, allow)
    }

    /// UI-LEFT-2 PP12: another net's via is judged on each layer at that layer's own diameter (`PCB_VIA::GetWidth( layer )`, a front/inner/back padstack).
    #[test]
    fn another_via_is_judged_at_each_layers_own_diameter() {
        use eda_model::via_extra::{ViaPadstack, ViaProps};
        let m = four_layer_model();
        // 0.9 mm from `via_a`'s centre: 0.3 mm clear of its 0.6 mm front ring (the clearance is 0.2 mm), touching a 1.2 mm inner ring.
        let mut b = crate::pcb_transform_tests::board(&m);
        b.apply(&place_vcc(20.9, 50.0, false)).unwrap();
        let mut b = crate::pcb_transform_tests::board(&m);
        let stack = ViaPadstack { mode: eda_model::padstack::PadStackMode::FrontInnerBack, sizes: [("Inner".to_string(), mm(1.2)), ("B.Cu".to_string(), mm(0.6))].into_iter().collect() };
        b.apply(&Cmd::SetViaProps { id: "via_a".into(), props: ViaProps { padstack: Some(stack), ..Default::default() } }).unwrap();
        assert_eq!(b.apply(&place_vcc(20.9, 50.0, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        // further out than the inner ring and its clearance: placed
        b.apply(&place_vcc(21.2, 50.0, false)).unwrap();
    }

    /// Round 6 C6-4: a plated pad on a layer it does not flash is its hole there (`PAD::GetEffectiveShape`), judged at the copper clearance, not only at the
    /// hole clearance.
    #[test]
    fn an_unflashed_plated_pad_is_its_hole_at_the_copper_clearance() {
        // U1's pads 1 and 3 plated through-hole, removing every layer nothing connects (`REMOVE_ALL`; the studio folds a board's pad edits into the model's
        // footprint, as Remove Unused Pads leaves them)
        let tht = |mode: eda_model::padstack::UnconnectedLayers, clearance: f64| {
            let mut m = four_layer_model();
            for pad in m.footprints[0].pads.iter_mut().filter(|p| p.number != "2") {
                pad.kind = eda_model::PadKind::ThroughHole;
                pad.drill = Some(mm(0.3));
                pad.ext.stack.unconnected = mode;
            }
            // copper clearance `clearance`, hole clearance 0.25 mm
            m.board.clearance = mm(clearance);
            m
        };
        // U1.1 (GND) is at (28.5, 19.4) mm; a VCC via 0.8 mm away is 0.35 mm from its hole's edge and 0.2 mm from its 0.6 mm pad's
        let at = (29.3, 19.4);
        let keep = tht(eda_model::padstack::UnconnectedLayers::KeepAll, 0.5);
        assert!(crate::pcb_transform_tests::board(&keep).apply(&place_vcc(at.0, at.1, false)).is_err(), "the pad's copper is too near");
        let removed = tht(eda_model::padstack::UnconnectedLayers::RemoveAll, 0.5);
        let mut b = crate::pcb_transform_tests::board(&removed);
        assert_eq!(b.apply(&place_vcc(at.0, at.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC), "its hole, at the 0.5 mm clearance, is too near");
        // with a copper clearance under 0.35 mm the hole alone lets it go down; the pad that keeps its copper still refuses it at 0.3 mm
        let removed = tht(eda_model::padstack::UnconnectedLayers::RemoveAll, 0.3);
        crate::pcb_transform_tests::board(&removed).apply(&place_vcc(at.0, at.1, false)).unwrap();
        let keep = tht(eda_model::padstack::UnconnectedLayers::KeepAll, 0.3);
        assert!(crate::pcb_transform_tests::board(&keep).apply(&place_vcc(at.0, at.1, false)).is_err(), "0.2 mm from the pad's copper");
    }

    /// Round 6 C6-5: a table and a dimension on copper refuse a via (`PCB_TABLE`, `PCB_DIMENSION_BASE` are no connected items); a text box, a `PCB_SHAPE` of
    /// no net, never does (`connectedItemNet == 0`).
    #[test]
    fn a_copper_table_and_dimension_refuse_a_via_and_a_text_box_does_not() {
        let m = four_layer_model();
        let with = |edit: &dyn Fn(&mut eda_model::ir::DrawingsSection)| {
            let mut d = crate::pcb_transform_tests::design();
            edit(d.drawings.as_mut().unwrap());
            Board::new(d, &m, mm(0.1), mm(0.3))
        };
        let pt = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        // a 10 x 4 mm table at (60, 60) mm: a via inside it, and one 1 mm right of it
        let table = |dr: &mut eda_model::ir::DrawingsSection| dr.tables.push(eda_model::table::Table { id: "tbl".into(), ..eda_model::table::Table::new_grid("F.Cu", pt(60.0, 60.0), 2, 2, mm(5.0), mm(2.0), mm(0.1)) });
        assert_eq!(with(&table).apply(&place_vcc(62.0, 61.0, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        with(&table).apply(&place_vcc(71.0, 61.0, false)).unwrap();
        // a dimension on F.Cu from (70, 30) to (80, 30) mm: its line 2 mm above (`height`), a via on the line refused, one 1 mm off placed
        let dim = |dr: &mut eda_model::ir::DrawingsSection| {
            let mut d = crate::pcb_transform_tests::dimension("dim_cu", eda_model::ir::DimensionKind::Aligned { height: mm(-2.0) }, pt(70.0, 30.0), pt(80.0, 30.0));
            d.layer = "F.Cu".into();
            dr.dimensions.push(d);
        };
        let g = eda_connectivity::dimension::compute_dimension_geometry(&{
            let mut d = crate::pcb_transform_tests::dimension("dim_cu", eda_model::ir::DimensionKind::Aligned { height: mm(-2.0) }, pt(70.0, 30.0), pt(80.0, 30.0));
            d.layer = "F.Cu".into();
            d
        });
        let crossbar = g.crossbar.expect("an aligned dimension has a crossbar");
        let on_bar = ((crossbar.0.x.as_f64() + crossbar.1.x.as_f64()) / 2.0e6 - 3.0, crossbar.0.y.as_f64() / 1.0e6);
        assert_eq!(with(&dim).apply(&place_vcc(on_bar.0, on_bar.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        with(&dim).apply(&place_vcc(on_bar.0, on_bar.1 + 3.0, false)).unwrap();
        // a text box on F.Cu with a border: never a refusal
        let text_box = |dr: &mut eda_model::ir::DrawingsSection| {
            let tb = eda_model::pcb_items::TextBox { id: "tbx".into(), text: "X".into(), ..eda_model::pcb_items::TextBox::new("F.Cu", pt(60.0, 80.0), pt(70.0, 85.0), mm(1.0), mm(0.2)) };
            dr.board_items.text_boxes.push(tb);
        };
        with(&text_box).apply(&place_vcc(60.0, 82.0, false)).unwrap();
    }

    /// Round 6 C6-6: with Allow DRC violations the via goes down and "Via location violates DRC." is still said (`PlaceItem`'s `ShowInfoBarError` before
    /// `if( !m_allowDRCViolations ) return false;`); a clean placement says nothing.
    #[test]
    fn an_allowed_violation_is_placed_and_still_said() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&track("VCC", (0.0, 5.0), (10.0, 5.0))).unwrap();
        crate::notice::clear();
        b.apply(&place("GND", 5.0, 5.45, None, true)).unwrap();
        assert_eq!(crate::notice::take_drc_violation().as_deref(), Some(VIA_VIOLATES_DRC));
        crate::notice::clear();
        b.apply(&place("GND", 2.0, 8.0, None, true)).unwrap();
        assert_eq!(crate::notice::take_drc_violation(), None);
        assert_eq!(b.design().routing.as_ref().unwrap().vias.len(), 2);
    }

    /// R7-4: a knockout text is its strokes' box grown by half the pen and `GetKnockoutTextMargin` (`buildBoundingHull`), a rectangle, not their hull: the
    /// corner under the right end of a T's bar is in it.
    #[test]
    fn a_knockout_text_is_its_bounding_rectangle() {
        use eda_model::board_text::TextExtra;
        use eda_model::ir::{Text, TextJustify};
        let text = Text { id: String::new(), content: "T".into(), at: Point { x: mm(10.0), y: mm(10.0) }, angle: 0, layer: "F.Cu".into(), size_nm: mm(1.5), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false };
        let Some(Shape::Strokes { segs, r }) = board_text_effective_shape(&text, None) else { panic!("a text is its strokes") };
        let Some(Shape::Polygon { pts }) = board_text_effective_shape(&text, Some(&TextExtra { knockout: true, ..Default::default() })) else { panic!("a knockout text is a polygon") };
        let margin = Nm::kiround(mm(0.15).as_f64() / 2.0).max(Nm::kiround(mm(1.5).as_f64() / 9.0));
        let grow = r + margin;
        let xs = segs.iter().flat_map(|s| [s.a.x, s.b.x]);
        let ys = segs.iter().flat_map(|s| [s.a.y, s.b.y]);
        let (x0, x1, y0, y1) = (xs.clone().min().unwrap() - grow, xs.max().unwrap() + grow, ys.clone().min().unwrap() - grow, ys.max().unwrap() + grow);
        assert_eq!(pts, vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]);
        // the box's bottom right corner, which the T's strokes come nowhere near
        let corner = Shape::Circle { c: Point { x: x1 - mm(0.05), y: y1 - mm(0.05) }, r: Nm::ZERO };
        assert!(Shape::Polygon { pts }.collides(&corner, Nm::ZERO).is_some());
        assert!(Shape::Strokes { segs, r }.collides(&corner, Nm::ZERO).is_none());
    }

    /// A rule area's hole lets a via in (`SHAPE_POLY_SET::Collide`: a point in a hole is outside), unless the via reaches the hole's edge.
    #[test]
    fn a_via_in_a_keepouts_hole_is_placed() {
        let m = model();
        let mut d = design();
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        d.routing = Some(serde_json::from_str(r#"{"tracks": [], "vias": []}"#).unwrap());
        d.routing.as_mut().unwrap().zones.push(Zone { id: "ra".into(), layer: "F.Cu".into(), outline: vec![p(20.0, 20.0), p(30.0, 20.0), p(30.0, 30.0), p(20.0, 30.0)], holes: vec![vec![p(24.0, 24.0), p(26.0, 24.0), p(26.0, 26.0), p(24.0, 26.0)]], is_rule_area: true, keepout_vias: true, ..Default::default() });
        let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
        b.apply(&place("", 25.0, 25.0, None, false)).unwrap();
        assert!(b.apply(&place("", 25.0, 24.2, None, false)).is_err(), "its ring reaches the hole's edge");
        assert!(b.apply(&place("", 22.0, 22.0, None, false)).is_err());
    }

    #[test]
    fn a_rule_area_that_keeps_vias_out_refuses_one() {
        let m = model();
        let mut d = design();
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        d.routing = Some(serde_json::from_str(r#"{"tracks": [], "vias": []}"#).unwrap());
        d.routing.as_mut().unwrap().zones.push(Zone { id: "ra".into(), layer: "F.Cu".into(), outline: vec![p(20.0, 20.0), p(30.0, 20.0), p(30.0, 30.0), p(20.0, 30.0)], is_rule_area: true, keepout_vias: true, ..Default::default() });
        let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
        assert_eq!(b.apply(&place("", 25.0, 25.0, None, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        b.apply(&place("", 35.0, 25.0, None, false)).unwrap();
    }
}
