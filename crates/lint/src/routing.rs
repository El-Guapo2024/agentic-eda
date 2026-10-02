//! `routing_track_width`: net-class track-width conformance. A net that
//! belongs to a class must carry that class's assigned width, not merely
//! clear the board's manufacturability floor -- a power net routed at
//! signal width is exactly the failure the class exists to prevent, and it
//! looks clean to KiCad's own `track_width` rule, which only knows the
//! absolute minimum. KiCad has no notion of "the intent assigned this net
//! a class", so this is ours.
//!
//! One finding per track segment (an arc is one segment), named
//! `<track id>#<segment>` the way the exported board names them.

use crate::finding::{Check, Finding, Item};
use eda_model::ir::{Design, Point};
use eda_model::ConstraintModel;

pub fn check(design: &Design, model: &ConstraintModel) -> Vec<Finding> {
    let mut out = Vec::new();
    let Some(rt) = design.routing.as_ref() else { return out };
    let rules = &model.board;
    for t in &rt.tracks {
        if t.net.is_empty() {
            continue;
        }
        let want = rules.width_of(&t.net);
        if t.width >= want {
            continue;
        }
        let class = rules.class_of(&t.net).map(|c| c.name.as_str()).unwrap_or("default");
        let mut segments: Vec<(String, Point)> = Vec::new();
        if let Some((start, _mid, _end)) = t.arc() {
            segments.push((format!("{}#0", t.id), start));
        } else {
            segments.extend(t.pts.windows(2).enumerate().map(|(i, w)| (format!("{}#{i}", t.id), w[0])));
        }
        for (id, at) in segments {
            let item = Item { description: format!("Track [{}] on {}", t.net, t.layer), pos: (at.x, at.y), id };
            out.push(Finding::new(Check::NetClassTrackWidth, format!("width {} < {} required by net class {class}", t.width, want), vec![item]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Provenance, RoutingSection, Track};

    fn design(width: i64) -> Design {
        let track = Track { id: "t1".into(), net: "A".into(), pins: vec![], layer: "F.Cu".into(), width, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 1000 }], arc_mid_offset: None };
        Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![track], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
        }
    }

    fn model_with_class(width: i64) -> ConstraintModel {
        let mut m = ConstraintModel::default();
        m.board.net_classes.push(eda_model::NetClass { name: "power".into(), nets: vec!["A".into()], track_width: Some(width), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        m
    }

    #[test]
    fn a_track_narrower_than_its_class_reports_once_per_segment() {
        let v = check(&design(200), &model_with_class(400));
        assert_eq!(v.len(), 2, "{v:#?}");
        assert_eq!(v[0].check, "routing_track_width");
        assert_eq!(v[0].items[0].id, "t1#0");
        assert_eq!(v[1].items[0].id, "t1#1");
        assert!(v[0].description.contains("required by net class power"), "{}", v[0].description);
    }

    #[test]
    fn a_track_at_its_class_width_is_clean() {
        assert!(check(&design(400), &model_with_class(400)).is_empty());
    }
}
