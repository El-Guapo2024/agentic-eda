//! UI-LEFT-3 B3-1 and B3-6 (slow tier): DRC exclusions of the markers the studio places by every place KiCad's order could pick (`eda_kicad::drc_markers`)
//! reach kicad-cli, and the rule-area checks the studio leaves to it -- a keepout on two layers, a custom `(constraint disallow ..)` on items that are not
//! in a keepout, a text on Edge.Cuts -- are reported by it and can be excluded.

use super::tests::{scratch, setup};
use super::{load, save, step};
use eda_model::units::{mm, Nm};
use eda_ops::Cmd;
use serde_json::{json, Value};

fn slow_tests_off() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return true;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return true;
    }
    false
}

/// The DRC exclusion the web client sends for a violation of the studio's report (`drcExclusionSpec`).
fn drc_exclusion_of(v: &Value) -> eda_model::ir::DrcExclusion {
    let strings = |key: &str| -> Vec<String> { v["items"].as_array().unwrap().iter().map(|i| i[key].as_str().unwrap_or("").to_string()).collect() };
    eda_model::ir::DrcExclusion {
        check: v["type"].as_str().unwrap().to_string(),
        items: strings("uuid"),
        ids: strings("id"),
        positions_nm: v["marker_nm"].as_array().unwrap().iter().map(|p| [Nm::new(p[0].as_i64().unwrap()), Nm::new(p[1].as_i64().unwrap())]).collect(),
        comment: String::new(),
    }
}

fn row_key(v: &Value) -> String {
    let uuids: Vec<&str> = v["items"].as_array().unwrap().iter().map(|i| i["uuid"].as_str().unwrap_or("")).collect();
    format!("{} {}", v["type"].as_str().unwrap_or(""), uuids.join(" "))
}

fn describe(v: &Value) -> String {
    let items: Vec<&str> = v["items"].as_array().unwrap().iter().map(|i| i["description"].as_str().unwrap_or("")).collect();
    format!("{} [{}] {}", v["type"].as_str().unwrap_or(""), v["description"].as_str().unwrap_or(""), items.join(" | "))
}

/// UI-LEFT-3 B3-1, B3-6: on the two-part board, U2 moved over U1's courtyard (each footprint with a silkscreen corner, so the two corners cross), an arc track
/// beside a straight one, a track laid along a GND pour after the pour was filled, a keepout on F.Cu and B.Cu crossed by a track and holding a via, a custom
/// rule disallowing VCC's vias, and a text on Edge.Cuts. kicad-cli reports each; every one of them is excluded, and the next run reports each as excluded by
/// kicad-cli itself (`kicad_matched`).
#[test]
fn the_markers_placed_by_their_candidates_reach_kicad_cli() {
    if slow_tests_off() {
        return;
    }
    let dir = scratch("review_drc_candidates");
    setup(&dir);
    // a silkscreen corner on the footprint: a line along its top and one down its right side
    let intent = dir.join("intent.yaml");
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    for (a, b) in [((-1.5, -0.8), (1.5, -0.8)), ((1.5, -0.8), (1.5, 0.8))] {
        let shape: eda_model::fp_art::FpShape = serde_json::from_value(json!({ "shape": { "kind": "segment", "layer": "F.SilkS", "stroke_width": 120000,
            "start": { "x": mm(a.0).get(), "y": mm(a.1).get() }, "end": { "x": mm(b.0).get(), "y": mm(b.1).get() } } }))
        .unwrap();
        model.footprints[0].art.shapes.push(shape);
    }
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    let (_, mut design, _) = load(&dir).unwrap();
    // U2 over U1's courtyard, its pads clear of U1's
    let u2 = &mut design.placement.as_mut().unwrap().footprints[1];
    u2.at = eda_model::ir::Point { x: mm(6.0), y: mm(5.9) };
    let p = |x: f64, y: f64| eda_model::ir::Point { x: mm(x), y: mm(y) };
    // an arc track (VCC) bulging down to y = 11 and a GND track 0.3 mm below its middle
    let (start, mid, end) = (p(10.0, 10.0), p(12.0, 11.0), p(14.0, 10.0));
    let mut arc: eda_model::ir::Track = serde_json::from_value(json!({ "id": "trk_arc", "net": "VCC", "layer": "F.Cu", "width": mm(0.2).get(), "pts": [] })).unwrap();
    arc.pts = eda_model::ir::tessellate_arc(start, mid, end, eda_model::ir::TRACK_ARC_SEGMENTS);
    arc.arc_mid_offset = Some(eda_model::ir::Point { x: mid.x - start.x, y: mid.y - start.y });
    let mut routing = design.routing.take().unwrap_or_else(|| serde_json::from_value(json!({ "tracks": [], "vias": [], "zones": [] })).unwrap());
    routing.tracks.push(arc);
    // a GND pour on B.Cu, and a keepout on both copper layers
    let zone = |v: Value| -> eda_model::ir::Zone { serde_json::from_value(v).unwrap() };
    routing.zones.push(zone(json!({ "id": "zone_gnd", "net": "GND", "layer": "B.Cu", "outline": [
        { "x": mm(2.0).get(), "y": mm(13.0).get() }, { "x": mm(18.0).get(), "y": mm(13.0).get() }, { "x": mm(18.0).get(), "y": mm(18.0).get() }, { "x": mm(2.0).get(), "y": mm(18.0).get() }] })));
    routing.zones.push(zone(json!({ "id": "zone_keepout", "net": "", "layer": "F.Cu", "layers": ["F.Cu", "B.Cu"], "is_rule_area": true, "keepout_tracks": true, "keepout_vias": true,
        "outline": [{ "x": mm(14.0).get(), "y": mm(2.0).get() }, { "x": mm(19.0).get(), "y": mm(2.0).get() }, { "x": mm(19.0).get(), "y": mm(8.0).get() }, { "x": mm(14.0).get(), "y": mm(8.0).get() }] })));
    design.routing = Some(routing);
    save(&dir, &design).unwrap();
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(11.0, 11.3), p(13.0, 11.3)] }, false, "test").unwrap();
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "B.Cu".into(), width: mm(0.2), pts: vec![p(11.5, 5.0), p(16.0, 5.0)] }, false, "test").unwrap();
    step(&dir, Cmd::AddVia { net: "GND".into(), x: mm(17.0), y: mm(7.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }, false, "test").unwrap();
    step(&dir, Cmd::AddVia { net: "VCC".into(), x: mm(10.0), y: mm(1.5), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }, false, "test").unwrap();
    step(&dir, Cmd::SetCustomRules { text: "(version 1)\n(rule no_vcc_vias\n  (constraint disallow via)\n  (condition \"A.NetName == 'VCC'\"))\n".into() }, false, "test").unwrap();
    let text: eda_model::ir::Text = serde_json::from_value(json!({ "content": "EDGE", "at": { "x": mm(3.0).get(), "y": mm(19.0).get() }, "layer": "Edge.Cuts", "size_nm": mm(1.0).get(), "stroke_width": mm(0.15).get() })).unwrap();
    step(&dir, Cmd::AddText { text }, false, "test").unwrap();
    // the pour filled, then a VCC track laid 0.1 mm off its edge: the fill the next run judges is the one made before the track
    let refill = eda_kicad_engine::DrcOptions { refill_zones: true, all_track_errors: true, ..Default::default() };
    crate::kicad_engine::drc_with_options(&dir, refill).expect("kicad-cli ran");
    step(&dir, Cmd::AddTrack { net: "VCC".into(), layer: "B.Cu".into(), width: mm(0.2), pts: vec![p(4.0, 12.8), p(8.0, 12.8)] }, false, "test").unwrap();
    let keep = eda_kicad_engine::DrcOptions { refill_zones: false, all_track_errors: true, ..Default::default() };
    let first = crate::kicad_engine::drc_with_options(&dir, keep).expect("kicad-cli ran");
    let all: Vec<Value> = first["violations"].as_array().unwrap().clone();
    eprintln!("{}", all.iter().map(describe).collect::<Vec<_>>().join("\n"));
    let names = |v: &Value| v["items"].as_array().unwrap().iter().map(|i| i["description"].as_str().unwrap_or("").to_lowercase()).collect::<Vec<_>>().join(" | ");
    let ids = |v: &Value| v["items"].as_array().unwrap().iter().filter_map(|i| i["id"].as_str().map(String::from)).collect::<Vec<_>>();
    let wanted: Vec<(&str, Box<dyn Fn(&Value) -> bool>)> = vec![
        ("courtyards", Box::new(|v: &Value| v["type"] == "courtyards_overlap")),
        ("silk", Box::new(move |v: &Value| v["type"] == "silk_overlap" && !names(v).contains("field"))),
        ("arc", Box::new(move |v: &Value| v["type"] == "clearance" && ids(v).iter().any(|i| i.starts_with("trk_arc")))),
        ("zone", Box::new(move |v: &Value| v["type"] == "clearance" && names(v).contains("zone"))),
        ("keepout", Box::new(|v: &Value| v["type"] == "items_not_allowed")),
        ("edge text", Box::new(|v: &Value| v["type"] == "text_on_edge_cuts")),
    ];
    let mut rows: Vec<Value> = Vec::new();
    for (what, hit) in &wanted {
        let found: Vec<&Value> = all.iter().filter(|v| hit(v)).collect();
        assert!(!found.is_empty(), "kicad-cli reports the {what} finding");
        rows.extend(found.into_iter().cloned());
    }
    assert!(rows.iter().filter(|v| v["type"] == "items_not_allowed").count() >= 3, "the keepout's track and via and the custom rule's via: {}", rows.iter().map(describe).collect::<Vec<_>>().join("\n"));
    rows.sort_by_key(row_key);
    rows.dedup_by_key(|v| row_key(v));
    step(&dir, Cmd::AddDrcExclusions { exclusions: rows.iter().map(drc_exclusion_of).collect() }, false, "ui").unwrap();
    let after = crate::kicad_engine::drc_with_options(&dir, keep).expect("kicad-cli ran");
    let now: Vec<&Value> = after["violations"].as_array().unwrap().iter().collect();
    let mut bad = Vec::new();
    for r in &rows {
        match now.iter().find(|v| row_key(v) == row_key(r)) {
            Some(v) if v["excluded"] == true && v["kicad_matched"] == true => {}
            Some(v) => bad.push(format!("not matched by kicad-cli: {} (excluded {}, kicad_matched {}, tried {})", describe(v), v["excluded"], v["kicad_matched"], r["marker_nm"])),
            None => bad.push(format!("gone: {}", describe(r))),
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    eprintln!("{} exclusions matched by kicad-cli", rows.len());
    let _ = std::fs::remove_dir_all(&dir);
}

/// UI-LEFT-3 B3-1: two chamfered pads with their other corners rounded (`TransformRoundChamferedRectToPolygon`, the arcs cut to the board's 0.005 mm): U2
/// beside U1, the clearance between U1.2 and U2.1 excluded and matched by kicad-cli at the place the polygon's sides give.
#[test]
fn a_chamfered_pad_with_rounded_corners_reaches_kicad_cli() {
    if slow_tests_off() {
        return;
    }
    let dir = scratch("review_drc_round_chamfer");
    setup(&dir);
    let intent = dir.join("intent.yaml");
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    for pad in &mut model.footprints[0].pads {
        pad.shape = eda_model::PadShape::ChamferedRect;
        pad.roundrect_ratio = Some(0.25);
        pad.ext.chamfer_ratio = Some(0.2);
        pad.ext.chamfer_corners = eda_model::ir::ChamferCorners { top_left: true, top_right: false, bottom_left: false, bottom_right: false };
    }
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    let (_, mut design, _) = load(&dir).unwrap();
    // U2.1 (GND) off U1.2's (VCC) lower right corner, which is rounded, its own upper left corner chamfered: the two corners face each other
    let u2 = &mut design.placement.as_mut().unwrap().footprints[1];
    u2.at = eda_model::ir::Point { x: mm(7.7), y: mm(5.8) };
    save(&dir, &design).unwrap();
    let every = eda_kicad_engine::DrcOptions { all_track_errors: true, ..Default::default() };
    let first = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    let pads = |v: &Value| v["type"] == "clearance" && v["items"].as_array().unwrap().iter().all(|i| i["description"].as_str().unwrap_or("").to_lowercase().contains("pad "));
    let rows: Vec<Value> = first["violations"].as_array().unwrap().iter().filter(|v| pads(v)).cloned().collect();
    assert!(!rows.is_empty(), "two pads too near: {}", first["violations"].as_array().unwrap().iter().map(describe).collect::<Vec<_>>().join("\n"));
    step(&dir, Cmd::AddDrcExclusions { exclusions: rows.iter().map(drc_exclusion_of).collect() }, false, "ui").unwrap();
    let after = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    let waived: Vec<&Value> = after["violations"].as_array().unwrap().iter().filter(|v| pads(v)).collect();
    assert_eq!(waived.len(), rows.len());
    for v in &waived {
        assert!(v["excluded"] == true && v["kicad_matched"] == true, "kicad-cli matched the exclusion: {} tried {}", describe(v), rows.iter().find(|r| row_key(r) == row_key(v)).map(|r| r["marker_nm"].to_string()).unwrap_or_default());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// UI-LEFT-3 B3-1: a through-hole pad that removes its unconnected layers is not flashed where nothing meets it (`PAD::FlashLayer`), and the copper test
/// takes its hole there (`GetEffectiveHoleShape`): a GND track over U1.2's (VCC, connected to nothing) copper but clear of its hole is a clearance finding
/// against the hole, placed where the hole meets the track, and its exclusion is matched by kicad-cli.
#[test]
fn an_unflashed_pads_exclusion_reaches_kicad_cli() {
    if slow_tests_off() {
        return;
    }
    let dir = scratch("review_drc_unflashed");
    setup(&dir);
    let intent = dir.join("intent.yaml");
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    for pad in &mut model.footprints[0].pads {
        pad.kind = eda_model::PadKind::ThroughHole;
        pad.drill = Some(mm(0.4));
        pad.ext.stack.unconnected = eda_model::padstack::UnconnectedLayers::RemoveAll;
    }
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    let p = |x: f64, y: f64| eda_model::ir::Point { x: mm(x), y: mm(y) };
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(5.0, 5.0), p(5.0, 5.45), p(7.0, 5.45)] }, false, "test").unwrap();
    let every = eda_kicad_engine::DrcOptions { all_track_errors: true, ..Default::default() };
    let first = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    let all = first["violations"].as_array().unwrap().clone();
    eprintln!("{}", all.iter().map(describe).collect::<Vec<_>>().join("\n"));
    let near = |v: &Value| (v["type"] == "clearance" || v["type"] == "shorting_items" || v["type"] == "hole_clearance") && v["items"].as_array().unwrap().iter().any(|i| i["id"] == "U1.2");
    let rows: Vec<Value> = all.iter().filter(|v| near(v)).cloned().collect();
    assert!(!rows.is_empty(), "the track is too near U1.2's hole");
    step(&dir, Cmd::AddDrcExclusions { exclusions: rows.iter().map(drc_exclusion_of).collect() }, false, "ui").unwrap();
    let after = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    for v in after["violations"].as_array().unwrap().iter().filter(|v| near(v)) {
        assert!(v["excluded"] == true && v["kicad_matched"] == true, "kicad-cli matched the exclusion: {} tried {}", describe(v), rows.iter().find(|r| row_key(r) == row_key(v)).map(|r| r["marker_nm"].to_string()).unwrap_or_default());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// UI-LEFT-3 B3-1: `copper_sliver` -- a GND pour on B.Cu whose outline has a narrow spike, its minimum width small enough that the filler does not prune it
/// (`postKnockoutMinWidthPrune` leaves a fill alone under 0.004 mm): the fill ends in the spike's point (`DRC_TEST_PROVIDER_SLIVER_CHECKER`). The finding names
/// no item; its marker is that point, which the studio finds on the fill kicad-cli made ([`eda_kicad::drc_markers::sliver_points`]), and kicad-cli matches the
/// exclusion after the refill.
#[test]
fn a_copper_sliver_exclusion_reaches_kicad_cli() {
    if slow_tests_off() {
        return;
    }
    let dir = scratch("review_drc_sliver");
    setup(&dir);
    let p = |x: f64, y: f64| eda_model::ir::Point { x: mm(x), y: mm(y) };
    step(&dir, Cmd::AddZone { net: "GND".into(), layer: "B.Cu".into(), outline: vec![p(1.0, 9.0), p(10.0, 9.0), p(18.0, 9.6), p(10.0, 10.2), p(10.0, 19.0), p(1.0, 19.0)] }, false, "test").unwrap();
    let (_, mut design, _) = load(&dir).unwrap();
    for z in &mut design.routing.as_mut().unwrap().zones {
        z.min_thickness = Nm::new(2_000);
    }
    save(&dir, &design).unwrap();
    let refill = eda_kicad_engine::DrcOptions { refill_zones: true, all_track_errors: true, ..Default::default() };
    let first = crate::kicad_engine::drc_with_options(&dir, refill).expect("kicad-cli ran");
    let all = first["violations"].as_array().unwrap().clone();
    let rows: Vec<Value> = all.iter().filter(|v| v["type"] == "copper_sliver").cloned().collect();
    assert!(!rows.is_empty(), "a sliver: {}", all.iter().map(describe).collect::<Vec<_>>().join("\n"));
    assert!(rows.iter().all(|v| !v["marker_nm"].as_array().unwrap().is_empty()), "the spike's point is offered: {rows:#?}");
    step(&dir, Cmd::AddDrcExclusions { exclusions: rows.iter().take(1).map(drc_exclusion_of).collect() }, false, "ui").unwrap();
    let after = crate::kicad_engine::drc_with_options(&dir, refill).expect("kicad-cli ran");
    let now: Vec<&Value> = after["violations"].as_array().unwrap().iter().filter(|v| v["type"] == "copper_sliver").collect();
    assert_eq!(now.len(), rows.len());
    for v in &now {
        assert!(v["excluded"] == true && v["kicad_matched"] == true, "kicad-cli matched the exclusion: {} tried {}", describe(v), rows[0]["marker_nm"]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
