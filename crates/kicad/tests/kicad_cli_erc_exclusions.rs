//! Slow tier (`EDA_SLOW_TESTS=1`, kicad-cli on the path): the ERC markers `eda_kicad::erc_exclusions` puts together for the checks whose report names fewer
//! items than the marker, or a field by a uuid the file does not keep (UI-LEFT-3 B3-2) -- `four_way_junction`, `no_connect_connected` of a pin and of a flag,
//! `unresolved_variable` of a text and of a symbol's field, `generic-warning` of a text, `generic-error` of a symbol's field, and `duplicate_pins` -- are the
//! markers kicad-cli makes: written to the project as records, each turns its violation excluded with its own comment, and no record is left over as an
//! excluded marker of its own (an ERC record that matches nothing becomes one, `SCHEMATIC::ResolveERCExclusions`).
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use eda_kicad::erc_exclusions::{entry, marker_with, tree_facts, witnesses, MarkerItem};
use eda_model::units::Nm;
use serde_json::{json, Value};

const ROOT: &str = "11111111-1111-1111-1111-000000000000";

fn u(n: u32) -> String {
    format!("{n:08x}-0000-0000-0000-{n:012x}")
}

fn wire(a: (f64, f64), b: (f64, f64), n: u32) -> String {
    format!("(wire (pts (xy {} {}) (xy {} {})) (stroke (width 0) (type default)) (uuid \"{}\"))", a.0, a.1, b.0, b.1, u(n))
}

fn symbol(lib: &str, reference: &str, x: f64, y: f64, n: u32, pins: &str, extra: &str) -> String {
    format!(
        "(symbol (lib_id \"{lib}\") (at {x} {y} 0) (unit 1) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (uuid \"{}\")
 (property \"Reference\" \"{reference}\" (at {x} {} 0) (effects (font (size 1.27 1.27))))
 (property \"Value\" \"X\" (at {x} {} 0) (effects (font (size 1.27 1.27))))
 {extra} {pins}
 (instances (project \"p\" (path \"/{ROOT}\" (reference \"{reference}\") (unit 1)))))",
        u(n),
        y - 3.0,
        y + 3.0
    )
}

/// A sheet with one of each finding: four wire ends at a point; a no-connect-type pin on a wire, and another with two wires, a junction and a label on it; a
/// no-connect flag on a pin wired to another symbol's pin; a symbol with a field holding `${NOPE2}` and one holding `${ERC_ERROR boom}`; texts with `${NOPE}`
/// and `${ERC_WARNING watch}`; a symbol with two pins numbered 1 on two nets.
fn sheet() -> String {
    let pin = |kind: &str, x: f64, angle: u32, name: &str, number: &str| format!("(pin {kind} line (at {x} 0 {angle}) (length 2.54) (name \"{name}\" (effects (font (size 1.27 1.27)))) (number \"{number}\" (effects (font (size 1.27 1.27)))))");
    let lib = |name: &str, pins: String| {
        format!(
            "(symbol \"T:{name}\" (pin_names (offset 1.016)) (exclude_from_sim no) (in_bom yes) (on_board yes)
 (property \"Reference\" \"U\" (at 0 2.54 0) (effects (font (size 1.27 1.27)))) (property \"Value\" \"{name}\" (at 0 -2.54 0) (effects (font (size 1.27 1.27))))
 (symbol \"{name}_1_1\" {pins}))"
        )
    };
    let libs = format!("(lib_symbols {} {})", lib("X", pin("passive", -3.81, 0, "A", "1") + &pin("no_connect", 3.81, 180, "N", "2")), lib("D", pin("passive", -3.81, 0, "A", "1") + &pin("passive", 3.81, 180, "B", "1")));
    let pins = |n: u32| format!("(pin \"1\" (uuid \"{}\")) (pin \"2\" (uuid \"{}\"))", u(n + 1), u(n + 2));
    let label = |name: &str, x: f64, y: f64, n: u32| format!("(label \"{name}\" (at {x} {y} 0) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid \"{}\"))", u(n));
    let text = |t: &str, x: f64, y: f64, n: u32| format!("(text \"{t}\" (exclude_from_sim no) (at {x} {y} 0) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid \"{}\"))", u(n));
    let c = (76.2, 20.32);
    let items = [
        wire((71.12, 20.32), c, 110),
        wire((81.28, 20.32), c, 111),
        wire((76.2, 15.24), c, 112),
        wire((76.2, 25.4), c, 113),
        symbol(
            "T:X",
            "U1",
            50.8,
            50.8,
            200,
            &pins(200),
            "(property \"Note\" \"${NOPE2}\" (at 50.8 57 0) (effects (font (size 1.27 1.27)))) (property \"Chk\" \"${ERC_ERROR boom}\" (at 50.8 59 0) (effects (font (size 1.27 1.27))))",
        ),
        wire((54.61, 50.8), (60.96, 50.8), 210),
        symbol("T:X", "U2", 50.8, 76.2, 300, &pins(300), ""),
        symbol("T:X", "U3", 40.64, 101.6, 400, &pins(400), ""),
        wire((36.83, 101.6), (36.83, 76.2), 310),
        wire((36.83, 76.2), (46.99, 76.2), 311),
        format!("(no_connect (at 46.99 76.2) (uuid \"{}\"))", u(320)),
        symbol("T:X", "U4", 50.8, 127.0, 700, &pins(700), ""),
        wire((54.61, 127.0), (60.96, 127.0), 600),
        wire((54.61, 127.0), (54.61, 134.62), 601),
        format!("(junction (at 54.61 127) (diameter 0) (color 0 0 0 0) (uuid \"{}\"))", u(603)),
        label("NL", 54.61, 127.0, 604),
        text("${NOPE}", 101.6, 101.6, 500),
        text("${ERC_WARNING watch}", 101.6, 111.76, 501),
        symbol("T:D", "U5", 101.6, 50.8, 800, &format!("(pin \"1\" (uuid \"{}\")) (pin \"1\" (uuid \"{}\"))", u(801), u(802)), ""),
        wire((97.79, 50.8), (92.71, 50.8), 810),
        wire((105.41, 50.8), (110.49, 50.8), 811),
        label("NA", 92.71, 50.8, 812),
        label("NB", 110.49, 50.8, 813),
    ];
    format!("(kicad_sch (version 20250114) (generator \"eeschema\") (generator_version \"9.0\") (uuid \"{ROOT}\") (paper \"A4\")\n{libs}\n{}\n(sheet_instances (path \"/\" (page \"1\"))))", items.join("\n"))
}

fn run(cli: &Path, dir: &Path, exclusions: &[Value]) -> Vec<Value> {
    let pro = json!({ "meta": { "filename": "p.kicad_pro", "version": 3 }, "erc": { "meta": { "version": 1 }, "erc_exclusions": exclusions,
        "rule_severities": { "four_way_junction": "warning", "lib_symbol_issues": "ignore" } } });
    std::fs::write(dir.join("p.kicad_pro"), pro.to_string()).unwrap();
    let out = dir.join("r.json");
    let _ = std::fs::remove_file(&out);
    Command::new(cli).args(["sch", "erc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&out).arg(dir.join("p.kicad_sch")).output().unwrap();
    let r: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    r["sheets"].as_array().unwrap().iter().flat_map(|s| s["violations"].as_array().unwrap().clone()).collect()
}

#[test]
fn the_markers_put_together_from_the_tree_reach_kicad_cli() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = Command::new("which").arg("kicad-cli").output().ok().filter(|o| o.status.success()).map(|o| std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string())) else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("erc_witnesses_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.kicad_sch"), sheet()).unwrap();
    let first = run(&cli, &dir, &[]);
    let facts = tree_facts("p.kicad_sch", &|name| std::fs::read_to_string(dir.join(name)).ok());
    let wanted = ["four_way_junction", "no_connect_connected", "unresolved_variable", "generic-warning", "generic-error", "duplicate_pins"];
    let mut records = Vec::new();
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    for (i, v) in first.iter().enumerate() {
        let check = v["type"].as_str().unwrap();
        if !wanted.contains(&check) {
            continue;
        }
        let mut items: Vec<MarkerItem> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|it| MarkerItem {
                uuid: it["uuid"].as_str().unwrap().into(),
                x_nm: Nm::kiround(it["pos"]["x"].as_f64().unwrap() * 1e6),
                y_nm: Nm::kiround(it["pos"]["y"].as_f64().unwrap() * 1e6),
                description: it["description"].as_str().unwrap().into(),
            })
            .collect();
        let child = witnesses(&facts, check, &mut items).unwrap_or_else(|| panic!("{check}: the tree says where its marker is: {v:#}"));
        let mut m = marker_with(check, v["severity"].as_str().unwrap(), &items, &format!("/{ROOT}"), 1, &|_| Vec::new()).unwrap_or_else(|| panic!("{check}: a marker"));
        m.child = child;
        let comment = format!("{check} #{i}");
        records.push(entry(&m, &comment));
        expected.insert(comment, format!("{check}: {}", v["description"]));
    }
    assert_eq!(expected.len(), 9, "every finding of the fixture: {first:#?}");
    let after = run(&cli, &dir, &records);
    assert_eq!(after.len(), first.len(), "no record became an excluded marker of its own: {after:#?}");
    let mut got: BTreeMap<String, String> = BTreeMap::new();
    for v in after.iter().filter(|v| v["excluded"] == true) {
        got.insert(v["comment"].as_str().unwrap().into(), format!("{}: {}", v["type"].as_str().unwrap(), v["description"]));
    }
    assert_eq!(got, expected, "each record excluded its own violation");
    let _ = std::fs::remove_dir_all(&dir);
}
