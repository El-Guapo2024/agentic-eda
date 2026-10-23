//! File > Export > Specctra DSN and File > Import > Specctra Session through the CLI (`eda board export specctra`, `eda board import-ses`), round trip: a routed board's
//! DSN, the session a router that keeps that routing sends back (`eda_kicad::specctra::session::echo_session`, FreeRouting's shape of a session), imported onto the board
//! with its routing taken away, as one undo step. The slow tier (`EDA_SLOW_TESTS=1`, kicad-cli) holds the board that comes back to kicad-cli's DRC: no violation it did
//! not have before.
//!
//! The DSN is kicad-cli's where kicad-cli has `pcb export specctra`, else our own exporter's (`specctra_api::engine`). The fast tier forces the fallback
//! (`EDA_KICAD_CLI_NO_SPECCTRA`), so it runs without kicad-cli and holds our exporter; the slow tier exports through kicad-cli.
//!
//! The board is `examples/opamp_filter.yaml` placed and routed by `eda pipeline`, in a folder made by `eda board new` (the examples are never written to).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What makes the export take our own exporter, as on a machine whose kicad-cli has no `pcb export specctra` (`specctra_api::NO_CLI_ENV`).
const NO_CLI: &str = "EDA_KICAD_CLI_NO_SPECCTRA";

fn eda_with(args: &[&str], fallback: bool) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_eda"));
    cmd.args(args);
    if fallback {
        cmd.env(NO_CLI, "1");
    } else {
        cmd.env_remove(NO_CLI);
    }
    let out = cmd.output().expect("run eda");
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn eda(args: &[&str]) -> (bool, String) {
    eda_with(args, false)
}

fn ok_with(args: &[&str], fallback: bool) -> String {
    let (good, log) = eda_with(args, fallback);
    assert!(good, "eda {}:\n{log}", args.join(" "));
    log
}

fn ok(args: &[&str]) -> String {
    ok_with(args, false)
}

fn kicad_cli() -> Option<&'static str> {
    ["/usr/local/bin/kicad-cli", "/usr/bin/kicad-cli", "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"].into_iter().find(|c| Path::new(c).is_file())
}

/// Whether the kicad-cli of this machine has `pcb export specctra` (its `pcb export --help` lists it).
fn cli_has_specctra(cli: &str) -> bool {
    Command::new(cli).args(["pcb", "export", "--help"]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().next().is_some_and(|l| l.contains("specctra")))
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_cli_specctra_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A board folder whose design is the routed opamp filter.
fn routed_board(tag: &str) -> PathBuf {
    let root = scratch(tag);
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml");
    let p = root.join("pipeline");
    ok(&["pipeline", example.to_str().unwrap(), "-o", p.to_str().unwrap(), "--seed", "0"]);
    let b = root.join("board");
    ok(&["board", "new", example.to_str().unwrap(), "-o", b.to_str().unwrap()]);
    std::fs::copy(p.join("design.json"), b.join("design.json")).unwrap();
    b
}

fn design(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).unwrap()).unwrap()
}

fn counts(d: &Value) -> (usize, usize) {
    (d["routing"]["tracks"].as_array().map_or(0, Vec::len), d["routing"]["vias"].as_array().map_or(0, Vec::len))
}

/// Export (by our own exporter when `fallback`, else by the engine the probe picks), take the routing away, import the session: the board has the routing back
/// (segment for segment), the parts where they were, and one Undo takes the import back. Answers the board folder and the session file.
fn round_trip(tag: &str, fallback: bool) -> (PathBuf, PathBuf, Value) {
    let b = routed_board(tag);
    let routed = design(&b);
    let (tracks, vias) = counts(&routed);
    assert!(tracks > 0 && vias > 0, "the pipeline routed the board with vias: {tracks} tracks, {vias} vias");
    let segments: usize = routed["routing"]["tracks"].as_array().unwrap().iter().map(|t| t["pts"].as_array().unwrap().len() - 1).sum();

    let dsn = b.parent().unwrap().join("opamp.dsn");
    let log = ok_with(&["board", "export", "specctra", "-o", dsn.to_str().unwrap(), "-C", b.to_str().unwrap()], fallback);
    assert!(log.contains("BOARD exported OK"), "{log}");
    if fallback {
        assert!(log.contains("(engine: eda)"), "the probe forced off, our own exporter writes the file: {log}");
    } else if kicad_cli().is_some_and(cli_has_specctra) {
        assert!(log.contains("(engine: kicad-cli)"), "a kicad-cli with pcb export specctra writes the file: {log}");
    }
    let text = std::fs::read_to_string(&dsn).unwrap();
    let pcb = eda_kicad::specctra::parse::load_pcb(&text, "opamp.dsn").expect("the DSN reads back");
    assert_eq!(pcb.pcbname, dsn.display().to_string(), "the design is named by the file (`m_pcbname = aFilename`)");
    let ses = b.parent().unwrap().join("opamp.ses");
    std::fs::write(&ses, eda_kicad::specctra::session::echo_session(&pcb, "opamp.ses")).unwrap();

    // The routing goes (a direct edit of design.json: a step of its own), then the session brings it.
    let mut stripped = routed.clone();
    stripped["routing"]["tracks"] = Value::Array(vec![]);
    stripped["routing"]["vias"] = Value::Array(vec![]);
    std::fs::write(b.join("design.json"), serde_json::to_string_pretty(&stripped).unwrap()).unwrap();
    let log = ok(&["board", "import-ses", ses.to_str().unwrap(), "-C", b.to_str().unwrap()]);
    assert!(log.contains("Session file imported and merged OK."), "{log}");
    let after = design(&b);
    assert_eq!(counts(&after), (segments, vias), "a track of each segment of the session's paths, and its vias");
    assert_eq!(after["placement"]["footprints"], routed["placement"]["footprints"], "the parts are where the session places them: where they were");

    ok(&["board", "undo", "-C", b.to_str().unwrap()]);
    assert_eq!(counts(&design(&b)), (0, 0), "one Undo takes the whole import back");
    ok(&["board", "redo", "-C", b.to_str().unwrap()]);
    assert_eq!(counts(&design(&b)), (segments, vias));
    (b, ses, routed)
}

#[test]
fn a_routed_board_comes_back_from_the_session_of_its_own_dsn_in_one_undo_step() {
    // The fallback, our own exporter, with the probe forced off: what a machine whose kicad-cli has no `pcb export specctra` gets.
    let (b, ses, _) = round_trip("fast", true);
    // A session of another board names a part this one has not got: KiCad's words, and nothing changes.
    let other = std::fs::read_to_string(&ses).unwrap().replacen("(place ", "(place NOPE_", 1);
    let bad = b.parent().unwrap().join("bad.ses");
    std::fs::write(&bad, other).unwrap();
    let before = std::fs::read_to_string(b.join("design.json")).unwrap();
    let (good, log) = eda(&["board", "import-ses", bad.to_str().unwrap(), "-C", b.to_str().unwrap()]);
    assert!(!good && log.contains("Reference 'NOPE_"), "{log}");
    assert_eq!(std::fs::read_to_string(b.join("design.json")).unwrap(), before);
    let _ = std::fs::remove_dir_all(b.parent().unwrap());
}

/// kicad-cli's DRC of the board the session made, against the routed board's: no kind of violation is more common, and nothing is left unconnected that was not.
#[test]
fn the_imported_session_makes_no_new_drc_violation() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let drc = |dir: &Path| -> Value { serde_json::from_str(&ok(&["board", "drc", "-C", dir.to_str().unwrap()]).lines().skip_while(|l| !l.starts_with('{')).collect::<Vec<_>>().join("\n")).expect("the DRC report") };
    let (b, _, routed) = round_trip("slow", false);
    let after = drc(&b);
    // The routed board as the pipeline left it, in a folder of its own.
    let was = b.parent().unwrap().join("was");
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml");
    ok(&["board", "new", example.to_str().unwrap(), "-o", was.to_str().unwrap()]);
    std::fs::write(was.join("design.json"), serde_json::to_string_pretty(&routed).unwrap()).unwrap();
    let before = drc(&was);
    eprintln!("before: {} / unconnected {}", before["counts"], before["unconnected_items"].as_array().map_or(0, Vec::len));
    eprintln!("after:  {} / unconnected {}", after["counts"], after["unconnected_items"].as_array().map_or(0, Vec::len));
    let count = |r: &Value, k: &str| r["counts"][k].as_u64().unwrap_or(0);
    for (kind, n) in after["counts"].as_object().cloned().unwrap_or_default() {
        assert!(n.as_u64().unwrap_or(0) <= count(&before, &kind), "{kind}: {n} after the import, {} before", count(&before, &kind));
    }
    assert!(after["unconnected_items"].as_array().map_or(0, Vec::len) <= before["unconnected_items"].as_array().map_or(0, Vec::len));
    let _ = std::fs::remove_dir_all(b.parent().unwrap());
}

/// kicad-cli's own exporter (`kicad-cli pcb export specctra`, which the 10.99 nightly has) on the board file the studio derives, against our own exporter (`eda
/// board export specctra` with the probe forced off: the fallback, deprecated): the same text from `(structure` down to `(wiring` -- the layers, the boundary, the rules, the placement, every image and padstack, the nets and their
/// classes. The wiring is left out: the nightly writes a wire per track `(type protect)`, later KiCad than the source this port follows (8303b2ad), which joins the
/// tracks of a net into paths `(type route)`.
#[test]
fn the_dsn_is_kicad_clis_down_to_the_wiring() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = kicad_cli().filter(|c| cli_has_specctra(c)) else {
        eprintln!("skipped: no kicad-cli with pcb export specctra");
        return;
    };
    let b = routed_board("kicad");
    // The derived board file kicad-cli is run on.
    ok(&["board", "export", "--kicad", "pos", "-C", b.to_str().unwrap()]);
    let pcb = std::fs::read_dir(b.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board");
    let (theirs, ours) = (b.parent().unwrap().join("kicad"), b.parent().unwrap().join("eda"));
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::create_dir_all(&ours).unwrap();
    let out = Command::new(cli).args(["pcb", "export", "specctra", "-o", theirs.join("b.dsn").to_str().unwrap(), pcb.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let log = ok_with(&["board", "export", "specctra", "-o", ours.join("b.dsn").to_str().unwrap(), "-C", b.to_str().unwrap()], true);
    assert!(log.contains("(engine: eda)"), "{log}");
    let body = |f: &Path| -> String {
        let t = std::fs::read_to_string(f).unwrap();
        let (a, z) = (t.find("  (structure").expect("a structure"), t.find("  (wiring").expect("a wiring"));
        t[a..z].to_string()
    };
    let (want, got) = (body(&theirs.join("b.dsn")), body(&ours.join("b.dsn")));
    let first = want.lines().zip(got.lines()).enumerate().find(|(_, (a, b))| a != b).map(|(i, (a, b))| format!("line {}:\n  kicad-cli {a:?}\n  eda       {b:?}", i + 1));
    assert!(want == got, "{}", first.unwrap_or_else(|| format!("{} lines against {}", want.lines().count(), got.lines().count())));
    assert!(want.contains("(image ") && want.contains("(padstack ") && want.contains("(net "), "a board with parts and nets");
    let _ = std::fs::remove_dir_all(b.parent().unwrap());
}

/// `eda board export specctra` where kicad-cli has `pcb export specctra`: kicad-cli writes the file (the engine says so), the design in it is named by the file
/// as given (`m_pcbname = aFilename`, here a path relative to the folder the command runs in), and our DSN parser reads it back as a board with parts, padstacks,
/// nets and the routing's wires and vias. A board kicad-cli cannot read is refused in kicad-cli's words.
#[test]
fn the_dsn_is_written_by_kicad_cli_and_our_parser_reads_it() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if !kicad_cli().is_some_and(cli_has_specctra) {
        eprintln!("skipped: no kicad-cli with pcb export specctra");
        return;
    }
    let b = routed_board("cli");
    let root = b.parent().unwrap().to_path_buf();
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).current_dir(&root).env_remove(NO_CLI).args(["board", "export", "specctra", "-o", "routed.dsn", "-C", "board"]).output().unwrap();
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && log.contains("BOARD exported OK: routed.dsn (engine: kicad-cli)"), "{log}");
    let text = std::fs::read_to_string(root.join("routed.dsn")).unwrap();
    let pcb = eda_kicad::specctra::parse::load_pcb(&text, "routed.dsn").expect("our parser reads kicad-cli's DSN");
    assert_eq!(pcb.pcbname, "routed.dsn", "the design is named by the file as given");
    assert!(text.contains("(image ") && text.contains("(padstack ") && text.contains("(net "), "a board with parts and nets");
    let wiring = pcb.wiring.as_ref().expect("the routing is in the wiring");
    assert!(!wiring.wires.is_empty() && !wiring.wire_vias.is_empty(), "the routed board's wires and vias");

    // kicad-cli's words when it cannot write: a folder it may not create.
    let (good, log) = eda(&["board", "export", "specctra", "-o", "/proc/no/such/dir/x.dsn", "-C", b.to_str().unwrap()]);
    assert!(!good && log.contains("Failed to create output directory"), "{log}");
    let _ = std::fs::remove_dir_all(&root);
}
