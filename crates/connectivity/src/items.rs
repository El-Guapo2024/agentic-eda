//! `CN_ITEM` / `CN_ANCHOR` (`pcbnew/connectivity/connectivity_items.{h,cpp}`),
//! adapted to build from `(Design, ConstraintModel)` instead of a live
//! `BOARD`.
//!
//! Three adaptations from upstream, all forced by the shape of our model
//! rather than a choice to simplify:
//!
//! - **Tracks are polylines here, KiCad's `PCB_TRACK` never is.** A
//!   `Track` with N points is decomposed into N-1 two-point segments, one
//!   [`CnItem`] each -- exactly the granularity `export_kicad_pcb` already
//!   writes (`crates/kicad/src/pcb.rs` emits one `(segment ...)` per
//!   consecutive point pair) and `import_kicad_pcb` already reads back
//!   (`crates/kicad/src/import.rs` never merges consecutive segments into
//!   one `Track`). So this is not an approximation of KiCad's model, it's
//!   the same granularity a `.kicad_pcb` always has.
//! - **A pad's net comes from `ConstraintModel::nets`, not from a stored
//!   net code**, since that's where our IR keeps it. Track/via/zone items
//!   already carry an explicit `net: String` field, so unlike KiCad's
//!   engine (which infers/propagates codes for items that might not have
//!   one) there is nothing to propagate -- see the crate root docs.
//! - **A zone is its fill, one `CnItem` per disjoint fill fragment --
//!   not a triangulated mesh.** `CN_ZONE_LAYER` in KiCad wraps one polygon
//!   of an already-computed `SHAPE_POLY_SET` fill and builds an R-tree of
//!   its fill *triangles* for collision; the fills handed to [`build_items`]
//!   (`eda_drc::zone_copper::ZoneCopper`: KiCad's own, made by kicad-cli and
//!   kept beside `design.json`) give us the same per-fragment polygons, and
//!   an item connects to a zone when one of its anchors lies inside one of
//!   them (`CN_VISITOR::checkZoneItemConnection`'s own first, fast check,
//!   generalised to be the *only* check here) -- no triangulation needed
//!   since we're testing containment, not rendering. A zone with no fill
//!   yet (never filled, or edited since) has no copper and connects
//!   nothing, as in KiCad until the zones are filled.

use std::collections::HashMap;

use eda_model::footprint::{placed_pads, PadKind, PlacedPad};
use eda_model::padstack::UnconnectedLayers;
use eda_model::ir::{Design, Point, Nm};
use eda_model::{ConstraintModel, Part};

use crate::geom::bbox_of;
use eda_drc::zone_copper::ZoneCopper;

/// Which board item a [`CnItem`] stands for. Carries the id strings a
/// caller needs for reporting rather than raw indices, since `CnItem`
/// already holds all the geometry [`crate::algo`]/[`crate::ratsnest`] need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemRef {
    Pad { reference: String, number: String },
    /// One segment of a (possibly multi-point) `Track`; `seg_index` is
    /// this segment's position within that track's polyline. A track that is an arc is one item, `seg_index` 0 ([`ItemShape::Arc`]).
    TrackSeg { track_id: String, seg_index: usize },
    Via { via_id: String },
    ZoneOutline { zone_id: String },
}

/// The geometry a [`CnItem`] presents for touch-testing (see
/// [`crate::geom::touches`]).
#[derive(Debug, Clone)]
pub enum ItemShape {
    Pad(PlacedPad),
    Segment { a: Point, b: Point, half_width: f64 },
    /// A track arc (`PCB_ARC`), by its tessellation (`Track::pts`, start to end): one item, as `CN_LIST::Add( PCB_ARC* )` makes it, whose two
    /// anchors are its ends. KiCad collides the true arc (`SHAPE_ARC`); the tessellation is within a nanometre or so of it.
    Arc { pts: Vec<Point>, half_width: f64 },
    Via { center: Point, radius: f64 },
    Zone { outline: Vec<Point> },
}

/// One `CN_ITEM` equivalent: a physical thing with a net, a layer span,
/// one or more connection anchors, and (once [`crate::algo::build_graph`]
/// has run) the indices of every other item it physically touches.
pub struct CnItem {
    pub item: ItemRef,
    /// "" means no net (KiCad's `netcode <= 0`) -- an unconnected pin, a
    /// mounting hole, or a mis-tagged item.
    pub net: String,
    /// Pads and zones carry a fixed net (KiCad's `CN_ITEM(parent, false)`);
    /// tracks, arcs and vias can have theirs inferred/changed
    /// (`CN_ITEM(parent, true)`). Only used to reproduce
    /// `CN_VISITOR`'s cheap early-out for two fixed-net items of different
    /// nets -- see [`crate::algo::build_graph`].
    pub can_change_net: bool,
    /// Inclusive copper layer span as indices into `board.layers`
    /// (KiCad's `StartLayer()`/`EndLayer()`, with `B_Cu`'s "always highest"
    /// trick replaced by plain `layers.len() - 1`, since we have no
    /// sentinel-layer convention to match).
    pub layer_lo: i32,
    pub layer_hi: i32,
    pub anchors: Vec<Point>,
    pub shape: ItemShape,
    pub bbox: (Nm, Nm, Nm, Nm),
    /// Filled by [`crate::algo::build_graph`]; empty until then.
    pub connected: Vec<usize>,
    /// A pad whose copper differs from layer to layer ([`eda_model::padstack`]) is one item for each group of layers that shares a shape (a `CN_ITEM` per layer
    /// range, as `PADSTACK::ForEachUniqueLayer` has them); the pieces of one pad carry the same number here and [`crate::algo::build_graph`] joins them.
    pub pad_piece_of: Option<usize>,
    /// A plated through-hole pad or a via that removes its unused copper: the layers it is `ConditionallyFlashed` on (every layer it spans but the
    /// ends of its drill that it keeps), and the shape it has there to another item -- its hole (`GetEffectiveShape( layer, NEVER_FLASHED )`), as
    /// `CN_VISITOR::operator()` collides it. `None` for an item that has its copper on every layer it spans.
    pub never_flashed: Option<(Vec<i32>, ItemShape)>,
}

impl CnItem {
    /// The shape of the item on copper layer `layer` to another item that is not a zone (`CN_VISITOR::operator()`'s `FLASHING`).
    pub fn shape_on(&self, layer: i32) -> &ItemShape {
        match &self.never_flashed {
            Some((layers, hole)) if layers.contains(&layer) => hole,
            _ => &self.shape,
        }
    }

    /// Whether the item is its hole on `layer` ([`CnItem::shape_on`]).
    pub fn is_hole_on(&self, layer: i32) -> bool {
        self.never_flashed.as_ref().is_some_and(|(layers, _)| layers.contains(&layer))
    }
}

/// `PAD::ConditionallyFlashed` / `PCB_VIA::ConditionallyFlashed` over the layers `lo..=hi` an item spans, whose drill starts at `lo` and ends at `hi`:
/// none for `KEEP_ALL`, every one for `REMOVE_ALL`, every one but the drill's two ends for `REMOVE_EXCEPT_START_AND_END` and `START_END_ONLY`.
fn conditionally_flashed(mode: UnconnectedLayers, lo: i32, hi: i32) -> Vec<i32> {
    match mode {
        UnconnectedLayers::KeepAll => Vec::new(),
        UnconnectedLayers::RemoveAll => (lo..=hi).collect(),
        UnconnectedLayers::RemoveExceptStartAndEnd | UnconnectedLayers::StartEndOnly => (lo..=hi).filter(|l| *l != lo && *l != hi).collect(),
    }
}

/// A plated through-hole pad's hole on the board (`PAD::GetEffectiveHoleShape`): the drill's circle, or its slot as a stadium.
fn pad_hole(fp: &eda_model::ir::FootprintInstance, source: &eda_model::footprint::Pad, center: Point) -> Option<ItemShape> {
    if let Some((w, h)) = source.drill_slot {
        let frame = eda_model::pad_geom::PadFrame::of(fp, source, center);
        let half = (w.max(h) - w.min(h)).as_f64() / 2.0;
        let (a, b) = if w >= h { ((-half, 0.0), (half, 0.0)) } else { ((0.0, -half), (0.0, half)) };
        let (a, b) = (frame.to_board(a), frame.to_board(b));
        let pt = |q: (f64, f64)| Point { x: Nm::kiround(q.0), y: Nm::kiround(q.1) };
        return Some(ItemShape::Segment { a: pt(a), b: pt(b), half_width: (w.min(h)).as_f64() / 2.0 });
    }
    source.drill.map(|d| ItemShape::Via { center, radius: d.as_f64() / 2.0 })
}

/// [`CnItem::never_flashed`] of a plated through-hole pad (its drill from the first copper layer to the last).
fn pad_never_flashed(fp: &eda_model::ir::FootprintInstance, source: &eda_model::footprint::Pad, center: Point, last: i32, lo: i32, hi: i32) -> Option<(Vec<i32>, ItemShape)> {
    if source.kind != PadKind::ThroughHole {
        return None;
    }
    let layers: Vec<i32> = conditionally_flashed(source.ext.stack.unconnected, 0, last).into_iter().filter(|l| (lo..=hi).contains(l)).collect();
    if layers.is_empty() {
        return None;
    }
    pad_hole(fp, source, center).map(|hole| (layers, hole))
}

/// Do two items' layer spans share at least one copper layer? KiCad's
/// `LSET commonLayers = parentA->GetLayerSet() & parentB->GetLayerSet()`,
/// simplified to inclusive-range overlap -- exact for every item kind we
/// build (a pad is either one named layer or the whole stack; a via
/// spans a contiguous range; a track/zone item is one layer).
pub fn layers_overlap(a: &CnItem, b: &CnItem) -> bool {
    a.layer_lo.max(b.layer_lo) <= a.layer_hi.min(b.layer_hi)
}

fn layer_index(layers: &[String], name: &str) -> i32 {
    layers.iter().position(|l| l == name).map(|i| i as i32).unwrap_or(0)
}

/// "REF.PIN" -> net name, from the intent's declared nets -- the only
/// place a pad's net lives in our model (see module docs).
fn net_of_pin_map(model: &ConstraintModel) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for net in &model.nets {
        for pin in &net.pins {
            map.insert(pin.clone(), net.name.clone());
        }
    }
    map
}

/// The footprint's own pads of `part`, in the same order `placed_pads` returns the placed ones -- both are the same stable sort by pad number over the same
/// source list. A pad is a plated through-hole to the connectivity when KiCad's `CN_LIST::Add(PAD*)` spans the *whole* copper stack for it
/// (`PAD_ATTRIB::PTH`); SMD, `NPTH` and `CONN` pads (our `PadKind::Smd` and `PadKind::NonPlatedHole`) are single-layer, same as an SMD pad.
fn sorted_pads(model: &ConstraintModel, part: &Part) -> Vec<eda_model::footprint::Pad> {
    let Some(fp) = model.footprint_of(part) else { return Vec::new() };
    let mut pads = fp.pads;
    pads.sort_by(|a, b| a.number.cmp(&b.number));
    pads
}

/// A pad turned by other than a multiple of 90 degrees (its own rotation and its footprint's) is given its copper as pieces
/// ([`eda_model::pad_geom::pad_pieces`], `PAD::buildEffectiveShape`), so a touch is measured to the shape KiCad collides
/// (`PAD::GetEffectiveShape`) and not to the axis-aligned box round it, which [`PlacedPad`] is otherwise (exact only at a multiple of 90 degrees).
fn exact_turned_copper(fp: &eda_model::ir::FootprintInstance, source: &eda_model::footprint::Pad, pad: &mut PlacedPad) {
    if !pad.pieces.is_empty() || (fp.rot as i64 + source.rot as i64).rem_euclid(90_000) == 0 {
        return;
    }
    let frame = eda_model::pad_geom::PadFrame::of(fp, source, pad.center);
    pad.pieces = eda_model::pad_geom::pad_pieces(source).iter().map(|q| q.map(&|v| frame.to_board(v))).collect();
}

/// Build every [`CnItem`] on the board: pads (from the placement),
/// track segments, vias and zone outlines (from the routing). `connected`
/// is left empty; [`crate::algo::build_graph`] fills it in.
pub fn build_items(design: &Design, model: &ConstraintModel, copper: &ZoneCopper) -> Vec<CnItem> {
    let mut items = Vec::new();
    let layers = &model.board.layers;
    let last_idx = (layers.len().max(1) - 1) as i32;
    let f_cu = layer_index(layers, "F.Cu");
    let b_cu = if layers.iter().any(|l| l == "B.Cu") { layer_index(layers, "B.Cu") } else { last_idx };
    let net_of_pin = net_of_pin_map(model);

    if let Some(pl) = &design.placement {
        for fp in &pl.footprints {
            let Some(part) = model.part(&fp.id) else { continue };
            let Some(pads) = placed_pads(model, part, fp) else { continue };
            let sources = sorted_pads(model, part);
            for (i, pad) in pads.into_iter().enumerate() {
                let net = net_of_pin.get(&format!("{}.{}", fp.id, pad.number)).cloned().unwrap_or_default();
                let (is_pth, opposite) = sources.get(i).map(|p| (p.kind == PadKind::ThroughHole, p.opposite_side)).unwrap_or((false, false));
                // A padstack: an item for each run of layers that shares a shape, each measured by its own copper (`PADSTACK::ForEachUniqueLayer`).
                if let Some(source) = sources.get(i).filter(|p| is_pth && p.has_stack()).cloned() {
                    let piece_of = Some(items.len());
                    for group in source.shape_groups(fp.side, layers) {
                        let mut idx: Vec<i32> = group.layers.iter().map(|l| layer_index(layers, l)).collect();
                        idx.sort_unstable();
                        idx.dedup();
                        let view = group.shape.lay_on(&source);
                        let mut k = 0;
                        while k < idx.len() {
                            let mut end = k;
                            while end + 1 < idx.len() && idx[end + 1] == idx[end] + 1 {
                                end += 1;
                            }
                            let geom = eda_model::footprint::placed_pad_of(fp, &view);
                            let never_flashed = pad_never_flashed(fp, &source, geom.center, last_idx, idx[k], idx[end]);
                            let (hw, hh) = (geom.size.0 / 2, geom.size.1 / 2);
                            let bbox = (geom.center.x - hw, geom.center.y - hh, geom.center.x + hw, geom.center.y + hh);
                            items.push(CnItem {
                                item: ItemRef::Pad { reference: fp.id.clone(), number: pad.number.clone() },
                                net: net.clone(),
                                can_change_net: false,
                                layer_lo: idx[k],
                                layer_hi: idx[end],
                                anchors: vec![geom.center],
                                shape: ItemShape::Pad(geom),
                                bbox,
                                connected: Vec::new(),
                                pad_piece_of: piece_of,
                                never_flashed,
                            });
                            k = end + 1;
                        }
                    }
                    continue;
                }
                let (layer_lo, layer_hi) = if is_pth {
                    (0, last_idx)
                } else if (fp.side == eda_model::ir::Side::Top) != opposite {
                    (f_cu, f_cu)
                } else {
                    (b_cu, b_cu)
                };
                let mut pad = pad;
                let mut never_flashed = None;
                if let Some(source) = sources.get(i) {
                    exact_turned_copper(fp, source, &mut pad);
                    never_flashed = pad_never_flashed(fp, source, pad.center, last_idx, layer_lo, layer_hi);
                }
                let (hw, hh) = (pad.size.0 / 2, pad.size.1 / 2);
                let bbox = (pad.center.x - hw, pad.center.y - hh, pad.center.x + hw, pad.center.y + hh);
                let anchors = vec![pad.center];
                items.push(CnItem { item: ItemRef::Pad { reference: fp.id.clone(), number: pad.number.clone() }, net, can_change_net: false, layer_lo, layer_hi, anchors, shape: ItemShape::Pad(pad), bbox, connected: Vec::new(), pad_piece_of: None, never_flashed });
            }
        }
    }

    if let Some(rt) = &design.routing {
        for t in &rt.tracks {
            let layer = layer_index(layers, &t.layer);
            let half_width = t.width.as_f64() / 2.0;
            let margin = Nm::new(half_width.ceil() as i64).max(Nm::ZERO);
            // An arc is one item (`CN_LIST::Add( PCB_ARC* )`: `AddAnchor( GetStart() )`, `AddAnchor( GetEnd() )`), not the pieces of its tessellation:
            // a piece in the middle is no end of anything, and the importer's rounding of a small arc to nanometres repeats points of it.
            if t.arc().is_some() {
                let (a, b) = (t.pts[0], t.pts[t.pts.len() - 1]);
                let (x0, y0, x1, y1) = bbox_of(&t.pts);
                items.push(CnItem {
                    item: ItemRef::TrackSeg { track_id: t.id.clone(), seg_index: 0 },
                    net: t.net.clone(),
                    can_change_net: true,
                    layer_lo: layer,
                    layer_hi: layer,
                    anchors: vec![a, b],
                    shape: ItemShape::Arc { pts: t.pts.clone(), half_width },
                    bbox: (x0 - margin, y0 - margin, x1 + margin, y1 + margin),
                    connected: Vec::new(),
                    pad_piece_of: None,
                    never_flashed: None,
                });
                continue;
            }
            for (seg_index, w) in t.pts.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                let bbox = (a.x.min(b.x) - margin, a.y.min(b.y) - margin, a.x.max(b.x) + margin, a.y.max(b.y) + margin);
                items.push(CnItem {
                    item: ItemRef::TrackSeg { track_id: t.id.clone(), seg_index },
                    net: t.net.clone(),
                    can_change_net: true,
                    layer_lo: layer,
                    layer_hi: layer,
                    anchors: vec![a, b],
                    shape: ItemShape::Segment { a, b, half_width },
                    bbox,
                    connected: Vec::new(),
                    pad_piece_of: None,
                    never_flashed: None,
                });
            }
        }

        for v in &rt.vias {
            let (lo, hi) = {
                let (a, b) = (layer_index(layers, &v.from_layer), layer_index(layers, &v.to_layer));
                (a.min(b), a.max(b))
            };
            let radius = v.diameter.as_f64() / 2.0;
            let margin = (v.diameter / 2).max(Nm::ZERO);
            let bbox = (v.at.x - margin, v.at.y - margin, v.at.x + margin, v.at.y + margin);
            let mode = design.drawings.as_ref().and_then(|d| d.via_extras.iter().find(|x| x.id == v.id)).map(|x| x.unconnected_layers()).unwrap_or_default();
            let flashed_off = conditionally_flashed(mode, lo, hi);
            let never_flashed = (!flashed_off.is_empty()).then(|| (flashed_off, ItemShape::Via { center: v.at, radius: v.drill.as_f64() / 2.0 }));
            items.push(CnItem { item: ItemRef::Via { via_id: v.id.clone() }, net: v.net.clone(), can_change_net: true, layer_lo: lo, layer_hi: hi, anchors: vec![v.at], shape: ItemShape::Via { center: v.at, radius }, bbox, connected: Vec::new(), pad_piece_of: None, never_flashed });
        }

        if !rt.zones.is_empty() {
            // One set of items per copper layer of a zone (`CN_ZONE_LAYER`); a non-copper zone is not in the connectivity at all.
            for z in &rt.zones {
                for layer_name in z.copper_layers() {
                    let layer = layer_index(layers, layer_name);
                    // No fill for the zone on this layer: no copper there yet.
                    let fragments: Vec<Vec<Point>> = match copper.get(&z.id, layer_name) {
                        Some(fill) => fill.polys.iter().filter_map(|poly| poly.first()).map(|chain| chain.iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect()).collect(),
                        None => Vec::new(),
                    };

                    for outline in fragments {
                        if outline.len() < 3 {
                            continue;
                        }
                        let bbox = bbox_of(&outline);
                        items.push(CnItem {
                            item: ItemRef::ZoneOutline { zone_id: z.id.clone() },
                            net: z.net.clone(),
                            can_change_net: false,
                            layer_lo: layer,
                            layer_hi: layer,
                            anchors: outline.clone(),
                            shape: ItemShape::Zone { outline },
                            bbox,
                            connected: Vec::new(),
                            pad_piece_of: None,
                            never_flashed: None,
                        });
                    }
                }
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::tests_support::two_pad_model;

    #[test]
    fn builds_one_item_per_pad_and_per_track_segment() {
        let (mut design, model) = two_pad_model();
        design.routing.as_mut().unwrap().tracks.push(eda_model::ir::Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(0.1), y: Nm::ZERO }, Point { x: mm(0.1), y: mm(0.1) }], arc_mid_offset: None });
        let items = build_items(&design, &model, &ZoneCopper::default());
        let pads = items.iter().filter(|i| matches!(i.item, ItemRef::Pad { .. })).count();
        let segs = items.iter().filter(|i| matches!(i.item, ItemRef::TrackSeg { .. })).count();
        assert_eq!(pads, 4, "2 parts x 2 pads each (0603 is a 2-pad package)");
        assert_eq!(segs, 2, "a 3-point track must decompose into 2 segments");
    }

    #[test]
    fn through_hole_pad_spans_every_layer() {
        let (design, mut model) = two_pad_model();
        model.footprints.push(eda_model::Footprint {
            name: "THPAD".into(),
            pads: vec![eda_model::Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(1.0), mm(1.0)), shape: eda_model::PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(mm(0.5)), drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        });
        model.parts[0].footprint = Some("THPAD".into());
        model.parts[0].package = Some("THPAD".into());
        let items = build_items(&design, &model, &ZoneCopper::default());
        let pad = items.iter().find(|i| matches!(&i.item, ItemRef::Pad { reference, .. } if reference == "R1")).unwrap();
        assert_eq!(pad.layer_lo, 0);
        assert_eq!(pad.layer_hi, 1, "a 2-layer board's through-hole pad must span both layers");
    }
}
