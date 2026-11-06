//! Board Setup > Design Rules > Component Classes through a board directory (UI-LEFT B1): the verb (`Cmd::SetComponentClasses`) keeps the page in the project
//! file kicad-cli reads (`component_class_settings` of the derived `.kicad_pro`), one undo step; the page's server half (`POST /api/component_classes`) says
//! what each assignment matches; and -- the slow tier -- kicad-cli's DRC sees a class the page defined through a rule that reads `A.hasComponentClass(..)`.

use super::tests::{drc_counts, scratch, setup};
use super::{load, step, undo};
use eda_model::component_classes::{ClassAssignment, ClassCondition, ComponentClassSettings, ConditionKind};
use eda_ops::{Cmd, Domain};
use std::path::Path;

fn assignment(class: &str, all: bool, conditions: &[(ConditionKind, &str, &str)]) -> ClassAssignment {
    ClassAssignment { class: class.into(), all, conditions: conditions.iter().map(|(kind, p, s)| ClassCondition { kind: *kind, primary: p.to_string(), secondary: s.to_string() }).collect() }
}

fn page(assignments: Vec<ClassAssignment>) -> ComponentClassSettings {
    ComponentClassSettings { sheet_classes: false, assignments }
}

/// The derived project file kicad-cli reads (`eda_kicad::export_kicad_pro_for`).
fn project(dir: &Path) -> serde_json::Value {
    let (_, design, model) = load(dir).unwrap();
    serde_json::from_str(&eda_kicad::export_kicad_pro_for(&design, &model)).unwrap()
}

#[test]
fn the_component_classes_page_is_kept_in_the_project_file_one_undo_step_at_a_time() {
    let dir = scratch("component_classes_verb");
    setup(&dir);
    assert!(load(&dir).unwrap().2.board.component_classes.is_default());
    let settings = page(vec![assignment("BIG", true, &[(ConditionKind::Reference, "U1", "")]), assignment("FRONT", false, &[(ConditionKind::Side, "Front", ""), (ConditionKind::Rotation, "Any", "")])]);
    step(&dir, Cmd::SetComponentClasses { settings: settings.clone() }, false, "ui").unwrap();
    assert_eq!(load(&dir).unwrap().2.board.component_classes, settings, "the page is the board's now");
    // The derived project carries it as KiCad writes it (`COMPONENT_CLASS_SETTINGS::saveAssignment`).
    let pro = project(&dir);
    let cc = &pro["component_class_settings"];
    assert_eq!(cc["assignments"][0], serde_json::json!({ "component_class": "BIG", "conditions": { "REFERENCE": { "primary": "U1" } }, "conditions_operator": "ALL" }), "{cc}");
    assert_eq!(cc["assignments"][1]["conditions_operator"], "ANY");
    assert_eq!(cc["sheet_component_classes"]["enabled"], false);

    // The page's server half: what each assignment matches, and the lists the condition rows offer.
    let reply = crate::board_setup_api::component_classes(&dir, serde_json::json!({ "assignments": settings.assignments }).to_string().as_bytes());
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["matches"], serde_json::json!([["U1"], ["U1", "U2"]]), "{reply}");
    assert_eq!(reply["compiles"], serde_json::json!([true, true]));
    assert_eq!(reply["fields"], serde_json::json!(["Datasheet", "Description", "Reference", "Value"]), "every footprint has KiCad's four mandatory fields");
    // A custom condition the compiler refuses gives no rule: nothing to highlight, and the page's OK is refused with KiCad's message.
    let bad = assignment("BAD", true, &[(ConditionKind::Custom, "A.Reference ==", "")]);
    let reply = crate::board_setup_api::component_classes(&dir, serde_json::json!({ "assignments": [bad.clone()] }).to_string().as_bytes());
    assert_eq!((reply["compiles"][0].clone(), reply["matches"][0].clone()), (serde_json::json!(false), serde_json::Value::Null), "{reply}");
    let err = step(&dir, Cmd::SetComponentClasses { settings: page(vec![bad]) }, false, "ui").unwrap_err();
    assert!(format!("{err:?}").contains("Error with conditions for component class assignment BAD"), "{err:?}");
    let err = step(&dir, Cmd::SetComponentClasses { settings: page(vec![assignment("", true, &[])]) }, false, "ui").unwrap_err();
    assert!(format!("{err:?}").contains("Error with conditions for component class assignment"), "an assignment with no class gives no rule: {err:?}");
    // Undo takes the page back.
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(load(&dir).unwrap().2.board.component_classes.is_default());
    assert!(project(&dir).get("component_class_settings").is_none());
}

/// kicad-cli's DRC judges the board by a class the page defined (slow tier): a rule that asserts every footprint of class `BIG` is U2 fails on U1 once the page
/// puts U1 in `BIG`, and passes again when the page changes or Undo takes it back.
#[test]
fn kicad_cli_sees_a_component_class_the_page_defined() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = scratch("component_classes_cli");
    setup(&dir);
    let rule = "(version 1)\n(rule \"big parts\"\n  (condition \"A.Type == 'Footprint' && A.hasComponentClass('BIG')\")\n  (constraint assertion \"A.Reference == 'U2'\"))\n";
    step(&dir, Cmd::SetCustomRules { text: rule.into() }, false, "ui").unwrap();
    let failures = |dir: &Path| drc_counts(dir).get("assertion_failure").copied().unwrap_or(0);
    assert_eq!(failures(&dir), 0, "no footprint is in BIG yet");
    step(&dir, Cmd::SetComponentClasses { settings: page(vec![assignment("BIG", true, &[(ConditionKind::Reference, "U1", "")])]) }, false, "ui").unwrap();
    assert_eq!(failures(&dir), 1, "U1 is in BIG and is not U2");
    // Both footprints are on the front: U1 fails, U2 passes.
    step(&dir, Cmd::SetComponentClasses { settings: page(vec![assignment("BIG", false, &[(ConditionKind::Side, "Front", "")])]) }, false, "ui").unwrap();
    assert_eq!(failures(&dir), 1, "U1 and U2 are in BIG, U2 is U2");
    // A class only U2 is in.
    step(&dir, Cmd::SetComponentClasses { settings: page(vec![assignment("BIG", true, &[(ConditionKind::Reference, "U2", ""), (ConditionKind::Side, "Front", "")])]) }, false, "ui").unwrap();
    assert_eq!(failures(&dir), 0);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(failures(&dir), 1);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(failures(&dir), 0, "Undo takes the class back");
}
