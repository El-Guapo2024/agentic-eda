//! The board's Draw Via tool (`pcbnew/tools/drawing_tool.cpp::DRAWING_TOOL::DrawVia`, its `VIA_PLACER`), the part that changes the board: `PlaceItem` after the
//! net was chosen (the studio's `kicad-port/viaPlacer.ts` ports `SnapItem` and the net choice of `PlaceItem`, which need the canvas's view: what is under the
//! cursor, the zone fills, the visible layers, the "Select Net:" menu).
//!
//! - The net may be none (`NETINFO_LIST::ORPHANED`): a free via where nothing is under it. (`SetIsFree( GetNetCode() > 0 )` for a via that took a zone's net
//!   is not modelled: the IR's via has no `free` flag, so the file gets no `(free yes)`.)
//! - `checkDRCViolation`, refused with KiCad's "Via location violates DRC." unless the router allows DRC violations (`m_allowDRCViolations`): [`via_violates_drc`].
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
/// sits in a rule area that keeps vias out, or a `disallow` rule takes it (`DISALLOW_CONSTRAINT`).
///
/// `hasDRCViolation` per item: a rule area with `GetDoNotAllowVias` hits when its outline collides with the via's circle; a copper zone is skipped ("stitching
/// vias bind to zones"); a connected item (pad, track, via, graphic) of no net or of the via's net never collides; otherwise, on each copper layer of the item,
/// the via's copper against the item's at `EvalRules( CLEARANCE_CONSTRAINT )` less the epsilon, and the via's copper against the item's hole at
/// `HOLE_CLEARANCE_CONSTRAINT`. Text on copper (a board text, a footprint's text) is no connected item and is judged by its strokes. Not judged: a constraint's
/// severity (`RPT_SEVERITY_IGNORE`), a pad's own clearance override beyond what the rules give.
pub fn via_violates_drc(design: &Design, model: &ConstraintModel, via: &Via) -> bool {
    let board: DrcBoard = eda_drc::board::build(design, model);
    let rules = &model.board;
    let rb = RuleBoard::new(design, model, &board, eda_drc::constraints::Units::Mm, rules.custom_rules_text.as_deref());
    let probe = rb.probe_via(&via.net, via.at, via.diameter, via.drill, &via.from_layer, &via.to_layer);
    let eval = |t: CType, other: &Rc<eda_drc::rule_item::RuleItem>, layer: Option<&str>| -> Nm { Nm::new(rb.engine().eval_rules(t, Some(&probe), Some(other), layer, None).value.min()) };
    let via_shape = Shape::Circle { c: via.at, r: via.diameter / 2 };
    let copper: Vec<String> = rules.layers.iter().filter(|l| l.ends_with(".Cu")).cloned().collect();
    let other_net = |net: &Option<String>| net.as_deref().is_some_and(|n| !n.is_empty() && n != via.net);
    let collides = |other: &Shape, clearance: Nm| clearance >= Nm::ZERO && via_shape.collides(other, sub_e(clearance)).is_some();

    // Rule areas that keep vias out (`zone->GetDoNotAllowVias()`: `zone->Outline()->Collide( aVia->GetPosition(), aVia->GetWidth( aLayer ) / 2 )`).
    for z in design.routing.iter().flat_map(|rt| rt.zones.iter()) {
        if z.is_rule_area && z.keepout_vias && z.outline.len() >= 3 && via_shape.collides(&Shape::Polygon { pts: z.outline.clone() }, Nm::ZERO).is_some() {
            return true;
        }
    }

    // Pads: each copper layer the pad has copper on, then its hole.
    let (first, last) = (copper.first().cloned().unwrap_or_default(), copper.last().cloned().unwrap_or_default());
    for (i, pad) in board.pads.iter().enumerate() {
        if !other_net(&pad.net) {
            continue;
        }
        let Some(item) = rb.pad_item_at(i) else { continue };
        for layer in &copper {
            if pad.flash(layer, &first, &last) == eda_model::padstack::Flash::Never {
                continue;
            }
            let Some(shape) = pad.copper_on(layer) else { continue };
            if collides(shape, eval(CType::Clearance, &item, Some(layer))) {
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
        if !other_net(&t.net) {
            continue;
        }
        let item = rb.segment_item_at(i);
        if collides(&t.shape(), eval(CType::Clearance, &item, Some(&t.layer))) {
            return true;
        }
    }
    // Vias: each copper layer of their span, then their hole.
    for (i, v) in board.vias.iter().enumerate() {
        if !other_net(&v.net) {
            continue;
        }
        let item = rb.via_item_at(i);
        let span = layer_span(&copper, &v.from_layer, &v.to_layer);
        for layer in &span {
            if collides(&v.shape(), eval(CType::Clearance, &item, Some(layer))) {
                return true;
            }
        }
        if collides(&v.hole(), eval(CType::HoleClearance, &item, None)) {
            return true;
        }
    }
    // Graphics on copper with a net of their own.
    for s in design.drawings.iter().flat_map(|d| d.shapes.iter()) {
        if !copper.iter().any(|l| l == s.layer()) {
            continue;
        }
        let net = design.drawings.as_ref().and_then(|d| d.board_items.net_of(s.id())).map(str::to_string);
        if !other_net(&net) {
            continue;
        }
        let item = rb.shape_item_of(s);
        let clearance = eval(CType::Clearance, &item, Some(s.layer()));
        if eda_drc::board::effective_shapes(s).iter().any(|part| collides(part, clearance)) {
            return true;
        }
    }
    // Text on copper: the board's own and the footprints'.
    for t in design.drawings.iter().flat_map(|d| d.texts.iter()).filter(|t| copper.contains(&t.layer)) {
        let item = rb.board_text_item_of(t);
        let size = (t.size_nm, t.size_nm);
        let pen = eda_drc::stroke_font::effective_pen_width(t.stroke_width, size, false);
        let halign = match t.justify {
            eda_model::ir::TextJustify::Left => -1,
            eda_model::ir::TextJustify::Center => 0,
            eda_model::ir::TextJustify::Right => 1,
        };
        let rot = (t.angle as f64 / 1000.0).rem_euclid(360.0);
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &t.content, pos: t.at, size, thickness: t.stroke_width, angle_deg: rot, mirror: t.mirror, halign, valign: 0 });
        if !segs.is_empty() && collides(&Shape::Strokes { segs, r: pen / 2 }, eval(CType::Clearance, &item, Some(&t.layer))) {
            return true;
        }
    }
    for piece in eda_drc::fp_art::footprint_copper(design, model) {
        if let eda_drc::fp_art::FpCopper::Text(t) = &piece {
            let item = rb.text_item_of("fp_text", t);
            if let Some(shape) = eda_drc::board::footprint_text_shape(t) {
                if collides(&shape, eval(CType::Clearance, &item, Some(&t.layer))) {
                    return true;
                }
            }
        }
    }

    // `EvalRules( DISALLOW_CONSTRAINT, aVia, nullptr, UNDEFINED_LAYER )`: `m_DisallowFlags`.
    rb.engine().eval_rules(CType::Disallow, Some(&probe), None, None, None).disallow != 0
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
    pub(crate) fn place_via(&mut self, net: &str, x: Nm, y: Nm, drill: Nm, diameter: Nm, from_layer: &str, to_layer: &str, split_track: Option<&str>, allow_drc_violations: bool) -> Result<(), Vec<CheckResult>> {
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
        if !allow_drc_violations && via_violates_drc(&self.design, self.model, &via) {
            return Err(vec![CheckResult::fail("ops_via_drc", net, VIA_VIOLATES_DRC)]);
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
        Cmd::PlaceVia { net: net.into(), x: mm(x), y: mm(y), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), split_track: split, allow_drc_violations: allow }
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
