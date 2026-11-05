//! Design variants made in the studio (UI-LEFT S2, KiCad 25ec2335), through kicad-cli: on the placed and routed opamp filter (`eda pipeline
//! examples/opamp_filter.yaml`, in a folder made by `eda board new`), served by `eda board serve`, two variants are added (`add_design_variant`), one of them
//! gets R1 marked Do not populate and R2's Value overridden (`set_symbol_variant`), and the exports that take the variant (`--variant`) are run for each:
//!
//! - the placement file (`POST /api/fab/pos`, "Exclude all footprints with no courtyard" off, "Exclude DNP" on): R1 is left out of Lite's file only, R2's value
//!   is Lite's in Lite's file and its own in the others (`PLACE_FILE_EXPORTER`: `GetDNPForVariant`, `GetFieldValueForVariant`, from the board's footprint
//!   variants, `PCB_IO_KICAD_SEXPR::format( FOOTPRINT )`'s `(variant ..)`);
//! - the BOM (`POST /api/sch/bom`, "Include 'DNP' Symbols" off): the same, from the schematic's `(variant ..)` records;
//! - the netlist and a schematic PDF of a variant are written;
//! - a variant removed (`remove_design_variant`) is gone from the derived project, and kicad-cli's export of it is the default variant again.
//!
//! Slow tier only (`EDA_SLOW_TESTS=1`, kicad-cli on the PATH or in `EDA_KICAD_CLI`).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn slow() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return false;
    }
    true
}

fn eda(args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(args).output().expect("run eda");
    assert!(out.status.success(), "eda {}:\n{}{}", args.join(" "), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// The placed and routed opamp filter, served by `eda board serve` with the real kicad-cli. Killed, and its folder removed, when dropped.
struct Studio {
    root: PathBuf,
    board: PathBuf,
    port: u16,
    server: Child,
}

impl Studio {
    fn start(tag: &str) -> Studio {
        let root = std::env::temp_dir().join(format!("eda_design_variants_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml");
        let pipeline = root.join("pipeline");
        eda(&["pipeline", example.to_str().unwrap(), "-o", pipeline.to_str().unwrap(), "--seed", "0"]);
        let board = root.join("board");
        eda(&["board", "new", example.to_str().unwrap(), "-o", board.to_str().unwrap()]);
        std::fs::copy(pipeline.join("design.json"), board.join("design.json")).unwrap();
        let free = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let mut server = Command::new(env!("CARGO_BIN_EXE_eda"))
            .args(["board", "serve", "-C", board.to_str().unwrap(), "--port", &free.to_string(), "--ui", root.join("no-ui").to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start eda board serve");
        let stderr = server.stderr.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(port) = line.split("http://127.0.0.1:").nth(1).and_then(|r| r.split('/').next()).and_then(|p| p.parse::<u16>().ok()) {
                    let _ = tx.send(port);
                }
            }
        });
        let port = rx.recv_timeout(Duration::from_secs(120)).expect("the server says where it listens");
        Studio { root, board, port, server }
    }

    fn post(&self, path: &str, body: Value) -> Value {
        let body = body.to_string();
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        write!(s, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let raw = String::from_utf8_lossy(&raw).into_owned();
        let (_, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        serde_json::from_str(body).unwrap_or_else(|e| panic!("POST {path}: {e}: {body}"))
    }

    fn cmd(&self, cmd: Value) -> Value {
        self.post("/api/cmd", json!({ "cmd": cmd, "strict": false }))
    }

    /// The files a reply names, each checked to be on disk and not empty.
    fn written(&self, what: &str, reply: &Value) -> Vec<String> {
        assert_eq!(reply["ok"], true, "{what}: {reply}");
        let files: Vec<String> = reply["files"].as_array().unwrap_or_else(|| panic!("{what}: no files: {reply}")).iter().filter_map(Value::as_str).map(str::to_string).collect();
        assert!(!files.is_empty(), "{what}: wrote nothing: {reply}");
        for f in &files {
            let len = std::fs::metadata(self.board.join(f)).unwrap_or_else(|e| panic!("{what}: {f}: {e}")).len();
            assert!(len > 0, "{what}: {f} is empty");
        }
        files
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The rows of a CSV file as kicad-cli writes them: each line's fields without their quotes.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| l.split(',').map(|f| f.trim().trim_matches('"').to_string()).collect()).collect()
}

/// The placement file of `variant` (`None`: the default) with Do-not-populate footprints left out: reference -> value.
fn placement(studio: &Studio, variant: Option<&str>) -> Vec<(String, String)> {
    let mut body = json!({ "format": "csv", "side": "both", "units_mm": true, "exclude_dnp": true, "single_file": true, "output_directory": format!("pos-{}/", variant.unwrap_or("default")) });
    if let Some(v) = variant {
        body["variant"] = json!(v);
    }
    let reply = studio.post("/api/fab/pos", body);
    let files = studio.written("placement", &reply);
    let text = std::fs::read_to_string(studio.board.join(&files[0])).unwrap();
    csv_rows(&text).into_iter().skip(1).map(|r| (r[0].clone(), r[1].clone())).collect()
}

/// The BOM of `variant`, one row per symbol (Reference, Value), with Do-not-populate symbols left out.
fn bom(studio: &Studio, variant: Option<&str>) -> Vec<(String, String)> {
    std::thread::sleep(Duration::from_millis(1200));
    let spec = json!({ "columns": [{ "name": "Reference", "show": true }, { "name": "Value", "show": true }], "group_symbols": false, "exclude_dnp": true });
    let mut body = json!({ "spec": spec, "path": format!("export/bom-{}.csv", variant.unwrap_or("default")) });
    if let Some(v) = variant {
        body["variant"] = json!(v);
    }
    let reply = studio.post("/api/sch/bom", body);
    let files = studio.written("BOM", &reply);
    let text = std::fs::read_to_string(studio.board.join(&files[0])).unwrap();
    csv_rows(&text).into_iter().skip(1).map(|r| (r[0].clone(), r[1].clone())).collect()
}

fn value_of<'a>(rows: &'a [(String, String)], reference: &str) -> Option<&'a str> {
    rows.iter().find(|(r, _)| r == reference).map(|(_, v)| v.as_str())
}

#[test]
fn a_variant_made_in_the_studio_is_what_kicad_clis_exports_of_it_show() {
    if !slow() {
        return;
    }
    let studio = Studio::start("variants");
    let ok = |what: &str, v: Value| assert_eq!(v["ok"], true, "{what}: {v}");
    ok("add Lite", studio.cmd(json!({ "op": "add_design_variant", "name": "Lite", "description": "no input resistor" })));
    ok("add Full", studio.cmd(json!({ "op": "add_design_variant", "name": "Full", "description": "" })));
    let refused = studio.cmd(json!({ "op": "add_design_variant", "name": "lite", "description": "" }));
    assert!(refused.to_string().contains("Variant 'Lite' already exists."), "{refused}");
    ok("R1 not populated in Lite", studio.cmd(json!({ "op": "set_symbol_variant", "variant": "Lite", "ids": ["R1"], "dnp": true })));
    ok("R2's value in Lite", studio.cmd(json!({ "op": "set_symbol_variant", "variant": "Lite", "ids": ["R2"], "fields": { "Value": "4k7" } })));

    let design: Value = serde_json::from_str(&std::fs::read_to_string(studio.board.join("design.json")).unwrap()).unwrap();
    let r2_own = design["schematic"]["symbols"].as_array().unwrap().iter().find(|s| s["id"] == "R2").and_then(|s| s["value"].as_str()).map(str::to_string);
    let base = placement(&studio, None);
    let r2_value = r2_own.unwrap_or_else(|| value_of(&base, "R2").unwrap().to_string());
    assert!(value_of(&base, "R1").is_some(), "R1 is placed in the default variant: {base:?}");
    assert_eq!(value_of(&base, "R2"), Some(r2_value.as_str()));
    let lite = placement(&studio, Some("Lite"));
    assert_eq!(value_of(&lite, "R1"), None, "Lite leaves R1 out of the placement file: {lite:?}");
    assert_eq!(value_of(&lite, "R2"), Some("4k7"), "and R2 has Lite's value: {lite:?}");
    let full = placement(&studio, Some("Full"));
    assert!(value_of(&full, "R1").is_some() && value_of(&full, "R2") == Some(r2_value.as_str()), "Full is the design's own: {full:?}");

    let base = bom(&studio, None);
    assert!(value_of(&base, "R1").is_some() && value_of(&base, "R2") == Some(r2_value.as_str()), "the default BOM: {base:?}");
    let lite = bom(&studio, Some("Lite"));
    assert_eq!(value_of(&lite, "R1"), None, "Lite's BOM leaves R1 out: {lite:?}");
    assert_eq!(value_of(&lite, "R2"), Some("4k7"), "with R2's value of Lite: {lite:?}");

    // the netlist and a schematic plot of a variant are written
    std::thread::sleep(Duration::from_millis(1200));
    let net = studio.post("/api/sch/netlist", json!({ "format": "xml", "variant": "Lite" }));
    let files = studio.written("netlist", &net);
    let xml = std::fs::read_to_string(studio.board.join(&files[0])).unwrap();
    assert!(xml.contains("4k7"), "the variant's value is in its netlist");
    studio.written("schematic PDF", &studio.post("/api/sch/plot", json!({ "format": "pdf", "variant": "Lite", "output_directory": "plot-lite/" })));

    // the derived project lists both; a variant removed is gone from it, and its records with it
    let pro = |studio: &Studio| -> Value {
        let dir = studio.board.join(".kicad");
        let f = std::fs::read_dir(&dir).unwrap().flatten().find(|e| e.path().extension().is_some_and(|x| x == "kicad_pro")).unwrap();
        serde_json::from_str(&std::fs::read_to_string(f.path()).unwrap()).unwrap()
    };
    assert_eq!(pro(&studio)["schematic"]["variants"], json!([{ "name": "Full" }, { "name": "Lite", "description": "no input resistor" }]));
    ok("remove Lite", studio.cmd(json!({ "op": "remove_design_variant", "name": "Lite" })));
    let after = placement(&studio, Some("Lite"));
    assert!(value_of(&after, "R1").is_some() && value_of(&after, "R2") == Some(r2_value.as_str()), "kicad-cli exports the default for a variant it does not know: {after:?}");
    assert_eq!(pro(&studio)["schematic"]["variants"], json!([{ "name": "Full" }]));
}
