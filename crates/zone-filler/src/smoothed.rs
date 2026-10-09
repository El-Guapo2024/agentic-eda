//! `ZONE::BuildSmoothedPoly` (`pcbnew/zone.cpp`): the polygon a zone's fill starts from.
//!
//! The zone's outline, with
//!
//! * the outlines of same-net zones it touches merged in before any smoothing (so a corner at an intersection of two
//!   zones is not smoothed into a divot, issue 2752) -- unless a higher-priority zone of another net encloses this one
//!   completely, which isolates it (issue 13915);
//! * the board outline taken out;
//! * its corners chamfered or filleted by the zone's `corner_radius` (`ZONE_SETTINGS::SMOOTHING_CHAMFER` /
//!   `SMOOTHING_FILLET`; never for a teardrop, whose shape is final);
//! * for the fill to start from, an *apron*: the smoothed outline grown by the zone's minimum thickness within the
//!   same-net envelope, so the deflate/inflate cycle of the minimum-width pass does not cut divots between zones that
//!   meet; the plain smoothed outline is what the fill is finally trimmed to (`aMaxExtents`).
//!
//! `m_ZoneKeepExternalFillets` (the pre-5.1 behaviour of keeping the fillets of concave corners) is a board setting this
//! IR does not carry; it is off, KiCad's default.

use crate::{bboxes_intersect, chain_bbox, chain_from_ir, corner, inflate_bbox, FillInput, FillZoneRef};
use eda_model::ir::{Zone, ZoneSmoothing};
use eda_shape_poly_set::{CornerStrategy, ShapePolySet};

/// What `fillSingleZone` gets out of `BuildSmoothedPoly( maxExtents, aLayer, boardOutline, &smoothedPoly )`.
pub struct Smoothed {
    /// `smoothedPoly`: the fill's starting polygon, `aSmoothedOutline` of `fillCopperZone`.
    pub with_apron: ShapePolySet,
    /// `maxExtents`: the fill is trimmed to this at the end, `aMaxExtents`.
    pub max_extents: ShapePolySet,
}

fn smooth(zone: &Zone, poly: &ShapePolySet, max_error: i64) -> ShapePolySet {
    if zone.teardrop {
        return poly.clone();
    }
    match zone.smoothing {
        ZoneSmoothing::Chamfer => corner::chamfer(poly, zone.corner_radius),
        ZoneSmoothing::Fillet => corner::fillet(poly, zone.corner_radius, max_error),
        ZoneSmoothing::None => poly.clone(),
    }
}

/// `ZONE::GetInteractingZones`: the other copper zones on `layer` whose bounding box touches this one's, split into the
/// ones of the same net whose outline collides with it and the ones of another net.
fn interacting_zones<'a>(zone: &Zone, layer: &str, others: &'a [FillZoneRef], zone_net: Option<&str>, zone_outline: &ShapePolySet) -> (Vec<&'a FillZoneRef>, Vec<&'a FillZoneRef>) {
    let epsilon = 1;
    let bbox = inflate_bbox(chain_bbox(&chain_from_ir(&zone.outline)), epsilon);
    let mut same_net_colliding = Vec::new();
    let mut other_net = Vec::new();
    for candidate in others {
        if candidate.layer != layer || (!candidate.id.is_empty() && candidate.id == zone.id) || candidate.teardrop {
            continue;
        }
        if !bboxes_intersect(chain_bbox(&candidate.outline), bbox) {
            continue;
        }
        if candidate.net.as_deref() == zone_net {
            // `m_Poly->Collide( candidate->m_Poly )`: touching counts, so grow by a micrometre.
            let mut grown = ShapePolySet::from_outline(candidate.outline.clone());
            grown.inflate(epsilon, CornerStrategy::ChamferAllCorners, 5, false);
            let mut probe = zone_outline.clone();
            probe.boolean_intersection(&grown);
            if !probe.is_empty() {
                same_net_colliding.push(candidate);
            }
        } else {
            other_net.push(candidate);
        }
    }
    (same_net_colliding, other_net)
}

pub fn build_smoothed_poly(zone: &Zone, layer: &str, input: &FillInput, max_error: i64) -> Smoothed {
    let zone_net: Option<&str> = if zone.net.is_empty() { None } else { Some(zone.net.as_str()) };
    // Zone arcs are flattened already (this IR keeps no arcs in an outline).
    let flattened = ShapePolySet::from_outline(chain_from_ir(&zone.outline));
    let smooth_requested = !zone.teardrop && matches!(zone.smoothing, ZoneSmoothing::Chamfer | ZoneSmoothing::Fillet) && zone.corner_radius > 0;

    let mut smoothed = flattened.clone();

    // Same-net, intersecting zones are merged in so the smoothing does not leave divots where they meet. After the
    // smoothing everything outside this zone is taken out again.
    let (same_net_colliding, diff_net_intersecting) = if smooth_requested || input.other_zones.iter().any(|o| o.layer == layer) {
        interacting_zones(zone, layer, &input.other_zones, zone_net, &flattened)
    } else {
        (Vec::new(), Vec::new())
    };
    for same in &same_net_colliding {
        let same_bbox = chain_bbox(&same.outline);
        // The same-net zone might get knocked out along its border by a higher-priority zone of another net (issue 12797).
        let mut diff_net_poly = ShapePolySet::new();
        for diff in &diff_net_intersecting {
            if diff.higher_priority_than(same.teardrop, same.priority, &same.id) && bboxes_intersect(chain_bbox(&diff.outline), same_bbox) {
                diff_net_poly.boolean_add(&ShapePolySet::from_outline(diff.outline.clone()));
            }
        }
        // After unioning those together, check whether they enclose this zone completely: then it is isolated from the
        // outer zone, not connected to it (issue 13915).
        let mut isolated = false;
        if diff_net_poly.outline_count() > 0 {
            let mut this_poly = flattened.clone();
            this_poly.boolean_subtract(&diff_net_poly);
            isolated = this_poly.outline_count() == 0;
        }
        if !isolated {
            smoothed.boolean_add(&ShapePolySet::from_outline(same.outline.clone()));
        }
    }

    // `BuildSmoothedPoly( .., aBoardOutline )`: the board's outlines with their cutouts, when they are well-formed (`m_brdOutlinesValid`).
    if let Some(board) = input.board_outline.as_ref().filter(|b| !input.board_outline_invalid && !b.is_empty()) {
        smoothed.boolean_intersection(board);
    }

    let with_same_net = smoothed.clone();
    if smooth_requested {
        smoothed = smooth(zone, &smoothed, max_error);
    }

    // The apron: `maxExtents` grown by the minimum thickness, within the same-net-intersecting-zones envelope.
    let mut poly = flattened.clone();
    poly.inflate(zone.min_thickness, CornerStrategy::RoundAllCorners, max_error as i32, false);
    poly.boolean_intersection(&with_same_net);
    let mut with_apron = smoothed.clone();
    with_apron.boolean_intersection(&poly);

    smoothed.boolean_intersection(&flattened);
    Smoothed { with_apron, max_extents: smoothed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Point;

    fn zone(smoothing: ZoneSmoothing, radius: i64) -> Zone {
        let pt = |x, y| Point { x, y };
        Zone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![pt(0, 0), pt(10_000, 0), pt(10_000, 10_000), pt(0, 10_000)], smoothing, corner_radius: radius, ..Default::default() }
    }

    #[test]
    fn an_unsmoothed_zone_starts_from_its_outline() {
        let s = build_smoothed_poly(&zone(ZoneSmoothing::None, 0), "F.Cu", &FillInput::default(), 5);
        assert!((s.max_extents.area() - 100_000_000.0).abs() < 1.0);
        assert!((s.with_apron.area() - 100_000_000.0).abs() < 1.0);
    }

    #[test]
    fn a_fillet_rounds_the_corners_of_the_outline() {
        let s = build_smoothed_poly(&zone(ZoneSmoothing::Fillet, 2_000), "F.Cu", &FillInput::default(), 5);
        let want = 100_000_000.0 - (4.0 - std::f64::consts::PI) * 4_000_000.0;
        assert!((s.max_extents.area() - want).abs() / want < 0.001, "{} vs {want}", s.max_extents.area());
        // the starting polygon is the same shape here (no same-net zone to bridge to)
        assert!((s.with_apron.area() - s.max_extents.area()).abs() < 10.0);
    }

    #[test]
    fn a_chamfer_cuts_the_corners_of_the_outline() {
        let s = build_smoothed_poly(&zone(ZoneSmoothing::Chamfer, 1_000), "F.Cu", &FillInput::default(), 5);
        let want = 100_000_000.0 - 4.0 * 0.5 * 1_000.0 * 1_000.0;
        assert!((s.max_extents.area() - want).abs() < 100.0, "{} vs {want}", s.max_extents.area());
    }
}
