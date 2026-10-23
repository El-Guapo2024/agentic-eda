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
    assert_eq!(syntax["message"], "Syntax check passed with warnings:\nNo errors found.\n");
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

/// What `GetLayerName`, `CuStack` and `UIOrder` give for a `.kicad_pcb`: the layers of its `(layers ...)` table, a user name when it has one.
fn qa_board(pcb: &str) -> eda_drc::rule_editor::EditorBoard {
    let mut b = eda_drc::rule_editor::EditorBoard::default();
    let Some(start) = pcb.find("\n\t(layers") else { return eda_drc::rule_editor::EditorBoard::two_layer() };
    for line in pcb[start + 1..].lines().skip(1) {
        let t = line.trim();
        if t == ")" {
            break;
        }
        let q: Vec<&str> = t.split('"').collect();
        let Some(name) = q.get(1) else { continue };
        b.enabled.push(name.to_string());
        if name.ends_with(".Cu") {
            b.copper.push(name.to_string());
        }
        if let Some(user) = q.get(3000) {
            b.names.insert(name.to_string(), user.to_string());
        }
    }
    // `CuStack`: F.Cu, the inner layers in order, B.Cu.
    b.copper.sort_by_key(|l| match l.as_str() {
        "F.Cu" => 0,
        "B.Cu" => 1_000_000,
        l => l.trim_start_matches("In").trim_end_matches(".Cu").parse::<i32>().unwrap_or(999_000),
    });
    b
}

/// kicad-cli's findings on a QA board, without the report's date and paths; `None` when kicad-cli refuses the rules.
fn qa_findings(cli: &Path, dir: &Path, name: &str) -> Option<Vec<String>> {
    let out = dir.join("drc.json");
    let _ = std::fs::remove_file(&out);
    let st = std::process::Command::new(cli)
        .args(["pcb", "drc", "--format", "json", "--severity-all", "-o"])
        .arg(&out)
        .arg(dir.join(format!("{name}.kicad_pcb")))
        .output()
        .unwrap();
    if !st.status.success() && !out.exists() {
        eprintln!("{name}: kicad-cli: {}", String::from_utf8_lossy(&st.stdout));
        return None;
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&out).ok()?).ok()?;
    // (A marker on the board itself gets a new id each run.)
    let mut f: Vec<String> = ["violations", "unconnected_items"]
        .iter()
        .flat_map(|k| v[*k].as_array().cloned().unwrap_or_default())
        .map(|mut x| {
            for item in x["items"].as_array_mut().into_iter().flatten() {
                if item["description"] == "PCB" {
                    item["uuid"] = Value::Null;
                }
            }
            x.to_string()
        })
        .collect();
    f.sort();
    Some(f)
}

/// The slow tier, on KiCad's own QA boards (`qa/data/pcbnew/*.kicad_dru` and the boards beside them): each judged by kicad-cli with its rules
/// file, then with the file the editor writes back unedited -- the findings are the same, item for item -- and then, for each panel kind the file
/// has, with its first rule saved once through its panel with a comment added (`TransferDataFromWindow`, the rule generated from the panel): kicad-cli
/// takes the file and finds the same. Where Save refuses, it is for KiCad's own reasons, in KiCad's words (`onSaveButtonClicked`'s numbered list:
/// the other panel of a two-panel rule has the name, a value of 0, a layer assertion with no side).
#[test]
fn kicad_cli_judges_kicads_qa_boards_alike_by_the_rules_the_editor_wrote_back() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let (Some(cli), Some(src)) = (eda_kicad_engine::find_cli(), std::env::var_os("KICAD_SRC_DIR")) else {
        eprintln!("kicad-cli or KICAD_SRC_DIR missing; skipping");
        return;
    };
    use eda_drc::rule_editor::{apply_edit, load, save, selected_token};
    let qa = Path::new(&src).join("qa/data/pcbnew");
    let mut names: Vec<String> = std::fs::read_dir(&qa)
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".kicad_dru")).map(str::to_string))
        .filter(|n| !n.starts_with("._") && qa.join(format!("{n}.kicad_pcb")).exists())
        .collect();
    names.sort();
    assert!(names.len() >= 20, "{names:?}");
    let (mut saved, mut refused) = (0, Vec::new());
    for name in &names {
        let dir = scratch(&format!("re_qa_{name}"));
        for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
            let from = qa.join(format!("{name}.{ext}"));
            if from.exists() {
                std::fs::copy(&from, dir.join(format!("{name}.{ext}"))).unwrap();
            }
        }
        let dru = dir.join(format!("{name}.kicad_dru"));
        let text = std::fs::read_to_string(&dru).unwrap();
        let board = qa_board(&std::fs::read_to_string(dir.join(format!("{name}.kicad_pcb"))).unwrap());
        let base = qa_findings(&cli, &dir, name).unwrap_or_else(|| panic!("{name}: kicad-cli refused KiCad's own rules"));
        let entries = load(&text, &board.env()).unwrap_or_else(|e| panic!("{name}: {e}"));

        let unedited = save(&entries, &board);
        std::fs::write(&dru, &unedited).unwrap();
        assert_eq!(qa_findings(&cli, &dir, name).as_ref(), Some(&base), "{name}: written back unedited, the rules judge the board otherwise\n{unedited}");

        let mut kinds = std::collections::BTreeSet::new();
        for (i, e) in entries.iter().enumerate() {
            if !kinds.insert(e.panel) {
                continue;
            }
            let edit = RuleEdit {
                panel: e.panel,
                name: e.rule_name.clone(),
                comment: if e.comment.is_empty() { "saved by the editor".to_string() } else { format!("{}\nsaved by the editor", e.comment) },
                condition: e.rule_condition.clone(),
                layer: selected_token(e.panel, &e.layers, &e.layer_source, &board),
                fields: e.fields.clone(),
                value_text: None,
                taken_names: Vec::new(),
                severity: None,
                constraint_code: None,
            };
            let mut list = entries.clone();
            if let Err(m) = apply_edit(&mut list, Some(i), &edit, &board) {
                assert!(m.starts_with("1. "), "{name}: {m}");
                refused.push(format!("{name} {:?} {:?}: {}", e.panel, e.rule_name, m.trim_end().replace('\n', " ")));
                continue;
            }
            let written = save(&list, &board);
            assert!(written.contains("# saved by the editor"), "{name}:\n{written}");
            std::fs::write(&dru, &written).unwrap();
            assert_eq!(
                qa_findings(&cli, &dir, name).as_ref(),
                Some(&base),
                "{name}: rule {i} ({:?}) saved through its panel judges the board otherwise\n--- read\n{}\n--- written\n{}",
                e.panel,
                e.original_text,
                list[i].generated_rule
            );
            saved += 1;
        }
    }
    eprintln!("{} QA rule files; {saved} rules saved through their panels; Save refused, as KiCad does:\n  {}", names.len(), refused.join("\n  "));
    assert!(saved >= 25_000, "{saved}");
}
