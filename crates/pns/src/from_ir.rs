//! Builds a [`crate::node::Node`] from this project's own IR
//! (`eda_model::ir::Design` + `ConstraintModel`) -- the "NODE built from
//! the IR" the task's rules require, so the router works on the one JSON
//! source of truth rather than a separate board model.
//!
//! Reuses `eda_drc::board::build`, the exact same flattening DRC already
//! does (pads/tracks/vias to board-space shapes, net resolution, layer
//! names) -- so the router and the DRC engine agree on what a pad/track/via
//! *is*, geometrically, by construction. Zones are deliberately not
//! inserted as `NODE` items: `PNS::ITEM::PnsKind` has no `ZONE_T` in real
//! KiCad either (confirmed while reading `pns_item.h`) -- the interactive
//! router routes straight through/over a zone's fill area, which gets
//! recomputed afterward, exactly like real KiCad.

use crate::item::{net_of, Item, Segment, Solid, Via};
use crate::layer::LayerMap;
use crate::node::Node;
use eda_model::ir::Design;
use eda_model::ConstraintModel;

/// The id/segment-index pair a `"trk_xxx#3"` flattened id decodes to, the
/// inverse of `eda_drc::board::build`'s own `format!("{}#{i}", t.id)`.
fn split_track_seg_id(flat_id: &str) -> Option<(String, usize)> {
    let (track_id, idx) = flat_id.rsplit_once('#')?;
    Some((track_id.to_string(), idx.parse().ok()?))
}

/// Build the router's world `Node` plus the [`LayerMap`] it was built
/// against (callers need the map to translate back to layer names when
/// emitting `Cmd`s).
pub fn build_node(design: &Design, model: &ConstraintModel) -> (Node, LayerMap) {
    let layers = LayerMap::new(&model.board.layers);
    let drc_board = eda_drc::board::build(design, model);
    let mut node = Node::new();

    for pad in &drc_board.pads {
        let layer_range = pad.layers.iter().filter_map(|n| layers.index_of(n)).fold(None, |acc: Option<crate::layer::LayerRange>, i| {
            let r = crate::layer::LayerRange::single(i);
            Some(match acc {
                Some(mut a) => {
                    a.merge(&r);
                    a
                }
                None => r,
            })
        });
        let layer_range = layer_range.unwrap_or_else(|| layers.all());
        node.add(Item::Solid(Solid { net: pad.net.as_deref().map(net_of).unwrap_or(None), layers: layer_range, pos: pad.center, shape: pad.copper.clone(), source: pad.id.clone() }));
    }

    for t in &drc_board.tracks {
        let layer = layers.index_of(&t.layer).unwrap_or(0);
        let source_track = split_track_seg_id(&t.id);
        node.add(Item::Segment(Segment { net: t.net.as_deref().map(net_of).unwrap_or(None), layer, a: t.a, b: t.b, width: t.width, source_track, locked: false }));
    }

    for v in &drc_board.vias {
        let layer_range = layers.range_of(&v.from_layer, &v.to_layer);
        node.add(Item::Via(Via { net: v.net.as_deref().map(net_of).unwrap_or(None), layers: layer_range, pos: v.at, diameter: v.diameter, drill: v.drill, source_via: Some(v.id.clone()), locked: false }));
    }

    (node, layers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track};
    use eda_model::{ConstraintModel, Part, Pin, PinKind};

    fn two_pad_board() -> (Design, ConstraintModel) {
        let part_a = Part { reference: "U1".into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part_a.clone()], ..Default::default() };
        let fp = FootprintInstance { id: "U1".into(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: LabelSide::Above };
        let design = Design {
            footprint_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![fp], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![Track { id: "trk1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 1000 }] }], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![] }),
            drawings: None,
        };
        (design, model)
    }

    #[test]
    fn builds_a_node_with_a_pad_and_a_two_segment_track() {
        let (design, model) = two_pad_board();
        let (node, layers) = build_node(&design, &model);
        assert!(layers.index_of("F.Cu").is_some());
        let segs = node.iter().filter(|(_, it)| matches!(it, Item::Segment(_))).count();
        assert_eq!(segs, 2, "a 3-point track must flatten into 2 segments");
        let pads = node.iter().filter(|(_, it)| matches!(it, Item::Solid(_))).count();
        assert!(pads >= 1);
    }
}
