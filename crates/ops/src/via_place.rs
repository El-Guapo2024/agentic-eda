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
//!   contains it, the first part keeping the track's id and the second a new one (`Clone()`, `ResetUuidDirect()`), as locked as the first. An arc track is cut
//!   as KiCad cuts it (UI-LEFT-3 P3-32): `SetEnd` and `SetStart` on a `PCB_ARC` keep its mid, so with the via on its chord (where `SnapItem` puts it,
//!   `AlignToSegment` on the chord) each half is the arc through the via's place and the old mid.
//! - One undo step for the via and the split (`doInteractiveItemPlacement`'s `commit.Push( "Place via" )`).

use super::Board;
use eda_drc::board::DrcBoard;
use eda_drc::constraints::CType;
use eda_drc::kimath::Shape;
use eda_drc::rule_board::RuleBoard;
use eda_model::ir::{Design, Point, Track, Via};
use eda_model::units::Nm;
use eda_model::{CheckResult, ConstraintModel};
use eda_connectivity::copper_shapes::{barcode_box_shapes, board_text_effective_shape, dimension_effective_shapes, table_effective_shape};
use eda_model::via_extra::PadDrillProps;
use eda_model::Stackup;
use std::collections::BTreeMap;
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

/// What the via check reads beyond the board itself: the board's stackup (`GetStackupDescriptor`, how deep a post-machining reaches:
/// `BOARD_STACKUP::GetLayerDistance`) and the polygons of its barcodes on copper (`PCB_BARCODE::GetEffectiveShape`, from kicad-cli's plot: [`Cmd::PlaceVia`]'s
/// `barcode_shapes`).
#[derive(Default)]
pub struct ViaCheck<'a> {
    pub stackup: Option<&'a Stackup>,
    pub barcode_shapes: Option<&'a BTreeMap<String, Vec<Vec<Point>>>>,
}

impl ViaCheck<'_> {
    fn barcode(&self) -> &BTreeMap<String, Vec<Vec<Point>>> {
        static NONE: BTreeMap<String, Vec<Vec<Point>>> = BTreeMap::new();
        self.barcode_shapes.unwrap_or(&NONE)
    }
}

/// `SHAPE_POLY_SET::Collide( SHAPE_CIRCLE( c, r ) )` of a barcode's polygon `contours` (its plot's, filled even-odd): the centre inside it, or an edge within `r`.
fn contours_collide_circle(contours: &[Vec<Point>], c: Point, r: Nm) -> bool {
    let near = contours.iter().filter(|k| k.len() >= 2).any(|k| (0..k.len()).any(|i| eda_drc::kimath::Seg::new(k[i], k[(i + 1) % k.len()]).sq_distance_to_point(c) < r.get() as i128 * r.get() as i128));
    if near {
        return true;
    }
    let (px, py) = (c.x.as_f64(), c.y.as_f64());
    let mut inside = false;
    for k in contours.iter().filter(|k| k.len() >= 3) {
        for i in 0..k.len() {
            let (a, b) = (k[i], k[(i + 1) % k.len()]);
            let (ax, ay, bx, by) = (a.x.as_f64(), a.y.as_f64(), b.x.as_f64(), b.y.as_f64());
            if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax {
                inside = !inside;
            }
        }
    }
    inside
}

/// The drill a [`DrcPad`](eda_drc::board::DrcPad) has (`GetDrillSize().x`): its round drill, else its slot's width.
fn pad_drill_of(pad: &eda_drc::board::DrcPad) -> Nm {
    pad.drill_round.or_else(|| pad.drill_slot.map(|(w, h)| w.min(h))).unwrap_or(Nm::ZERO)
}

/// Each pad's backdrill and post-machining, in [`DrcBoard::pads`]' order: the board's pad edit's (`PadEdit::drill_props`), else what its board file's tokens said
/// (`PadMaskInfo::tokens`).
fn pad_drill_props(design: &Design, model: &ConstraintModel) -> Vec<PadDrillProps> {
    let mut out = Vec::new();
    for fp in design.placement.iter().flat_map(|pl| pl.footprints.iter()) {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(footprint) = model.footprint_of(part) else { continue };
        let mut seen: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        for (i, lp) in footprint.pads.iter().enumerate() {
            let nth = {
                let n = seen.entry(lp.number.as_str()).or_insert(0);
                *n += 1;
                *n
            };
            let edit = design.footprint_edit(&fp.id).and_then(|e| e.pad(&lp.number, nth)).and_then(|e| e.drill_props.clone());
            let tokens: &[String] = design.pad_mask_info(&fp.id, i, footprint.pads.len()).map(|m| m.tokens.as_slice()).unwrap_or_default();
            out.push(edit.unwrap_or_else(|| PadDrillProps::from_tokens(tokens)));
        }
    }
    out
}

/// `PAD::IsBackdrilledOrPostMachined( aLayer )` / `PCB_VIA::IsBackdrilledOrPostMachined( aLayer )` and the hole `GetEffectiveShape( aLayer )` takes there: the
/// diameter of the larger of the front and back post-machining and the backdrill (`SecondaryDrill`), else the drill, on a copper layer `layer` a backdrill
/// (`SecondaryDrill`, `TertiaryDrill`) spans or a post-machining reaches (`GetPostMachiningKnockout( aLayer ) > 0`); `None` elsewhere. A pad's span is
/// measured in `IsBackdrilledOrPostMachined`'s layer ordinals (`pad_ordinals`: F.Cu 0, B.Cu the last, another layer `id / 2 + 1`, a start or end on B.Cu or F.Cu
/// as its formula has it), a via's by `LAYER_RANGE`.
fn drill_knockout(d: &PadDrillProps, drill: Nm, layer: &str, copper: &[String], stackup: Option<&Stackup>, pad_ordinals: bool) -> Option<Nm> {
    if !copper.iter().any(|l| l == layer) {
        return None;
    }
    let count = copper.len() as i64;
    let id = |l: &str| eda_model::layers::pcb_layer_id(l).map(|i| i as i64);
    let enabled = |l: &str| copper.iter().any(|c| c == l);
    // `startOrdinal`, `endOrdinal`, `layerOrdinal` of `PAD::IsBackdrilledOrPostMachined`
    let start_ord = |l: &str| if !enabled(l) { -1 } else if l == "F.Cu" { 0 } else { id(l).map_or(-1, |i| i / 2 + 1) };
    let end_ord = |l: &str| if !enabled(l) { -1 } else if l == "B.Cu" { count - 1 } else { id(l).map_or(-1, |i| i / 2 + 1) };
    let layer_ord = |l: &str| if !enabled(l) { -1 } else if l == "F.Cu" { 0 } else if l == "B.Cu" { count - 1 } else { id(l).map_or(-1, |i| i / 2 + 1) };
    let spans = |b: &eda_model::via_extra::ViaDrillProps| -> bool {
        if b.size <= Nm::ZERO || b.start.is_empty() || b.end.is_empty() {
            return false;
        }
        if pad_ordinals {
            let (mut s, mut e, l) = (start_ord(&b.start), end_ord(&b.end), layer_ord(layer));
            if l < 0 || s < 0 || e < 0 {
                return false;
            }
            if s > e {
                std::mem::swap(&mut s, &mut e);
            }
            l >= s && l <= e
        } else {
            layer_span(copper, &b.start, &b.end).iter().any(|l| l == layer)
        }
    };
    let drilled = d.backdrill.as_ref().is_some_and(spans) || d.tertiary_drill.as_ref().is_some_and(spans) || post_machining_knockout(d, layer, copper, stackup) > Nm::ZERO;
    if !drilled {
        return None;
    }
    // "Return the larger of the backdrill or post-machining hole"
    let mut hole = Nm::ZERO;
    for pm in [&d.front_post_machining, &d.back_post_machining].into_iter().flatten() {
        hole = hole.max(pm.size);
    }
    if let Some(b) = d.backdrill.as_ref().filter(|b| !b.start.is_empty() && !b.end.is_empty()) {
        hole = hole.max(b.size);
    }
    Some(if hole > Nm::ZERO { hole } else { drill })
}

/// `GetPostMachiningKnockout( aLayer )`: the diameter a front (from F.Cu) or back (from B.Cu) counterbore or countersink still has at `layer`, when the layer is
/// closer to its side than its depth (`stackup.GetLayerDistance`; a countersink without a depth reaches `( size / 2 ) / tan( angle / 2 )`, and narrows
/// with depth); 0 when neither reaches it.
fn post_machining_knockout(d: &PadDrillProps, layer: &str, copper: &[String], stackup: Option<&Stackup>) -> Nm {
    use eda_model::via_extra::PostMachiningMode;
    for (pm, side) in [(&d.front_post_machining, "F.Cu"), (&d.back_post_machining, "B.Cu")] {
        let Some(pm) = pm.as_ref().filter(|pm| pm.size > Nm::ZERO) else { continue };
        let half_angle = (pm.angle_ddeg as f64 / 10.0).to_radians() / 2.0;
        let countersink = pm.mode == PostMachiningMode::Countersink && pm.angle_ddeg > 0;
        let mut depth = pm.depth.get();
        if depth <= 0 && countersink {
            depth = ((pm.size.as_f64() / 2.0) / half_angle.tan()) as i64;
        }
        if depth <= 0 {
            continue;
        }
        let dist = stackup.map_or(Nm::ZERO, |s| crate::drill_chart::layer_distance_nm(s, copper, side, layer)).get();
        if dist < depth {
            return if countersink { Nm::new((pm.size.get() - (2.0 * dist as f64 * half_angle.tan()) as i64).max(0)) } else { pm.size };
        }
    }
    Nm::ZERO
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
/// segments, solid circles); a barcode's polygon (`PCB_BARCODE::GetEffectiveShape`, `m_poly`: zint's modules and the text under them, the plot kicad-cli
/// makes of it, [`ViaCheck::barcode_shapes`]; its box and its text's box when there is none, round 6 C6-5). The board's and every footprint's tables,
/// dimensions and barcodes (`checkDRCViolation` skips a footprint itself and checks its children). On a layer a pad or via is backdrilled or post-machined
/// on (`IsBackdrilledOrPostMachined`), its copper is the larger hole there ([`drill_knockout`], UI-LEFT-3 P3-32). An item's own clearance (a pad's, its
/// footprint's) is the rules' (`EvalRules`' local overrides: `RuleBoard`'s items carry them).
///
/// `hidden_layers` are the layers the Appearance panel hides: `checkDRCViolation` takes its candidates from `view->Query`, which skips the view layers that are
/// not shown (round 6 C6-1), so an item none of whose layers is shown is not judged. A via (its hole layers) and a plated pad (the plated-hole layers) are
/// always shown; a track, an SMD or NPTH pad, a graphic, a text, a table, a dimension and a barcode by their copper layers.
pub fn via_violates_drc(design: &Design, model: &ConstraintModel, via: &Via, hidden_layers: &[String], check: &ViaCheck) -> bool {
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

    // Each pad's backdrill and post-machining (`PADSTACK::SecondaryDrill` ..): its edit's, else its board file's tokens, in `board.pads`' order.
    let pad_drills = pad_drill_props(design, model);
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
        let drills = pad_drills.get(i).cloned().unwrap_or_default();
        for layer in &copper {
            let Some(shape) = pad.copper_on(layer) else { continue };
            // `PAD::GetEffectiveShape( layer )`: "Check if this layer has copper removed by backdrill or post-machining" before anything else
            if let Some(d) = pad.hole.as_ref().and(drill_knockout(&drills, pad_drill_of(pad), layer, &copper, check.stackup, true)) {
                if collides_on(layer, &Shape::Circle { c: pad.center, r: d / 2 }, eval(CType::Clearance, &item, Some(layer))) {
                    return true;
                }
                continue;
            }
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
        let drills = design.drawings.as_ref().and_then(|d| d.via_extra(&v.id)).map(|e| e.props()).map(|p| PadDrillProps { backdrill: p.backdrill, tertiary_drill: p.tertiary_drill, front_post_machining: p.front_post_machining, back_post_machining: p.back_post_machining }).unwrap_or_default();
        for layer in &layer_span(&copper, &v.from_layer, &v.to_layer) {
            // `PCB_VIA::GetEffectiveShape( layer )`: the backdrill's or post-machining's hole where it takes the copper away
            let shape = match drill_knockout(&drills, v.drill, layer, &copper, check.stackup, false) {
                Some(d) => Shape::Circle { c: v.at, r: d / 2 },
                None if flashes(kept, layer) => v.shape_on(layer),
                None => v.hole(),
            };
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
    // Tables, dimensions and barcodes on copper: items of no net (`PCB_TABLE`, `PCB_DIMENSION_BASE`, `PCB_BARCODE` are no `BOARD_CONNECTED_ITEM`), the board's
    // and each footprint's ("check against children, but not against footprint itself").
    let mut sets: Vec<eda_model::fp_items::FootprintItems> = Vec::new();
    if let Some(d) = design.drawings.as_ref() {
        sets.push(eda_model::fp_items::FootprintItems { tables: d.tables.clone(), dimensions: d.dimensions.clone(), barcodes: d.barcodes.clone(), ..Default::default() });
    }
    for fp in design.placement.iter().flat_map(|pl| pl.footprints.iter()) {
        let Some(items) = model.part(&fp.id).and_then(|part| eda_model::fp_items_place::items_of(design, model, part)) else { continue };
        sets.push(eda_model::fp_items_place::placed(items, fp));
    }
    let judged_by = |id: &str, layer: &str, class: eda_drc::rule_item::Class, type_name: &str, hit: &dyn Fn(Nm) -> bool| -> bool {
        if !copper.iter().any(|l| l == layer) || !shown(&[layer]) {
            return false;
        }
        let item = Rc::new(other_item(id, layer, class, type_name));
        hit(eval(CType::Clearance, &item, Some(layer)))
    };
    let judged = |id: &str, layer: &str, class: eda_drc::rule_item::Class, type_name: &str, shapes: &[Shape]| -> bool {
        !shapes.is_empty() && judged_by(id, layer, class, type_name, &|clearance| shapes.iter().any(|shape| collides_on(layer, shape, clearance)))
    };
    for set in &sets {
        for t in &set.tables {
            if judged(&t.id, &t.layer, eda_drc::rule_item::Class::Shape, "Table", &[table_effective_shape(t)]) {
                return true;
            }
        }
        for dim in &set.dimensions {
            let class = if matches!(dim.kind, eda_model::ir::DimensionKind::Leader) { eda_drc::rule_item::Class::Leader } else { eda_drc::rule_item::Class::Dimension };
            if judged(&dim.id, &dim.layer, class, class.type_name(), &dimension_effective_shapes(dim)) {
                return true;
            }
        }
        for b in &set.barcodes {
            let hit = match check.barcode().get(&b.id) {
                // `SHAPE_POLY_SET::Collide`: the via's copper on the barcode's layer within the clearance of the polygon (inside it, or near an edge)
                Some(contours) => {
                    let (c, r) = match via_shape_on(&b.layer) {
                        Shape::Circle { c, r } => (c, r),
                        _ => (via.at, via.diameter / 2),
                    };
                    judged_by(&b.id, &b.layer, eda_drc::rule_item::Class::Shape, "Barcode", &|clearance| clearance >= Nm::ZERO && contours_collide_circle(contours, c, r + sub_e(clearance)))
                }
                None => judged(&b.id, &b.layer, eda_drc::rule_item::Class::Shape, "Barcode", &barcode_box_shapes(b)),
            };
            if hit {
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

/// Where a via at `at` cuts a track ([`split_at`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Split {
    /// The polyline's `k`-th segment contains it.
    Segment(usize),
    /// A track arc (`PCB_ARC`) whose chord contains it: its start, mid and end.
    Arc(Point, Point, Point),
}

/// Where `track` is cut by a via at `at` (`PlaceItem`: `if( viaPos == trackStart || viaPos == trackEnd ) return true; if( !trackSeg.Contains( viaPos ) ) return
/// true;`): a polyline at the segment that contains it, unless the via is on an end of that segment (a joint); an arc where its chord (`SEG( GetStart(),
/// GetEnd() )`) contains it and it is on neither end.
fn split_at(track: &Track, at: Point) -> Option<Split> {
    if let Some((start, mid, end)) = track.arc() {
        return (at != start && at != end && seg_contains(start, end, at)).then_some(Split::Arc(start, mid, end));
    }
    let k = track.pts.windows(2).position(|w| seg_contains(w[0], w[1], at))?;
    (track.pts[k] != at && track.pts[k + 1] != at).then_some(Split::Segment(k))
}

impl Board<'_> {
    /// `Cmd::PlaceVia`: `VIA_PLACER::PlaceItem` with the net chosen ([`self`](crate::via_place) module doc).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place_via(&mut self, net: &str, x: Nm, y: Nm, drill: Nm, diameter: Nm, from_layer: &str, to_layer: &str, split_track: Option<&str>, allow_drc_violations: bool, hidden_layers: &[String], barcode_shapes: &BTreeMap<String, Vec<Vec<Point>>>) -> Result<(), Vec<CheckResult>> {
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
        let (_, stackup) = self.effective_stackup();
        if via_violates_drc(&self.design, self.model, &via, hidden_layers, &ViaCheck { stackup: Some(&stackup), barcode_shapes: Some(barcode_shapes) }) {
            if !allow_drc_violations {
                return Err(vec![CheckResult::fail("ops_via_drc", net, VIA_VIOLATES_DRC)]);
            }
            crate::notice::note_drc_violation(VIA_VIOLATES_DRC);
        }
        // The split, before the via goes in: the track must be the one under the via, of its net.
        let split = match split_track {
            Some(id) => {
                let t = self.design.routing.as_ref().and_then(|rt| rt.tracks.iter().find(|t| t.id == id)).ok_or_else(|| vec![CheckResult::fail("ops_unknown_track", id, "no track with this id")])?;
                split_at(t, at).map(|k| (t.clone(), k))
            }
            None => None,
        };
        let locked = split.as_ref().is_some_and(|(t, _)| self.design.drawings.as_ref().is_some_and(|d| d.locked_ids.iter().any(|l| *l == t.id)));
        let rt = self.routing_mut();
        rt.vias.push(via);
        let mut new_track_seed = None;
        if let Some((t, k)) = split {
            // `track->SetEnd( viaPos ); newTrack->SetStart( viaPos )`: a polyline is cut in two; an arc's `SetEnd` and `SetStart` keep its mid (`PCB_ARC::m_Mid`), so
            // each half is the arc through the via's place and the old mid (UI-LEFT-3 P3-32)
            let (head, tail) = match k {
                Split::Segment(k) => {
                    let mut first: Vec<Point> = t.pts[..=k].to_vec();
                    first.push(at);
                    let mut second = vec![at];
                    second.extend_from_slice(&t.pts[k + 1..]);
                    let tail = Track { id: String::new(), net: t.net.clone(), pins: Vec::new(), layer: t.layer.clone(), width: t.width, pts: second, arc_mid_offset: None };
                    ((first, None), tail)
                }
                Split::Arc(start, mid, end) => {
                    let first = Track::new_arc(t.net.clone(), t.layer.clone(), t.width, start, mid, at);
                    let tail = Track::new_arc(t.net.clone(), t.layer.clone(), t.width, at, mid, end);
                    ((first.pts, first.arc_mid_offset), tail)
                }
            };
            if let Some(orig) = rt.tracks.iter_mut().find(|x| x.id == t.id) {
                orig.pts = head.0;
                orig.arc_mid_offset = head.1;
                orig.pins.clear();
            }
            new_track_seed = Some(tail.pts.clone());
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
        Cmd::PlaceVia { net: net.into(), x: mm(x), y: mm(y), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), split_track: split, allow_drc_violations: allow, hidden_layers: Vec::new(), barcode_shapes: Default::default() }
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
        b.apply(&Cmd::PlaceVia { net, x, y, drill, diameter, from_layer, to_layer, split_track, allow_drc_violations, hidden_layers: vec!["F.Cu".into()], barcode_shapes: Default::default() }).unwrap();
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

    /// UI-LEFT-3 P3-32: a pad's own clearance (`PAD::GetClearanceOverrides`, Pad Properties' Clearance) refuses a via the rules alone let in, as `EvalRules` takes
    /// a local override over the rules; so does its footprint's.
    #[test]
    fn a_pads_own_clearance_refuses_a_via_the_rules_let_in() {
        let m = four_layer_model();
        // U1.1 (GND, a 0.6 mm pad) at (28.5, 19.4) mm; a VCC via 1.0 mm away is 0.4 mm clear of it, over the 0.2 mm clearance
        let at = (29.5, 19.4);
        crate::pcb_transform_tests::board(&m).apply(&place_vcc(at.0, at.1, false)).unwrap();
        let overrides = |clearance: f64| Cmd::SetPadZoneOverrides { part: "U1".into(), pad: "1".into(), zone_connection: None, thermal_gap: None, thermal_spoke_width: None, thermal_spoke_angle_mdeg: None, clearance: Some(mm(clearance)) };
        let mut b = crate::pcb_transform_tests::board(&m);
        b.apply(&overrides(0.5)).unwrap();
        assert_eq!(b.apply(&place_vcc(at.0, at.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC), "0.4 mm from a pad that asks 0.5 mm");
        let mut b = crate::pcb_transform_tests::board(&m);
        b.apply(&Cmd::SetFootprintZoneConnection { part: "U1".into(), zone_connection: None, clearance: Some(mm(0.5)) }).unwrap();
        assert_eq!(b.apply(&place_vcc(at.0, at.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC), "its footprint asks 0.5 mm");
    }

    /// UI-LEFT-3 P3-32, `PCB_VIA::GetEffectiveShape( layer )` with `IsBackdrilledOrPostMachined( layer )`: on the layers a backdrill takes the copper from, another
    /// net's via is the backdrill's hole -- a 0.8 mm backdrill from F.Cu to In1.Cu is wider than its 0.6 mm ring there, and refuses a via the ring lets in; a
    /// counterbore from the front deeper than the board's first dielectric does the same.
    #[test]
    fn a_backdrilled_or_post_machined_layer_is_the_larger_hole() {
        use eda_model::via_extra::{PostMachiningMode, PostMachiningProps, ViaDrillProps, ViaProps};
        let m = four_layer_model();
        // 0.85 mm from `via_a`'s centre: 0.25 mm clear of its 0.6 mm ring, 0.15 mm from a 0.8 mm hole
        let at = (20.85, 50.0);
        crate::pcb_transform_tests::board(&m).apply(&place_vcc(at.0, at.1, false)).unwrap();
        let with = |props: ViaProps| {
            let mut b = crate::pcb_transform_tests::board(&m);
            b.apply(&Cmd::SetViaProps { id: "via_a".into(), props }).unwrap();
            b
        };
        let backdrill = ViaProps { backdrill: Some(ViaDrillProps { size: mm(0.8), start: "F.Cu".into(), end: "In1.Cu".into() }), ..Default::default() };
        assert_eq!(with(backdrill).apply(&place_vcc(at.0, at.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        let counterbore = |depth: f64| ViaProps { front_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Counterbore, size: mm(0.8), depth: mm(depth), angle_ddeg: 0 }), ..Default::default() };
        assert_eq!(with(counterbore(0.3)).apply(&place_vcc(at.0, at.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC), "it reaches In1.Cu");
        // a counterbore that does not reach past F.Cu (`GetLayerDistance( F_Cu, F_Cu )` is 0, under any depth): F.Cu alone is the hole, and F.Cu is enough
        assert!(with(counterbore(0.01)).apply(&place_vcc(at.0, at.1, false)).is_err(), "F.Cu itself is post-machined");
    }

    /// UI-LEFT-3 P3-32: `checkDRCViolation` checks a footprint's children, its own dimensions and tables among them (`FootprintExtra::items`, placed with it).
    #[test]
    fn a_footprints_own_dimension_on_copper_refuses_a_via() {
        let m = four_layer_model();
        let mut d = crate::pcb_transform_tests::design();
        // a dimension on F.Cu in U1's frame, U1 at (30, 20) mm: its crossbar 2 mm below the points
        let mut dim = crate::pcb_transform_tests::dimension("dim_fp", eda_model::ir::DimensionKind::Aligned { height: mm(-2.0) }, Point { x: mm(-10.0), y: mm(10.0) }, Point { x: mm(0.0), y: mm(10.0) });
        dim.layer = "F.Cu".into();
        let mut items = eda_model::fp_items::FootprintItems::default();
        items.dimensions.push(dim);
        d.drawings.as_mut().unwrap().footprint_extras.push(eda_model::ir::FootprintExtra { id: "U1".into(), items: items.clone(), ..Default::default() });
        let fp = d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().clone();
        let placed = eda_model::fp_items_place::placed(&items, &fp);
        let g = eda_connectivity::dimension::compute_dimension_geometry(&placed.dimensions[0]);
        let bar = g.crossbar.expect("a crossbar");
        let on_bar = ((bar.0.x.as_f64() + bar.1.x.as_f64()) / 2.0e6 - 3.0, bar.0.y.as_f64() / 1.0e6);
        assert_eq!(Board::new(d.clone(), &m, mm(0.1), mm(0.3)).apply(&place_vcc(on_bar.0, on_bar.1, false)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC));
        Board::new(d, &m, mm(0.1), mm(0.3)).apply(&place_vcc(on_bar.0, on_bar.1 + 3.0, false)).unwrap();
    }

    /// UI-LEFT-3 P3-32, `PCB_BARCODE::GetEffectiveShape` (`m_poly`): a barcode is judged by its polygon when the board directory handed one over -- a via in a
    /// light module of the code, clear of the dark ones, goes down -- and by its box when it did not.
    #[test]
    fn a_barcode_is_judged_by_its_polygon_when_it_has_one() {
        let m = four_layer_model();
        let mut d = crate::pcb_transform_tests::design();
        let b = eda_model::barcode::Barcode { id: "bar_1".into(), at: Point { x: mm(60.0), y: mm(60.0) }, angle: 0, layer: "F.Cu".into(), width_nm: mm(10.0), height_nm: mm(10.0), text: "X".into(), text_height_nm: mm(1.0), kind: Default::default(), ecc: Default::default(), show_text: false, knockout: false, margin_x_nm: Nm::ZERO, margin_y_nm: Nm::ZERO };
        d.drawings.as_mut().unwrap().barcodes.push(b);
        let pt = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        // the plot: a frame 1 mm wide round the symbol, the inside light
        let shapes: BTreeMap<String, Vec<Vec<Point>>> = [("bar_1".to_string(), vec![vec![pt(55.0, 55.0), pt(65.0, 55.0), pt(65.0, 65.0), pt(55.0, 65.0)], vec![pt(56.0, 56.0), pt(64.0, 56.0), pt(64.0, 64.0), pt(56.0, 64.0)]])].into();
        let with_shapes = |x: f64, y: f64, shapes: BTreeMap<String, Vec<Vec<Point>>>| {
            let Cmd::PlaceVia { net, x, y, drill, diameter, from_layer, to_layer, split_track, allow_drc_violations, hidden_layers, .. } = place_vcc(x, y, false) else { unreachable!() };
            Cmd::PlaceVia { net, x, y, drill, diameter, from_layer, to_layer, split_track, allow_drc_violations, hidden_layers, barcode_shapes: shapes }
        };
        assert_eq!(Board::new(d.clone(), &m, mm(0.1), mm(0.3)).apply(&with_shapes(60.0, 60.0, BTreeMap::new())).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC), "its box");
        Board::new(d.clone(), &m, mm(0.1), mm(0.3)).apply(&with_shapes(60.0, 60.0, shapes.clone())).unwrap();
        assert_eq!(Board::new(d, &m, mm(0.1), mm(0.3)).apply(&with_shapes(60.0, 56.3, shapes)).unwrap_err()[0].hint.as_deref(), Some(VIA_VIOLATES_DRC), "near the frame");
    }

    /// UI-LEFT-3 P3-32, `PlaceItem`'s split of a `PCB_ARC`: `SetEnd( viaPos )` and the clone's `SetStart( viaPos )` keep the arc's mid, so a via on the arc's chord
    /// cuts it into the two arcs through the via's place and the old mid; a via on an end cuts nothing.
    #[test]
    fn a_via_on_an_arcs_chord_cuts_it_as_kicad_does() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        let (start, mid, end) = (Point { x: mm(10.0), y: mm(10.0) }, Point { x: mm(15.0), y: mm(5.0) }, Point { x: mm(20.0), y: mm(10.0) });
        let arc = Track::new_arc("GND".into(), "F.Cu".into(), mm(0.2), start, mid, end);
        b.routing_mut().tracks.push(arc);
        b.routing_mut().assign_missing_ids();
        let id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
        b.apply(&place("GND", 14.0, 10.0, Some(id.clone()), false)).unwrap();
        let rt = b.design().routing.as_ref().unwrap();
        assert_eq!(rt.tracks.len(), 2);
        let at = Point { x: mm(14.0), y: mm(10.0) };
        assert_eq!(rt.tracks.iter().find(|t| t.id == id).and_then(|t| t.arc()), Some((start, mid, at)), "the first keeps the id, from the start through the old mid");
        assert_eq!(rt.tracks.iter().find(|t| t.id != id).and_then(|t| t.arc()), Some((at, mid, end)));
        b.apply(&place("GND", 20.0, 10.0, Some(id), false)).unwrap();
        assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 2, "on an end: no cut");
    }
}
