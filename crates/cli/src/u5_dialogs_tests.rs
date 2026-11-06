//! UI-LEFT X5 and B5 (`task-u5-dialogs`): the exports' small differences and the text variables, proved through the studio's own routes and,
//! in the slow tier (`EDA_SLOW_TESTS=1`), through the real kicad-cli.

use crate::board::{load, save, step, Meta};
use crate::sch_hierarchy_tests::{kicad_cli_ready, scratch};
use eda_model::ir::Point;
use eda_model::units::mm;
use eda_model::ConstraintModel;
use eda_ops::Cmd;
use serde_json::{json, Value};
use std::path::PathBuf;

/// A board with an empty schematic and no parts (`sch_hierarchy_tests`' own), in a scratch folder of its own.
fn empty_board(name: &str) -> PathBuf {
    let dir = scratch(name);
    let model = ConstraintModel::default();
    let intent = dir.join("empty.yaml");
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    dir
}

fn files_of(reply: &Value) -> Vec<String> {
    reply["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect()
}

/// UI-LEFT X5 (r4 X4-2's leftover): two sheets numbered "2" (Edit Sheet Page Number); Plot Current Page on the second plots that sheet alone, as
/// KiCad's dialog plots `CurrentSheet()`, where kicad-cli's `--pages 2` alone would plot both; Plot All still plots the three pages.
#[test]
fn slow_plot_current_page_plots_one_sheet_when_two_share_its_page_number() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = empty_board("u5_x4_2_pages");
    let at = |x: f64| Point { x: mm(x), y: mm(50.8) };
    step(&dir, Cmd::AddSheet { name: "Alpha".into(), file: "alpha.kicad_sch".into(), at: at(25.4), size: (mm(25.4), mm(12.7)) }, false, "test").unwrap();
    step(&dir, Cmd::AddSheet { name: "Beta".into(), file: "beta.kicad_sch".into(), at: at(76.2), size: (mm(25.4), mm(12.7)) }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let ids: Vec<String> = design.schematic.as_ref().unwrap().sheets.iter().map(|s| s.id.clone()).collect();
    for id in &ids {
        step(&dir, Cmd::SetSheetPage { sheet: id.clone(), page: "2".into(), path: None }, false, "test").unwrap();
    }
    let beta = design.schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "Beta").unwrap().id.clone();
    let body = json!({ "format": "svg", "plot_all": false, "sheet_path": beta });
    let reply = crate::sch_output_api::plot(&dir, body.to_string().as_bytes());
    assert_eq!(reply["ok"], true, "{reply}");
    let files = files_of(&reply);
    assert_eq!(files.len(), 1, "one sheet plotted: {files:?}");
    assert!(files[0].ends_with("Beta.svg"), "{files:?}");
    // the page plotted shows its own number
    let svg = std::fs::read_to_string(dir.join(&files[0])).unwrap();
    assert!(!svg.contains("2~"), "the run-only number of the other sheet is not on this page");
    // kicad-cli's `--pages 2` by itself keeps both sheets numbered 2 (`TrimToPageNumbers`): what the renumbering is for
    let both = crate::sch_output_api::plot(&dir, json!({ "format": "svg", "pages": ["2"] }).to_string().as_bytes());
    assert_eq!(files_of(&both).len(), 2, "{both}");
    let all = crate::sch_output_api::plot(&dir, json!({ "format": "svg", "plot_all": true }).to_string().as_bytes());
    assert_eq!(files_of(&all).len(), 3, "{all}");
    // the design itself is as it was: both sheets still numbered 2
    let (_, after, _) = load(&dir).unwrap();
    assert!(after.schematic.as_ref().unwrap().sheets.iter().all(|s| s.page == "2"));
}

fn git(dir: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=Ada", "-c", "user.email=ada@example.com"]).args(args).output().is_ok_and(|o| o.status.success())
}

/// An SVG plot without the lines that name the file or the time.
fn svg_body(svg: &str) -> String {
    svg.lines().filter(|l| !l.contains("<title>") && !l.contains("<!--") && !l.contains("<desc>") && !l.contains("Created by") && !l.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

fn board_text(content: &str) -> eda_model::ir::Text {
    eda_model::ir::Text { id: String::new(), content: content.into(), at: Point { x: mm(20.0), y: mm(20.0) }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: eda_model::ir::TextJustify::Left, mirror: false }
}

/// UI-LEFT B5: a board text's `@{..}` (the schematic's whole grammar, one engine for both), `${VCSSHORTHASH}`, the `@{vcs..}` functions and the project's
/// variables show in the studio as kicad-cli plots them: the board in a git repository, its F.SilkS plotted by kicad-cli, is the plot of a board with the
/// studio's shown text written out.
#[test]
fn slow_kicad_cli_plots_a_board_texts_expressions_and_version_control_as_the_studio_shows_them() {
    if !kicad_cli_ready() {
        return;
    }
    let with = empty_board("u5_b5_board_vcs");
    if !git(&with, &["init", "-q", "-b", "trunk"]) {
        eprintln!("skipped: no git");
        return;
    }
    assert!(git(&with, &["add", "."]) && git(&with, &["commit", "-q", "-m", "first"]));
    step(&with, Cmd::SetTextVariables { variables: [("REV".to_string(), "C3".to_string())].into_iter().collect() }, false, "test").unwrap();
    let raw = "N@{2*3+0.5} ${VCSSHORTHASH} @{vcsbranch()} @{vcsidentifier(6)} @{upper(\"rev\")}-${REV} @{concat(\"R\", 4k7/1k)} @{vcsauthor()}";
    step(&with, Cmd::AddText { text: board_text(raw) }, false, "test").unwrap();
    let (meta, design, model) = load(&with).unwrap();
    let shown = crate::text_vars_json::PcbShown::new(&with, &design, &model, crate::text_vars_json::project(&with, &meta.intent, eda_model::text_vars::Clock::utc_now())).text_shown(raw, "F.SilkS");
    let head = std::process::Command::new("git").arg("-C").arg(&with).args(["rev-parse", "HEAD"]).output().unwrap();
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    assert_eq!(shown, format!("N6.5 {} trunk {} REV-C3 R4.7 Ada", &head[..8], &head[..6]));
    let without = empty_board("u5_b5_board_plain");
    step(&without, Cmd::SetTextVariables { variables: [("REV".to_string(), "C3".to_string())].into_iter().collect() }, false, "test").unwrap();
    step(&without, Cmd::AddText { text: board_text(&shown) }, false, "test").unwrap();
    let plot = |dir: &std::path::Path| -> String {
        let done = crate::kicad_engine::export(dir, "svg", &["--layers".into(), "F.SilkS".into(), "--mode-single".into()]).expect("kicad-cli plotted the board");
        let file = done["files"].as_array().unwrap().iter().filter_map(Value::as_str).find(|f| f.ends_with(".svg")).expect("an svg").to_string();
        std::fs::read_to_string(dir.join(file)).unwrap()
    };
    let (a, b) = (plot(&with), plot(&without));
    assert!(svg_body(&a).matches("<path").count() + svg_body(&a).matches("<polyline").count() > 10, "the text is drawn: {}", &a[..a.len().min(2000)]);
    assert_eq!(svg_body(&a), svg_body(&b), "the plot of the text with expressions is the plot of what the studio shows");
}

/// UI-LEFT B5: a schematic text box's variables and expressions (`SCH_TEXTBOX::GetShownText`) and a power symbol's Value (`SCH_FIELD::GetShownText` through the
/// symbol) show in the studio (`graphic_shown`, the field's `shown`) as kicad-cli plots them.
#[test]
fn slow_kicad_cli_plots_a_text_boxs_and_a_power_symbols_variables_as_the_studio_shows_them() {
    if !kicad_cli_ready() {
        return;
    }
    use eda_model::sch_extras::{SchGraphic, SchGraphicKind};
    let build = |name: &str, box_text: &str, power: &str| -> PathBuf {
        let dir = empty_board(name);
        step(&dir, Cmd::SetTextVariables { variables: [("RAIL".to_string(), "+5V".to_string()), ("REV".to_string(), "B7".to_string())].into_iter().collect() }, false, "test").unwrap();
        let shape = SchGraphicKind::TextBox { start: Point { x: mm(50.8), y: mm(50.8) }, end: Point { x: mm(101.6), y: mm(63.5) }, text: box_text.into(), angle: 0, size_nm: mm(1.27), bold: false, italic: false, h_align: Default::default(), v_align: Default::default(), margin_nm: eda_model::units::Nm::ZERO };
        step(&dir, Cmd::sch(eda_ops::sch_edit::SchCmd::AddGraphic { graphic: SchGraphic::new(shape) }), false, "test").unwrap();
        step(&dir, Cmd::AddPowerSymbol { lib_id: "power:+5V".into(), at: Point { x: mm(25.4), y: mm(25.4) }, rot_millideg: 0, net: power.into(), pin: String::new(), annotate: None }, false, "test").unwrap();
        dir
    };
    let with = build("u5_b5_sch_vars", "Rev ${REV} @{3*4} @{lower(\"ABC\")}", "${RAIL}");
    let sch = crate::studio::schematic_json(&with, "", crate::text_vars_json::clock_of("/api/schematic")).unwrap();
    let shown_box = sch["graphic_shown"].as_object().and_then(|m| m.values().next()).and_then(Value::as_str).unwrap_or_default().to_string();
    assert_eq!(shown_box, "Rev B7 12 abc", "{}", sch["graphic_shown"]);
    let value = sch["power_symbols"][0]["fields"].as_array().unwrap().iter().find(|f| f["name"] == "Value").unwrap().clone();
    assert_eq!(value["shown"], "+5V", "{value}");
    let without = build("u5_b5_sch_plain", &shown_box, "+5V");
    let plot = |dir: &std::path::Path| -> String {
        let done = crate::kicad_engine::export_sch(dir, "svg", &[]).expect("kicad-cli plotted the schematic");
        std::fs::read_to_string(dir.join(done["files"][0].as_str().unwrap())).unwrap()
    };
    let (a, b) = (plot(&with), plot(&without));
    assert!(a.len() > 2_000, "a real plot: {} bytes", a.len());
    assert_eq!(svg_body(&a), svg_body(&b), "the plot of the sheet with variables is the plot of the sheet with what the studio shows written out");
}

/// UI-LEFT B6: Find by Properties' Query evaluates the geometry functions and `fromTo` on the board (`pcbexpr_functions.cpp`: `intersectsCourtyardFunc`,
/// `intersectsAreaFunc`, `enclosedByAreaFunc`, `fromToFunc` over `FROM_TO_CACHE`), `A.Net` is the net's code, and a microvia is one. U1 and U2 (two pads each, GND
/// on their pads 1) joined by a GND track that goes round below them, a zone over U2, a via.
#[test]
fn find_by_properties_queries_the_boards_shapes_paths_and_net_codes() {
    let dir = scratch("u5_b6_query");
    crate::board::tests::setup(&dir);
    let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(4.0, 5.0), p(4.0, 8.0), p(14.0, 8.0), p(14.0, 5.0)] }, false, "test").unwrap();
    step(&dir, Cmd::AddTrack { net: "VCC".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(6.0, 5.0), p(6.0, 2.0)] }, false, "test").unwrap();
    step(&dir, Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![p(12.0, 3.0), p(18.0, 3.0), p(18.0, 7.0), p(12.0, 7.0)] }, false, "test").unwrap();
    step(&dir, Cmd::AddVia { net: "VCC".into(), x: mm(6.0), y: mm(2.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let rt = design.routing.as_ref().unwrap();
    let (gnd, vcc) = (rt.tracks.iter().find(|t| t.net == "GND").unwrap().id.clone(), rt.tracks.iter().find(|t| t.net == "VCC").unwrap().id.clone());
    let zone = rt.zones[0].id.clone();
    let via = rt.vias[0].id.clone();
    let item = |id: &str, class: &str, type_name: &str, net: &str, fp: Option<&str>| {
        let mut v = json!({ "id": id, "class": class, "type_name": type_name, "layers": ["F.Cu"], "net": net });
        if let Some(r) = fp {
            v["footprint"] = json!({ "reference": r, "fpid": "2PAD" });
        }
        v
    };
    let mut via_item = item(&via, "PCB_VIA", "Via", "VCC", None);
    via_item["layers"] = json!(["F.Cu", "B.Cu"]);
    via_item["via_type"] = json!("Through");
    let items = json!([
        item("U1", "FOOTPRINT", "Footprint", "", Some("U1")),
        item("U2", "FOOTPRINT", "Footprint", "", Some("U2")),
        item("U1.1", "PAD", "Pad", "GND", Some("U1")),
        item("U1.2", "PAD", "Pad", "VCC", Some("U1")),
        item("U2.1", "PAD", "Pad", "GND", Some("U2")),
        item(&gnd, "PCB_TRACK", "Track", "GND", None),
        item(&vcc, "PCB_TRACK", "Track", "VCC", None),
        via_item,
    ]);
    let ids = |expression: &str| {
        let body = json!({ "expression": expression, "items": items, "env": { "layer_entries": [["F.Cu", "F.Cu"], ["B.Cu", "B.Cu"]], "nets": ["GND", "VCC"] } });
        let r = crate::inspect_api::pcb_query(&dir, body.to_string().as_bytes());
        assert_eq!(r["ok"], true, "{expression}: {r}");
        r["ids"].as_array().unwrap().iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()
    };
    // U1's courtyard (2 x 1 mm half extents round (5, 5)): its own pads and the start of both tracks; not U2's pad
    let in_u1 = ids("A.intersectsCourtyard('U1')");
    assert!(in_u1.contains(&"U1.1".to_string()) && in_u1.contains(&"U1.2".to_string()) && in_u1.contains(&gnd) && !in_u1.contains(&"U2.1".to_string()), "{in_u1:?}");
    // the zone over U2: U2's pad 1 and the end of the GND track; U2 by its courtyard
    let in_zone = ids(&format!("A.intersectsArea('{zone}')"));
    assert!(in_zone.contains(&"U2.1".to_string()) && in_zone.contains(&gnd) && !in_zone.contains(&"U1.1".to_string()), "{in_zone:?}");
    assert!(ids(&format!("A.enclosedByArea('{zone}')")).contains(&"U2.1".to_string()));
    assert!(!ids(&format!("A.enclosedByArea('{zone}')")).contains(&gnd), "the track leaves the zone");
    // the copper path from U1's pad 1 to U2's pad 1: both pads and the track, not the VCC items
    let path = ids("A.fromTo('U1-1', 'U2-1')");
    assert_eq!(path, vec!["U1.1".to_string(), "U2.1".to_string(), gnd.clone()], "{path:?}");
    assert!(ids("A.fromTo('U1-2', 'U2-2')").is_empty(), "VCC is not routed to U2");
    // `A.Net` is the code: GND 1, VCC 2 (the writer's numbering, by name); a name matches nothing, as in KiCad
    assert_eq!(ids("A.Net == 1"), vec!["U1.1".to_string(), "U2.1".to_string(), gnd.clone()]);
    assert!(ids("A.Net == 'GND'").is_empty());
    assert_eq!(ids("A.NetName == 'VCC' && A.Type == 'Via'"), vec![via.clone()]);
    // a microvia is one to the language, whatever the request said its span made it
    assert!(ids("A.isMicroVia()").is_empty());
    let micro: Cmd = serde_json::from_value(json!({ "op": "set_via_layers", "ids": [via], "kind": "micro" })).unwrap();
    step(&dir, micro, false, "test").expect("the via becomes a microvia");
    assert_eq!(ids("A.isMicroVia()"), vec![via.clone()], "the board's via kind");
}

/// UI-LEFT B6: Clearance and Constraints Resolution see the tuning profiles' implicit rules (KiCad 25ec2335 `loadImplicitRules`' `addTuningSingleRule`), read the
/// rules text with its text variables expanded (`DRC_ENGINE::loadRules`) and evaluate `fromTo` on the board's paths.
#[test]
fn constraints_resolution_sees_tuning_profiles_variables_and_from_to() {
    use eda_model::setup_pages::{ProfileLayerEntry, ProfileType, TuningProfile, TuningProfiles};
    let dir = scratch("u5_b6_inspect");
    crate::board::tests::setup(&dir);
    let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(4.0, 5.0), p(4.0, 8.0), p(14.0, 8.0), p(14.0, 5.0)] }, false, "test").unwrap();
    step(&dir, Cmd::AddTrack { net: "VCC".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(6.0, 5.0), p(6.0, 2.0)] }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let rt = design.routing.as_ref().unwrap();
    let (gnd, vcc) = (rt.tracks.iter().find(|t| t.net == "GND").unwrap().id.clone(), rt.tracks.iter().find(|t| t.net == "VCC").unwrap().id.clone());
    let entry = ProfileLayerEntry { signal_layer: "F.Cu".into(), top_reference_layer: None, bottom_reference_layer: None, width_nm: mm(0.35), diff_pair_gap_nm: eda_model::units::Nm::ZERO, delay_as_per_mm: 0 };
    let profile = TuningProfile { name: "50R".into(), profile_type: ProfileType::Single, target_impedance: 50.0, enable_time_domain_tuning: false, layer_entries: vec![entry], via_propagation_delay_as: 0, via_overrides: vec![] };
    let settings = TuningProfiles { profiles: vec![profile], class_profiles: [("Default".to_string(), "50R".to_string())].into_iter().collect() };
    step(&dir, Cmd::SetTuningProfiles { settings }, false, "test").unwrap();
    let constraints = |id: &str| crate::inspect_api::constraints(&dir, json!({ "item": id }).to_string().as_bytes()).to_string();
    let report = constraints(&gnd);
    assert!(report.contains("tuning profile '50R'"), "the profile's width rule: {report}");
    // a custom rule with a project variable and one with `fromTo`
    step(&dir, Cmd::SetTextVariables { variables: [("HV_GAP".to_string(), "0.8mm".to_string())].into_iter().collect() }, false, "test").unwrap();
    let rules = "(version 1)\n(rule gap_by_variable\n  (constraint clearance (min ${HV_GAP}))\n  (condition \"A.NetName == 'VCC'\"))\n(rule path_width\n  (constraint track_width (min 0.11mm) (opt 0.12mm) (max 0.13mm))\n  (condition \"A.fromTo('U1-1', 'U2-1')\"))\n";
    step(&dir, Cmd::SetCustomRules { text: rules.into() }, false, "test").unwrap();
    let clearance = crate::inspect_api::clearance(&dir, json!({ "a": vcc, "b": gnd }).to_string().as_bytes()).to_string();
    assert!(clearance.contains("gap_by_variable") && clearance.contains("0.8000 mm") || clearance.contains("0.8 mm"), "the variable's value is the rule's: {clearance}");
    assert!(!clearance.contains("Unresolved text variable"), "{clearance}");
    let on_path = constraints(&gnd);
    assert!(on_path.contains("path_width"), "the GND track is on the path from U1-1 to U2-1: {on_path}");
    let off_path = constraints(&vcc);
    assert!(!off_path.contains("Checking rule 'path_width'") || off_path.contains("Rule ignored") || !off_path.contains("0.1200 mm"), "{off_path}");
}
