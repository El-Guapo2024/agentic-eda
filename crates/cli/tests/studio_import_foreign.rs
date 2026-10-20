//! File > Import > Non-KiCad Board / Schematic, through the real studio server (`POST /api/import_foreign`, then `/api/cmd`).
//!
//! The server is started on a free port with `EDA_KICAD_CLI` pointing at a stand-in kicad-cli (a shell script that makes what `kicad-cli pcb import` and `sch import` make: a
//! KiCad file, a report, the importers' lines) and these check what the page would: the file posted as base64 comes back converted -- text, format, findings, source layers,
//! counts -- the import is a command of one undo step that an Undo takes back and a Redo brings again, a file no importer recognises is said so, and the layer map reaches
//! kicad-cli. Nothing here needs the real kicad-cli; `crates/kicad-engine/tests/import_foreign_qa.rs` and `crates/cli/src/import_kicad.rs` run that on KiCad's QA files.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// A board as kicad-cli 10.99 writes one: the nets named where they are used. One part, J7 (the served board has R1, R2 and C1).
const FOREIGN_BOARD: &str = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
    (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
    (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
    (footprint "eagle-import-fps:0603" (layer "F.Cu") (uuid "11111111-1111-1111-1111-111111111111") (at 20 20)
      (property "Reference" "J7" (at 0 -1 0) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111112") (effects (font (size 1 1) (thickness 0.15))))
      (property "Value" "10k" (at 0 1 0) (layer "F.Fab") (uuid "11111111-1111-1111-1111-111111111113") (effects (font (size 1 1) (thickness 0.15))))
      (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net "VCC") (uuid "11111111-1111-1111-1111-111111111114"))
      (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net "GND") (uuid "11111111-1111-1111-1111-111111111115")))
    (segment (start 19.2 20) (end 12 20) (width 0.25) (layer "F.Cu") (net "VCC") (uuid "22222222-2222-2222-2222-222222222221"))
    (gr_line (start 0 0) (end 40 0) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555551"))
    (gr_line (start 40 0) (end 40 40) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555552"))
    (gr_line (start 40 40) (end 0 40) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555553"))
    (gr_line (start 0 40) (end 0 0) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555554"))
)"#;

/// A project as kicad-cli 10.99's `import` writes one beside the board: the design rules and the net classes the importer set.
const FOREIGN_PROJECT: &str = r#"{ "board": { "design_settings": { "rules": { "min_clearance": 0.2032, "min_track_width": 0.2 } } },
  "net_settings": { "classes": [ { "name": "Default", "clearance": 0.2, "track_width": 0.2 }, { "name": "highpower", "clearance": 0.2, "track_width": 0.3048 } ],
                    "netclass_patterns": [ { "netclass": "highpower", "pattern": "VCC" } ] } }"#;

/// What `kicad-cli pcb import`, `sch import` and the project-level `import` do, as far as the studio can tell: the output is written where `-o` says (the board from `$FAKE_BOARD`,
/// the schematic and its sheets from the folder `$FAKE_SCH_DIR`, its root being `whole.kicad_sch`, the project from `$FAKE_PROJECT`), the report where `--report-file` says, the
/// importers' lines go to stderr, and a layer map is kept.
const IMPORTING_CLI: &str = r#"#!/bin/sh
if [ "$1" = version ]; then echo 10.99.0-fake; exit 0; fi
scope=$1; out=""; report=""; map=""; prev=""; in=""
for a in "$@"; do
  case "$prev" in -o) out="$a";; --report-file) report="$a";; --layer-map) map="$a";; esac
  prev="$a"; in="$a"
done
echo "$*" >> "$FAKE_LOG"
[ -n "$map" ] && cp "$map" "$FAKE_LOG.map"
if [ "$scope" = import ]; then cp "$FAKE_PROJECT" "$out.kicad_pro"; exit 0; fi
case "$in" in *.xyz) echo "No PCB importer recognizes the file format of '$in'"; exit 3;; esac
echo "Importing '$in' using Eagle format..."
if [ "$scope" = pcb ]; then
  cp "$FAKE_BOARD" "$out"
  mapped="auto"; [ -n "$map" ] && mapped="explicit"
  cat > "$report" <<JSON
{ "errors": [], "output_file": "$(basename "$out")", "source_file": "$(basename "$in")", "source_format": "Eagle",
  "statistics": { "footprints": 1, "tracks": 1, "vias": 0, "zones": 0 }, "warnings": ["KiCad design rules are different from Eagle ones."],
  "layer_mapping": { "Top": { "kicad_layer": "F.Cu", "method": "auto" }, "tNames": { "kicad_layer": "F.SilkS", "method": "$mapped" }, "200bmp": { "kicad_layer": "UNDEFINED", "method": "auto" } } }
JSON
else
  cp -R "$FAKE_SCH_DIR"/. "$(dirname "$out")"/
  mv "$(dirname "$out")/whole.kicad_sch" "$out"
  cat > "$report" <<JSON
{ "errors": [], "output_file": "$(basename "$out")", "source_file": "$(basename "$in")", "source_format": "Altium", "statistics": { "sheets": 4, "symbols": 98, "renamed_board_nets": 0 }, "warnings": [] }
JSON
fi
echo "12:33:13 AM: Error: No library symbol found for schematic symbol 'R1 10k'." >&2
echo "12:33:13 AM: Error: No library symbol found for schematic symbol 'R1 10k'." >&2
echo "12:33:14 AM: Setting schematic symbol 'D1 GREEN' library identifier to 'a:b'." >&2
echo "Successfully saved imported board to '$out'"
"#;

struct Studio {
    root: PathBuf,
    port: u16,
    server: Child,
}

impl Studio {
    fn start(name: &str) -> Studio {
        let root = std::env::temp_dir().join(format!("eda_cli_import_foreign_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // The files the stand-in makes: the board, and the mcu30 example drawn as module sheets (a root that places three, a screen each).
        std::fs::write(root.join("foreign.kicad_pcb"), FOREIGN_BOARD).unwrap();
        std::fs::write(root.join("foreign.kicad_pro"), FOREIGN_PROJECT).unwrap();
        let sheets = root.join("sheets");
        std::fs::create_dir_all(&sheets).unwrap();
        let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml")).unwrap()).unwrap();
        let design = eda_engine::derive_schematic_modules(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
        for (file, text) in eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-10", title: "t" }, "whole.kicad_sch").unwrap() {
            std::fs::write(sheets.join(file), text).unwrap();
        }
        let script = root.join("importing.sh");
        std::fs::write(&script, IMPORTING_CLI).unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The first run of a new executable can take a second on macOS (the system scans it first).
        let _ = Command::new(&script).arg("version").output();

        let eda = |args: &[&str]| {
            let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(args).output().expect("run eda");
            assert!(out.status.success(), "eda {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        };
        let board = root.join("board");
        let intent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/two_pin_nets.yaml");
        eda(&["board", "new", intent.to_str().unwrap(), "-o", board.to_str().unwrap()]);
        for (part, at) in [("R1", "3,3"), ("R2", "6,3"), ("C1", "4.5,6")] {
            eda(&["board", "place", part, "--at", at, "-C", board.to_str().unwrap()]);
        }
        let free = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let mut server = Command::new(env!("CARGO_BIN_EXE_eda"))
            .args(["board", "serve", "-C", board.to_str().unwrap(), "--port", &free.to_string(), "--ui", root.join("no-ui").to_str().unwrap()])
            .env("EDA_KICAD_CLI", &script)
            .env("EDA_KICAD_CONFIG_HOME", root.join("settings"))
            .env("FAKE_LOG", root.join("kicad-cli.log"))
            .env("FAKE_BOARD", root.join("foreign.kicad_pcb"))
            .env("FAKE_PROJECT", root.join("foreign.kicad_pro"))
            .env("FAKE_SCH_DIR", &sheets)
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
        let port = rx.recv_timeout(Duration::from_secs(60)).expect("the server says where it listens");
        Studio { root, port, server }
    }

    fn post(&self, path: &str, body: Value) -> Value {
        let (status, text) = request(self.port, "POST", path, &body.to_string());
        assert_eq!(status, 200, "POST {path}: {text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("POST {path}: {e}: {text}"))
    }

    fn get(&self, path: &str) -> Value {
        let (status, text) = request(self.port, "GET", path, "");
        assert_eq!(status, 200, "GET {path}: {text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("GET {path}: {e}: {text}"))
    }

    /// `POST /api/cmd`, the way the studio sends an edit.
    fn cmd(&self, cmd: Value, strict: bool) -> Value {
        self.post("/api/cmd", json!({ "cmd": cmd, "strict": strict }))
    }

    fn undo(&self, domain: &str) -> Value {
        self.post("/api/undo", json!({ "domain": domain }))
    }

    fn redo(&self, domain: &str) -> Value {
        self.post("/api/redo", json!({ "domain": domain }))
    }

    /// The references of the parts on the board, sorted.
    fn parts(&self) -> Vec<String> {
        let state = self.get("/api/state");
        let mut refs: Vec<String> = state["parts"].as_array().unwrap().iter().map(|p| p["ref"].as_str().unwrap().to_string()).collect();
        refs.sort();
        refs
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("kicad-cli.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn request(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    (head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), body.to_string())
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        s.push(A[(n >> 18) as usize & 63] as char);
        s.push(A[(n >> 12) as usize & 63] as char);
        s.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    s
}

fn upload(kind: &str, name: &str) -> Value {
    // Binary on purpose: an Altium file is not text.
    let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    json!({ "kind": kind, "name": name, "data": base64(&bytes) })
}

#[test]
fn a_board_file_comes_back_converted_and_the_import_is_one_undo_step_that_is_taken_back_and_brought_again() {
    let studio = Studio::start("board");
    assert_eq!(studio.parts(), vec!["C1", "R1", "R2"]);

    // The page posts the file; the server has kicad-cli convert it and answers with what the import needs.
    let r = studio.post("/api/import_foreign", upload("board", "My Board.brd"));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((r["kind"].as_str(), r["format"].as_str(), r["name"].as_str()), (Some("board"), Some("Eagle"), Some("My Board.brd")));
    assert_eq!(r["text"].as_str().unwrap(), FOREIGN_BOARD);
    assert_eq!(r["counts"], json!({ "footprints": 1, "nets": 2, "tracks": 1, "vias": 0, "zones": 0 }), "what our reader found in the file");
    assert_eq!(r["statistics"]["footprints"], 1, "and what kicad-cli says it converted");
    assert_eq!(r["maps_layers"], true, "Eagle's importer asks which KiCad layer each layer goes to");
    let layers = r["layers"].as_array().unwrap();
    assert!(layers.contains(&json!({ "source": "Top", "kicad": "F.Cu", "explicit": false })) && layers.contains(&json!({ "source": "200bmp", "kicad": null, "explicit": false })), "{layers:?}");
    // The findings: the importers' error said twice is one finding with its count, and the report's warning; the information line is not one.
    let findings: Vec<(String, u64)> = r["findings"].as_array().unwrap().iter().map(|f| (format!("{}: {}", f["severity"].as_str().unwrap(), f["message"].as_str().unwrap()), f["count"].as_u64().unwrap())).collect();
    assert_eq!(findings, vec![("error: No library symbol found for schematic symbol 'R1 10k'.".to_string(), 2), ("warning: KiCad design rules are different from Eagle ones.".to_string(), 1)]);
    // Importing is not a change: nothing happened to the board.
    assert_eq!(studio.parts(), vec!["C1", "R1", "R2"]);
    // The file kicad-cli was given is the file the page posted, under its name.
    assert!(studio.calls()[0].starts_with("pcb import --report-format json --report-file "), "{:?}", studio.calls());
    assert!(studio.calls()[0].contains("My Board.brd"), "{:?}", studio.calls());
    // And a second run makes the project of the board, which has the design rules and the net classes the importer set and the board file has not.
    assert_eq!(studio.calls().len(), 2, "{:?}", studio.calls());
    assert!(studio.calls()[1].starts_with("import -o ") && studio.calls()[1].contains("My Board.brd"), "{:?}", studio.calls());
    assert_eq!(r["project"].as_str().unwrap(), FOREIGN_PROJECT);
    assert_eq!(r["has_project"], true);
    assert!(studio.get("/api/state")["board_rules"]["net_classes"].as_array().unwrap().is_empty(), "nothing yet: importing is not a change");

    // The import: the command the page sends, with Strict on (the gates would refuse a move that makes a board worse; an import is not one).
    let done = studio.cmd(json!({ "op": "replace_board", "text": r["text"], "name": "My Board.brd", "project": r["project"] }), true);
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(studio.parts(), vec!["J7"], "the file's board is the board; the intent's R1, R2 and C1 are the old board's");
    let rules = studio.get("/api/state")["board_rules"].clone();
    assert_eq!(rules["net_classes"][0]["name"], "highpower", "the importer's net class, from the project: {rules}");
    assert_eq!(rules["net_classes"][0]["track_width"], 305);
    let state = studio.get("/api/state");
    assert_eq!(state["outline"].as_array().map(|o| o.len()), Some(4), "the file's outline: {}", state["outline"]);
    assert_eq!(state["outline"][2], json!([40_000, 40_000]), "and it is the file's, not the old board's");

    // One Undo (from the PCB tab) brings the old board back with its parts and nets; a Redo, the imported one.
    assert_eq!(studio.undo("pcb")["ok"], true);
    assert_eq!(studio.parts(), vec!["C1", "R1", "R2"], "the old board whole, the model with it");
    assert!(studio.get("/api/state")["board_rules"]["net_classes"].as_array().unwrap().is_empty(), "and the rules with it");
    // (The served board has the steps it was made with, placing R1, R2 and C1: this Undo is the import's, the next would be theirs.)
    assert_eq!(studio.redo("pcb")["ok"], true);
    assert_eq!(studio.parts(), vec!["J7"]);
    assert_eq!(studio.get("/api/state")["board_rules"]["net_classes"][0]["name"], "highpower");

    // A layer map is passed to kicad-cli, and the layer the person moved is explicit in the answer.
    let again = studio.post("/api/import_foreign", json!({ "kind": "board", "name": "My Board.brd", "data": upload("board", "x")["data"], "layer_map": { "tNames": "F.Fab" } }));
    assert_eq!(again["ok"], true, "{again}");
    assert!(again["layers"].as_array().unwrap().contains(&json!({ "source": "tNames", "kicad": "F.SilkS", "explicit": true })), "{again}");
    let map: Value = serde_json::from_str(&std::fs::read_to_string(studio.root.join("kicad-cli.log.map")).unwrap()).unwrap();
    assert_eq!(map, json!({ "tNames": "F.Fab" }));
}

#[test]
fn a_schematic_file_comes_back_as_a_sheet_file_and_the_import_replaces_the_drawing_in_one_undo_step() {
    let studio = Studio::start("schematic");
    let r = studio.post("/api/import_foreign", upload("schematic", "mcu.SchDoc"));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!((r["kind"].as_str(), r["format"].as_str()), (Some("schematic"), Some("Altium")));
    assert!(studio.calls()[0].starts_with("sch import --report-format json "), "{:?}", studio.calls());
    assert_eq!(r["maps_layers"], false, "layers are a board's");
    assert_eq!(r["counts"]["sheets"], 4, "the root and its three sheets");
    assert!(r["counts"]["symbols"].as_u64().unwrap() > 30, "{}", r["counts"]);
    assert!(r["content"]["root"]["sheets"].as_array().unwrap().len() == 3 && r["content"]["screens"].as_object().unwrap().len() == 3);
    assert!(r["text"].is_null(), "a schematic's file travels as its sheets, not as text");

    // The page sends what the server answered, and the schematic is the file's.
    let done = studio.cmd(json!({ "op": "replace_schematic", "content": r["content"], "name": "mcu.SchDoc" }), true);
    assert_eq!(done["ok"], true, "{done}");
    let sch = studio.get("/api/schematic");
    assert_eq!(sch["sheets"].as_array().map(|s| s.len()), Some(3), "three sheet symbols on the root: {}", sch.to_string().chars().take(300).collect::<String>());
    // The nets of the drawing are the design's now (traced against nothing, as the new drawing says): a pin of a symbol of the first sheet is on a net.
    let mut on_a_net = 0;
    for sheet in sch["sheets"].as_array().unwrap() {
        let id = sheet["id"].as_str().unwrap();
        let inside = studio.get(&format!("/api/schematic?sheet={id}"));
        on_a_net += inside["symbols"].as_array().unwrap().iter().flat_map(|s| s["pins"].as_array().unwrap().iter()).filter(|p| p["net"].as_str().is_some_and(|n| !n.is_empty())).count();
    }
    assert!(on_a_net > 40, "pins on a net, over the three sheets: {on_a_net}");
    // The board is where it was, and the symbols of the file are parts it has not placed.
    assert!(studio.parts().contains(&"U1".to_string()) && studio.parts().contains(&"R1".to_string()) && studio.parts().contains(&"C1".to_string()), "{:?}", studio.parts());

    // One Undo from the schematic's tab takes the old drawing back: the board started with none stored.
    assert_eq!(studio.undo("schematic")["ok"], true);
    let back = studio.get("/api/schematic");
    assert!(back["sheets"].as_array().map_or(0, |s| s.len()) != 3 || back["symbols"].as_array().map_or(0, |s| s.len()) != sch["symbols"].as_array().map_or(0, |s| s.len()), "the drawing is the board's own again");
    assert_eq!(studio.redo("schematic")["ok"], true);
    assert_eq!(studio.get("/api/schematic")["sheets"].as_array().map(|s| s.len()), Some(3));
}

#[test]
fn a_file_no_importer_recognises_is_said_so_and_a_name_with_folders_in_it_is_a_file_name() {
    let studio = Studio::start("unknown");
    let r = studio.post("/api/import_foreign", upload("board", "photo.xyz"));
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(r["unrecognized"], true);
    assert!(r["message"].as_str().unwrap().contains("No PCB importer recognizes the file format of 'photo.xyz'"), "kicad-cli's words, the file by its name and not by a folder of the server's: {r}");
    // `../../x.brd` is `x.brd` in the run's folder: the stand-in's argument shows where it was put.
    let r = studio.post("/api/import_foreign", upload("board", "../../escape.brd"));
    assert_eq!(r["ok"], true, "{r}");
    let call = studio.calls().last().cloned().unwrap();
    assert!(call.contains("/in/escape.brd") && !call.contains(".."), "{call}");
    // Not a request.
    let (status, text) = request(studio.port, "POST", "/api/import_foreign", "not json");
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["ok"], false);
    let _ = Path::new(".");
}
