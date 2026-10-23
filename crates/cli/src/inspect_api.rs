//! HTTP handlers for Inspect > Clearance Resolution and Constraints Resolution (`BOARD_INSPECTION_TOOL::InspectClearance` / `InspectConstraints`):
//! `POST /api/inspect/{clearance,constraints}`.
//!
//! Read-only and stateless: every call reads `design.json` fresh, builds the board's rule engine (`eda_drc::rule_board`: the implicit rules of the board setup and
//! the net classes, and the `.kicad_dru` of the board) and answers with the report pages (`eda_drc::inspect`): KiCad's own report text, the winning constraint of
//! each page as numbers and every rule it considered. Nothing is written and nothing is checked: this shows which rule wins where, kicad-cli stays the only DRC engine.
//!
//! Request: `{ "a": "<id>", "b": "<id>", "a_at": [x, y], "b_at": [x, y], "active_layer": "F.Cu", "units": "mm" }` for the clearance report,
//! `{ "item": "<id>", "at": [x, y], "units": "mm" }` for the constraints report. An id is what the studio selects things by (a footprint's reference, a pad's
//! `REF.NUMBER`, a track, via, zone, graphic, text or group id); a track is a polyline here and a run of segments in KiCad, so `at` (millimetres) says which one.

use crate::board;
use eda_drc::constraints::Units;
use eda_drc::inspect;
use eda_drc::rule_board::RuleBoard;
use eda_model::ir::Point;
use serde_json::{json, Value};
use std::path::Path;

fn fail(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// `[x, y]` in millimetres as the IR's micrometres.
fn at_of(req: &Value, key: &str) -> Option<Point> {
    let a = req.get(key)?.as_array()?;
    let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    Some(Point { x: (x * 1000.0).round() as i64, y: (y * 1000.0).round() as i64 })
}

fn units_of(req: &Value) -> Units {
    Units::parse(req.get("units").and_then(Value::as_str).unwrap_or("mm"))
}

/// Run `f` on the rule board of the design in `dir`.
fn with_rule_board(dir: &Path, units: Units, f: impl FnOnce(&RuleBoard) -> Value) -> Value {
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return fail(board::reasons(&e)),
    };
    let drc_board = eda_drc::board::build(&design, &model);
    let dru = model.board.custom_rules_text.clone();
    let rb = RuleBoard::new(&design, &model, &drc_board, units, dru.as_deref());
    f(&rb)
}

/// `POST /api/inspect/clearance`.
pub fn clearance(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (Some(a_id), Some(b_id)) = (req.get("a").and_then(Value::as_str), req.get("b").and_then(Value::as_str)) else {
        return fail("name two items: a and b");
    };
    if a_id == b_id {
        return fail("Select two different items for clearance resolution.");
    }
    let active = req.get("active_layer").and_then(Value::as_str).map(str::to_string);
    let (a_at, b_at) = (at_of(&req, "a_at"), at_of(&req, "b_at"));
    with_rule_board(dir, units_of(&req), |rb| {
        let (a, b) = match (rb.item(a_id, a_at), rb.item(b_id, b_at)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return fail(e),
        };
        // `reportClearance` after the groups are resolved: two items that are one.
        if a.id == b.id {
            return fail("Select two different items for clearance resolution.");
        }
        inspect::inspect_clearance(rb, a, b, active.as_deref(), rb.units).to_json()
    })
}

/// `POST /api/inspect/constraints`.
pub fn constraints(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(id) = req.get("item").and_then(Value::as_str) else {
        return fail("name an item");
    };
    let at = at_of(&req, "at");
    with_rule_board(dir, units_of(&req), |rb| match rb.item(id, at) {
        Ok(item) => inspect::inspect_constraints(rb, item, rb.units).to_json(),
        Err(e) => fail(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Design, FootprintInstance, PlacementSection, Provenance, Side};
    use eda_model::{ConstraintModel, Footprint, Net, NetClass, Pad, PadKind, PadShape, Part, Pin, PinKind};

    struct Scratch(std::path::PathBuf);
    impl std::ops::Deref for Scratch {
        type Target = std::path::PathBuf;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("eda_cli_inspect_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    /// One footprint, `U1`, with a pad `1` on net HV above a pad `2` on net GND `pitch` micrometres apart (0.4 mm square copper: the copper gap is `pitch - 400`),
    /// the board's rules as given.
    pub(crate) fn setup(dir: &Path, pitch: i64, rules: eda_model::BoardRules) {
        let pad = |n: &str, y: i64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (0, y), size: (400, 400), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
        let fp = Footprint { name: "PAIR2".into(), pads: vec![pad("1", -pitch / 2), pad("2", pitch - pitch / 2)], courtyard: Some((2000, pitch + 1000)), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        let part = Part {
            reference: "U1".into(),
            mpn: None,
            lcsc: None,
            value: Some("PAIR".into()),
            package: Some("PAIR2".into()),
            footprint: Some("PAIR2".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_nm: None,
            symbol: None,
            datasheet: None,
            edge: None,
        };
        let model = ConstraintModel {
            parts: vec![part],
            nets: vec![Net { name: "HV".into(), pins: vec!["U1.1".into()] }, Net { name: "GND".into(), pins: vec!["U1.2".into()] }],
            footprints: vec![fp],
            board: rules,
            ..Default::default()
        };
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();
        let design = Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection {
                outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
                footprints: vec![FootprintInstance { id: "U1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 0, side: Side::Top, label: Default::default() }],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        board::save(dir, &design).unwrap();
        let meta = board::Meta { intent: intent_path.display().to_string(), snap_nm: 100, spacing_nm: 300 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    }

    fn hv_rules(dru: Option<&str>) -> eda_model::BoardRules {
        eda_model::BoardRules {
            clearance: 200,
            net_classes: vec![NetClass { name: "HV".into(), nets: vec!["HV".into()], track_width: None, clearance: Some(500), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }],
            custom_rules_text: dru.map(String::from),
            ..Default::default()
        }
    }

    fn body(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    fn resolved_min(reply: &Value) -> Option<i64> {
        reply["pages"][0]["resolutions"][0]["min"].as_i64()
    }

    #[test]
    fn the_clearance_report_names_the_winner_and_the_rules_it_considered() {
        let dir = scratch("net_class");
        setup(&dir, 1_000, hv_rules(None));
        let reply = clearance(&dir, &body(json!({"a": "U1.1", "b": "U1.2", "active_layer": "F.Cu"})));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["title"], "Clearance Report");
        let page = &reply["pages"][0];
        assert_eq!(page["title"], "F.Cu");
        assert_eq!(resolved_min(&reply), Some(500_000), "the HV class's 0.5 mm: {reply}");
        assert_eq!(page["resolutions"][0]["rule"], "netclass 'HV'");
        let lines = page["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(lines.contains("<h7>Clearance resolution for:</h7>"), "{lines}");
        assert!(lines.contains("Pad 1 [HV] of U1 on F.Cu [netclass HV and Default]") || lines.contains("Pad 1 [HV] of U1 on F.Cu [netclass HV"), "{lines}");
        assert!(lines.contains("Resolved min clearance: 0.5 mm."), "{lines}");
        assert!(page["resolutions"][0]["considered"].as_array().unwrap().len() >= 3);
        // the same item twice is refused, as KiCad refuses it
        let same = clearance(&dir, &body(json!({"a": "U1.1", "b": "U1.1"})));
        assert_eq!((same["ok"].clone(), same["message"].clone()), (json!(false), json!("Select two different items for clearance resolution.")));
    }

    #[test]
    fn a_rules_file_rule_beats_the_net_class_and_a_bad_file_is_reported() {
        let dir = scratch("dru");
        setup(&dir, 1_000, hv_rules(Some("(version 1)\n(rule \"hv pads\"\n\t(condition \"A.NetClass == 'HV' && B.Type == 'Pad'\")\n\t(constraint clearance (min 0.6mm)))\n")));
        let reply = clearance(&dir, &body(json!({"a": "U1.1", "b": "U1.2"})));
        assert_eq!(resolved_min(&reply), Some(600_000), "{reply}");
        assert_eq!(reply["pages"][0]["resolutions"][0]["rule"], "rule 'hv pads'");
        assert_eq!(reply["compile_error"], json!(null));
        // a file with a mistake in it: the implicit rules alone, and the report says so
        let bad = scratch("dru_bad");
        setup(&bad, 1_000, hv_rules(Some("(version 1)\n(rule x (constraint clearance (min 1)))\n")));
        let reply = clearance(&bad, &body(json!({"a": "U1.1", "b": "U1.2"})));
        assert_eq!(resolved_min(&reply), Some(500_000), "{reply}");
        assert!(reply["compile_error"].as_str().unwrap().starts_with("ERROR: Missing units"), "{reply}");
        let text = reply["pages"][0]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("Report incomplete: could not compile custom design rules."), "{text}");
    }

    #[test]
    fn same_net_items_have_no_clearance_and_the_other_pages_come_with_the_items() {
        let dir = scratch("pages");
        setup(&dir, 1_000, hv_rules(None));
        // a footprint with two different pads is itself; two pads on different nets get the per-layer clearance page and the physical clearances page
        let reply = clearance(&dir, &body(json!({"a": "U1.1", "b": "U1.2"})));
        let titles: Vec<_> = reply["pages"].as_array().unwrap().iter().map(|p| p["title"].as_str().unwrap().to_string()).collect();
        assert_eq!(titles, ["F.Cu", "Physical Clearances"], "{reply}");
        // a footprint against a pad: no copper pairs, so only the physical clearances page
        let fp = clearance(&dir, &body(json!({"a": "U1", "b": "U1.1"})));
        assert_eq!(fp["ok"], json!(true), "{fp}");
        // unknown ids say so
        assert_eq!(clearance(&dir, &body(json!({"a": "U1.1", "b": "nope"})))["ok"], json!(false));
    }

    #[test]
    fn the_constraints_report_of_a_pad_has_the_pages_a_pad_has() {
        let dir = scratch("constraints");
        setup(&dir, 1_000, hv_rules(Some("(version 1)(rule \"fat pads\" (condition \"A.Type == 'Pad'\") (constraint assertion \"A.Size_X >= 0.5mm\"))")));
        let reply = constraints(&dir, &body(json!({"item": "U1.1"})));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["title"], "Constraints Report");
        let titles: Vec<_> = reply["pages"].as_array().unwrap().iter().map(|p| p["title"].as_str().unwrap().to_string()).collect();
        assert_eq!(titles, ["Solder Mask", "Solder Paste", "Keepouts", "Assertions"], "{reply}");
        let assertions = reply["pages"][3]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(assertions.contains("Checking rule assertion 'A.Size_X &gt;= 0.5mm'."), "{assertions}");
        assert!(assertions.contains("--&gt; Assertion failed. &lt;--"), "{assertions}");
        let keepouts = reply["pages"][2]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(keepouts.contains("Item allowed at current location."), "{keepouts}");
    }

    #[test]
    fn a_track_is_the_segment_the_click_was_nearest_to_and_the_report_follows_the_frames_units() {
        use eda_ops::Cmd;
        let dir = scratch("track");
        setup(&dir, 3_000, hv_rules(None));
        // a two-segment track on GND: 4 mm up, then 6 mm along
        let pts = vec![Point { x: 2_000, y: 2_000 }, Point { x: 2_000, y: 6_000 }, Point { x: 8_000, y: 6_000 }];
        board::step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 250, pts }, false, "test").unwrap();
        let (_, design, _) = board::load(&dir).unwrap();
        let id = design.routing.unwrap().tracks[0].id.clone();
        let first = clearance(&dir, &body(json!({"a": "U1.1", "b": id, "b_at": [2.0, 3.0]})));
        let second = clearance(&dir, &body(json!({"a": "U1.1", "b": id, "b_at": [6.0, 6.0]})));
        let text = |r: &Value| r["pages"][0]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(text(&first).contains("Track [GND] on F.Cu, length 4.0000 mm [netclass Default]"), "{}", text(&first));
        assert!(text(&second).contains("Track [GND] on F.Cu, length 6.0000 mm [netclass Default]"), "{}", text(&second));
        // with no point: the first segment
        let none = clearance(&dir, &body(json!({"a": "U1.1", "b": id})));
        assert!(text(&none).contains("length 4.0000 mm"), "{}", text(&none));
        // the constraints report of the track: its width, the board floor and the class's width
        let c = constraints(&dir, &body(json!({"item": id, "at": [6.0, 6.0]})));
        let titles: Vec<_> = c["pages"].as_array().unwrap().iter().map(|p| p["title"].as_str().unwrap().to_string()).collect();
        assert_eq!(titles, ["Track Width", "Solder Mask", "Keepouts", "Assertions"], "{c}");
        let width = &c["pages"][0]["resolutions"][0];
        assert_eq!((width["min"].clone(), width["opt"].clone()), (json!(200_000), json!(200_000)), "{c}");
        // the units: the same width in mils
        let mils = constraints(&dir, &body(json!({"item": id, "units": "mils"})));
        let t = mils["pages"][0]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(t.contains("Resolved width constraints: min 7.87402 mils; opt 7.87402 mils; max <i>undefined</i>."), "{t}");
        assert!(t.contains("length 157.48 mils"), "a track's description is in the frame's units too: {t}");
        // a via: the pages of a via, its description by its layers
        board::step(&dir, Cmd::AddVia { net: "GND".into(), x: 9_000, y: 3_000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }, false, "test").unwrap();
        let (_, design, _) = board::load(&dir).unwrap();
        let via = design.routing.unwrap().vias[0].id.clone();
        let v = constraints(&dir, &body(json!({"item": via})));
        let vt: Vec<_> = v["pages"].as_array().unwrap().iter().map(|p| p["title"].as_str().unwrap().to_string()).collect();
        assert_eq!(vt, ["Via Diameter", "Via Annular Width", "Hole Size", "Solder Mask", "Keepouts", "Assertions"], "{v}");
        let head = v["pages"][0]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n");
        assert!(head.contains("Via [GND] on all copper layers [netclass Default]"), "{head}");
        // the via and a pad: the hole pages come with them
        let hv = clearance(&dir, &body(json!({"a": via, "b": "U1.1"})));
        let titles: Vec<_> = hv["pages"].as_array().unwrap().iter().map(|p| p["title"].as_str().unwrap().to_string()).collect();
        assert!(titles.contains(&"Hole".to_string()) && titles.contains(&"F.Cu".to_string()) && titles.contains(&"Clearance".to_string()) == false, "{titles:?}");
    }

    /// The proof against kicad-cli: the clearance the report resolves is the one kicad-cli's DRC enforces at that spot -- a violation 10 um under it, none 10 um over.
    #[test]
    fn the_resolved_clearance_is_what_kicad_cli_enforces() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("skipped: kicad-cli not found");
            return;
        }
        let cases: [(&str, Option<&str>, i64); 3] = [
            ("net class", None, 500),
            ("explicit rule", Some("(version 1)\n(rule \"hv pads\"\n\t(condition \"A.NetClass == 'HV' && B.Type == 'Pad'\")\n\t(constraint clearance (min 0.65mm)))\n"), 650),
            ("loosening rule", Some("(version 1)\n(rule \"loose\"\n\t(condition \"A.NetName == 'HV'\")\n\t(constraint clearance (min 0.3mm)))\n"), 300),
        ];
        for (name, dru, expected_nm) in cases {
            // what the report says, on a board whose pads are far apart
            let dir = scratch(&format!("proof_{}", name.replace(' ', "_")));
            setup(&dir, 3_000, hv_rules(dru));
            let reply = clearance(&dir, &body(json!({"a": "U1.1", "b": "U1.2"})));
            let resolved = resolved_min(&reply).unwrap_or_else(|| panic!("{name}: {reply}"));
            assert_eq!(resolved, expected_nm * 1000, "{name}: {reply}");
            let um = resolved / 1000;
            // kicad-cli on the same board with the pads (copper gap) 10 um under and 10 um over it
            for (delta, violation) in [(-10, true), (10, false)] {
                let board_dir = scratch(&format!("proof_{}_{delta}", name.replace(' ', "_")));
                setup(&board_dir, 400 + um + delta, hv_rules(dru));
                let (_, design, model) = board::load(&board_dir).unwrap();
                let report = eda_kicad_engine::drc_scratch(&design, &model).unwrap_or_else(|e| panic!("{name}: kicad-cli: {e:?}"));
                let clearance_violations: Vec<_> = report.violations.iter().filter(|v| v.kind == "clearance").collect();
                assert_eq!(!clearance_violations.is_empty(), violation, "{name}: gap {} um against a resolved {um} um: {:?}", um + delta, report.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
            }
        }
    }
}
