//! The fab-output dialogs' formats through the real kicad-cli (UI-LEFT X1-X4): the studio's own routes, served by `eda board serve`, on the
//! placed and routed opamp filter (`eda pipeline examples/opamp_filter.yaml`, in a folder made by `eda board new`; the examples are never
//! written to).
//!
//! - Plot (`POST /api/fab/plot`, `DIALOG_PLOT::Plot`): Gerber, PostScript, SVG, DXF, PDF and PNG, one file per layer, and the one-document
//!   PDF and DXF; into the board's output folder (`(outputdirectory ..)`, stored by `set_plot_settings` as the dialog's
//!   `applyPlotSettings` stores it).
//! - Generate Drill Files (`POST /api/fab/drill`, `/api/fab/drill_report`): Excellon in inches with suppressed leading zeros and a PDF map,
//!   Gerber X2 with its map, and the report file alone.
//! - Plot Schematic (`POST /api/sch/plot`): PostScript, PDF, SVG, DXF and PNG.
//! - Export Netlist (`POST /api/sch/netlist`): every format of `kicad-cli sch export netlist --format`.
//!
//! Each file must exist and not be empty. Slow tier only (`EDA_SLOW_TESTS=1`, kicad-cli on the PATH or in `EDA_KICAD_CLI`).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
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
        let root = std::env::temp_dir().join(format!("eda_fab_plot_formats_{}_{tag}", std::process::id()));
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

fn ext(f: &str) -> String {
    Path::new(f).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

#[test]
fn every_pcb_plot_format_drill_format_schematic_plot_and_netlist_writes_its_files() {
    if !slow() {
        return;
    }
    let studio = Studio::start("all");
    let layers = json!(["F.Cu", "B.Cu", "F.SilkS", "Edge.Cuts"]);

    // ---- Plot: every format, one file per layer, into the output folder the dialog stored.
    let stored = studio.cmd(json!({ "op": "set_plot_settings", "options": { "outputdirectory": "plots/", "plotframeref": "yes", "drillshape": "2" } }));
    assert_eq!(stored["ok"], true, "{stored}");
    for (format, extension, per_layer) in [("gerber", "gbr", true), ("ps", "ps", true), ("svg", "svg", true), ("dxf", "dxf", true), ("pdf", "pdf", true), ("png", "png", true)] {
        let reply = studio.post("/api/fab/plot", json!({ "format": format, "layers": layers, "check_zones": false }));
        let files = studio.written(format, &reply);
        let plotted: Vec<&String> = files.iter().filter(|f| ext(f) == extension).collect();
        assert!(files.iter().all(|f| f.starts_with("plots/")), "{format}: into the output folder: {files:?}");
        if per_layer {
            assert_eq!(plotted.len(), 4, "{format}: one file per layer: {files:?}");
        }
        let message = reply["message"].as_str().unwrap_or_default();
        assert!(message.starts_with("Plotted to '") && message.ends_with("Done."), "{format}: the plotter's report: {message}");
        if format == "gerber" {
            assert!(files.iter().any(|f| f.ends_with(".gbrjob")), "the job file: {files:?}");
        }
    }
    // "Generate Gerber job file" unticked: kicad-cli writes the job file anyway, the route deletes it.
    assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "creategerberjobfile": "no", "outputdirectory": "plots/nojob/" } }))["ok"], true);
    let nojob = studio.written("Gerber, no job file", &studio.post("/api/fab/plot", json!({ "format": "gerber", "layers": layers })));
    assert!(nojob.iter().all(|f| !f.ends_with(".gbrjob")), "no job file reported: {nojob:?}");
    let on_disk: Vec<String> = std::fs::read_dir(studio.board.join("plots/nojob")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(on_disk.len() == 4 && on_disk.iter().all(|f| !f.ends_with(".gbrjob")), "and none on disk: {on_disk:?}");
    assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "creategerberjobfile": "yes" } }))["ok"], true);
    // One document: PDF's "Single document" (stored) and DXF's (the session's).
    assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "pdf_single_document": "yes", "outputdirectory": "plots/one/" } }))["ok"], true);
    let pdf = studio.written("one PDF", &studio.post("/api/fab/plot", json!({ "format": "pdf", "layers": layers })));
    assert_eq!(pdf.len(), 1, "one PDF of every layer: {pdf:?}");
    // The export lists the files of its folder written since a second before it started (`run_export`: some file systems' times are that
    // coarse), so the next one waits that second out.
    std::thread::sleep(Duration::from_millis(1200));
    let dxf = studio.written("one DXF", &studio.post("/api/fab/plot", json!({ "format": "dxf", "layers": layers, "dxf_single_document": true })));
    assert_eq!(dxf.len(), 1, "one DXF of every layer: {dxf:?}");
    // A folder outside the board's is refused.
    assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "outputdirectory": "../outside/" } }))["ok"], true);
    let refused = studio.post("/api/fab/plot", json!({ "format": "svg", "layers": layers }));
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(refused["message"].as_str().unwrap_or_default().contains("Could not write plot files to folder"), "{refused}");
    assert_eq!(studio.cmd(json!({ "op": "set_plot_settings", "options": { "outputdirectory": "" } }))["ok"], true);

    // ---- Generate Drill Files: Excellon with every option and a PDF map, Gerber X2 with its map, and the report alone.
    let excellon = studio.post("/api/fab/drill", json!({ "format": "excellon", "units_mm": false, "zeros": "suppressleading", "mirror_y": true, "minimal_header": true, "merge_pth_npth": true, "generate_map": true, "map_format": "pdf" }));
    let files = studio.written("Excellon", &excellon);
    assert!(files.iter().any(|f| ext(f) == "drl") && files.iter().any(|f| ext(f) == "pdf"), "a drill file and its PDF map: {files:?}");
    let drl = std::fs::read_to_string(studio.board.join(files.iter().find(|f| ext(f) == "drl").unwrap())).unwrap();
    assert!(drl.contains("INCH") || drl.contains("M72"), "inches: {}", &drl[..drl.len().min(400)]);
    assert!(!drl.contains("; DRILL file"), "the minimal header has no comment block: {}", &drl[..drl.len().min(400)]);
    let gerber = studio.post("/api/fab/drill", json!({ "format": "gerber", "generate_map": true, "map_format": "gerberx2" }));
    let files = studio.written("Gerber X2 drill", &gerber);
    assert!(files.iter().any(|f| f.contains("-drl_map")) && files.iter().any(|f| f.ends_with("-drl.gbr")), "Gerber X2 drill files and maps: {files:?}");
    let text = std::fs::read_to_string(studio.board.join(files.iter().find(|f| f.ends_with("-drl.gbr")).unwrap())).unwrap();
    assert!(text.contains("%TF.FileFunction,Plated") || text.contains("%TF.FileFunction,NonPlated"), "an X2 drill file");
    let report = studio.post("/api/fab/drill_report", json!({ "format": "excellon" }));
    let files = studio.written("drill report", &report);
    assert!(files.len() == 1 && files[0].ends_with("-drl.rpt"), "{files:?}");
    assert_eq!(report["message"].as_str().unwrap_or_default(), format!("Report file '{}' created.", files[0]));
    assert!(std::fs::read_to_string(studio.board.join(&files[0])).unwrap().contains("Drill report"), "kicad-cli's report");

    // ---- Plot Schematic: every format.
    for (format, extension) in [("ps", "ps"), ("pdf", "pdf"), ("svg", "svg"), ("dxf", "dxf"), ("png", "png")] {
        let reply = studio.post("/api/sch/plot", json!({ "format": format, "theme": "_builtin_default", "plot_all": true }));
        let files = studio.written(&format!("schematic {format}"), &reply);
        assert!(files.iter().all(|f| ext(f) == extension), "schematic {format}: {files:?}");
    }

    // ---- Export Netlist: every format, named by it. Allegro's exporter also writes its device files into a `devices` folder beside the
    // netlist (`NETLIST_EXPORTER_ALLEGRO::WriteNetlist`). Each run waits out the second-long window of the file list (see above).
    for (format, extension) in [("kicad", "net"), ("orcadpcb2", "net"), ("allegro", "txt"), ("cadstar", "frp"), ("pads", "asc"), ("spice", "cir"), ("spicemodel", "cir"), ("xml", "xml")] {
        std::thread::sleep(Duration::from_millis(1200));
        let reply = studio.post("/api/sch/netlist", json!({ "format": format }));
        let files: Vec<String> = studio.written(&format!("netlist {format}"), &reply).into_iter().filter(|f| !f.ends_with("/devices")).collect();
        assert!(files.len() == 1 && ext(&files[0]) == extension, "netlist {format}: {files:?}");
    }
}
