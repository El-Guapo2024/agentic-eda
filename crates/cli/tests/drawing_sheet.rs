//! Page Settings' drawing sheet file reaches kicad-cli's plots (UI-LEFT C3): the studio's own routes, served by `eda board serve`, on a scratch copy of the
//! opamp filter (`eda board new examples/opamp_filter.yaml`; the examples are never written to).
//!
//! KiCad's QA sheet (`qa/data/cli/basic_test/custom_ds.kicad_wks`, copied to `web/studio/src/kicad/drawing_sheets/`) has four "CUSTOM DRAWING SHEET" texts KiCad's
//! default sheet has not. It is put in the board's folder (`POST /api/drawing_sheet/upload`) and chosen for the board and for the schematic
//! (`set_drawing_sheet`); then the board's SVG plot (`POST /api/fab/plot`, the drawing sheet on: `plotframeref yes`) and the schematic's
//! (`POST /api/sch/plot`) carry that text -- kicad-cli writes each plotted text into the SVG as a searchable `<text>` -- and they did not before.
//! The derived `.kicad_pro` names the sheet (`pcbnew.page_layout_descr_file`, `schematic.page_layout_descr_file`), whole, and the state reports it.
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

struct Studio {
    root: PathBuf,
    board: PathBuf,
    port: u16,
    server: Child,
}

impl Studio {
    fn start() -> Studio {
        let root = std::env::temp_dir().join(format!("eda_drawing_sheet_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml");
        let board = root.join("board");
        eda(&["board", "new", example.to_str().unwrap(), "-o", board.to_str().unwrap()]);
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

    fn request(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let raw = String::from_utf8_lossy(&raw).into_owned();
        let (_, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{method} {path}: {e}: {body}"))
    }

    fn post(&self, path: &str, body: Value) -> Value {
        self.request("POST", path, Some(body))
    }

    fn get(&self, path: &str) -> Value {
        self.request("GET", path, None)
    }

    fn cmd(&self, cmd: Value) -> Value {
        self.post("/api/cmd", json!({ "cmd": cmd, "strict": false }))
    }

    /// The text of every SVG a plot reply names.
    fn svg_text(&self, what: &str, reply: &Value) -> String {
        assert_eq!(reply["ok"], true, "{what}: {reply}");
        let files: Vec<&str> = reply["files"].as_array().unwrap_or_else(|| panic!("{what}: no files: {reply}")).iter().filter_map(Value::as_str).filter(|f| f.ends_with(".svg")).collect();
        assert!(!files.is_empty(), "{what}: no SVG: {reply}");
        files.iter().map(|f| std::fs::read_to_string(self.board.join(f)).unwrap_or_else(|e| panic!("{what}: {f}: {e}"))).collect()
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const MARK: &str = "CUSTOM DRAWING SHEET";

#[test]
fn a_custom_drawing_sheet_reaches_kicad_clis_board_and_schematic_plots() {
    if !slow() {
        return;
    }
    let studio = Studio::start();
    let sheet = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/src/kicad/drawing_sheets/custom_ds.kicad_wks")).unwrap();
    assert!(sheet.contains(MARK));
    assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "plotframeref": "yes" } }))["ok"], true);
    let board_plot = |dir: &str| {
        assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "outputdirectory": dir } }))["ok"], true);
        studio.svg_text("board SVG", &studio.post("/api/fab/plot", json!({ "format": "svg", "layers": ["Edge.Cuts"], "check_zones": false })))
    };
    let sch_plot = |dir: &str| studio.svg_text("schematic SVG", &studio.post("/api/sch/plot", json!({ "format": "svg", "output_directory": dir })));

    // KiCad's default sheet first: none of the custom texts
    let before = board_plot("plots/before/");
    assert!(before.contains("Size: A4") || before.contains("Size:"), "the default sheet is plotted with the frame on");
    assert!(!before.contains(MARK));
    assert!(!sch_plot("plots/sch-before/").contains(MARK));

    // the QA sheet into the board's folder, then chosen for both documents
    let up = studio.post("/api/drawing_sheet/upload", json!({ "name": "custom_ds.kicad_wks", "text": sheet }));
    assert_eq!((up["ok"].as_bool(), up["path"].as_str()), (Some(true), Some("custom_ds.kicad_wks")), "{up}");
    assert_eq!(studio.cmd(json!({ "op": "batch", "cmds": [{ "op": "set_drawing_sheet", "schematic": false, "path": "custom_ds.kicad_wks" }] }))["ok"], true);
    assert_eq!(studio.cmd(json!({ "op": "set_drawing_sheet", "schematic": true, "path": "custom_ds.kicad_wks" }))["ok"], true);
    let state = studio.get("/api/state");
    assert_eq!(state["drawing_sheet"]["path"], "custom_ds.kicad_wks");
    assert!(state["drawing_sheet"]["text"].as_str().unwrap_or("").contains(MARK), "the state carries the sheet's text for the studio to draw");
    let sch = studio.get("/api/schematic");
    assert_eq!(sch["drawing_sheet"]["path"], "custom_ds.kicad_wks");
    assert_eq!(sch["sheet_page"]["first"], true);

    let after = board_plot("plots/after/");
    assert!(after.contains(MARK), "kicad-cli's board plot draws the chosen sheet");
    assert!(sch_plot("plots/sch-after/").contains(MARK), "kicad-cli's schematic plot draws the chosen sheet");

    // the derived project names the sheet, whole (its folder is `.kicad`, not the board's)
    let pro: Value = std::fs::read_dir(studio.board.join(".kicad"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.path().extension().is_some_and(|x| x == "kicad_pro"))
        .map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap())
        .expect("a derived project");
    let whole = studio.board.join("custom_ds.kicad_wks").display().to_string();
    assert_eq!(pro["pcbnew"]["page_layout_descr_file"].as_str(), Some(whole.as_str()));
    assert_eq!(pro["schematic"]["page_layout_descr_file"].as_str(), Some(whole.as_str()));

    // undo puts the default sheet back, each in its own editor's history
    assert_eq!(studio.post("/api/undo", json!({ "domain": "schematic" }))["ok"], true);
    assert_eq!(studio.get("/api/schematic")["drawing_sheet"], Value::Null);
    assert_eq!(studio.get("/api/state")["drawing_sheet"]["path"], "custom_ds.kicad_wks", "the board's is in the board's history");
    assert_eq!(studio.post("/api/undo", json!({ "domain": "pcb" }))["ok"], true);
    assert_eq!(studio.get("/api/state")["drawing_sheet"], Value::Null);
}
