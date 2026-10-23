//! The DRC Rule Editor through a board directory (P9b): its verb (`Cmd::EditDrcRule`) stores the rules file as Board Setup > Custom Rules does, one undo
//! step each, refuses an edit made against rules that changed since, and -- the slow tier -- kicad-cli's DRC judges the board by the rule it saved.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_drc::rule_editor::{Fields, PanelType, RuleEdit};
use eda_ops::{Cmd, Domain, DrcRuleAction};
use serde_json::Value;
use std::path::Path;

fn rules(dir: &Path) -> String {
    load(dir).unwrap().2.board.custom_rules_text.unwrap_or_default()
}

fn clearance(name: &str, mm: &str) -> RuleEdit {
    RuleEdit {
        panel: PanelType::MinimumClearance,
        name: name.into(),
        comment: String::new(),
        condition: String::new(),
        layer: String::new(),
        fields: Fields::default(),
        value_text: Some(mm.into()),
        taken_names: Vec::new(),
        severity: None,
        constraint_code: None,
    }
}

fn save_rule(entry: Option<usize>, name: &str, rule: RuleEdit) -> Cmd {
    Cmd::EditDrcRule { action: DrcRuleAction::Save, entry, name: name.into(), rule: Some(rule) }
}

#[test]
fn the_rule_editors_verb_writes_the_rules_file_one_undo_step_at_a_time() {
    let dir = scratch("rule_editor_verb");
    setup(&dir);
    // A new rule: added to an empty file.
    step(&dir, save_rule(None, "", clearance("wide", "1.5")), false, "ui").unwrap();
    assert_eq!(rules(&dir), "(version 1)\n(rule wide\n\t(constraint clearance (min 1.5mm))\n)\n");
    // The editor's view of it: one rule, under Minimum clearance, with the layer selector on Any.
    let view = crate::rule_editor_api::view(&dir);
    assert_eq!(view["ok"], true, "{view}");
    assert_eq!(view["entries"][0]["panel"], "minimum_clearance");
    assert_eq!(view["entries"][0]["fields"]["value"].as_f64(), Some(1.5));
    assert_eq!(view["entries"][0]["layer_token"], "");
    assert_eq!(view["layer_options"]["courtyard_clearance"][1]["label"], "Top");
    assert_eq!(view["nets"], serde_json::json!(["GND", "VCC"]));
    // Edited: the same rule, regenerated.
    let mut e = clearance("wide", "1.0");
    e.layer = "F.Cu".into();
    step(&dir, save_rule(Some(0), "wide", e), false, "ui").unwrap();
    assert_eq!(rules(&dir), "(version 1)\n(rule wide\n\t(layer \"F.Cu\")\n\t(constraint clearance (min 1mm))\n)\n");
    // A second one, then Move Up puts it first.
    step(&dir, save_rule(None, "", clearance("narrow", "0.1")), false, "ui").unwrap();
    step(&dir, Cmd::EditDrcRule { action: DrcRuleAction::MoveUp, entry: Some(1), name: "narrow".into(), rule: None }, false, "ui").unwrap();
    assert!(rules(&dir).starts_with("(version 1)\n(rule narrow\n"), "{}", rules(&dir));
    // An edit against rules that changed since is refused, and so is one Save would refuse.
    let err = step(&dir, Cmd::EditDrcRule { action: DrcRuleAction::Delete, entry: Some(0), name: "wide".into(), rule: None }, false, "ui").unwrap_err();
    assert!(err[0].hint.as_deref().unwrap_or("").contains("reopen the DRC Rule Editor"), "{err:?}");
    let err = step(&dir, save_rule(None, "", clearance("wide", "0")), false, "ui").unwrap_err();
    assert_eq!(err[0].hint.as_deref().unwrap_or(""), "1. Rule name must be unique.\n2. Value must be greater than 0.\n");
    // The check the dialog runs before Save says the same, and changes nothing.
    let body = serde_json::json!({ "entry": null, "rule": clearance("wide", "0") }).to_string();
    let checked = crate::rule_editor_api::check(&dir, body.as_bytes());
    assert_eq!((checked["valid"].as_bool(), checked["message"].as_str()), (Some(false), Some("1. Rule name must be unique.\n2. Value must be greater than 0.\n")));
    let syntax = crate::rule_editor_api::check(&dir, serde_json::json!({ "syntax": { "name": "x", "body": "(constraint clearance (min 1mm))" } }).to_string().as_bytes());
    assert_eq!(syntax["message"], "Syntax OK");
    // Delete, then each step undoes on its own.
    step(&dir, Cmd::EditDrcRule { action: DrcRuleAction::Delete, entry: Some(0), name: "narrow".into(), rule: None }, false, "ui").unwrap();
    assert_eq!(rules(&dir), "(version 1)\n(rule wide\n\t(layer \"F.Cu\")\n\t(constraint clearance (min 1mm))\n)\n");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(rules(&dir).contains("(rule narrow"));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(rules(&dir), "(version 1)\n(rule wide\n\t(constraint clearance (min 1.5mm))\n)\n");
    // A file that does not parse is not opened, and the verb leaves it alone.
    step(&dir, Cmd::SetCustomRules { text: "(version 1)\n(rule broken (constraint clearance (min 1mm))".into() }, false, "ui").unwrap();
    let view = crate::rule_editor_api::view(&dir);
    assert_eq!(view["ok"], false);
    assert!(view["message"].as_str().unwrap().contains("Board Setup > Custom Rules"), "{view}");
    assert!(step(&dir, save_rule(None, "", clearance("x", "1")), false, "ui").is_err());
}

fn clearance_findings(report: &Value) -> Vec<String> {
    report["violations"].as_array().unwrap().iter().filter(|v| v["type"] == "clearance").map(|v| v.to_string()).collect()
}

/// The slow tier: a rule saved by the editor's verb is the rule kicad-cli's DRC judges the board by. U1's two pads (GND and VCC) are 1.2 mm apart:
/// a 1.5 mm clearance rule makes kicad-cli report them, naming the rule, and the rule edited to 1 mm makes the finding go.
#[test]
fn kicad_cli_judges_the_board_by_the_rule_the_editor_saved() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = scratch("rule_editor_kicad_cli");
    setup(&dir);
    let before = crate::kicad_engine::drc(&dir, false).expect("kicad-cli ran");
    assert!(clearance_findings(&before).is_empty(), "{before:#}");

    step(&dir, save_rule(None, "", clearance("pads apart", "1.5")), false, "ui").unwrap();
    let derived = std::fs::read_to_string(dir.join(".kicad").join("board.kicad_dru")).unwrap_or_default();
    let after = crate::kicad_engine::drc(&dir, false).expect("kicad-cli ran");
    let found = clearance_findings(&after);
    assert!(!found.is_empty(), "kicad-cli reports the pads by the new rule: {after:#}\n{derived}");
    assert!(found.iter().all(|f| f.contains("pads apart")), "the findings name the rule: {found:#?}");

    let mut e = clearance("pads apart", "1");
    e.comment = "relaxed".into();
    step(&dir, save_rule(Some(0), "pads apart", e), false, "ui").unwrap();
    let relaxed = crate::kicad_engine::drc(&dir, false).expect("kicad-cli ran");
    assert!(clearance_findings(&relaxed).is_empty(), "{relaxed:#}");
}
