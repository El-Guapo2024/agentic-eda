//! Board Setup's verbs: each stores its page in `design.drawings.rules`, validates like the page it comes from,
//! and the overlay then changes the rules the board is judged by.

use super::*;
use eda_model::ir::{PlacementSection, Provenance, RoutingSection, Shape, Text};
use eda_model::layers::{LayerSetup, LayerType};
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

fn vars(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
    rows.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
}

#[test]
fn text_variables_are_stored_as_a_whole_page() {
    let base = model();
    let mut b = Board::new(design(), &base, 100, 300);
    assert_eq!(overlay_of(&b).text_variables, None, "nothing is stored until the page is used");
    let page = vars(&[("REVISION", "B"), ("Designer name", "Ada"), ("PART_NO", "${REVISION}-12")]);
    b.apply(&Cmd::SetTextVariables { variables: page.clone() }).unwrap();
    assert_eq!(overlay_of(&b).text_variables, Some(page), "the rows are kept as typed, a value naming another variable included");
    b.apply(&Cmd::SetTextVariables { variables: vars(&[("REVISION", "C")]) }).unwrap();
    assert_eq!(overlay_of(&b).text_variables, Some(vars(&[("REVISION", "C")])), "a page replaces the whole set: the rows left out are deleted");
    b.apply(&Cmd::SetTextVariables { variables: BTreeMap::new() }).unwrap();
    assert_eq!(overlay_of(&b).text_variables, Some(BTreeMap::new()), "every row deleted is a page too, and it is not 'no page'");
    assert_eq!(effective(&b, &base).board, base.board, "text variables change no rule the board is judged by");
}

#[test]
fn a_bad_text_variables_page_is_refused_with_the_variables_name_and_stores_nothing() {
    let base = model();
    let mut b = Board::new(design(), &base, 100, 300);
    let kept = vars(&[("REVISION", "B")]);
    b.apply(&Cmd::SetTextVariables { variables: kept.clone() }).unwrap();
    for (bad, location) in [
        (vars(&[("", "x")]), "(unnamed)"),
        (vars(&[("REV.ISION", "x")]), "REV.ISION"),
        (vars(&[("A", "ok"), ("B(1)", "x")]), "B(1)"),
        (vars(&[("KIPRJMOD", "/x")]), "KIPRJMOD"),
        (vars(&[("NUL", "a\0b")]), "NUL"),
    ] {
        let e = b.apply(&Cmd::SetTextVariables { variables: bad.clone() }).unwrap_err();
        assert_eq!((e[0].check.as_str(), e[0].location.as_deref()), ("ops_bad_text_variables", Some(location)), "{bad:?}");
        assert_eq!(overlay_of(&b).text_variables, Some(kept.clone()), "a refused page changes nothing");
    }
}

fn layers_page(f: impl FnOnce(&mut LayerSetup)) -> LayerSetup {
    let mut s = LayerSetup::default();
    f(&mut s);
    s
}

fn four_layer_model() -> ConstraintModel {
    let mut m = model();
    m.board.layers = copper_layer_names(4);
    m
}

#[test]
fn board_layers_are_stored_as_a_whole_page_and_laid_over_the_board() {
    let base = four_layer_model();
    let mut b = Board::new(design(), &base, 100, 300);
    assert_eq!(overlay_of(&b).board_layers, None, "nothing is stored until the page is used");
    assert!(base.board.layer_setup.is_default(), "a board starts with every standard technical layer");
    let page = layers_page(|s| {
        s.enabled.push("User.2".into());
        s.enabled.push("User.1".into());
        s.enabled.retain(|l| l != "Eco2.User");
        s.names.insert("User.1".into(), "Notes".into());
        s.names.insert("In1.Cu".into(), "GND plane".into());
        s.names.insert("F.SilkS".into(), "F.Silkscreen".into());
        s.types.insert("In1.Cu".into(), LayerType::Power);
        s.types.insert("In2.Cu".into(), LayerType::Signal);
        s.types.insert("User.2".into(), LayerType::Back);
    });
    b.apply(&Cmd::SetBoardLayers { settings: page.clone() }).unwrap();
    let stored = overlay_of(&b).board_layers.expect("the page is stored");
    assert_eq!(stored, page.canonical(), "stored in KiCad's order, with a name or a type that is the default's dropped");
    assert_eq!(stored.enabled[stored.enabled.len() - 2..], ["User.1", "User.2"]);
    assert!(!stored.enabled.contains(&"Eco2.User".to_string()), "a technical layer can be turned off");
    assert_eq!((stored.names.len(), stored.types.len()), (2, 2));

    let m = effective(&b, &base);
    assert_eq!(m.board.layer_setup, stored);
    assert_eq!((m.board.layer_setup.display_name("User.1"), m.board.layer_setup.display_name("F.SilkS"), m.board.layer_setup.layer_type("In1.Cu"), m.board.layer_setup.layer_type("User.2")), ("Notes".to_string(), "F.Silkscreen".to_string(), LayerType::Power, LayerType::Back));
    assert_eq!(m.board.layers, base.board.layers, "the copper stack is the stackup's");

    b.apply(&Cmd::SetBoardLayers { settings: LayerSetup::default() }).unwrap();
    assert_eq!(overlay_of(&b).board_layers, Some(LayerSetup::default()), "the start is a page too: it is not 'no page'");
    assert_eq!(effective(&b, &base).board, base.board, "a page that says what the board already had changes nothing");
}

#[test]
fn a_bad_board_layers_page_is_refused_with_the_layers_name_and_stores_nothing() {
    let base = four_layer_model();
    let mut b = Board::new(design(), &base, 100, 300);
    let kept = layers_page(|s| {
        s.enabled.push("User.1".into());
        s.names.insert("User.1".into(), "Notes".into());
    });
    b.apply(&Cmd::SetBoardLayers { settings: kept.clone() }).unwrap();
    for (bad, location) in [
        (layers_page(|s| s.enabled.retain(|l| l != "Edge.Cuts")), "Edge.Cuts"),
        (layers_page(|s| s.enabled.retain(|l| l != "Margin")), "Margin"),
        (layers_page(|s| s.enabled.push("User.46".into())), "User.46"),
        (layers_page(|s| s.enabled.push("In1.Cu".into())), "In1.Cu"),
        (layers_page(|s| drop(s.names.insert("F.Mask".into(), "B.Mask".into()))), "B.Mask"),
        (layers_page(|s| drop(s.names.insert("F.Fab".into(), "".into()))), "F.Fab"),
        (layers_page(|s| drop(s.names.insert("F.Fab".into(), "a/b".into()))), "F.Fab"),
        (layers_page(|s| drop(s.names.insert("In2.Cu".into(), "signal".into()))), "In2.Cu"),
        (layers_page(|s| drop(s.names.insert("User.9".into(), "Nope".into()))), "User.9"),
        (layers_page(|s| { s.types.insert("In1.Cu".into(), LayerType::Front); }), "In1.Cu"),
        (layers_page(|s| { s.types.insert("F.SilkS".into(), LayerType::Back); }), "F.SilkS"),
    ] {
        let e = b.apply(&Cmd::SetBoardLayers { settings: bad.clone() }).unwrap_err();
        assert_eq!((e[0].check.as_str(), e[0].location.as_deref()), ("ops_bad_board_layers", Some(location)), "{bad:?}: {e:?}");
        assert_eq!(overlay_of(&b).board_layers, Some(kept.canonical()), "a refused page changes nothing");
    }
}

#[test]
fn a_layer_cannot_be_turned_off_while_items_are_on_it_and_can_once_they_are_not() {
    let base = four_layer_model();
    let mut b = Board::new(design(), &base, 100, 300);
    let text = Text { id: String::new(), content: "note".into(), at: Point { x: 5_000, y: 5_000 }, angle: 0, layer: "Eco1.User".into(), size_um: 1_000, stroke_width: 150, justify: Default::default(), mirror: false };
    b.apply(&Cmd::AddText { text }).unwrap();
    let without_eco1 = layers_page(|s| s.enabled.retain(|l| l != "Eco1.User"));
    let e = b.apply(&Cmd::SetBoardLayers { settings: without_eco1.clone() }).unwrap_err();
    assert_eq!((e[0].check.as_str(), e[0].location.as_deref()), ("ops_bad_board_layers", Some("Eco1.User")));
    assert!(e[0].hint.as_deref().unwrap().contains("1 item(s) are on User.Eco1"), "{e:?}");
    assert_eq!(overlay_of(&b).board_layers, None);
    // Another layer, with nothing on it, goes; and a layer the board never had cannot be "removed" by being left out of a page again.
    b.apply(&Cmd::SetBoardLayers { settings: layers_page(|s| s.enabled.retain(|l| l != "Eco2.User")) }).unwrap();
    assert!(!overlay_of(&b).board_layers.unwrap().enabled.contains(&"Eco2.User".to_string()));
    b.apply(&Cmd::SetBoardLayers { settings: layers_page(|s| s.enabled.retain(|l| l != "Eco2.User")) }).unwrap();
}

#[test]
fn nothing_new_goes_on_a_layer_the_board_has_turned_off_and_user_layers_that_are_on_take_items() {
    let mut base = four_layer_model();
    base.board.layer_setup = layers_page(|s| {
        s.enabled.retain(|l| l != "Eco1.User");
        s.enabled.push("User.3".into());
        s.names.insert("User.3".into(), "Notes".into());
    })
    .canonical();
    let mut b = Board::new(design(), &base, 100, 300);
    let line = |layer: &str| Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 0 } };
    let text = |layer: &str| Text { id: String::new(), content: "t".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: layer.into(), size_um: 1_000, stroke_width: 150, justify: Default::default(), mirror: false };
    let e = b.apply(&Cmd::AddShape { shape: line("Eco1.User") }).unwrap_err();
    assert_eq!((e[0].check.as_str(), e[0].location.as_deref()), ("ops_layer_off", Some("shape")));
    assert!(e[0].hint.as_deref().unwrap().contains("User.Eco1 is not a layer this board has"), "{e:?}");
    assert_eq!(b.apply(&Cmd::AddText { text: text("Eco1.User") }).unwrap_err()[0].check, "ops_layer_off");
    b.apply(&Cmd::AddShape { shape: line("User.3") }).unwrap();
    b.apply(&Cmd::AddText { text: text("User.3") }).unwrap();
    b.apply(&Cmd::AddShape { shape: line("F.SilkS") }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();
    let e = b.apply(&Cmd::EditShape { id, layer: "User.4".into(), stroke_width: 150, filled: false }).unwrap_err();
    assert_eq!(e[0].check, "ops_layer_off", "a shape cannot be moved to a layer that is off either");
    assert_eq!(b.design().drawings.as_ref().unwrap().shapes.len() + b.design().drawings.as_ref().unwrap().texts.len(), 3, "the refused ones added nothing");
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
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_board_layers", "settings": { "enabled": ["Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "User.1"], "names": { "User.1": "Notes" }, "types": { "In1.Cu": "power", "User.2": "front" } } })).unwrap();
    assert!(matches!(&cmd, Cmd::SetBoardLayers { settings } if settings.enabled.len() == 5 && settings.names["User.1"] == "Notes" && settings.types["In1.Cu"] == LayerType::Power && settings.types["User.2"] == LayerType::Front));
    assert_eq!(cmd.domain(), Domain::Pcb);
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_board_layers", "settings": {} })).unwrap();
    assert!(matches!(&cmd, Cmd::SetBoardLayers { settings } if settings.is_default()), "a page that says nothing is the start of a board");
    assert!(serde_json::from_value::<Cmd>(serde_json::json!({ "op": "set_board_layers", "settings": { "enabled": [], "colour": "red" } })).is_err(), "a field the page does not have is refused");
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_text_variables", "variables": { "REVISION": "B", "Designer name": "Ada" } })).unwrap();
    assert!(matches!(&cmd, Cmd::SetTextVariables { variables } if variables.len() == 2 && variables["REVISION"] == "B"));
    assert_eq!(cmd.domain(), Domain::Pcb);
    assert!(!cmd.clears_routing());
}
