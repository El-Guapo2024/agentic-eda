//! The fab dialogs' defaults through kicad-cli, on a KiCad QA board brought in with `eda import-kicad` (round 2's r2-editors-exports, X1-X4).
//!
//! - Plot (Gerbers): the board's own plot settings decide the files, as KiCad's Plot dialog's do (`DIALOG_PLOT::Plot` plots with
//!   `GetPlotSettings()`): QA `issue5093.kicad_pcb` says `(usegerberextensions false)`, so the files are `-F_Cu.gbr`, not kicad-cli's
//!   Protel `-F_Cu.gtl`.
//! - Generate Drill Files: two files (`-PTH.drl`, `-NPTH.drl`) and slots routed (`G00` .. `M15`), as `DIALOG_GENDRILL` writes them by default.
//!
//! Slow tier only (`EDA_SLOW_TESTS=1`, the real kicad-cli, `KICAD_QA_DATA` the QA data folder).

use std::path::{Path, PathBuf};
use std::process::Command;

fn slow() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    let qa = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|s| PathBuf::from(s).join("qa/data")));
    let board = qa.map(|q| q.join("pcbnew/issue5093.kicad_pcb")).filter(|b| b.exists());
    if board.is_none() {
        eprintln!("skipped: no KICAD_QA_DATA (or KICAD_SRC_DIR) with pcbnew/issue5093.kicad_pcb");
    }
    board
}

fn eda(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_eda")).args(args).output().expect("run eda");
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "eda {}:\n{log}", args.join(" "));
    log
}

fn imported(tag: &str, board: &Path) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_fab_dialogs_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    eda(&["import-kicad", board.to_str().unwrap(), "-o", d.to_str().unwrap()]);
    d
}

fn written(log: &str) -> Vec<String> {
    log.lines().filter_map(|l| l.strip_prefix("wrote ")).map(|p| Path::new(p).file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn the_gerbers_are_named_and_made_as_the_boards_plot_settings_say() {
    let Some(board) = slow() else { return };
    let dir = imported("gerbers", &board);
    let files = written(&eda(&["fab", "gerbers", dir.to_str().unwrap(), "--layers", "F.Cu,B.Cu,Edge.Cuts"]));
    let mut gerbers: Vec<&String> = files.iter().filter(|f| !f.ends_with(".gbrjob")).collect();
    gerbers.sort();
    assert!(gerbers.len() == 3 && gerbers.iter().all(|f| f.ends_with("-B_Cu.gbr") || f.ends_with("-F_Cu.gbr") || f.ends_with("-Edge_Cuts.gbr")), "KiCad's .gbr names (usegerberextensions false): {files:?}");
}

#[test]
fn the_drill_files_are_two_with_routed_slots() {
    let Some(board) = slow() else { return };
    let dir = imported("drill", &board);
    let files = written(&eda(&["fab", "drill", dir.to_str().unwrap()]));
    assert!(files.iter().any(|f| f.ends_with("-PTH.drl")) && files.iter().any(|f| f.ends_with("-NPTH.drl")), "PTH and NPTH apart: {files:?}");
    let text: String = std::fs::read_dir(dir.join("export/kicad/drill")).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "drl")).map(|e| std::fs::read_to_string(e.path()).unwrap()).collect();
    assert!(text.contains("M15") && text.lines().any(|l| l.starts_with("G00X")), "slots are routed (G00 .. M15)");
    assert!(!text.contains("G85"), "no G85 slot (the alternate drill mode is off)");
    // "Use alternate drill mode for oval holes" checked: G85.
    let _ = std::fs::remove_dir_all(dir.join("export/kicad/drill"));
    eda(&["fab", "drill", dir.to_str().unwrap(), "--alt-drill-mode", "--merge-pth-npth"]);
    let text: String = std::fs::read_dir(dir.join("export/kicad/drill")).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "drl")).map(|e| std::fs::read_to_string(e.path()).unwrap()).collect();
    assert!(text.contains("G85"), "the alternate mode writes G85 slots");
}
