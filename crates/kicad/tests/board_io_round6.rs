//! Round 6's board-file findings (`work/findings/round6/r6-board-io.md`, F1-F3 and F6): a legacy board's `(setup ..)` design settings, the project's Default
//! class, the Dimensions defaults and the project's out-of-range rule values, each read as KiCad's loaders read them (8303b2ad
//! `pcb_io_kicad_sexpr_parser.cpp::parseSetup` / `parseDefaults`, `board.cpp::SetProject`, `net_settings.cpp`'s `readNetClass`, `board_design_settings.cpp`'s
//! `PARAM`s). The QA boards are read from `$KICAD_QA_DATA` (or `$KICAD_SRC_DIR/qa/data`) and skipped without it; the slow test (`EDA_SLOW_TESTS=1`) has
//! kicad-cli judge the original and ours.

use eda_kicad::{export_kicad_pcb, export_kicad_pro_for, import_kicad_pcb, merge_project_design_rules, merge_project_dimension_defaults, merge_project_net_classes, merge_project_predefined_sizes, project_json_for_board, ExportMeta};
use eda_model::ir::{DiffPairPreset, DimensionUnits, ViaPreset};
use eda_model::units::{mm, Nm};
use std::path::PathBuf;

fn qa(name: &str) -> Option<String> {
    let dir = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")));
    let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("pcbnew").join(name)).ok()) else {
        eprintln!("skipped: {name} needs KICAD_QA_DATA (or KICAD_SRC_DIR)");
        return None;
    };
    Some(text)
}

/// A KiCad 5.99 board with the legacy settings `parseSetup` reads (the shape of QA `issue4139` and `issue4257`).
const LEGACY: &str = r#"(kicad_pcb (version 20200512) (host pcbnew "5.99")
  (general (thickness 1.6))
  (page "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (setup
    (last_trace_width 0.25)
    (user_trace_width 1)
    (user_trace_width 0.5)
    (trace_clearance 0.3)
    (trace_min 0.15)
    (via_size 0.9)
    (via_drill 0.45)
    (user_via 1.2 0.6)
    (uvia_size 0.35)
    (uvia_drill 0.15)
    (uvia_min_size 0.25)
    (user_diff_pair 0.2 0.25 0.25)
    (user_diff_pair 0.3 0.35 0.4)
    (max_error 0.007)
    (defaults
      (edge_clearance 0.01)
      (edge_cuts_line_width 0.05)
      (courtyard_line_width 0.06)
      (copper_line_width 0.2)
      (copper_text_dims (size 1.5 1.5) (thickness 0.3) keep_upright)
      (silk_line_width 0.12)
      (silk_text_dims (size 1 1.1) (thickness 0.15) italic)
      (fab_layers_line_width 0.1)
      (fab_layers_text_dims (size 1 1) (thickness 0.15))
      (other_layers_line_width 0.1)
      (other_layers_text_dims (size 1 1) (thickness 0.15))
      (dimension_units 0)
      (dimension_precision 2)
    )
    (pad_size 1.524 1.524)
    (pad_drill 0.762)
    (pad_to_mask_clearance 0.05)
  )
  (net 0 "")
  (gr_line (start 100 100) (end 130 100) (layer "Edge.Cuts") (width 0.05))
  (gr_line (start 130 100) (end 130 120) (layer "Edge.Cuts") (width 0.05))
  (gr_line (start 130 120) (end 100 120) (layer "Edge.Cuts") (width 0.05))
  (gr_line (start 100 120) (end 100 100) (layer "Edge.Cuts") (width 0.05))
)"#;

/// F1: every legacy token of `(setup ..)` and of `(setup (defaults ..))` is the board's: the copper edge clearance (only ever in `(defaults ..)`), the
/// Default class's clearance and via and microvia sizes, the minimums, the Text & Graphics defaults (the italic and keep-upright flags off unless a
/// `*_text_dims` turns them on), the pre-defined sizes (the first `user_diff_pair` sits in the net class's place) and the dimension defaults.
#[test]
fn a_legacy_boards_setup_is_its_design_settings() {
    let (design, model, notes) = import_kicad_pcb(LEGACY).expect("import");
    assert!(notes.legacy_design_settings && !notes.legacy_net_classes);
    let b = &model.board;
    assert_eq!(b.copper_edge_clearance_nm, Some(mm(0.01)));
    assert_eq!((b.clearance, b.via_diameter, b.via_drill), (mm(0.3), mm(0.9), mm(0.45)));
    let d = b.default_class.as_ref().expect("the Default class's microvia");
    assert_eq!((d.microvia_diameter, d.microvia_drill), (Some(mm(0.35)), Some(mm(0.15))));
    assert_eq!((b.track_width_min_nm, b.microvia_diameter_min_nm, b.max_error_nm), (mm(0.15), mm(0.25), mm(0.007)));
    let t = &b.text_graphics;
    assert_eq!((t.silk.line_width_nm, t.silk.text_width_nm, t.silk.text_height_nm, t.silk.text_thickness_nm), (mm(0.12), mm(1.0), mm(1.1), mm(0.15)));
    assert_eq!((t.copper.text_thickness_nm, t.courtyard_line_width_nm), (mm(0.3), mm(0.06)));
    assert_eq!((t.silk.italic, t.silk.upright, t.copper.italic, t.copper.upright, t.fab.upright), (true, false, false, true, false));
    let routing = design.routing.as_ref().expect("the pre-defined sizes");
    assert_eq!(routing.track_width_presets, [mm(1.0), mm(0.5)]);
    assert_eq!(routing.via_presets, [ViaPreset { diameter: mm(1.2), drill: mm(0.6) }]);
    assert_eq!(routing.diff_pair_presets, [DiffPairPreset { width: mm(0.3), gap: mm(0.35), via_gap: mm(0.4) }]);
    let dims = &design.drawings.as_ref().expect("drawings").dimension_settings;
    assert_eq!((dims.units, dims.precision), (DimensionUnits::Inch, 2));

    // The derived project says so to kicad-cli.
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let ds = &pro["board"]["design_settings"];
    assert_eq!(ds["rules"]["min_copper_edge_clearance"], 0.01);
    assert_eq!(ds["defaults"]["silk_line_width"], 0.12);
    assert_eq!(ds["track_widths"], serde_json::json!([0.0, 1.0, 0.5]));
    assert_eq!(ds["defaults"]["dimension_units"], 0);
}

/// F1: a board without legacy settings is not one (the stackup, the mask margins and the plot parameters are a current board's own).
#[test]
fn a_current_boards_setup_is_not_legacy() {
    let text = LEGACY.replace(
        &LEGACY[LEGACY.find("  (setup").unwrap()..LEGACY.find("  (net 0").unwrap()],
        "  (setup (pad_to_mask_clearance 0.05) (aux_axis_origin 0 0) (pcbplotparams (layerselection 0x010fc_ffffffff)))\n",
    );
    let (design, model, notes) = import_kicad_pcb(&text).expect("import");
    assert!(!notes.legacy_design_settings);
    assert_eq!(model.board.copper_edge_clearance_nm, None);
    assert!(model.board.text_graphics.silk.upright, "KiCad's project default");
    assert!(design.routing.is_none());
}

/// F1 (`board.cpp::SetProject`): a board with legacy design settings ignores the project's `board.design_settings`; a board with `(net_class ..)`
/// blocks replaces the project's net classes and patterns with its own. The rest of the project stays.
#[test]
fn a_legacy_board_keeps_its_own_settings_over_the_projects() {
    let project = r#"{"board":{"design_settings":{"rules":{"min_copper_edge_clearance":0.5},"track_widths":[0.0,2.0]}},"net_settings":{"classes":[{"name":"Default","clearance":0.9}],"netclass_patterns":[{"netclass":"X","pattern":"*"}],"netclass_assignments":{"A":["Y"]}},"text_variables":{"V":"1"}}"#;
    let (mut design, mut model, notes) = import_kicad_pcb(LEGACY).expect("import");
    let kept = project_json_for_board(project, &notes);
    let v: serde_json::Value = serde_json::from_str(&kept).unwrap();
    assert!(v["board"].get("design_settings").is_none() && v["text_variables"]["V"] == "1");
    assert!(v["net_settings"]["classes"].is_array(), "the board has no (net_class ..): the project's classes are read");
    merge_project_design_rules(&mut model, &kept);
    merge_project_predefined_sizes(&mut design, &kept);
    assert_eq!(model.board.copper_edge_clearance_nm, Some(mm(0.01)));
    assert_eq!(design.routing.as_ref().unwrap().track_width_presets, [mm(1.0), mm(0.5)]);

    let with_class = LEGACY.replace("  (net 0 \"\")", "  (net 0 \"\")\n  (net_class \"Default\" \"\" (clearance 0.2) (trace_width 0.25) (via_dia 0.8) (via_drill 0.4) (uvia_dia 0.3) (uvia_drill 0.1))");
    let (_, model, notes) = import_kicad_pcb(&with_class).expect("import");
    assert!(notes.legacy_net_classes);
    assert_eq!(model.board.clearance, mm(0.2), "the (net_class \"Default\" ..) block, read after (setup ..), replaces its Default class");
    assert_eq!(model.board.default_class, None, "with KiCad's own microvia");
    let v: serde_json::Value = serde_json::from_str(&project_json_for_board(project, &notes)).unwrap();
    assert!(v["net_settings"].get("classes").is_none() && v["net_settings"].get("netclass_patterns").is_none());
    assert_eq!(v["net_settings"]["netclass_assignments"]["A"][0], "Y");
    // A current board's project is read as it is.
    let notes = eda_kicad::ImportNotes::default();
    assert_eq!(project_json_for_board(project, &notes), project);
}

/// F1 on the QA boards the finding names.
#[test]
fn issue4139_and_issue4257_keep_their_legacy_settings() {
    if let Some(text) = qa("issue4139.kicad_pcb") {
        let (_, model, _) = import_kicad_pcb(&text).expect("import");
        assert_eq!(model.board.copper_edge_clearance_nm, Some(Nm::new(10_000)));
        assert_eq!(model.board.text_graphics.silk.line_width_nm, Nm::new(120_000));
        assert_eq!(model.board.text_graphics.copper.text_thickness_nm, Nm::new(300_000));
    }
    if let Some(text) = qa("issue4257.kicad_pcb") {
        let (design, _, _) = import_kicad_pcb(&text).expect("import");
        assert_eq!(design.routing.as_ref().expect("routing").track_width_presets, [mm(1.0)]);
    }
}

/// F2: the project's Default class keeps its microvia and differential-pair sizes, and the board file's Default block carries them.
#[test]
fn the_projects_default_class_keeps_its_microvia_and_pair_sizes() {
    let project = r#"{"net_settings":{"classes":[{"name":"Default","clearance":0.127,"track_width":0.154,"via_diameter":0.4572,"via_drill":0.254,"microvia_diameter":0.254,"microvia_drill":0.4572,"diff_pair_width":0.154,"diff_pair_gap":0.127,"diff_pair_via_gap":0.25,"priority":2147483647}],"meta":{"version":5}}}"#;
    let text = qa("issue17429.kicad_pcb").unwrap_or_else(|| LEGACY.to_string());
    let (design, mut model, _) = import_kicad_pcb(&text).expect("import");
    merge_project_net_classes(&mut model, project);
    let d = model.board.default_class.as_ref().expect("a Default class");
    assert_eq!((d.diff_pair_width, d.diff_pair_gap, d.diff_pair_via_gap), (Some(Nm::new(154_000)), Some(Nm::new(127_000)), Some(mm(0.25))));
    assert_eq!((d.microvia_diameter, d.microvia_drill), (Some(Nm::new(254_000)), Some(Nm::new(457_200))));
    let out = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "r6" }).expect("export");
    let block = &out[out.find("(net_class \"Default\"").expect("a Default block")..];
    let block = &block[..block.find("\n\t)").unwrap()];
    for want in ["(uvia_dia 0.254)", "(uvia_drill 0.4572)", "(diff_pair_width 0.154)", "(diff_pair_gap 0.127)"] {
        assert!(block.contains(want), "{want} missing from:\n{block}");
    }
}

/// F3: the Dimensions defaults are read from the project (every key as its `PARAM` loads it, the schema-0 precision migrated) and written back.
#[test]
fn the_projects_dimension_defaults_are_read_and_written() {
    let (mut design, model, _) = import_kicad_pcb(LEGACY).expect("import");
    let project = r#"{"board":{"design_settings":{"defaults":{"dimension_units":2,"dimension_precision":2,"dimensions":{"units_format":2,"suppress_zeroes":false,"text_position":1,"keep_text_aligned":false,"arrow_length":2540000,"extension_offset":250000}}}}}"#;
    merge_project_dimension_defaults(&mut design, project);
    let d = design.drawings.as_ref().unwrap().dimension_settings;
    assert_eq!((d.units, d.precision, d.suppress_trailing_zeros, d.keep_text_aligned), (DimensionUnits::Mm, 2, false, false));
    assert_eq!((d.units_format, d.text_position), (eda_model::ir::DimensionUnitsFormat::ParenSuffix, eda_model::ir::DimensionTextPosition::Inline));
    assert_eq!((d.arrow_length, d.extension_offset), (Nm::new(2_540_000), Nm::new(250_000)));
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let defaults = &pro["board"]["design_settings"]["defaults"];
    assert_eq!((defaults["dimension_units"].clone(), defaults["dimension_precision"].clone()), (serde_json::json!(2), serde_json::json!(2)));
    assert_eq!(defaults["dimensions"]["arrow_length"], 2_540_000);
    assert_eq!(defaults["dimensions"]["text_position"], 1);
    // Read back: the same.
    let mut back = design.clone();
    back.drawings.as_mut().unwrap().dimension_settings = Default::default();
    merge_project_dimension_defaults(&mut back, &pro.to_string());
    assert_eq!(back.drawings.as_ref().unwrap().dimension_settings, d);

    // Schema 0: the precision in its old meaning (`migrateSchema0to1`: inches +3); an out-of-range value or a missing key is KiCad's default.
    merge_project_dimension_defaults(&mut design, r#"{"board":{"design_settings":{"meta":{"version":0},"defaults":{"dimension_units":0,"dimension_precision":1,"dimensions":{"text_position":2}}}}}"#);
    let d = design.drawings.as_ref().unwrap().dimension_settings;
    assert_eq!((d.units, d.precision, d.text_position, d.arrow_length), (DimensionUnits::Inch, 4, eda_model::ir::DimensionTextPosition::Outside, Nm::new(1_270_000)));
    // KiCad's own defaults are not written.
    let mut plain = design.clone();
    plain.drawings.as_mut().unwrap().dimension_settings = Default::default();
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&plain, &model)).unwrap();
    assert!(pro["board"]["design_settings"]["defaults"].get("dimension_units").is_none());
}

/// F6: a project's rule value outside KiCad's range is KiCad's default (`PARAM_SCALED<int>::Load`); the defaults are read without a `rules` object.
#[test]
fn an_out_of_range_rule_value_reads_as_kicads_default() {
    let (_, mut model, _) = import_kicad_pcb(LEGACY).expect("import");
    merge_project_design_rules(&mut model, r#"{"board":{"design_settings":{"rules":{"max_error":0,"min_clearance":30,"min_silk_clearance":-5,"min_copper_edge_clearance":-0.01,"min_resolved_spokes":120,"min_track_width":0.1}}}}"#);
    let b = &model.board;
    assert_eq!(b.max_error_nm, mm(0.005), "ARC_HIGH_DEF");
    assert_eq!(b.min_clearance_nm, Nm::ZERO, "25 mm at most; DEFAULT_MINCLEARANCE");
    assert_eq!(b.silk_clearance_nm, mm(-5.0), "in range");
    assert_eq!(b.copper_edge_clearance_nm, Some(mm(-0.01)), "the legacy flag is in range");
    assert_eq!(b.min_resolved_spokes, 2);
    assert_eq!(b.track_width_min_nm, mm(0.1));
    merge_project_design_rules(&mut model, r#"{"board":{"design_settings":{"defaults":{"silk_line_width":0.3,"copper_text_size_h":0,"copper_line_width":200}}}}"#);
    let t = &model.board.text_graphics;
    assert_eq!((t.silk.line_width_nm, t.copper.text_width_nm, t.copper.line_width_nm), (mm(0.3), mm(1.5), mm(0.2)));
}

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> PathBuf {
    std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"))
}

/// kicad-cli's DRC of `dir/b.kicad_pcb`: the count of each violation type.
fn drc(dir: &std::path::Path) -> std::collections::BTreeMap<String, usize> {
    let report = dir.join("drc.json");
    let out = std::process::Command::new(kicad_cli()).args(["pcb", "drc", "--format", "json", "--severity-all", "-o"]).arg(&report).arg(dir.join("b.kicad_pcb")).output().expect("kicad-cli runs");
    assert!(report.exists(), "kicad-cli pcb drc: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let mut counts = std::collections::BTreeMap::new();
    for item in v["violations"].as_array().into_iter().flatten().chain(v["unconnected_items"].as_array().into_iter().flatten()) {
        *counts.entry(item["type"].as_str().unwrap_or("").to_string()).or_insert(0) += 1;
    }
    counts
}

/// F1 against kicad-cli: QA `issue4139` with a B.Cu track 0.38 mm from the edge (its `(defaults (edge_clearance 0.01))` allows it). kicad-cli finds no
/// `copper_edge_clearance` on the original, and none on ours (before the fix: one, "board setup constraints edge clearance 0.5000 mm").
#[test]
fn kicad_cli_judges_our_legacy_board_by_its_own_edge_clearance() {
    if slow_tests_off() {
        return;
    }
    let Some(text) = qa("issue4139.kicad_pcb") else { return };
    let track = "  (segment (start 124.46 67.183) (end 124.46 62.0) (width 0.25) (layer \"B.Cu\") (net 1) (tstamp b716ea7e-5437-4b8d-8adf-5979da4797ff))\n";
    let end = text.rfind(')').unwrap();
    let original = format!("{}{track}{}", &text[..end], &text[end..]);
    let root = std::env::temp_dir().join(format!("eda_board_io_round6_{}", std::process::id()));
    let (theirs, ours) = (root.join("theirs"), root.join("ours"));
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::create_dir_all(&ours).unwrap();
    std::fs::write(theirs.join("b.kicad_pcb"), &original).unwrap();
    let (design, model, _) = import_kicad_pcb(&original).expect("import");
    std::fs::write(ours.join("b.kicad_pcb"), export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "b" }).expect("export")).unwrap();
    std::fs::write(ours.join("b.kicad_pro"), export_kicad_pro_for(&design, &model)).unwrap();
    let (t, o) = (drc(&theirs), drc(&ours));
    assert_eq!(t.get("copper_edge_clearance"), None, "the original: {t:?}");
    assert_eq!(o.get("copper_edge_clearance"), None, "ours: {o:?} (the original: {t:?})");
    assert_eq!(o.get("track_dangling"), t.get("track_dangling"), "ours: {o:?}, the original: {t:?}");
    let _ = std::fs::remove_dir_all(&root);
}
