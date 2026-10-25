//! A studio project opened in KiCad and updated from its schematic (Update PCB from Schematic, F8) renames nothing and duplicates nothing (fix batch A,
//! `docs/parity/findings/p1-p2-footprints.md` F1-F5): what KiCad's netlist of the derived schematic gives each footprint -- the net of every pad
//! (`/<sheet>/<label>`, `Net-(D1-Pad1)`, `unconnected-(..)`), the footprint (`Lib:Name`), the link to its symbol (`(path ..)`, `sheetname`, `sheetfile`),
//! the symbol's fields, every pad's `pinfunction` and `pintype` -- is what the derived board already has (`BOARD_NETLIST_UPDATER`, which F8 runs, would
//! change nothing). Held against kicad-cli twice: its DRC with `--schematic-parity` finds no difference, and its netlist (`kicad-cli sch export netlist`)
//! agrees with the board item by item.
//!
//! Slow tier (`EDA_SLOW_TESTS=1`, the real kicad-cli). The boards are examples placed and routed by `eda place` and `eda route`, in a folder made by `eda board
//! new` (the design those wrote holds the intent's net names, so the board's loader renaming them is held too).

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn eda(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(args).output().expect("run eda");
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "eda {}:\n{log}", args.join(" "));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn slow() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return false;
    }
    true
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_kicad_names_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn kicad_cli() -> PathBuf {
    std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"))
}

// ------------------------------------------------------------------------------------------------- a small s-expression reader

#[derive(Debug, Clone)]
enum S {
    Atom(String),
    List(Vec<S>),
}

fn parse(text: &str) -> S {
    let mut stack: Vec<Vec<S>> = vec![Vec::new()];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' => stack.push(Vec::new()),
            ')' => {
                let list = stack.pop().unwrap();
                stack.last_mut().unwrap().push(S::List(list));
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => s.extend(chars.next()),
                        '"' => break,
                        c => s.push(c),
                    }
                }
                stack.last_mut().unwrap().push(S::Atom(s));
            }
            c if c.is_whitespace() => {}
            c => {
                let mut s = c.to_string();
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '(' || n == ')' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                stack.last_mut().unwrap().push(S::Atom(s));
            }
        }
    }
    stack.pop().unwrap().into_iter().next().unwrap()
}

impl S {
    fn items(&self) -> &[S] {
        match self {
            S::List(l) => l,
            S::Atom(_) => &[],
        }
    }
    fn tag(&self) -> Option<&str> {
        match self.items().first() {
            Some(S::Atom(a)) => Some(a),
            _ => None,
        }
    }
    fn all<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a S> + 'a {
        self.items().iter().filter(move |s| s.tag() == Some(tag))
    }
    fn one(&self, tag: &str) -> Option<&S> {
        self.items().iter().find(|s| s.tag() == Some(tag))
    }
    /// The `i`-th item as text (`(name "x")`'s 1 is `x`).
    fn txt(&self, i: usize) -> Option<&str> {
        match self.items().get(i) {
            Some(S::Atom(a)) => Some(a),
            _ => None,
        }
    }
    fn val(&self, tag: &str) -> Option<&str> {
        self.one(tag).and_then(|s| s.txt(1))
    }
}

// ------------------------------------------------------------------------------------------------- what KiCad's netlist says

/// One netlist component, as `BOARD_NETLIST_UPDATER` reads it.
#[derive(Debug, Default)]
struct Component {
    footprint: String,
    /// `COMPONENT::GetPath` + the first of `GetKIIDs`: the footprint's `(path ..)` after F8.
    path: String,
    sheetname: String,
    sheetfile: String,
    /// every field but Reference, Value and Footprint (`compFields`)
    fields: BTreeMap<String, String>,
}

/// (ref, pad) -> (net name, pinfunction, pintype).
type Nodes = BTreeMap<(String, String), (String, String, String)>;

fn netlist(sch: &Path) -> (BTreeMap<String, Component>, Nodes) {
    let out = sch.with_extension("net");
    let run = Command::new(kicad_cli()).args(["sch", "export", "netlist", "--format", "kicadsexpr", "-o"]).arg(&out).arg(sch).output().expect("kicad-cli runs");
    assert!(out.exists(), "kicad-cli wrote no netlist: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let root = parse(&std::fs::read_to_string(&out).unwrap());
    let mut comps = BTreeMap::new();
    for comp in root.one("components").unwrap().all("comp") {
        let reference = comp.val("ref").unwrap().to_string();
        let mut c = Component { footprint: comp.val("footprint").unwrap_or("").to_string(), ..Default::default() };
        let sheetpath = comp.one("sheetpath").unwrap();
        let stamps = sheetpath.val("tstamps").unwrap().trim_end_matches('/');
        c.path = format!("{stamps}/{}", comp.val("tstamps").unwrap());
        c.sheetname = sheetpath.val("names").unwrap().to_string();
        for p in comp.all("property") {
            if p.val("name") == Some("Sheetfile") {
                c.sheetfile = p.val("value").unwrap_or("").to_string();
            }
        }
        if let Some(fields) = comp.one("fields") {
            for f in fields.all("field") {
                let name = f.val("name").unwrap().to_string();
                if !["Reference", "Value", "Footprint"].contains(&name.as_str()) {
                    c.fields.insert(name, f.txt(2).unwrap_or("").to_string());
                }
            }
        }
        comps.insert(reference, c);
    }
    let mut nodes = Nodes::new();
    for net in root.one("nets").unwrap().all("net") {
        let name = net.val("name").unwrap().to_string();
        for n in net.all("node") {
            let key = (n.val("ref").unwrap().to_string(), n.val("pin").unwrap().to_string());
            nodes.insert(key, (name.clone(), n.val("pinfunction").unwrap_or("").to_string(), n.val("pintype").unwrap_or("").to_string()));
        }
    }
    (comps, nodes)
}

// ------------------------------------------------------------------------------------------------- what the board has

#[derive(Debug, Default)]
struct BoardFootprint {
    fpid: String,
    path: String,
    sheetname: String,
    sheetfile: String,
    properties: BTreeMap<String, String>,
    /// pad -> (net name, pinfunction, pintype)
    pads: BTreeMap<String, (String, String, String)>,
}

fn board(pcb: &Path) -> BTreeMap<String, BoardFootprint> {
    let root = parse(&std::fs::read_to_string(pcb).unwrap());
    let mut out = BTreeMap::new();
    for fp in root.all("footprint") {
        let mut b = BoardFootprint { fpid: fp.txt(1).unwrap().to_string(), ..Default::default() };
        b.path = fp.val("path").unwrap_or("").to_string();
        b.sheetname = fp.val("sheetname").unwrap_or("").to_string();
        b.sheetfile = fp.val("sheetfile").unwrap_or("").to_string();
        for p in fp.all("property") {
            b.properties.insert(p.txt(1).unwrap().to_string(), p.txt(2).unwrap_or("").to_string());
        }
        for pad in fp.all("pad") {
            let net = pad.one("net").and_then(|n| n.txt(2)).unwrap_or("").to_string();
            b.pads.insert(pad.txt(1).unwrap().to_string(), (net, pad.val("pinfunction").unwrap_or("").to_string(), pad.val("pintype").unwrap_or("").to_string()));
        }
        out.insert(b.properties["Reference"].clone(), b);
    }
    out
}

/// The example placed and routed, in a board folder; then kicad-cli's parity DRC, which writes the derived project in `.kicad/`.
fn derived_project(example: &str) -> (PathBuf, Value) {
    let intent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/{example}.yaml"));
    let root = scratch(example);
    // placed, then routed (`eda place`, `eda route`: the pipeline's stages without its schematic gates)
    let (placed, routed) = (root.join("placed"), root.join("routed"));
    eda(&["place", intent.to_str().unwrap(), "-o", placed.to_str().unwrap(), "--seed", "0"]);
    eda(&["route", intent.to_str().unwrap(), "--design", placed.join("design.json").to_str().unwrap(), "-o", routed.to_str().unwrap()]);
    let b = root.join("board");
    eda(&["board", "new", intent.to_str().unwrap(), "-o", b.to_str().unwrap()]);
    std::fs::copy(routed.join("design.json"), b.join("design.json")).unwrap();
    let drc: Value = serde_json::from_str(&eda(&["board", "drc", "-C", b.to_str().unwrap(), "--schematic-parity"])).expect("the DRC report is JSON");
    (b, drc)
}

/// Every difference between the derived board and what KiCad's netlist of the derived schematic would make of it in F8.
fn f8_differences(dir: &Path) -> Vec<String> {
    let (comps, nodes) = netlist(&dir.join(".kicad").join("board.kicad_sch"));
    let fps = board(&dir.join(".kicad").join("board.kicad_pcb"));
    let mut out = Vec::new();
    for (reference, c) in &comps {
        let Some(fp) = fps.get(reference) else {
            out.push(format!("{reference}: no footprint on the board (F8 adds one)"));
            continue;
        };
        if fp.fpid != c.footprint {
            out.push(format!("{reference}: footprint {} where the symbol names {}", fp.fpid, c.footprint));
        }
        if fp.path != c.path {
            out.push(format!("{reference}: path {:?} where the symbol is {:?} (F8 relinks or, with the defaults, matches nothing)", fp.path, c.path));
        }
        if (fp.sheetname.as_str(), fp.sheetfile.as_str()) != (c.sheetname.as_str(), c.sheetfile.as_str()) {
            out.push(format!("{reference}: sheet {:?} {:?} where the symbol is on {:?} {:?}", fp.sheetname, fp.sheetfile, c.sheetname, c.sheetfile));
        }
        for (name, text) in &c.fields {
            match fp.properties.get(name) {
                None if text.is_empty() && ["Datasheet", "Description"].contains(&name.as_str()) => {} // a footprint always has these two
                None => out.push(format!("{reference}: no field {name:?} (the symbol's is {text:?})")),
                Some(t) if t != text => out.push(format!("{reference}: field {name:?} is {t:?}, the symbol's {text:?}")),
                Some(_) => {}
            }
        }
    }
    for ((reference, pin), (net, function, pin_type)) in &nodes {
        let Some(fp) = fps.get(reference) else { continue };
        match fp.pads.get(pin) {
            None => out.push(format!("{reference}.{pin}: no pad")),
            Some((n, f, t)) => {
                if n != net {
                    out.push(format!("{reference}.{pin}: net {n:?}, the schematic's {net:?}"));
                }
                if (f, t) != (function, pin_type) {
                    out.push(format!("{reference}.{pin}: pinfunction {f:?} pintype {t:?}, the schematic's {function:?} {pin_type:?}"));
                }
            }
        }
    }
    out
}

fn assert_kicad_agrees(example: &str) {
    let (dir, drc) = derived_project(example);
    let parity = drc["schematic_parity"].as_array().cloned().unwrap_or_default();
    let said: Vec<String> = parity.iter().map(|v| format!("{}: {}", v["type"].as_str().unwrap_or("?"), v["description"].as_str().unwrap_or("?"))).collect();
    assert!(said.is_empty(), "{example}: kicad-cli's parity DRC finds {} differences:\n{}", said.len(), said.join("\n"));
    let differences = f8_differences(&dir);
    assert!(differences.is_empty(), "{example}: F8 would change {} things:\n{}", differences.len(), differences.join("\n"));
    // the routed board after the loader renamed its nets: no copper of two nets touches, and every net is connected
    let shorts: Vec<&Value> = drc["violations"].as_array().into_iter().flatten().filter(|v| v["type"] == "shorting_items").collect();
    assert!(shorts.is_empty(), "{example}: kicad-cli's DRC finds shorts: {shorts:#?}");
    let unconnected = drc["unconnected_items"].as_array().map_or(0, Vec::len);
    assert_eq!(unconnected, 0, "{example}: kicad-cli's DRC finds unconnected items: {:#?}", drc["unconnected_items"]);
    // (but for the pin headers: their built-in pads are centred where KiCad's have pin 1 at the origin, so they keep the built-in pads -- PARITY-pcb.md
    // section 53 -- and the check says those differ)
    let intent: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/{example}.yaml"))).unwrap()).unwrap();
    let headers: Vec<String> = intent["parts"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter(|p| p["package"].as_str().is_some_and(|k| k.starts_with("PINHEADER")))
        .filter_map(|p| p["reference"].as_str().map(|r| format!("Footprint {r}")))
        .collect();
    let library: Vec<String> = library_differences(&dir).into_iter().filter(|d| !headers.iter().any(|h| d.ends_with(h.as_str()))).collect();
    assert!(library.is_empty(), "{example}: Update Footprints from Library would change {} footprints:\n{}", library.len(), library.join("\n"));
}

/// kicad-cli's library parity DRC (`DRC_TEST_PROVIDER_LIBRARY_PARITY`, `FOOTPRINT::FootprintNeedsUpdate`) of the board against KiCad's installed
/// libraries: every footprint whose library copy differs from it. The board's project ignores the test (it is KiCad's default, `lib_footprint_issues` and
/// `lib_footprint_mismatch` warn only with a library table); this copy of it turns them on, with a project table of the libraries the footprints name.
fn library_differences(dir: &Path) -> Vec<String> {
    let root = eda_kicad::default_footprint_library_root();
    let copy = dir.join("library_parity");
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(&copy).unwrap();
    let kicad = dir.join(".kicad");
    for name in ["board.kicad_pcb", "board.kicad_pro"] {
        std::fs::copy(kicad.join(name), copy.join(name)).unwrap();
    }
    let pro = std::fs::read_to_string(copy.join("board.kicad_pro")).unwrap();
    let pro = pro.replace("\"lib_footprint_issues\": \"ignore\"", "\"lib_footprint_issues\": \"error\"").replace("\"lib_footprint_mismatch\": \"ignore\"", "\"lib_footprint_mismatch\": \"error\"");
    std::fs::write(copy.join("board.kicad_pro"), pro).unwrap();
    let mut libs: Vec<String> = board(&copy.join("board.kicad_pcb")).values().filter_map(|f| f.fpid.split_once(':').map(|(l, _)| l.to_string())).collect();
    libs.sort();
    libs.dedup();
    let mut table = String::from("(fp_lib_table\n  (version 7)\n");
    for lib in &libs {
        table.push_str(&format!("  (lib (name \"{lib}\")(type \"KiCad\")(uri \"{}\")(options \"\")(descr \"\"))\n", root.join(format!("{lib}.pretty")).display()));
    }
    table.push_str(")\n");
    std::fs::write(copy.join("fp-lib-table"), table).unwrap();
    let report = copy.join("drc.json");
    Command::new(kicad_cli()).args(["pcb", "drc", "--format", "json", "--severity-all", "-o"]).arg(&report).arg(copy.join("board.kicad_pcb")).output().expect("kicad-cli runs");
    let drc: Value = serde_json::from_str(&std::fs::read_to_string(&report).expect("kicad-cli wrote its DRC")).unwrap();
    drc["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"].as_str().is_some_and(|t| t.starts_with("lib_footprint")))
        .map(|v| format!("{}: {}", v["type"].as_str().unwrap_or("?"), v["items"][0]["description"].as_str().unwrap_or("?")))
        .collect()
}

/// A flat sheet: every net a local label names is `/<label>` (the root's path), the rest `Net-(..)`; the footprints are the installed ones.
#[test]
fn kicad_cli_parity_is_clean_on_a_flat_board_made_from_an_example() {
    if !slow() {
        return;
    }
    assert_kicad_agrees("opamp_filter");
    assert_kicad_agrees("ldo");
}

/// Module sheets: a net between two sheets is named by the hierarchical label of the first sheet (`/Header (J1)/RESET`), a net inside one by its pins
/// (`Net-(D1-Pad1)`), a power net by its symbol (`GND`); a no-connect pin is on its own `unconnected-(..)` net; every footprint links to its symbol on its
/// sheet, with the symbol's fields (`Sim.Pins` of the LEDs).
#[test]
fn kicad_cli_parity_is_clean_on_a_hierarchical_board_made_from_an_example() {
    if !slow() {
        return;
    }
    assert_kicad_agrees("mcu_board_30plus");
    // (`nc_pins` is not here: its U1 has a no-connect pin 4 the built-in SOT-223 has no pad for, and KiCad's parity test says "No pad found for pin 4
    // in schematic" -- the intent's pinout against the footprint, nothing this test is about)
}
