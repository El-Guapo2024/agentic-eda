//! `ZONE_FILLER::Fill` (`pcbnew/zone_filler.cpp`): every zone of a board filled together.
//!
//! * The zones are filled from the highest priority down (KiCad schedules them in dependency waves: a zone waits for the
//!   higher-priority zones of another net that overlap it, which is the same order). A zone is knocked out by the *filled
//!   copper* of those zones, not by their outlines (`ZONE::TransformShapeToPolygon`), so each fill is handed on to the zones
//!   below it as soon as it is done.
//! * Then the islands: fragments whose copper cluster has no pad are removed according to each zone's island mode
//!   ([`crate::islands`]).
//! * Then the iterative refill (`ADVANCED_CFG::m_ZoneFillIterativeRefill`, on by default): a zone that lost islands frees the
//!   space they filled, so every lower-priority zone that overlaps it is refilled from its cached pre-knockout fill
//!   (`refillZoneFromCache`) against the higher-priority fills as they are now, and loses its own islands in turn.
//! * Last, islands that lie mostly outside the board outline are dropped.

use crate::{fill_copper_zone, islands, refill_zone_from_cache, FillInput, FillZoneRef};
use eda_model::ir::{IslandRemovalMode, Zone};
use eda_shape_poly_set::ShapePolySet;
use std::sync::Arc;

/// `ZONE::HigherPriority` over zone indices: teardrops first, then the assigned priority, then (KiCad: the UUID) the id.
fn higher(zones: &[Zone], a: usize, b: usize) -> bool {
    let (za, zb) = (&zones[a], &zones[b]);
    if za.teardrop != zb.teardrop {
        return za.teardrop;
    }
    if za.priority != zb.priority {
        return za.priority > zb.priority;
    }
    za.id > zb.id
}

fn bbox_of_outline(z: &Zone) -> (i64, i64, i64, i64) {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in &z.outline {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x0, y0, x1, y1)
}

/// Removes the islands `find_isolated_islands` reports for zone `zi` from `fill` according to its mode; whether any polygon
/// was deleted. (`ISLAND_REMOVAL_MODE::AREA` keeps the island if its outline's area reaches `min_island_area`.)
fn remove_islands(zone: &Zone, fill: &mut ShapePolySet, islands: &[usize]) -> bool {
    if islands.is_empty() || matches!(zone.island_removal_mode, IslandRemovalMode::Never) {
        return false;
    }
    // If *all* the polygons are islands, do not remove any of them.
    if islands.len() == fill.outline_count() {
        return false;
    }
    let mut removed = false;
    let mut sorted = islands.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    for idx in sorted {
        if idx >= fill.polys.len() {
            continue;
        }
        let remove = match zone.island_removal_mode {
            IslandRemovalMode::Always => true,
            IslandRemovalMode::Area => eda_clipper2::area(&fill.polys[idx][0]).abs() < zone.min_island_area as f64,
            IslandRemovalMode::Never => false,
        };
        if remove {
            fill.polys.remove(idx);
            removed = true;
        }
    }
    removed
}

/// `ZONE_FILLER::Fill` for `zones` (copper pours, each on its own layer): the fill of each, in the order given.
///
/// `input` holds the board's pads, tracks, vias, keepouts and outline; its `other_zones` are ignored (the zones themselves
/// are each other's neighbours here).
pub fn fill_board<F>(zones: &[Zone], input: &FillInput, clearance_fn: F, max_error: i64) -> Vec<ShapePolySet>
where
    F: Fn(Option<&str>, Option<&str>) -> i64,
{
    // Unique ids make the tie-break between equal priorities stable.
    let zones: Vec<Zone> = zones.iter().enumerate().map(|(i, z)| if z.id.is_empty() { Zone { id: format!("~{i:06}"), ..z.clone() } } else { z.clone() }).collect();
    let n = zones.len();

    let mut input = input.clone();
    input.other_zones = zones
        .iter()
        .map(|z| FillZoneRef {
            id: z.id.clone(),
            net: if z.net.is_empty() { None } else { Some(z.net.clone()) },
            layer: z.layer.clone(),
            outline: crate::chain_from_ir(&z.outline),
            priority: z.priority,
            teardrop: z.teardrop,
            clearance: z.clearance,
            fill: None,
        })
        .collect();

    // Highest priority first: a zone is filled after every zone that can knock it out.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| if higher(&zones, a, b) { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater });

    let mut fills: Vec<ShapePolySet> = vec![ShapePolySet::new(); n];
    let mut caches: Vec<ShapePolySet> = vec![ShapePolySet::new(); n];
    for &i in &order {
        if zones[i].outline.len() < 3 {
            continue;
        }
        let stage = fill_copper_zone(&zones[i], &zones[i].layer, &input, &clearance_fn, max_error);
        input.other_zones[i].fill = Some(Arc::new(stage.fill.clone()));
        fills[i] = stage.fill;
        caches[i] = stage.pre_knockout;
    }

    // Islands, by connectivity across the whole board.
    fn detect(zones: &[Zone], fills: &[ShapePolySet], input: &FillInput, max_error: i64) -> Vec<Vec<usize>> {
        let view: Vec<(&Zone, &ShapePolySet)> = zones.iter().zip(fills.iter()).collect();
        islands::find_isolated_islands(&view, input, max_error)
    }
    let found = detect(&zones, &fills, &input, max_error);
    let mut zones_with_removed: Vec<usize> = Vec::new();
    for i in 0..n {
        if remove_islands(&zones[i], &mut fills[i], &found[i]) {
            zones_with_removed.push(i);
        }
        input.other_zones[i].fill = Some(Arc::new(fills[i].clone()));
    }

    // Iterative refill: zones below a zone that lost islands get the freed space.
    if !zones_with_removed.is_empty() {
        let worst = input.worst_clearance.max(zones.iter().map(|z| z.clearance).max().unwrap_or(0)).max(input.hole_clearance);
        let mut to_refill: Vec<usize> = Vec::new();
        for &zi in &zones_with_removed {
            let b = bbox_of_outline(&zones[zi]);
            let inflated = (b.0 - worst, b.1 - worst, b.2 + worst, b.3 + worst);
            for (li, l) in zones.iter().enumerate() {
                if li == zi || !higher(&zones, zi, li) || l.layer != zones[zi].layer || to_refill.contains(&li) || l.outline.len() < 3 {
                    continue;
                }
                let lb = bbox_of_outline(l);
                if lb.0 > inflated.2 || lb.2 < inflated.0 || lb.1 > inflated.3 || lb.3 < inflated.1 {
                    continue;
                }
                to_refill.push(li);
            }
        }
        if !to_refill.is_empty() {
            // Dependency order: highest priority first, so each refill sees the fills above it as they now are.
            to_refill.sort_by(|&a, &b| if higher(&zones, a, b) { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater });
            for &i in &to_refill {
                let refilled = refill_zone_from_cache(&zones[i], &zones[i].layer, &caches[i], &input, &clearance_fn, max_error);
                input.other_zones[i].fill = Some(Arc::new(refilled.clone()));
                fills[i] = refilled;
            }
            // Re-run the island detection for the refilled zones.
            let found = detect(&zones, &fills, &input, max_error);
            for &i in &to_refill {
                remove_islands(&zones[i], &mut fills[i], &found[i]);
                input.other_zones[i].fill = Some(Arc::new(fills[i].clone()));
            }
        }
    }

    // Islands that lie mostly outside the board edge (`island_area >= 3 x min_thickness^2` and less than half inside).
    if let Some(outline) = input.board_outline.as_ref().filter(|o| o.len() >= 3) {
        let board = ShapePolySet::from_outline(outline.clone());
        for (zi, fill) in fills.iter_mut().enumerate() {
            let min_area = 3.0 * (zones[zi].min_thickness as f64) * (zones[zi].min_thickness as f64);
            let mut j = fill.polys.len();
            while j > 0 {
                j -= 1;
                let area = eda_clipper2::area(&fill.polys[j][0]).abs();
                if area < min_area {
                    continue;
                }
                let mut island = ShapePolySet::from_outline(fill.polys[j][0].clone());
                island.boolean_intersection(&board);
                if island.area() < area / 2.0 {
                    fill.polys.remove(j);
                }
            }
        }
    }
    fills
}
