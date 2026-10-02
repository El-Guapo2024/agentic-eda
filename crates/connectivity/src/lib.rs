//! KiCad's connectivity engine and ratsnest, ported to Rust.
//!
//! Source-of-truth mapping (KiCad file -> module here):
//! - `pcbnew/connectivity/connectivity_items.{h,cpp}` (`CN_ITEM`, `CN_ANCHOR`,
//!   `CN_CLUSTER`) -> [`items`].
//! - `pcbnew/connectivity/connectivity_rtree.h` (`CN_RTREE`) -> [`grid`], a
//!   uniform grid standing in for the R-tree (see that module's docs).
//! - `pcbnew/connectivity/connectivity_algo.{h,cpp}`
//!   (`CN_CONNECTIVITY_ALGO`) -> [`algo`].
//! - `pcbnew/ratsnest/ratsnest_data.{h,cpp}` (`RN_NET`) -> [`ratsnest`].
//! - `pcbnew/connectivity/connectivity_data.cpp`'s
//!   `CONNECTIVITY_DATA::TestTrackEndpointDangling` -> [`dangling`].
//! - `thirdparty/delaunator` -> [`delaunay`] (see that module's docs).
//!
//! This crate works on the geometry IR (`eda_model::ir::Design`) plus the
//! `ConstraintModel`, not on a live, mutable board -- so the parts of
//! upstream that exist to make connectivity cheap to keep up to date while
//! a person drags things around (dirty-item tracking, incremental
//! add/remove, thread pools, a progress reporter) have no counterpart
//! here: we just rebuild the whole graph each time [`analyze`] is called.
//! Net propagation (`CN_CONNECTIVITY_ALGO::PropagateNets`, guessing a net
//! code for an item from its neighbours) is also out of scope -- every
//! item in our `Design` already carries its real net name, so there is
//! nothing to propagate. Zone connectivity is checked against each zone's
//! real, computed fill (`eda_zone_filler`, via `eda_drc::fill`), not its
//! raw outline -- see [`items`] for the (small) remaining differences from
//! KiCad's own triangulated-fill collision.

pub mod algo;
pub mod cleanup;
pub mod dangling;
pub mod delaunay;
pub mod dimension;
pub mod geom;
pub mod grid;
pub mod items;
pub mod ratsnest;
pub mod teardrop;

pub use algo::{build_graph, search_clusters, Cluster, ConnGraph};
pub use cleanup::{compute_cleanup, CleanupChange, CleanupKind, CleanupOptions, CleanupReport};
pub use dangling::{dangling_tracks_and_vias, DanglingItem, DanglingKind};
pub use items::{CnItem, ItemRef};
pub use ratsnest::{compute_ratsnest, RatsnestEdge};
pub use teardrop::generate_teardrops;

/// `eda_drc::run` with `DRCE_DANGLING_TRACK`/`DRCE_DANGLING_VIA` answered
/// by this crate's port of `CONNECTIVITY_DATA::TestTrackEndpointDangling`
/// over the real connectivity graph (zone fills, the per-layer via rule)
/// -- what `drc_test_provider_connectivity.cpp` does -- instead of
/// `eda_drc`'s geometric stand-in.
pub fn run_drc(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Vec<eda_drc::DrcViolation> {
    run_drc_with(design, model, false)
}

/// [`run_drc`] with the schematic-parity tests on or off (`aTestFootprints`).
pub fn run_drc_with(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, test_footprints: bool) -> Vec<eda_drc::DrcViolation> {
    eda_drc::run_with(design, model, Some(&dangling_violations), test_footprints)
}

fn dangling_violations(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Vec<eda_drc::DrcViolation> {
    use eda_drc::{DrcRefItem, DrcViolation, ErrorType};
    let graph = build_graph(design, model);
    let rt = design.routing.as_ref();
    let net_label = |n: &str| if n.is_empty() { "<no net>".to_string() } else { n.to_string() };
    let mut out = isolated_copper(design, &graph);
    let dangling: Vec<DrcViolation> = dangling_tracks_and_vias(&graph)
        .into_iter()
        .map(|d| match d.kind {
            DanglingKind::Track => {
                let base = d.id.split('#').next().unwrap_or(&d.id);
                let layer = rt.and_then(|rt| rt.tracks.iter().find(|t| t.id == base)).map(|t| t.layer.clone()).unwrap_or_default();
                let id = if d.id.contains('#') { d.id.clone() } else { format!("{}#0", d.id) };
                DrcViolation::new(ErrorType::TrackDangling, "", vec![DrcRefItem { description: format!("Track [{}] on {layer}", net_label(&d.net)), pos: (d.at.x, d.at.y), id }])
            }
            DanglingKind::Via => {
                let v = rt.and_then(|rt| rt.vias.iter().find(|v| v.id == d.id));
                let span = v.map(|v| format!("{}-{}", v.from_layer, v.to_layer)).unwrap_or_default();
                DrcViolation::new(ErrorType::ViaDangling, "", vec![DrcRefItem { description: format!("Via [{}] on {span}", net_label(&d.net)), pos: (d.at.x, d.at.y), id: d.id.clone() }])
            }
        })
        .collect();
    // `drc_test_provider_connectivity.cpp` reports dangling tracks/vias
    // first, then the isolated zone islands.
    let mut all = dangling;
    all.append(&mut out);
    all
}

/// `DRCE_ISOLATED_COPPER`: `CN_CONNECTIVITY_ALGO::FillIsolatedIslandsMap`
/// over `m_ZoneIsolatedIslandsMap` (every non-teardrop, non-rule-area zone
/// layer). A fill fragment is isolated when its connectivity cluster
/// (`SearchClusters( CSM_CONNECTIVITY_CHECK )`: same-net, net > 0) has no
/// pad (`CN_CLUSTER::IsOrphaned`); a zone layer with fill but no net is
/// never in any cluster, so it reports once, at outline 0.
fn isolated_copper(design: &eda_model::ir::Design, graph: &ConnGraph) -> Vec<eda_drc::DrcViolation> {
    use eda_drc::{DrcRefItem, DrcViolation, ErrorType};
    let mut out = Vec::new();
    let Some(rt) = design.routing.as_ref() else { return out };
    let clusters = search_clusters(graph);
    let mut cluster_of = vec![usize::MAX; graph.items.len()];
    for (ci, c) in clusters.iter().enumerate() {
        for &i in &c.items {
            cluster_of[i] = ci;
        }
    }
    let has_pad = |ci: usize| clusters[ci].items.iter().any(|&i| matches!(graph.items[i].item, ItemRef::Pad { .. }));
    for z in rt.zones.iter().filter(|z| !z.teardrop && !z.is_rule_area) {
        let frags: Vec<usize> = (0..graph.items.len()).filter(|&i| matches!(&graph.items[i].item, ItemRef::ZoneOutline { zone_id } if *zone_id == z.id)).collect();
        let report = |i: usize, out: &mut Vec<DrcViolation>| {
            let p = graph.items[i].anchors.first().copied().unwrap_or_default();
            let net = if z.net.is_empty() { "<no net>".to_string() } else { z.net.clone() };
            out.push(DrcViolation::new(ErrorType::IsolatedCopper, "", vec![DrcRefItem { description: format!("Zone [{net}] on {}", z.layer), pos: (p.x, p.y), id: z.id.clone() }]));
        };
        if z.net.is_empty() {
            if let Some(&first) = frags.first() {
                report(first, &mut out);
            }
            continue;
        }
        let islands: Vec<usize> = frags.iter().copied().filter(|&i| cluster_of[i] == usize::MAX || !has_pad(cluster_of[i])).collect();
        // What survives `ZONE_FILLER::Fill`'s island removal (same orphaned-
        // cluster test) is what DRC then finds isolated: with ALWAYS every
        // island is deleted unless *all* fragments are islands, with AREA
        // only those under the minimum area go.
        let all_islands = islands.len() == frags.len();
        for i in islands {
            let kept = match z.island_removal_mode {
                eda_model::ir::IslandRemovalMode::Never => true,
                eda_model::ir::IslandRemovalMode::Always => all_islands,
                eda_model::ir::IslandRemovalMode::Area => all_islands || geom::polygon_area(&graph.items[i].anchors) >= z.min_island_area as f64,
            };
            if kept {
                report(i, &mut out);
            }
        }
    }
    out
}

use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};

/// Everything this crate can say about a board's copper connectivity.
pub struct ConnectivityReport {
    pub graph: ConnGraph,
    pub clusters: Vec<Cluster>,
    pub ratsnest: Vec<RatsnestEdge>,
    pub dangling: Vec<DanglingItem>,
}

/// Build the connectivity graph, its clusters, the ratsnest and the
/// dangling-copper report, in one pass -- the batch equivalent of KiCad's
/// `CONNECTIVITY_DATA::Build` + `RecalculateRatsnest`.
pub fn analyze(design: &Design, model: &ConstraintModel) -> ConnectivityReport {
    let graph = algo::build_graph(design, model);
    let clusters = algo::search_clusters(&graph);
    let ratsnest = ratsnest::compute_ratsnest(&graph, &clusters);
    let dangling = dangling::dangling_tracks_and_vias(&graph);
    ConnectivityReport { graph, clusters, ratsnest, dangling }
}

/// `eda check`'s connectivity gate: KiCad's own DRC type names
/// (`unconnected_items`, `track_dangling`, `via_dangling`) as
/// [`CheckResult`]s, one per violation -- exactly what
/// `DRC_TEST_PROVIDER_CONNECTIVITY::Run` in
/// `pcbnew/drc/drc_test_provider_connectivity.cpp` reports against each
/// zone's real fill (minus `DRCE_ISOLATED_COPPER`'s own violation report --
/// `eda_zone_filler` already *removes* islands per each zone's
/// `island_removal_mode` the same way KiCad's filler does, it just doesn't
/// separately flag that it did so as a reportable violation here -- and
/// post-machined-layer checks, which need backdrill/post-machining fields
/// this workspace's model doesn't have).
pub fn check(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let report = analyze(design, model);
    let mut out = Vec::with_capacity(report.ratsnest.len() + report.dangling.len());

    for edge in &report.ratsnest {
        out.push(
            CheckResult::fail(
                "unconnected_items",
                format!("net {}", edge.net),
                format!("ratsnest: {} is not connected by copper from ({:.3}, {:.3}) mm to ({:.3}, {:.3}) mm", edge.net, um_to_mm(edge.from.x), um_to_mm(edge.from.y), um_to_mm(edge.to.x), um_to_mm(edge.to.y)),
            )
            .with_detail(serde_json::json!({
                "net": edge.net,
                "from": [edge.from.x, edge.from.y],
                "to": [edge.to.x, edge.to.y],
            })),
        );
    }

    for d in &report.dangling {
        let (check, what) = match d.kind {
            DanglingKind::Track => ("track_dangling", "track"),
            DanglingKind::Via => ("via_dangling", "via"),
        };
        out.push(
            CheckResult::fail(check, format!("net {}", d.net), format!("{what} {} has a dangling end at ({:.3}, {:.3}) mm", d.id, um_to_mm(d.at.x), um_to_mm(d.at.y)))
                .with_detail(serde_json::json!({ "net": d.net, "id": d.id, "at": [d.at.x, d.at.y] })),
        );
    }

    out
}

fn um_to_mm(um: eda_model::ir::Um) -> f64 {
    um as f64 / 1000.0
}

/// Tiny fixture models shared by this crate's own unit tests (`lib.rs`,
/// `items.rs`, `algo.rs`, ...) -- not part of the public API.
#[cfg(test)]
pub(crate) mod tests_support {
    use eda_model::ir::*;
    use eda_model::{BoardRules, ConstraintModel, Net, Part, Pin, PinKind};

    /// Two pads on one net, no track between them: exactly one ratsnest
    /// edge, no dangling anything (there is no copper yet to dangle).
    pub(crate) fn two_pad_model() -> (Design, ConstraintModel) {
        let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), symbol: None, datasheet: None, pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_um: None, edge: None };
        let model = ConstraintModel {
            parts: vec![part("R1"), part("R2")],
            nets: vec![Net { name: "N1".into(), pins: vec!["R1.1".into(), "R2.1".into()] }],
            board: BoardRules { layers: vec!["F.Cu".into(), "B.Cu".into()], ..Default::default() },
            ..Default::default()
        };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "t".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection {
                outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
                footprints: vec![
                    FootprintInstance { id: "R1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
                    FootprintInstance { id: "R2".into(), at: Point { x: 15_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
                ],
                modules: vec![],
            }),
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
        };
        (design, model)
    }

    /// The board-space center of `reference`'s pad `number`, computed the
    /// same way the router/exporter would -- so a test track that says it
    /// lands "on the pad" actually does, whatever a package's built-in pad
    /// offsets happen to be.
    pub(crate) fn pad_center(design: &Design, model: &ConstraintModel, reference: &str, number: &str) -> Point {
        let part = model.part(reference).expect("part");
        let fp = design.placement.as_ref().expect("placement").footprints.iter().find(|f| f.id == reference).expect("footprint instance");
        let pads = eda_model::footprint::placed_pads(model, part, fp).expect("placed_pads");
        pads.iter().find(|p| p.number == number).expect("pad number").center
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{pad_center, two_pad_model};
    use super::*;
    use eda_model::ir::Track;

    #[test]
    fn two_unconnected_pads_yield_one_ratsnest_edge() {
        let (design, model) = two_pad_model();
        let report = analyze(&design, &model);
        assert_eq!(report.ratsnest.len(), 1, "{:?}", report.ratsnest.iter().map(|e| &e.net).collect::<Vec<_>>());
        assert_eq!(report.ratsnest[0].net, "N1");
        assert!(report.dangling.is_empty());
    }

    #[test]
    fn a_routed_track_between_the_same_two_pads_clears_the_ratsnest() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec!["R1.1".into(), "R2.1".into()], layer: "F.Cu".into(), width: 200, pts: vec![a, b], arc_mid_offset: None });
        let report = analyze(&design, &model);
        assert!(report.ratsnest.is_empty(), "{:?}", report.ratsnest.iter().map(|e| &e.net).collect::<Vec<_>>());
        assert!(report.dangling.is_empty());
    }

    #[test]
    fn check_reports_kicad_type_names() {
        let (design, model) = two_pad_model();
        let checks = check(&design, &model);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].check, "unconnected_items");
    }
}
