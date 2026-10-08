//! Board Setup's verbs: each stores its page in `design.drawings.rules`, validates like the page it comes from,
//! and the overlay then changes the rules the board is judged by.

use super::*;
use eda_model::ir::{PlacementSection, Provenance, RoutingSection};
use eda_model::rules::{self, copper_layer_names, RulesOverlay};
use eda_model::{Net, NetClass};

fn design() -> Design {
    Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        routing: None,
        placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 50_000, y: 0 }, Point { x: 50_000, y: 50_000 }, Point { x: 0, y: 50_000 }], footprints: vec![], modules: vec![] }),
        drawings: None,
    }
}

fn model() -> ConstraintModel {
    ConstraintModel { nets: vec![Net { name: "GND".into(), pins: vec![] }, Net { name: "VBUS".into(), pins: vec![] }], ..Default::default() }
}

fn class(name: &str, nets: &[&str], clearance: Um) -> NetClass {
    NetClass { name: name.into(), nets: nets.iter().map(|s| s.to_string()).collect(), track_width: Some(300), clearance: Some(clearance), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }
}

fn overlay_of(b: &Board) -> RulesOverlay {
    b.design().drawings.as_ref().and_then(|d| d.rules.clone()).unwrap_or_default()
}

/// The model as the loader would hand it back after the edit: the overlay laid over the intent's.
fn effective(b: &Board, base: &ConstraintModel) -> ConstraintModel {
    let mut m = base.clone();
    overlay_of(b).apply(&mut m);
    m
}

#[test]
fn a_constraints_edit_is_stored_and_changes_the_effective_floors() {
    let base = model();
    let mut b = Board::new(design(), &base, 100, 300);
    assert!(overlay_of(&b).is_empty(), "nothing is stored until Board Setup is used");
    let constraints = Constraints { min_clearance_um: 250, min_track_width_um: 180, min_copper_edge_clearance_um: 300, ..Constraints::of(&base.board) };
    b.apply(&Cmd::SetConstraints { constraints: constraints.clone() }).unwrap();
    assert_eq!(overlay_of(&b).constraints, Some(constraints));

    let m = effective(&b, &base);
    assert_eq!((m.board.min_clearance_um, m.board.track_width_min_um, m.board.copper_edge_clearance_um), (250, 180, Some(300)));
    assert!(m.board.constraints_explicit);
    assert_eq!(base.board.min_clearance_um, 0, "the intent's own rules are untouched");
}

#[test]
fn the_verbs_leave_routing_and_the_other_editors_alone() {
    for cmd in [
        Cmd::SetConstraints { constraints: Constraints::of(&ConstraintModel::default().board) },
        Cmd::SetCustomRules { text: "(version 1)".into() },
        Cmd::SetRuleSeverities { severities: BTreeMap::new() },
    ] {
        assert_eq!(cmd.domain(), Domain::Pcb);
        assert!(!cmd.clears_routing());
    }
}

#[test]
fn an_out_of_range_constraint_is_refused_with_the_settings_name() {
    let base = model();
    let mut b = Board::new(design(), &base, 100, 300);
    let e = b.apply(&Cmd::SetConstraints { constraints: Constraints { min_clearance_um: 26_000, ..Constraints::of(&base.board) } }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_constraints");
    assert_eq!(e[0].location.as_deref(), Some("min_clearance"), "the dialog puts the message next to this field");
    assert!(overlay_of(&b).is_empty(), "a refused edit stores nothing");
}

#[test]
fn net_classes_with_patterns_replace_the_boards_classes_and_the_defaults() {
    let mut base = model();
    base.board.net_classes.push(class("old", &["GND"], 200));
    let mut b = Board::new(design(), &base, 100, 300);
    let settings = NetClassSettings {
        default: NetClass { name: "Default".into(), nets: vec![], track_width: Some(250), clearance: Some(220), via_diameter: Some(700), via_drill: Some(350), ..class("Default", &[], 0) },
        classes: vec![class("power", &["VBUS", "VBUS_*"], 300), class("fast", &["SPI_?"], 150)],
    };
    b.apply(&Cmd::SetNetClasses { settings }).unwrap();
    let m = effective(&b, &base);
    assert_eq!(m.board.net_classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["power", "fast"]);
    assert_eq!((m.board.track_width, m.board.clearance, m.board.via_diameter, m.board.via_drill), (250, 220, 700, 350));
    assert_eq!(m.board.clearance_of("VBUS_RAW"), 300, "a `*` pattern assigns a family of nets");
    assert_eq!(m.board.clearance_of("SPI_3"), 150, "a `?` pattern is one character");
    assert_eq!(m.board.clearance_of("SPI_33"), 220, "and no more: the whole name must match");
    assert_eq!(m.board.clearance_of("GND"), 220, "a net no pattern claims is in the Default class");

    let dup = NetClassSettings { default: class("Default", &[], 0), classes: vec![class("a", &["X"], 1), class("a", &["Y"], 1)] };
    assert_eq!(b.apply(&Cmd::SetNetClasses { settings: dup }).unwrap_err()[0].check, "ops_bad_netclasses");
}

#[test]
fn mask_paste_text_defaults_and_severities_validate_and_apply() {
    let base = model();
    let mut b = Board::new(design(), &base, 100, 300);

    let mask = MaskPaste { expansion_um: 50, min_width_um: 100, to_copper_clearance_um: 40, paste_margin_um: -50, paste_margin_ratio: -0.05, allow_bridges_in_footprints: true, tent_vias_front: false, tent_vias_back: true };
    b.apply(&Cmd::SetMaskPaste { settings: mask.clone() }).unwrap();
    assert_eq!(effective(&b, &base).board.solder_mask, mask);
    assert!(b.apply(&Cmd::SetMaskPaste { settings: MaskPaste { paste_margin_ratio: 2.0, ..mask } }).is_err(), "a paste ratio is within -100% .. 100%");

    let mut text = TextGraphicsDefaults::default();
    text.silk.text_height_um = 1_200;
    text.silk.text_width_um = 1_200;
    text.silk.text_thickness_um = 200;
    b.apply(&Cmd::SetTextGraphicsDefaults { settings: text }).unwrap();
    assert_eq!(effective(&b, &base).board.text_graphics.silk.text_height_um, 1_200);
    text.silk.text_thickness_um = 900;
    assert_eq!(b.apply(&Cmd::SetTextGraphicsDefaults { settings: text }).unwrap_err()[0].check, "ops_bad_text_defaults");

    let sev: BTreeMap<String, String> = [("clearance", "warning"), ("silk_overlap", "ignore")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    b.apply(&Cmd::SetRuleSeverities { severities: sev.clone() }).unwrap();
    assert_eq!(effective(&b, &base).board.rule_severities, sev);
    for bad in [("not_a_check", "error"), ("clearance", "loud")] {
        let e = b.apply(&Cmd::SetRuleSeverities { severities: [(bad.0.to_string(), bad.1.to_string())].into() }).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_severity", "{bad:?}");
    }
}

#[test]
fn custom_rules_text_is_kept_as_written() {
    let base = model();
    let mut b = Board::new(design(), &base, 100, 300);
    let text = "(version 1)\n(rule \"wide power\"\n  (constraint clearance (min 0.5mm))\n  (condition \"A.NetClass == 'power'\"))\n";
    b.apply(&Cmd::SetCustomRules { text: text.into() }).unwrap();
    assert_eq!(effective(&b, &base).board.custom_rules_text.as_deref(), Some(text));
    b.apply(&Cmd::SetCustomRules { text: "  \n".into() }).unwrap();
    assert_eq!(effective(&b, &base).board.custom_rules_text, None, "blank rules are no rules file");
    assert!(b.apply(&Cmd::SetCustomRules { text: "x".repeat(board_setup::MAX_CUSTOM_RULES_BYTES + 1) }).is_err());
}

#[test]
fn a_stackup_edit_changes_the_layers_and_the_thickness_but_not_while_copper_would_be_cut_off() {
    let base = model();
    let mut d = design();
    let mut four = StackupSettings::new(4, 1_600);
    four.stackup.layers.iter_mut().find(|l| l.name == "dielectric 1").unwrap().thickness_mm = Some(0.2);
    let mut b = Board::new(d.clone(), &base, 100, 300);
    b.apply(&Cmd::SetStackup { settings: four.clone() }).unwrap();
    let m = effective(&b, &base);
    assert_eq!(m.board.layers, copper_layer_names(4));
    assert_eq!(m.board.board_thickness_um, rules::stackup_thickness_um(&four.stackup));
    assert!(m.stackup.is_some(), "the physical stackup rides on the model");

    // Back to two layers is fine on an empty board but refused with a track on In1.Cu.
    let on_four = ConstraintModel { board: m.board.clone(), ..base.clone() };
    let two = StackupSettings::new(2, 1_600);
    d.routing = Some(RoutingSection {
        tracks: vec![Track { id: "t1".into(), net: "GND".into(), pins: vec![], layer: "In1.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1_000, y: 0 }], arc_mid_offset: None }],
        vias: vec![],
        zones: vec![],
        track_width_presets: vec![],
        via_presets: vec![],
        teardrop_settings: Default::default(),
    });
    let mut routed = Board::new(d, &on_four, 100, 300);
    let e = routed.apply(&Cmd::SetStackup { settings: two.clone() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_stackup");
    assert_eq!(e[0].location.as_deref(), Some("In1.Cu"));
    assert!(Board::new(design(), &on_four, 100, 300).apply(&Cmd::SetStackup { settings: two }).is_ok(), "nothing on the inner layers: they can go");

    assert!(b.apply(&Cmd::SetStackup { settings: StackupSettings { copper_layers: 3, ..four } }).is_err(), "a board has an even number of copper layers");
}

#[test]
fn the_verbs_read_from_the_json_the_studio_sends() {
    let cmd: Cmd = serde_json::from_value(serde_json::json!({
        "op": "set_constraints",
        "constraints": {
            "min_clearance_um": 200, "min_connection_um": 0, "min_track_width_um": 200, "min_annular_width_um": 100, "min_via_diameter_um": 500,
            "min_through_hole_um": 300, "min_microvia_diameter_um": 200, "min_microvia_drill_um": 100, "min_hole_to_hole_um": 250,
            "min_hole_clearance_um": 250, "min_copper_edge_clearance_um": 500, "min_silk_clearance_um": 0, "min_groove_width_um": 0,
            "min_silk_text_height_um": 800, "min_silk_text_thickness_um": 80, "max_error_um": 5, "min_resolved_spokes": 2,
            "use_height_for_length_calcs": true, "zones_allow_external_fillets": false
        }
    }))
    .unwrap();
    assert!(matches!(cmd, Cmd::SetConstraints { .. }));
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_mask_paste", "settings": { "expansion_um": 0, "paste_margin_um": 0, "paste_margin_ratio": -0.05 } })).unwrap();
    assert!(matches!(cmd, Cmd::SetMaskPaste { settings } if settings.paste_margin_ratio == -0.05));
}
