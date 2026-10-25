//! `PAD::FlashLayer` and `PCB_VIA::FlashLayer` (`pcbnew/pad.cpp`, `pcbnew/pcb_track.cpp`) over the connectivity graph: the copper layers a plated
//! through-hole pad or a via keeps once the layers nothing connects it on are removed (the Remove Unused Pads dialog, `remove_unused_layers`).
//!
//! A pad or via whose mode is `KEEP_ALL` has its copper on every layer it spans, and `START_END_ONLY` on its ends only; `REMOVE_EXCEPT_START_AND_END` keeps the
//! outside layers (a via's own ends) and checks the others, `REMOVE_ALL` checks every layer. Checking is `CONNECTIVITY_DATA::IsConnectedOnLayer( item, layer,
//! { trace, arc, via, pad } )`: some other item of the same net that touches the pad or via and has copper on the layer, that is not one of the pad's
//! own footprint's pads and is not a second pad that removes unconnected layers too (two such pads do not make each other flash). Zones are not among the
//! types, so a zone fill never keeps a layer (`ZLO_FORCE_FLASHED`, a layer a zone is told to flash the item on, is not modelled).

use std::collections::BTreeMap;

use eda_drc::zone_copper::ZoneCopper;
use eda_model::footprint::PadKind;
use eda_model::fp_edit::pad_id;
use eda_model::ir::Design;
use eda_model::padstack::UnconnectedLayers;
use eda_model::ConstraintModel;

use crate::algo::{build_graph, ConnGraph};
use crate::items::ItemRef;

/// The layers the pads and vias that remove their unconnected layers have copper on. A pad or via that keeps all its layers has no entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flashed {
    /// Studio pad id (`REF.NUM`, `REF.NUM#k`) to the copper layers that have its ring, front to back.
    pub pads: BTreeMap<String, Vec<String>>,
    /// Via id to the copper layers that have its ring, front to back.
    pub vias: BTreeMap<String, Vec<String>>,
}

impl Flashed {
    /// Nothing removes any layer on this board.
    pub fn is_empty(&self) -> bool {
        self.pads.is_empty() && self.vias.is_empty()
    }
}

/// For each item of the graph that is a pad: footprint, number and `nth`. The pieces of a padstack pad (one item for each run of layers that shares a shape) are one pad.
pub(crate) fn pad_of_items(graph: &ConnGraph) -> Vec<Option<(String, String, u32)>> {
    let mut seen: std::collections::BTreeMap<(String, String), Vec<usize>> = std::collections::BTreeMap::new();
    let mut out = vec![None; graph.items.len()];
    for (i, it) in graph.items.iter().enumerate() {
        let ItemRef::Pad { reference, number } = &it.item else { continue };
        let group = it.pad_piece_of.unwrap_or(i);
        let groups = seen.entry((reference.clone(), number.clone())).or_default();
        let nth = match groups.iter().position(|g| *g == group) {
            Some(k) => k as u32 + 1,
            None => {
                groups.push(group);
                groups.len() as u32
            }
        };
        out[i] = Some((reference.clone(), number.clone(), nth));
    }
    out
}

/// Whether the board has a pad or via that removes layers (so that [`flashed_layers`] is worth asking).
pub fn removes_any_layer(design: &Design, model: &ConstraintModel) -> bool {
    let vias = design.drawings.as_ref().is_some_and(|d| d.via_extras.iter().any(|e| e.unconnected_layers().removes()));
    vias || design.placement.as_ref().is_some_and(|pl| {
        pl.footprints.iter().any(|fp| model.part(&fp.id).and_then(|p| model.footprint_of(p)).is_some_and(|f| f.pads.iter().any(|p| p.kind == PadKind::ThroughHole && p.ext.stack.unconnected.removes())))
    })
}

/// [`Flashed`] for the board.
pub fn flashed_layers(design: &Design, model: &ConstraintModel) -> Flashed {
    let mut out = Flashed::default();
    if !removes_any_layer(design, model) {
        return out;
    }
    let layers = &model.board.layers;
    let (Some(first), Some(last)) = (layers.first(), layers.last()) else { return out };
    let graph = build_graph(design, model, &ZoneCopper::default());

    // The mode of every pad, by (footprint, number, the k-th with that number) in the order the items were built (`placed_pads`' order, which is sorted by
    // number), and the item index of each pad's pieces.
    let mut pad_modes: BTreeMap<(String, String), Vec<UnconnectedLayers>> = BTreeMap::new();
    for fp in design.placement.iter().flat_map(|pl| pl.footprints.iter()) {
        let Some(footprint) = model.part(&fp.id).and_then(|p| model.footprint_of(p)) else { continue };
        let mut pads: Vec<&eda_model::Pad> = footprint.pads.iter().collect();
        pads.sort_by(|a, b| a.number.cmp(&b.number));
        for pad in pads {
            let mode = if pad.kind == PadKind::ThroughHole { pad.ext.stack.unconnected } else { UnconnectedLayers::KeepAll };
            pad_modes.entry((fp.id.clone(), pad.number.clone())).or_default().push(mode);
        }
    }

    // Which pad each item is (a padstack pad is several items): footprint, number and the k-th with that number.
    let pad_of_item = pad_of_items(&graph);
    let mode_of_pad = |reference: &str, number: &str, nth: u32| -> UnconnectedLayers { pad_modes.get(&(reference.to_string(), number.to_string())).and_then(|v| v.get(nth as usize - 1)).copied().unwrap_or_default() };

    // IsConnectedOnLayer( item, layer ) for the item `members` (the pieces of one pad, or the one item of a via).
    let connected_on = |members: &[usize], layer: usize, own_footprint: Option<&str>, own_mode: UnconnectedLayers| -> bool {
        members.iter().any(|&m| {
            graph.items[m].connected.iter().any(|&c| {
                let other = &graph.items[c];
                if members.contains(&c) || (other.layer_lo as usize) > layer || (other.layer_hi as usize) < layer || other.net != graph.items[m].net {
                    return false;
                }
                match &other.item {
                    ItemRef::ZoneOutline { .. } => false,
                    ItemRef::Pad { reference, number } => {
                        if own_footprint == Some(reference.as_str()) {
                            return false;
                        }
                        let nth = pad_of_item[c].as_ref().map(|p| p.2).unwrap_or(1);
                        let conditional = |m: UnconnectedLayers| matches!(m, UnconnectedLayers::RemoveExceptStartAndEnd | UnconnectedLayers::RemoveAll);
                        !(conditional(own_mode) && conditional(mode_of_pad(reference, number, nth)))
                    }
                    ItemRef::TrackSeg { .. } | ItemRef::Via { .. } => true,
                }
            })
        })
    };

    // Pads: one entry for each plated through-hole pad that removes layers.
    let mut by_pad: BTreeMap<(String, String, u32), Vec<usize>> = BTreeMap::new();
    for (i, p) in pad_of_item.iter().enumerate() {
        if let Some(key) = p {
            by_pad.entry(key.clone()).or_default().push(i);
        }
    }
    for ((reference, number, nth), members) in &by_pad {
        let mode = mode_of_pad(reference, number, *nth);
        if !mode.removes() {
            continue;
        }
        let kept: Vec<String> = layers
            .iter()
            .enumerate()
            .filter(|(k, name)| match mode {
                UnconnectedLayers::KeepAll => true,
                UnconnectedLayers::StartEndOnly => *name == first || *name == last,
                UnconnectedLayers::RemoveExceptStartAndEnd if *name == first || *name == last => true,
                _ => connected_on(members, *k, Some(reference), mode),
            })
            .map(|(_, name)| name.clone())
            .collect();
        out.pads.insert(pad_id(reference, number, *nth), kept);
    }

    // Vias.
    let extras = design.drawings.as_ref().map(|d| &d.via_extras);
    for (i, it) in graph.items.iter().enumerate() {
        let ItemRef::Via { via_id } = &it.item else { continue };
        let mode = extras.and_then(|e| e.iter().find(|x| &x.id == via_id)).map(|x| x.unconnected_layers()).unwrap_or_default();
        if !mode.removes() {
            continue;
        }
        let (lo, hi) = (it.layer_lo as usize, it.layer_hi as usize);
        let kept: Vec<String> = layers
            .iter()
            .enumerate()
            .filter(|(k, _)| *k >= lo && *k <= hi)
            .filter(|(k, _)| match mode {
                UnconnectedLayers::KeepAll => true,
                UnconnectedLayers::StartEndOnly => *k == lo || *k == hi,
                UnconnectedLayers::RemoveExceptStartAndEnd if *k == lo || *k == hi => true,
                _ => connected_on(&[i], *k, None, mode),
            })
            .map(|(_, name)| name.clone())
            .collect();
        out.vias.insert(via_id.clone(), kept);
    }
    out
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
    use super::*;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{Point, Track, Via};
    use eda_model::{Footprint, Pad, PadShape};

    /// R1 and R2 with a two-pad plated through-hole footprint on a four-layer board; the first pad of each is on net N1.
    fn through_hole_board(mode: UnconnectedLayers) -> (Design, ConstraintModel) {
        let (design, mut model) = two_pad_model();
        model.board.layers = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
        let pad = |n: &str, x: i64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (Nm::from_raw_shim(x), Nm::ZERO), size: (Nm::from_raw_shim(1_200_000), Nm::from_raw_shim(1_200_000)), shape: PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(Nm::from_raw_shim(400_000)), drill_slot: None, rot: 0, roundrect_ratio: None };
        let mut fp = Footprint { name: "0603".into(), pads: vec![pad("1", -800_000), pad("2", 800_000)], courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        fp.pads[0].ext.stack.unconnected = mode;
        model.footprints = vec![fp];
        (design, model)
    }

    #[test]
    fn a_board_that_keeps_every_layer_has_nothing_to_report() {
        let (design, model) = through_hole_board(UnconnectedLayers::KeepAll);
        assert!(!removes_any_layer(&design, &model));
        assert!(flashed_layers(&design, &model).is_empty());
    }

    #[test]
    fn a_pad_that_removes_unconnected_layers_keeps_the_ones_a_track_or_a_via_of_its_net_reaches() {
        let (mut design, model) = through_hole_board(UnconnectedLayers::RemoveExceptStartAndEnd);
        let at = pad_center(&design, &model, "R1", "1");
        // Nothing connected: only the outside layers (R2.1 is on the net too, but far away).
        let flashed = flashed_layers(&design, &model);
        assert_eq!(flashed.pads["R1.1"], ["F.Cu", "B.Cu"]);
        assert!(flashed.pads.contains_key("R2.1"), "the footprint is the same for R2");
        assert!(!flashed.pads.contains_key("R1.2"), "pad 2 keeps all its layers");
        // A track of the net that ends on the pad on In1.Cu keeps that layer.
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "In1.Cu".into(), width: Nm::from_raw_shim(200_000), pts: vec![at, Point { x: at.x, y: at.y + Nm::from_raw_shim(2_000_000) }], arc_mid_offset: None });
        assert_eq!(flashed_layers(&design, &model).pads["R1.1"], ["F.Cu", "In1.Cu", "B.Cu"]);
        // A track of another net there does not.
        design.routing.as_mut().unwrap().tracks[0].net = "N2".into();
        assert_eq!(flashed_layers(&design, &model).pads["R1.1"], ["F.Cu", "B.Cu"]);
        // A via of the net on the pad, from the front to In2.Cu, keeps the layers it spans.
        design.routing.as_mut().unwrap().tracks.clear();
        design.routing.as_mut().unwrap().vias.push(Via { id: "v1".into(), net: "N1".into(), at, drill: Nm::from_raw_shim(300_000), diameter: Nm::from_raw_shim(600_000), from_layer: "F.Cu".into(), to_layer: "In2.Cu".into() });
        assert_eq!(flashed_layers(&design, &model).pads["R1.1"], ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    }

    #[test]
    fn remove_all_takes_the_outside_layers_too_and_start_end_only_keeps_nothing_else() {
        let (design, model) = through_hole_board(UnconnectedLayers::RemoveAll);
        assert!(flashed_layers(&design, &model).pads["R1.1"].is_empty());
        let (design, model) = through_hole_board(UnconnectedLayers::StartEndOnly);
        assert_eq!(flashed_layers(&design, &model).pads["R1.1"], ["F.Cu", "B.Cu"]);
    }

    #[test]
    fn two_pads_that_both_remove_unconnected_layers_do_not_make_each_other_flash() {
        let (mut design, mut model) = through_hole_board(UnconnectedLayers::RemoveAll);
        // R2 on top of R1: the first pads touch, on the same net, on every layer.
        let at = pad_center(&design, &model, "R1", "1");
        let r2 = design.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "R2").unwrap();
        r2.at = Point { x: at.x + Nm::from_raw_shim(800_000), y: at.y };
        let flashed = flashed_layers(&design, &model);
        assert!(flashed.pads["R1.1"].is_empty() && flashed.pads["R2.1"].is_empty(), "{flashed:?}");
        // Were R2's pad to keep all its layers, it would be the one that makes R1's flash on all of them.
        model.footprints[0].pads[0].ext.stack.unconnected = UnconnectedLayers::KeepAll;
        let flashed = flashed_layers(&design, &model);
        assert!(flashed.is_empty(), "no pad removes anything now: {flashed:?}");
    }
}
