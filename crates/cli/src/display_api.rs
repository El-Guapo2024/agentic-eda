//! `POST /api/display/clearances`: the clearance outlines of the board editor's active copper layer (Preferences > PCB Editor > Display Options,
//! "Clearance Outlines", UI-LEFT-3 P3-6).
//!
//! KiCad draws a track's, a via's and a pad's clearance outline on `CLEARANCE_LAYER_FOR( layer )` (`PCB_PAINTER::draw( const PCB_TRACK* )`,
//! `draw( const PCB_ARC* )`, `draw( const PCB_VIA* )`, `draw( const PAD* )`, pcbnew/pcb_painter.cpp) -- only the active copper layer's is visible
//! (`PCB_EDIT_FRAME::SetActiveLayer`: "Only show pad, via and track clearances when a copper layer is active and then only show the clearance layer
//! for that copper layer") -- each the item's `GetOwnClearance( layer )`: `DRC_ENGINE::GetCachedOwnClearance`, `EvalRules( CLEARANCE_CONSTRAINT,
//! item, nullptr, layer )` (`HOLE_CLEARANCE_CONSTRAINT` for a non-plated pad), its minimum or 0. The studio paints them (components/canvas/painter.ts);
//! this answers the clearances, by the board's rule engine (`eda_drc::rule_board`: the board setup's implicit rules, the net classes, the local
//! overrides and the `.kicad_dru`), read-only.
//!
//! Request `{ "layer": "F.Cu" }`. Reply `{ "ok": true, "layer": "F.Cu", "tracks": { "<track id>": [nm, ..] }, "vias": { "<via id>": nm },
//! "pads": { "<REF.NUMBER[#k]>": nm } }`: a track is a polyline in the studio and a run of segments in KiCad, so it gets one clearance per segment
//! (an arc is one); the items not on the layer are left out.

use crate::board;
use eda_drc::constraints::{CType, Units};
use eda_drc::rule_board::RuleBoard;
use eda_drc::rule_item::RuleItem;
use eda_model::footprint::PadKind;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::rc::Rc;

fn fail(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// `DRC_ENGINE::GetCachedOwnClearance( item, layer )`: `EvalRules( type, item, nullptr, layer )`'s minimum when it has one, else 0.
fn own_clearance(rb: &RuleBoard, item: &Rc<RuleItem>, ctype: CType, layer: &str) -> i64 {
    let c = rb.engine().eval_rules(ctype, Some(item), None, Some(layer), None);
    if c.is_null() {
        return 0;
    }
    c.value.min.unwrap_or(0)
}

/// `POST /api/display/clearances`.
pub fn clearances(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(layer) = req.get("layer").and_then(Value::as_str) else { return fail("name a copper layer") };
    let (meta, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return fail(board::reasons(&e)),
    };
    let drc_board = eda_drc::board::build(&design, &model);
    let dru = model.board.custom_rules_text.as_deref().map(|text| crate::inspect_api::expanded_rules(dir, &meta.intent, &design, &model, text));
    let rb = RuleBoard::new(&design, &model, &drc_board, Units::Mm, dru.as_deref());
    if dru.as_deref().is_some_and(|t| t.to_lowercase().contains("fromto")) {
        rb.enable_from_to_with(&crate::fill_api::cached(dir).map(|k| k.copper(&design)).unwrap_or_default());
    }
    if !rb.board_copper_layers().iter().any(|l| l == layer) {
        return fail(format!("{layer} is not a copper layer of the board"));
    }
    // the segments of a track in their order (`DrcBoard::tracks` keeps the IR's)
    let mut tracks: Map<String, Value> = Map::new();
    for (i, seg) in drc_board.tracks.iter().enumerate() {
        if seg.layer != layer {
            continue;
        }
        let id = seg.id.rsplit_once('#').map_or(seg.id.as_str(), |(base, _)| base);
        let c = own_clearance(&rb, &rb.segment_item_at(i), CType::Clearance, layer);
        if let Value::Array(a) = tracks.entry(id.to_string()).or_insert_with(|| Value::Array(Vec::new())) {
            a.push(json!(c));
        }
    }
    let copper = rb.board_copper_layers();
    let at = |l: &str| copper.iter().position(|c| c == l);
    let mut vias: Map<String, Value> = Map::new();
    for (i, v) in drc_board.vias.iter().enumerate() {
        // `PCB_VIA::ViewGetLayers`: the clearance layers of the copper layers its drill spans
        let (Some(a), Some(b), Some(l)) = (at(&v.from_layer), at(&v.to_layer), at(layer)) else { continue };
        if l < a.min(b) || l > a.max(b) {
            continue;
        }
        vias.insert(v.id.clone(), json!(own_clearance(&rb, &rb.via_item_at(i), CType::Clearance, layer)));
    }
    let mut pads: Map<String, Value> = Map::new();
    for (pid, idx) in rb.pad_ids() {
        let pad = &drc_board.pads[*idx];
        // `PAD::ViewGetLayers`: the clearance layers of its copper layers (a non-plated hole has the board's, for its hole's outline)
        if !(pad.layers.iter().any(|l| l == layer || l == "*.Cu") || pad.kind != PadKind::Smd) {
            continue;
        }
        let Some(item) = rb.pad_item_at(*idx) else { continue };
        let ctype = if pad.kind == PadKind::NonPlatedHole { CType::HoleClearance } else { CType::Clearance };
        pads.insert(pid.clone(), json!(own_clearance(&rb, &item, ctype, layer)));
    }
    json!({ "ok": true, "layer": layer, "tracks": tracks, "vias": vias, "pads": pads })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect_api::tests::{scratch, setup};
    use eda_model::units::mm;

    /// A pad's clearance is its net class's (`EvalRules( CLEARANCE_CONSTRAINT, pad, nullptr, layer )`), a track's and a via's too, each segment its own;
    /// the board's minimum clearance under it; an item off the layer is left out.
    #[test]
    fn every_item_on_the_layer_gets_its_own_clearance() {
        let dir = scratch("display_clearances");
        let mut rules = eda_model::BoardRules::default();
        rules.clearance = mm(0.2);
        rules.net_classes.push(eda_model::NetClass { name: "HV".into(), nets: vec!["HV".into()], track_width: None, clearance: Some(mm(0.5)), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        setup(&dir, mm(2.0), rules);
        let (_, mut design, _) = board::load(&dir).unwrap();
        let p = |x: f64, y: f64| eda_model::ir::Point { x: mm(x), y: mm(y) };
        let rt = design.routing.get_or_insert_with(|| eda_model::ir::RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() });
        rt.tracks.push(eda_model::ir::Track { id: "t1".into(), net: "HV".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.25), pts: vec![p(5.0, 5.0), p(8.0, 5.0), p(8.0, 8.0)], arc_mid_offset: None });
        rt.tracks.push(eda_model::ir::Track { id: "t2".into(), net: "GND".into(), pins: vec![], layer: "B.Cu".into(), width: mm(0.25), pts: vec![p(5.0, 9.0), p(8.0, 9.0)], arc_mid_offset: None });
        rt.vias.push(eda_model::ir::Via { id: "v1".into(), net: "GND".into(), at: p(9.0, 9.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        board::save(&dir, &design).unwrap();
        let r = clearances(&dir, br#"{"layer":"F.Cu"}"#);
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(r["tracks"]["t1"], json!([500_000, 500_000]), "one per segment, the class's: {r}");
        assert!(r["tracks"].get("t2").is_none(), "B.Cu's track is not on F.Cu: {r}");
        assert_eq!(r["vias"]["v1"], json!(200_000), "{r}");
        assert_eq!((r["pads"]["U1.1"].clone(), r["pads"]["U1.2"].clone()), (json!(500_000), json!(200_000)), "{r}");
        assert_eq!(clearances(&dir, br#"{"layer":"F.SilkS"}"#)["ok"], json!(false));
    }

    /// UI-LEFT-3 P3-7: the state carries `GetHolePlatingThickness()`, KiCad's `ADVANCED_CFG::m_HoleWallThickness` (0.020 mm), for the hole walls.
    #[test]
    fn the_state_carries_kicads_hole_plating_thickness() {
        let dir = scratch("display_plating");
        setup(&dir, mm(1.0), eda_model::BoardRules::default());
        let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
        let state = crate::studio::state(&dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap();
        assert_eq!(state["board_rules"]["hole_plating_nm"], json!(20_000));
    }
}
