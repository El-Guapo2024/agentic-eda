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
