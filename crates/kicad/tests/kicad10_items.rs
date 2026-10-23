//! KiCad 10's line endings, grid items, via stitching and drill charts and maps: the QA boards of KiCad 25ec2335 (the commit the box's kicad-cli nightly is built from)
//! that have them come back from `import_kicad_pcb` -> `export_kicad_pcb` so that kicad-cli loads them and its DRC finds what it finds on the original, and every one
//! of these items is written as KiCad writes it: kicad-cli's own re-save (`pcb upgrade --force`) of our file leaves the item as we wrote it.
//!
//! Slow: it needs kicad-cli 10.99 and KiCad's QA boards of a source new enough to have the items (`KICAD10_QA_DATA`, by default
//! `/home/team_braneboard_com/src/kicad-src-25ec2335/qa/data`; the 8303b2ad source of the rest of the port predates all four kinds). Run with
//! `EDA_SLOW_TESTS=1 cargo test -p eda-kicad --test kicad10_items -- --nocapture`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};

const QA_DEFAULT: &str = "/home/team_braneboard_com/src/kicad-src-25ec2335/qa/data";

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> Option<PathBuf> {
    let out = Command::new("which").arg("kicad-cli").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

fn qa_data() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("KICAD10_QA_DATA").unwrap_or_else(|_| QA_DEFAULT.to_string()));
    p.join("pcbnew").exists().then_some(p)
}

fn work_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join("eda_kicad10_items").join(tag);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// kicad-cli's DRC report of `pcb` (`--refill-zones --severity-all`, as JSON), `None` when it did not load the board.
fn drc(cli: &Path, pcb: &Path, tag: &str) -> Option<serde_json::Value> {
    let report = pcb.with_file_name(format!("{}.{tag}.drc.json", pcb.file_stem().unwrap().to_str().unwrap()));
    let _ = std::fs::remove_file(&report);
    let out = Command::new(cli).args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--output"]).arg(&report).arg(pcb).output().ok()?;
    if !report.exists() {
        eprintln!("kicad-cli did not load {} ({tag}): {} {}", pcb.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(&report).ok()?).ok()
}

/// A finding by its type, severity, description and its items' descriptions and places (micrometres).
fn findings(report: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["violations", "unconnected_items"] {
        for v in report.get(key).and_then(|v| v.as_array()).into_iter().flatten() {
            let s = |k: &str| v.get(k).and_then(|t| t.as_str()).unwrap_or("").to_string();
            let mut items: Vec<String> = v
                .get("items")
                .and_then(|i| i.as_array())
                .into_iter()
                .flatten()
                .map(|i| {
                    let mm = |p: &str| (i.pointer(p).and_then(|x| x.as_f64()).unwrap_or(0.0) * 1000.0).round() as i64;
                    format!("{} @ {},{}", i.get("description").and_then(|d| d.as_str()).unwrap_or(""), mm("/pos/x"), mm("/pos/y"))
                })
                .collect();
            items.sort();
            out.push(format!("{key}|{}|{}|{}|{}", s("type"), s("severity"), nets_in_order(&s("description")), items.join(" ; ")));
        }
    }
    out.sort();
    out
}

/// "Items shorting two nets (nets A and B)" names the two nets in the order of their net codes, which are the file's: a re-export numbers its nets anew, so the pair is
/// put in the order of the names.
fn nets_in_order(description: &str) -> String {
    let Some(at) = description.find("(nets ") else { return description.to_string() };
    let rest = &description[at + 6..];
    let Some(close) = rest.rfind(')') else { return description.to_string() };
    let Some((a, b)) = rest[..close].split_once(" and ") else { return description.to_string() };
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    format!("{}(nets {a} and {b}){}", &description[..at], &rest[close + 1..])
}

fn type_counts(report: &serde_json::Value) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for v in report.get("violations").and_then(|v| v.as_array()).into_iter().flatten() {
        *m.entry(v.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string()).or_insert(0) += 1;
    }
    m
}

/// The board `rel` (under `qa/data/pcbnew`) staged with its project and rules, read, written back with its derived project, and both judged by kicad-cli. Returns the re-export's
/// path and text, and panics with the two reports' differences when they differ.
fn reexport_judged_the_same(cli: &Path, src: &Path, rel: &str) -> (PathBuf, String) {
    let src = src.to_path_buf();
    let name = src.file_stem().unwrap().to_str().unwrap().to_string();
    let orig_dir = work_dir(&format!("{name}/orig"));
    for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
        let from = src.with_extension(ext);
        if from.exists() {
            std::fs::copy(&from, orig_dir.join(from.file_name().unwrap())).unwrap();
        }
    }
    let staged = orig_dir.join(src.file_name().unwrap());
    let original = drc(cli, &staged, "orig").unwrap_or_else(|| panic!("kicad-cli does not load the original {rel}"));

    let text = std::fs::read_to_string(&staged).unwrap();
    let (design, mut model, _notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {rel}: {e:?}"));
    if let Ok(pro) = std::fs::read_to_string(src.with_extension("kicad_pro")) {
        eda_kicad::merge_project_net_classes(&mut model, &pro);
        eda_kicad::merge_project_rule_severities(&mut model, &pro);
        eda_kicad::merge_project_design_rules(&mut model, &pro);
        eda_kicad::merge_project_setup_pages(&mut model, &pro);
    }
    if let Ok(dru) = std::fs::read_to_string(src.with_extension("kicad_dru")) {
        eda_kicad::merge_custom_rules(&mut model, &dru);
    }
    let out = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: &name }).unwrap_or_else(|e| panic!("export {rel}: {e:?}"));
    let dir = work_dir(&format!("{name}/reexport"));
    let path = dir.join(format!("{name}.kicad_pcb"));
    std::fs::write(&path, &out).unwrap();
    std::fs::write(dir.join(format!("{name}.kicad_pro")), eda_kicad::export_kicad_pro_for(&design, &model)).unwrap();
    if let Some(rules) = model.board.custom_rules_text.as_deref() {
        std::fs::write(dir.join(format!("{name}.kicad_dru")), rules).unwrap();
    }
    let back = drc(cli, &path, "reexport").unwrap_or_else(|| panic!("kicad-cli does not load our re-export of {rel} ({})", path.display()));
    let (a, b) = (findings(&original), findings(&back));
    let only_a: Vec<&String> = a.iter().filter(|f| !b.contains(f)).collect();
    let only_b: Vec<&String> = b.iter().filter(|f| !a.contains(f)).collect();
    assert!(
        only_a.is_empty() && only_b.is_empty() && type_counts(&original) == type_counts(&back),
        "{rel}: kicad-cli's DRC differs after the round trip\n  original {:?}\n  re-export {:?}\n  only in the original:\n    {}\n  only in the re-export:\n    {}",
        type_counts(&original),
        type_counts(&back),
        only_a.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n    "),
        only_b.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n    ")
    );
    eprintln!("{rel}: the same {} findings {:?}", a.len(), type_counts(&original));
    (path, out)
}

/// The QA boards of KiCad 25ec2335 with these items: line endings (`line_ending_drc`, `line_ending_zone_flood`), via stitching (`stitch_fp_rulearea`, `issue25265`, `issue25303`), a
/// drill chart and two drill maps (`api_kitchen_sink`).
const BOARDS: &[&str] = &[
    "line_ending_drc/line_ending_drc_fail.kicad_pcb",
    "line_ending_drc/line_ending_drc_pass.kicad_pcb",
    "line_ending_zone_flood/line_ending_zone_flood.kicad_pcb",
    "stitch_fp_rulearea.kicad_pcb",
    "issue25265.kicad_pcb",
    "issue25303.kicad_pcb",
    "api_kitchen_sink.kicad_pcb",
];

/// Boards of our own, for the kinds no QA board has: grid items (cartesian and polar, turned, locked, with a custom property), as kicad-cli 10.99 (25ec2335) saved them.
const FIXTURES: &[&str] = &["kicad10_grid_items.kicad_pcb"];

#[test]
fn kicad10_item_boards_reexport_as_kicad_cli_judges_the_originals() {
    if slow_tests_off() {
        return;
    }
    let (Some(cli), Some(qa)) = (kicad_cli(), qa_data()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's QA boards (KICAD10_QA_DATA)");
        return;
    };
    let only = std::env::var("EDA_KICAD10_ONLY").unwrap_or_default();
    let mut failures = Vec::new();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let boards = BOARDS.iter().map(|rel| (qa.join("pcbnew").join(rel), rel.to_string())).chain(FIXTURES.iter().map(|f| (fixtures.join(f), f.to_string())));
    for (src, rel) in boards.filter(|(_, b)| only.is_empty() || b.contains(only.as_str())) {
        let run = || {
            let (path, ours) = reexport_judged_the_same(&cli, &src, &rel);
            items_are_written_as_kicad_writes_them(&cli, &src, &rel, &path, &ours);
        };
        if let Err(e) = std::panic::catch_unwind(run) {
            failures.push(e.downcast_ref::<String>().cloned().unwrap_or_else(|| format!("{rel}: panicked")));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ------------------------------------------------------------------------------------------------ the items as KiCad writes them

/// A parsed s-expression: an atom (quotes taken off) or a list.
#[derive(Debug, Clone, PartialEq)]
enum Sx {
    Atom(String),
    List(Vec<Sx>),
}

fn parse(text: &str) -> Sx {
    fn go(chars: &[char], i: &mut usize) -> Sx {
        while chars[*i].is_whitespace() {
            *i += 1;
        }
        if chars[*i] == '(' {
            *i += 1;
            let mut items = Vec::new();
            loop {
                while chars[*i].is_whitespace() {
                    *i += 1;
                }
                if chars[*i] == ')' {
                    *i += 1;
                    return Sx::List(items);
                }
                items.push(go(chars, i));
            }
        }
        let mut atom = String::new();
        if chars[*i] == '"' {
            *i += 1;
            while chars[*i] != '"' {
                if chars[*i] == '\\' {
                    *i += 1;
                }
                atom.push(chars[*i]);
                *i += 1;
            }
            *i += 1;
        } else {
            while !chars[*i].is_whitespace() && chars[*i] != '(' && chars[*i] != ')' {
                atom.push(chars[*i]);
                *i += 1;
            }
        }
        Sx::Atom(atom)
    }
    let chars: Vec<char> = text.chars().collect();
    go(&chars, &mut 0)
}

impl Sx {
    fn tag(&self) -> Option<&str> {
        match self {
            Sx::List(l) => match l.first() {
                Some(Sx::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    fn items(&self) -> &[Sx] {
        match self {
            Sx::List(l) => l,
            _ => &[],
        }
    }
    fn find(&self, tag: &str) -> Option<&Sx> {
        self.items().iter().find(|i| i.tag() == Some(tag))
    }
    /// The canonical text: numbers as numbers (`3` and `3.000000` are one), the uuids of the item and its parts and the members a generator lists by uuid left out
    /// (ours are made by the exporter, not kept from the file).
    fn canon(&self) -> String {
        match self {
            Sx::Atom(a) => match a.parse::<f64>() {
                Ok(v) if a.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') => format!("{}", (v * 1e6).round() / 1e6),
                _ => a.clone(),
            },
            Sx::List(l) => {
                let parts: Vec<String> = l.iter().filter(|i| !matches!(i.tag(), Some("uuid") | Some("tstamp") | Some("members"))).map(Sx::canon).collect();
                format!("({})", parts.join(" "))
            }
        }
    }
}

/// The KiCad 10 items of a board file, each as its canonical text: the endings of every line, arc and curve (by its two ends), and every grid item, via stitching
/// generator, drill chart and drill map whole -- with the number of vias a generator lists, which the uuids it lists by stand for.
fn kicad10_items(text: &str) -> Vec<String> {
    let root = parse(text);
    let mut out = Vec::new();
    for item in root.items() {
        match item.tag() {
            Some("gr_line" | "gr_arc" | "gr_curve") => {
                // `(start_shape none)` is what no ending reads as; KiCad's writer leaves it out (`LINE_ENDING::Format`), a generated QA file has it.
                let ends: Vec<String> = ["start_shape", "end_shape"].iter().filter_map(|t| item.find(t)).filter(|e| e.items().get(1) != Some(&Sx::Atom("none".into()))).map(Sx::canon).collect();
                if !ends.is_empty() {
                    let at: Vec<String> = ["start", "end", "pts"].iter().filter_map(|t| item.find(t)).map(Sx::canon).collect();
                    out.push(format!("{} {} {}", item.tag().unwrap(), at.join(" "), ends.join(" ")));
                }
            }
            Some("grid_item" | "drill_chart" | "drill_map") => out.push(item.canon()),
            Some("generated") if item.find("type").and_then(|t| t.items().get(1)) == Some(&Sx::Atom("via_stitch".into())) => {
                let members = item.find("members").map_or(0, |m| m.items().len() - 1);
                out.push(format!("{} members {members}", item.canon()));
            }
            _ => {}
        }
    }
    out.sort();
    out
}

/// Every KiCad 10 item of the original is in our re-export as the original has it, and kicad-cli's own re-save of our file (`pcb upgrade --force`, which writes every
/// item with KiCad's `PCB_IO_KICAD_SEXPR::format`) leaves each one as we wrote it.
fn items_are_written_as_kicad_writes_them(cli: &Path, src: &Path, rel: &str, ours_path: &Path, ours: &str) {
    let original = std::fs::read_to_string(src).unwrap();
    let resaved_path = ours_path.with_file_name(format!("{}.resaved.kicad_pcb", ours_path.file_stem().unwrap().to_str().unwrap()));
    std::fs::write(&resaved_path, ours).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&resaved_path).output().unwrap();
    assert!(up.status.success(), "{rel}: kicad-cli pcb upgrade failed: {}", String::from_utf8_lossy(&up.stderr));
    let resaved = std::fs::read_to_string(&resaved_path).unwrap();
    let (a, b, c) = (kicad10_items(&original), kicad10_items(ours), kicad10_items(&resaved));
    let diff = |x: &[String], y: &[String]| -> Vec<String> { x.iter().filter(|i| !y.contains(i)).cloned().collect() };
    assert!(!a.is_empty(), "{rel}: no KiCad 10 item in the original");
    assert!(
        a == b && b == c,
        "{rel}: the KiCad 10 items differ\n  original only:\n    {}\n  ours only:\n    {}\n  kicad-cli's re-save of ours only:\n    {}",
        diff(&a, &b).join("\n    "),
        diff(&b, &a).join("\n    "),
        diff(&c, &b).join("\n    ")
    );
    eprintln!("{rel}: {} KiCad 10 items written as the original has them, and as kicad-cli writes them", a.len());
}
