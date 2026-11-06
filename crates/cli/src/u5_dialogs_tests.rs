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
