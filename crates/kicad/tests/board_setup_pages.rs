//! Formatting, Zone Hatch Offsets, Length-tuning Patterns and Tuning Profiles (`eda_model::setup_pages`) through the writer and the importer: the derived board file and project
//! carry them where KiCad keeps them (`pcbplotparams`, `zone_defaults`, `defaults.apply_defaults_to_fp_*`, `tuning_pattern_settings`, `tuning_profiles`, `net_settings`), and an
//! imported board and project bring them in. No external tool is needed here; kicad-cli's reading of them is proved in `eda-cli`'s board tests.

use eda_model::units::mm;
use eda_kicad::{export_kicad_pcb, export_kicad_pro_for, import_kicad_pcb, merge_project_predefined_sizes, merge_project_setup_pages, merge_project_teardrops, ExportMeta};
use eda_model::ir::{DiffPairPreset, Nm, RoutingSection, TeardropSettings, ViaPreset};
use std::path::PathBuf;
use eda_model::setup_pages::{CornerStyle, HatchOffset, MeanderSettings, ProfileLayerEntry, ProfileType, TuningProfile, ViaOverride};
use eda_model::{ConstraintModel, Net, NetClass};

const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
	(setup
		(pad_to_mask_clearance 0)
		(zone_defaults (property (layer "F.Cu") (hatch_position (xy 1.5 -2))) (property (layer "In1.Cu") (hatch_position (xy 0 0))) (property (layer "In7.Cu") (hatch_position (xy 4 4))))
		(pcbplotparams (layerselection 0x00010fc_ffffffff) (dashed_line_dash_ratio 8.000000) (dashed_line_gap_ratio 2.500000))
	)
	(net 0 "")
)
"#;

fn export(d: &eda_model::ir::Design, m: &ConstraintModel) -> String {
    export_kicad_pcb(d, m, &ExportMeta { date: "2026-01-01", title: "pages" }).expect("export_kicad_pcb")
}

fn profile() -> TuningProfile {
    TuningProfile {
        name: "USB 90".into(),
        profile_type: ProfileType::Differential,
        target_impedance: 90.0,
        enable_time_domain_tuning: true,
        layer_entries: vec![ProfileLayerEntry { signal_layer: "F.Cu".into(), top_reference_layer: None, bottom_reference_layer: Some("In1.Cu".into()), width_nm: mm(0.13), diff_pair_gap_nm: mm(0.15), delay_as_per_mm: 6_100_000 }],
        via_propagation_delay_as: 25_000_000,
        via_overrides: vec![ViaOverride { signal_layer_from: "F.Cu".into(), signal_layer_to: "B.Cu".into(), via_layer_from: "F.Cu".into(), via_layer_to: "B.Cu".into(), delay_as: 30_000_000 }],
    }
}

#[test]
fn the_board_file_brings_the_dash_ratios_and_the_hatch_offsets_in_and_out() {
    let (design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    assert_eq!((model.board.formatting.dash_length_ratio, model.board.formatting.gap_length_ratio), (8.0, 2.5));
    assert_eq!(model.board.hatch_offsets.keys().collect::<Vec<_>>(), ["F.Cu"], "a layer at 0, 0 has none and In7.Cu is not a layer of this board");
    assert_eq!(model.board.hatch_offsets["F.Cu"], HatchOffset { x_nm: mm(1.5), y_nm: mm(-2.0) });
    let text = export(&design, &model);
    assert!(text.contains("(dashed_line_dash_ratio 8.000000)") && text.contains("(dashed_line_gap_ratio 2.500000)"), "{text}");
    assert!(text.contains("(zone_defaults (property (layer \"F.Cu\") (hatch_position (xy 1.5 -2))))"), "{text}");
    let (_, again, _) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(again.board.formatting, model.board.formatting);
    assert_eq!(again.board.hatch_offsets, model.board.hatch_offsets);
}

#[test]
fn a_board_nobody_set_up_writes_the_defaults_and_none_of_the_pages_keys() {
    let (design, model, _) = import_kicad_pcb(r#"(kicad_pcb (version 20241229) (generator "pcbnew") (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user)) (net 0 ""))"#).expect("imports");
    assert!(model.board.formatting.is_default() && model.board.hatch_offsets.is_empty() && model.board.tuning_patterns.is_default() && model.board.tuning_profiles.is_empty());
    let text = export(&design, &model);
    assert!(text.contains("(dashed_line_dash_ratio 12.000000)") && text.contains("(dashed_line_gap_ratio 3.000000)") && !text.contains("zone_defaults"));
    assert!(text.contains("(net_class \"Default\""), "the legacy classes are still the board's");
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    assert!(pro.get("tuning_profiles").is_none() && pro.get("net_settings").is_none());
    assert!(pro["board"]["design_settings"].get("tuning_pattern_settings").is_none());
    assert!(pro["board"]["design_settings"].get("diff_pair_dimensions").is_none(), "no pair sizes saved: no list (an absent key reads back as none, as a fresh project's does)");
    assert!(pro["board"]["design_settings"].get("defaults").is_none_or(|d| d.get("apply_defaults_to_fp_fields").is_none()));
}

#[test]
fn the_project_carries_the_footprint_boxes_the_tuning_patterns_and_the_profiles() {
    let (design, mut model, _) = import_kicad_pcb(BOARD).expect("imports");
    model.board.formatting.apply_to_fp_fields = true;
    model.board.formatting.apply_to_fp_shapes = true;
    model.board.tuning_patterns.diff_pair = MeanderSettings { min_amplitude_nm: mm(0.3), max_amplitude_nm: mm(0.9), spacing_nm: mm(1.1), corner_style: CornerStyle::Chamfer, corner_radius_percentage: 40, single_sided: true };
    model.board.tuning_profiles.profiles = vec![profile()];
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let ds = &pro["board"]["design_settings"];
    assert_eq!((&ds["defaults"]["apply_defaults_to_fp_fields"], &ds["defaults"]["apply_defaults_to_fp_text"], &ds["defaults"]["apply_defaults_to_fp_shapes"], &ds["defaults"]["apply_defaults_to_fp_dimensions"]), (&serde_json::json!(true), &serde_json::json!(false), &serde_json::json!(true), &serde_json::json!(false)));
    assert_eq!(ds["tuning_pattern_settings"]["diff_pair_defaults"], serde_json::json!({ "min_amplitude": 0.3, "max_amplitude": 0.9, "spacing": 1.1, "corner_style": 0, "corner_radius_percentage": 40, "single_sided": true }));
    assert_eq!(ds["tuning_pattern_settings"]["single_track_defaults"]["spacing"], 0.6);
    let profiles = &pro["tuning_profiles"]["tuning_profiles_impedance_geometric"];
    assert_eq!(profiles[0]["profile_name"], "USB 90");
    assert_eq!((&profiles[0]["type"], &profiles[0]["target_impedance"], &profiles[0]["via_prop_delay"]), (&serde_json::json!(1), &serde_json::json!(90.0), &serde_json::json!(25_000_000)));
    assert_eq!(profiles[0]["layer_entries"][0], serde_json::json!({ "signal_layer": "F.Cu", "top_reference_layer": "UNDEFINED", "bottom_reference_layer": "In1.Cu", "width": 130_000, "diff_pair_gap": 150_000, "delay": 6_100_000 }));
    assert_eq!(profiles[0]["via_overrides"][0]["delay"], 30_000_000);
    assert!(pro.get("net_settings").is_none(), "no class names a profile, so the board file keeps the classes");

    // The same project read back brings every page in.
    let (_, mut back, _) = import_kicad_pcb(BOARD).expect("imports");
    merge_project_setup_pages(&mut back, &serde_json::to_string(&pro).unwrap());
    assert_eq!(back.board.formatting.apply_to_fp_fields, true);
    assert_eq!(back.board.formatting.apply_to_fp_text, false);
    assert_eq!(back.board.tuning_patterns, model.board.tuning_patterns);
    assert_eq!(back.board.tuning_profiles.profiles, vec![profile()]);
}

#[test]
fn a_class_that_names_a_profile_moves_the_net_classes_into_the_project() {
    let (design, mut model, _) = import_kicad_pcb(BOARD).expect("imports");
    model.nets = vec![Net { name: "GND".into(), pins: vec![] }, Net { name: "USB_DP".into(), pins: vec![] }, Net { name: "USB_DM".into(), pins: vec![] }];
    model.board.net_classes = vec![NetClass { name: "USB".into(), nets: vec!["USB_*".into()], track_width: Some(mm(0.13)), clearance: Some(mm(0.15)), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: Some(mm(0.13)), diff_pair_gap: Some(mm(0.15)), diff_pair_via_gap: None, priority: 0 }];
    model.board.tuning_profiles.class_profiles = [("USB".to_string(), "USB 90".to_string()), ("Default".to_string(), "".to_string()), ("Orphan".to_string(), "Gone".to_string())].into_iter().filter(|(_, p)| !p.is_empty()).collect();
    let text = export(&design, &model);
    assert!(!text.contains("(net_class "), "KiCad would move the board's classes into the project and drop the project's: {text}");
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let ns = &pro["net_settings"];
    assert_eq!(ns["meta"]["version"], 5);
    let classes = ns["classes"].as_array().unwrap();
    let class = |n: &str| classes.iter().find(|c| c["name"] == n).unwrap_or_else(|| panic!("{n} in {classes:?}")).clone();
    assert_eq!((class("Default")["tuning_profile"].as_str(), class("USB")["tuning_profile"].as_str(), class("Orphan")["tuning_profile"].as_str()), (Some(""), Some("USB 90"), Some("Gone")), "a profile the project does not have is what missing_tuning_profile reports");
    assert_eq!((class("USB")["track_width"].as_f64(), class("USB")["diff_pair_gap"].as_f64()), (Some(0.13), Some(0.15)));
    assert_eq!(ns["netclass_patterns"], serde_json::json!([{ "netclass": "USB", "pattern": "USB_DM" }, { "netclass": "USB", "pattern": "USB_DP" }]), "one pattern per net that has a class: the nets the class's patterns match");

    // And the project read back says the same classes name the same profiles.
    let (_, mut back, _) = import_kicad_pcb(BOARD).expect("imports");
    merge_project_setup_pages(&mut back, &serde_json::to_string(&pro).unwrap());
    assert_eq!(back.board.tuning_profiles.class_profiles, model.board.tuning_profiles.class_profiles);
}

#[test]
fn a_profile_the_page_would_refuse_is_not_imported() {
    let (_, mut model, _) = import_kicad_pcb(BOARD).expect("imports");
    let pro = serde_json::json!({ "tuning_profiles": { "meta": { "version": 0 }, "tuning_profiles_impedance_geometric": [
        { "profile_name": "ok", "type": 0, "target_impedance": 50.0, "enable_time_domain_tuning": false, "layer_entries": [{ "signal_layer": "F.Cu", "top_reference_layer": "UNDEFINED", "bottom_reference_layer": "UNDEFINED", "width": 100000, "diff_pair_gap": 0, "delay": 5600000 }], "via_prop_delay": 0, "via_overrides": [] },
        { "profile_name": "", "type": 0 },
        { "profile_name": "on a layer the board lacks", "type": 0, "layer_entries": [{ "signal_layer": "In7.Cu", "top_reference_layer": "UNDEFINED", "bottom_reference_layer": "UNDEFINED", "width": 100000, "diff_pair_gap": 0, "delay": 1 }] }
    ] } });
    merge_project_setup_pages(&mut model, &pro.to_string());
    assert_eq!(model.board.tuning_profiles.profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["ok"]);
    merge_project_setup_pages(&mut model, "not json");
    assert_eq!(model.board.tuning_profiles.profiles.len(), 1);
}

/// F3 (round 7): Board Setup > Teardrops is `BOARD_DESIGN_SETTINGS`' `teardrop_options` and `teardrop_parameters` (`board_design_settings.cpp:526-660`). The
/// reader takes the target kinds and the round block's sizes (this port's single block collapses KiCad's three, PARITY-pcb §15); the writer repeats it for the
/// round and rectangular targets and keeps the track-end target at `TEARDROP_PARAMETERS()`'s constructor defaults.
#[test]
fn the_projects_teardrop_parameters_are_read_and_written() {
    let (mut design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let pro = serde_json::json!({ "board": { "design_settings": {
        "teardrop_options": [{ "td_onvia": true, "td_onpthpad": false, "td_onsmdpad": true, "td_ontrackend": true, "td_onroundshapesonly": false }],
        "teardrop_parameters": [
            { "td_target_name": "td_round_shape", "td_maxlen": 0.5, "td_maxheight": 2.5, "td_length_ratio": 0.2, "td_height_ratio": 0.8, "td_curve_segcount": 4, "td_width_to_size_filter_ratio": 0.6 },
            { "td_target_name": "td_rect_shape", "td_maxlen": 0.9, "td_maxheight": 3.0, "td_length_ratio": 0.4 },
            { "td_maxlen": 0.1 },
            { "td_target_name": "td_track_end", "td_maxlen": 0.7, "td_length_ratio": 0.3 }
        ]
    } } });
    merge_project_teardrops(&mut design, &pro.to_string());
    let td = design.routing.as_ref().expect("the page's block").teardrop_settings;
    assert_eq!((td.target_vias, td.target_pth_pads, td.target_smd_pads), (true, false, true));
    assert_eq!((td.best_length_ratio, td.best_width_ratio, td.width_to_size_filter_ratio), (0.2, 0.8, 0.6));
    assert_eq!((td.max_len_nm, td.max_width_nm), (mm(0.5), mm(2.5)));
    assert_eq!(td, TeardropSettings { target_pth_pads: false, best_length_ratio: 0.2, best_width_ratio: 0.8, max_len_nm: mm(0.5), max_width_nm: mm(2.5), width_to_size_filter_ratio: 0.6, ..Default::default() }, "the rect and track entries do not feed the collapsed block");

    let project: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let ds = &project["board"]["design_settings"];
    let params = ds["teardrop_parameters"].as_array().expect("the parameter blocks");
    let entry = |name: &str| params.iter().find(|e| e["td_target_name"] == name).unwrap_or_else(|| panic!("{name} in {params:?}")).clone();
    for name in ["td_round_shape", "td_rect_shape"] {
        let e = entry(name);
        assert_eq!((e["td_maxlen"].as_f64(), e["td_maxheight"].as_f64(), e["td_length_ratio"].as_f64(), e["td_height_ratio"].as_f64(), e["td_width_to_size_filter_ratio"].as_f64()), (Some(0.5), Some(2.5), Some(0.2), Some(0.8), Some(0.6)), "{name}: {e}");
    }
    let track = entry("td_track_end");
    assert_eq!((track["td_maxlen"].as_f64(), track["td_maxheight"].as_f64(), track["td_length_ratio"].as_f64(), track["td_height_ratio"].as_f64(), track["td_width_to_size_filter_ratio"].as_f64()), (Some(1.0), Some(2.0), Some(0.5), Some(1.0), Some(0.9)), "KiCad's constructor defaults: {track}");
    assert_eq!(ds["teardrop_options"], serde_json::json!([{ "td_onvia": true, "td_onpthpad": false, "td_onsmdpad": true, "td_ontrackend": false, "td_onroundshapesonly": false }]));

    // Read back: the same block (the off-schema keys are KiCad's own defaults).
    let mut back = design.clone();
    back.routing.as_mut().unwrap().teardrop_settings = Default::default();
    merge_project_teardrops(&mut back, &project.to_string());
    assert_eq!(back.routing.as_ref().unwrap().teardrop_settings, td);
    // A project without the keys, or text that is not JSON, leaves the design alone.
    let before = back.routing.as_ref().unwrap().teardrop_settings;
    merge_project_teardrops(&mut back, r#"{"board":{"design_settings":{"rules":{}}}}"#);
    merge_project_teardrops(&mut back, "not json");
    assert_eq!(back.routing.as_ref().unwrap().teardrop_settings, before);
    // A never-touched page writes neither key (an absent key reads back as KiCad's defaults).
    let mut untouched = design.clone();
    untouched.routing.as_mut().unwrap().teardrop_settings = Default::default();
    let plain: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&untouched, &model)).unwrap();
    assert!(plain["board"]["design_settings"].get("teardrop_parameters").is_none() && plain["board"]["design_settings"].get("teardrop_options").is_none());
}

/// F3 on the QA project the finding names: `issue18142` keeps a 0.2 round-shape length ratio, read from the project's `teardrop_parameters`.
#[test]
fn issue18142_keeps_its_teardrop_length_ratio() {
    let dir = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")));
    let Some(project) = dir.and_then(|d| std::fs::read_to_string(d.join("pcbnew").join("issue18142.kicad_pro")).ok()) else {
        eprintln!("skipped: issue18142.kicad_pro needs KICAD_QA_DATA (or KICAD_SRC_DIR)");
        return;
    };
    let (mut design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    merge_project_teardrops(&mut design, &project);
    assert_eq!(design.routing.as_ref().unwrap().teardrop_settings.best_length_ratio, 0.2);
    let out: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let round = out["board"]["design_settings"]["teardrop_parameters"].as_array().unwrap().iter().find(|e| e["td_target_name"] == "td_round_shape").expect("the round block").clone();
    assert_eq!(round["td_length_ratio"], 0.2);
}

// r4 R4-3: the project's `board.design_settings.diff_pair_dimensions` (`BOARD_DESIGN_SETTINGS`' PARAM_LAMBDA) is written with its index-0 "use net
// class" dummy first and read back without it, row by row, skipping an entry KiCad's loader skips (one missing a key).
#[test]
fn the_project_carries_the_differential_pair_sizes() {
    let (mut design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let pairs = vec![DiffPairPreset { width: mm(0.2), gap: mm(0.15), via_gap: mm(0.18) }, DiffPairPreset { width: mm(0.25), gap: mm(0.2), via_gap: mm(0.2) }];
    design.routing = Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: pairs.clone(), teardrop_settings: Default::default() });
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    assert_eq!(
        pro["board"]["design_settings"]["diff_pair_dimensions"],
        serde_json::json!([
            { "width": 0.0, "gap": 0.0, "via_gap": 0.0 },
            { "width": 0.2, "gap": 0.15, "via_gap": 0.18 },
            { "width": 0.25, "gap": 0.2, "via_gap": 0.2 }
        ]),
        "KiCad's list starts with the net class's dummy row"
    );

    // The project read back: the dummy row is not a pair of ours.
    let (mut back, _, _) = import_kicad_pcb(BOARD).expect("imports");
    merge_project_predefined_sizes(&mut back, &serde_json::to_string(&pro).unwrap());
    assert_eq!(back.routing.as_ref().unwrap().diff_pair_presets, pairs);

    // A row missing a key is left out, as KiCad's loader leaves it out.
    let pro = serde_json::json!({ "board": { "design_settings": { "diff_pair_dimensions": [
        { "width": 0.0, "gap": 0.0, "via_gap": 0.0 },
        { "width": 0.2, "gap": 0.15 },
        { "width": 0.3, "gap": 0.2, "via_gap": 0.25 }
    ] } } });
    merge_project_predefined_sizes(&mut back, &pro.to_string());
    assert_eq!(back.routing.as_ref().unwrap().diff_pair_presets, vec![DiffPairPreset { width: mm(0.3), gap: mm(0.2), via_gap: mm(0.25) }]);
    assert_eq!(back.routing.as_ref().unwrap().diff_pair_presets[0].via_gap, Nm::new(250_000));
    merge_project_predefined_sizes(&mut back, "not json");
    assert_eq!(back.routing.as_ref().unwrap().diff_pair_presets.len(), 1, "unreadable JSON is a no-op");
}

// r5 R5-3: the project's `track_widths` and `via_dimensions` (`BOARD_DESIGN_SETTINGS`' PARAM_LAMBDAs) are written with the index-0 net-class dummy
// first, as the pairs are, and read back without it; p22 F5 for the reader.
#[test]
fn the_project_carries_the_predefined_track_widths_and_via_sizes() {
    let (mut design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let widths = vec![mm(0.4), mm(0.25)];
    let vias = vec![ViaPreset { diameter: mm(0.8), drill: mm(0.4) }];
    design.routing = Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: widths.clone(), via_presets: vias.clone(), diff_pair_presets: vec![], teardrop_settings: Default::default() });
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    assert_eq!(pro["board"]["design_settings"]["track_widths"], serde_json::json!([0.0, 0.4, 0.25]));
    assert_eq!(pro["board"]["design_settings"]["via_dimensions"], serde_json::json!([{ "diameter": 0.0, "drill": 0.0 }, { "diameter": 0.8, "drill": 0.4 }]));

    let (mut back, _, _) = import_kicad_pcb(BOARD).expect("imports");
    merge_project_predefined_sizes(&mut back, &serde_json::to_string(&pro).unwrap());
    let rt = back.routing.as_ref().unwrap();
    assert_eq!(rt.track_width_presets, widths);
    assert_eq!(rt.via_presets, vias);

    // KiCad's loaders skip a via row that is not an object or misses a key, and a null width, before the first entry is taken as the net class's (R5-11)
    let pro = serde_json::json!({ "board": { "design_settings": {
        "track_widths": [null, 0.0, 0.3],
        "via_dimensions": [{}, { "diameter": 0.0, "drill": 0.0 }, { "diameter": 0.6 }, { "diameter": 0.6, "drill": 0.3 }],
        "diff_pair_dimensions": [{}, { "width": 0.0, "gap": 0.0, "via_gap": 0.0 }, { "width": 0.2, "gap": 0.15, "via_gap": 0.15 }]
    } } });
    merge_project_predefined_sizes(&mut back, &pro.to_string());
    let rt = back.routing.as_ref().unwrap();
    assert_eq!(rt.track_width_presets, vec![mm(0.3)]);
    assert_eq!(rt.via_presets, vec![ViaPreset { diameter: mm(0.6), drill: mm(0.3) }]);
    assert_eq!(rt.diff_pair_presets, vec![DiffPairPreset { width: mm(0.2), gap: mm(0.15), via_gap: mm(0.15) }]);
}

fn class(name: &str, nets: &[&str], clearance: Option<eda_model::units::Nm>, track: Option<eda_model::units::Nm>) -> NetClass {
    NetClass { name: name.into(), nets: nets.iter().map(|s| s.to_string()).collect(), track_width: track, clearance, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }
}

/// A net in more than one class has KiCad compose its settings (`GetEffectiveNetClass`), which only the project can say: the classes are the project's, in precedence order, and each
/// net names each class it is in, exactly (the matching of patterns is the studio's, KiCad is told its result).
#[test]
fn a_net_in_several_classes_moves_the_net_classes_into_the_project_in_precedence_order() {
    let (design, mut model, _) = import_kicad_pcb(BOARD).expect("imports");
    model.nets = ["VBUS", "V3.3", "V3X3", "USB_DP", "GND"].iter().map(|n| Net { name: n.to_string(), pins: vec![] }).collect();
    model.board.net_classes = vec![class("power", &["VBUS", "V3\\.3"], Some(mm(0.4)), None), class("fast", &["USB_*", "VBUS"], Some(mm(0.1)), Some(mm(0.5)))];
    let text = export(&design, &model);
    assert!(!text.contains("(net_class "), "VBUS is in two classes: the project has them");
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design, &model)).unwrap();
    let classes = pro["net_settings"]["classes"].as_array().unwrap();
    let order: Vec<(&str, i64)> = classes.iter().map(|c| (c["name"].as_str().unwrap(), c["priority"].as_i64().unwrap())).collect();
    assert_eq!(order, [("Default", i32::MAX as i64), ("power", 0), ("fast", 1)], "a class's place in the list is its priority; the router's own NetClass::priority is not KiCad's");
    let pattern = |net: &str| -> Vec<String> { pro["net_settings"]["netclass_patterns"].as_array().unwrap().iter().filter(|p| p["pattern"] == net).map(|p| p["netclass"].as_str().unwrap().to_string()).collect() };
    assert_eq!(pattern("VBUS"), ["power", "fast"], "one entry for each class the net is in");
    assert_eq!(pattern("V3\\.3"), ["power"], "a name with a dot is written so that it does not match V3X3");
    assert!(pattern("V3X3").is_empty() && pattern("GND").is_empty(), "a net in no class is in the Default class: no entry");
    assert_eq!(pattern("USB_DP"), ["fast"]);

    // A net in one class alone each keeps the board file's classes.
    model.board.net_classes = vec![class("power", &["VBUS"], Some(mm(0.4)), None), class("fast", &["USB_*"], Some(mm(0.1)), Some(mm(0.5)))];
    assert!(export(&design, &model).contains("(net_class \"power\""), "one class for each net: the legacy blocks are enough");
    assert!(export_kicad_pro_for(&design, &model).contains("\"rule_severities\"") && !export_kicad_pro_for(&design, &model).contains("net_settings"));
}

/// A project's classes come in the order KiCad composes them in: by `priority` (the lowest number first, a class that says none is -1) and then by name, whatever order the file
/// lists them in (KiCad writes them by name).
#[test]
fn a_projects_net_classes_come_in_in_the_order_of_their_priority() {
    let pro = serde_json::json!({ "net_settings": { "classes": [
        { "name": "Default", "priority": 2147483647, "clearance": 0.2 },
        { "name": "alpha", "priority": 2, "clearance": 0.3 },
        { "name": "bravo", "priority": 0, "clearance": 0.4 },
        { "name": "charlie", "priority": 1, "clearance": 0.5 },
        { "name": "delta", "clearance": 0.6 }
    ], "netclass_patterns": [
        { "netclass": "alpha", "pattern": "A*" }, { "netclass": "bravo", "pattern": "B*" }, { "netclass": "charlie", "pattern": "C*" }, { "netclass": "delta", "pattern": "D*" }
    ] } });
    let classes = eda_kicad::parse_project_net_classes(&pro.to_string());
    assert_eq!(classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["delta", "bravo", "charlie", "alpha"], "-1 (none) first, then 0, 1, 2");
}

/// X3 (r2-editors-exports): the board's plot settings (`PCB_PLOT_PARAMS`) are the imported board's own after an import and a write, not a fixed block;
/// the layer selection of a file older than 20240819 is in the legacy layer numbers (`remapLegacyLayerLSET`), and is written in today's.
#[test]
fn pcbplotparams_survive_import_and_write() {
    let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
	(layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
	(setup
		(pcbplotparams (layerselection 0x0001000_80000001) (plot_on_all_layers_selection 0x0000000_00000000) (usegerberextensions yes) (subtractmaskfromsilk yes)
			(gerberprecision 5) (plotreference no) (useauxorigin yes) (dashed_line_dash_ratio 8.000000) (outputdirectory "gerbers/"))
	)
	(net 0 "")
)
"#;
    let (design, model, _) = import_kicad_pcb(board).expect("imports");
    let plot = design.drawings.as_ref().and_then(|d| d.plot_params.clone()).expect("the block is kept");
    assert_eq!(plot.layer_selection, ["F.Cu", "B.Cu", "Edge.Cuts"], "legacy F.Cu 0, B.Cu 31 and Edge.Cuts 44");
    let text = export(&design, &model);
    let block = &text[text.find("(pcbplotparams").unwrap()..];
    let block = &block[..block.find("\n\t\t)").unwrap()];
    for item in ["(layerselection 0x00000000_00000000_00000000_02000005)", "(usegerberextensions yes)", "(subtractmaskfromsilk yes)", "(gerberprecision 5)", "(useauxorigin yes)", "(outputdirectory \"gerbers/\")", "(dashed_line_dash_ratio 8.000000)", "(dashed_line_gap_ratio 3.000000)"] {
        assert!(block.contains(item), "{item} in {block}");
    }
    // `(plotreference ..)` is a legacy token `PCB_PLOT_PARAMS_PARSER` skips (`default: skipCurrent()`) and `Format` never writes (r3 X3-1).
    assert!(!block.contains("plotreference"), "{block}");
    let (again, _, _) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(again.drawings.and_then(|d| d.plot_params).map(|p| p.layer_selection), Some(plot.layer_selection), "the selection reads back the same in today's numbers");
    // A board with no block of its own writes KiCad's defaults (`PCB_PLOT_PARAMS::Format` of a default `PCB_PLOT_PARAMS`).
    let (d0, m0, _) = import_kicad_pcb(r#"(kicad_pcb (version 20241229) (generator "pcbnew") (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user)) (net 0 ""))"#).expect("imports");
    let text = export(&d0, &m0);
    assert!(text.contains("(layerselection 0x00000000_00000000_55555555_5755f5ff)") && text.contains("(usegerberextensions no)") && text.contains("(sketchdnponfab yes)"), "{text}");
}

/// R6-R7 (`BOARD_DESIGN_SETTINGS`' `tuning_pattern_settings` reader: each key taken as it is, no check): a diff-pair group with its minimum amplitude above
/// its maximum, as KiCad's panel lets one type it, is read as it is, and the single-track group next to it keeps its own spacing.
#[test]
fn merge_project_setup_pages_keeps_each_group_as_kicad_reads_it() {
    let (_, mut model, _) = import_kicad_pcb(BOARD).expect("imports");
    let pro = serde_json::json!({ "board": { "design_settings": { "tuning_pattern_settings": {
        "single_track_defaults": { "spacing": 0.9 },
        "diff_pair_defaults": { "min_amplitude": 1.2, "max_amplitude": 1.0, "corner_radius_percentage": 150 },
    } } } });
    merge_project_setup_pages(&mut model, &pro.to_string());
    assert_eq!(model.board.tuning_patterns.single_track.spacing_nm, mm(0.9));
    assert_eq!((model.board.tuning_patterns.diff_pair.min_amplitude_nm, model.board.tuning_patterns.diff_pair.max_amplitude_nm), (mm(1.2), mm(1.0)));
    assert_eq!(model.board.tuning_patterns.diff_pair.corner_radius_percentage, 150, "no clamp");
}
