//! Parity harness, section 3 of the task's measurement plan:
//! `eda_connectivity::analyze` (ratsnest + dangling copper) vs real
//! `kicad-cli pcb drc`'s `unconnected_items`/`track_dangling`/`via_dangling`,
//! per board. Complements the existing, narrower `kicad_cli_ratsnest.rs`
//! (which holds three hand-picked boards through placement-only / fully
//! routed / damaged-routing *scenarios* -- depth on few boards): this file
//! is breadth -- one best-effort-routed scenario each, across every
//! `examples/*.yaml` + `examples/ladder/*.yaml` board, `work/mcu30` +
//! `work/l1-order`, and a deterministic sample of KiCad's own QA board
//! corpus (real, already-routed KiCad boards, read via `import_kicad_pcb`).
//!
//! Counts only (not position-matched) -- same convention
//! `kicad_cli_ratsnest.rs` uses, and all the task's own wording ("our
//! unconnected/ratsnest vs KiCad's unconnected_items, per board") asks for.
//!
//! Writes `docs/parity/raw/connectivity.json`; `tools/parity_report.py`
//! folds every `raw/*.json` into `docs/parity/REPORT.md` +
//! `docs/parity/scores.json`. `#[ignore]`d -- the one command:
//!   cargo test -p eda-connectivity --test parity_connectivity -- --ignored --nocapture
//! Skips cleanly without kicad-cli; the QA-corpus portion additionally
//! skips cleanly if its directory isn't present (see `EDA_KICAD_QA_BOARDS`).

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_connectivity::{analyze, DanglingKind};
use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::Design;
use eda_model::ConstraintModel;
use eda_place::{place, PlaceOptions};

/// Our exporter writes the KiCad 9 file format; an older kicad-cli (e.g.
/// Ubuntu's 7.0) refuses to open it, so it can't serve as the oracle.
fn kicad_cli_reads_our_format(cli: &Path) -> bool {
    let v = Command::new(cli).arg("version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let major: u32 = v.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < 9 {
        eprintln!("kicad-cli {v} is older than the KiCad 9 format we export; skipping");
    }
    major >= 9
}

fn find_kicad_cli() -> Option<PathBuf> {
    find_kicad_cli_any().filter(|p| kicad_cli_reads_our_format(p))
}

fn find_kicad_cli_any() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().and_then(|p| p.parent()).expect("crates/connectivity -> repo root").to_path_buf()
}

const QA_ROOT_ENV: &str = "EDA_KICAD_QA_BOARDS";
const QA_ROOT_DEFAULT: &str = "/private/tmp/claude-501/-Users-juanantonioluera-ws/8eb77140-1019-4605-b5f4-960e15f5bf6d/scratchpad/kicad_qa_boards/qa/data";

fn qa_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(QA_ROOT_ENV) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let d = PathBuf::from(QA_ROOT_DEFAULT);
    d.exists().then_some(d)
}

/// Same deterministic-sample rule as `crates/drc/tests/parity_drc.rs`
/// (kept as an independent copy -- see that file's doc comment on why
/// these helpers are duplicated per test file rather than shared).
fn select_qa_pcb_boards(root: &Path, cap: usize) -> Vec<PathBuf> {
    let pcbnew = root.join("pcbnew");
    let mut named: Vec<PathBuf> = std::fs::read_dir(&pcbnew).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).collect();
    named.sort();
    let mut issue_dirs: Vec<PathBuf> = std::fs::read_dir(&pcbnew)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("issue")))
        .collect();
    issue_dirs.sort();
    let from_issues: Vec<PathBuf> = issue_dirs
        .into_iter()
        .filter_map(|d| {
            let mut pcbs: Vec<PathBuf> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).collect();
            pcbs.sort();
            pcbs.into_iter().next()
        })
        .collect();
    let mut all = Vec::new();
    let (mut ni, mut ii) = (named.into_iter(), from_issues.into_iter());
    loop {
        let (a, b) = (ni.next(), ii.next());
        if a.is_none() && b.is_none() {
            break;
        }
        all.extend(a);
        all.extend(b);
        if all.len() >= cap {
            break;
        }
    }
    all.truncate(cap);
    all
}

fn catch<F: FnOnce() -> R + panic::UnwindSafe, R>(f: F) -> Result<R, String> {
    panic::catch_unwind(f).map_err(|e| {
        if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic (non-string payload)".into()
        }
    })
}

#[derive(Default, Debug, Clone, Copy, serde::Serialize)]
struct Counts {
    unconnected: usize,
    track_dangling: usize,
    via_dangling: usize,
}

fn our_counts(design: &Design, model: &ConstraintModel) -> Counts {
    let report = analyze(design, model);
    let track_dangling = report.dangling.iter().filter(|d| d.kind == DanglingKind::Track).count();
    let via_dangling = report.dangling.iter().filter(|d| d.kind == DanglingKind::Via).count();
    Counts { unconnected: report.ratsnest.len(), track_dangling, via_dangling }
}

fn oracle_counts(cli: &Path, pcb: &Path, refill: bool) -> Counts {
    let stem = pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("board");
    let report_path = pcb.with_file_name(format!("{stem}.conn.{}.json", if refill { "refill" } else { "norefill" }));
    let mut args: Vec<String> = vec!["pcb".into(), "drc".into()];
    if refill {
        args.push("--refill-zones".into());
    }
    args.extend(["--format".into(), "json".into(), "--severity-all".into(), "--exit-code-violations".into(), "--output".into(), report_path.display().to_string(), pcb.display().to_string()]);
    let out = Command::new(cli).args(&args).output().expect("failed to run kicad-cli pcb drc");
    if !report_path.exists() {
        eprintln!("kicad-cli produced no report for {}: stdout={} stderr={}", pcb.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        return Counts::default();
    }
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap_or_default()).unwrap_or_default();
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            *by_type.entry(ty).or_default() += 1;
        }
    }
    let unconnected = report.get("unconnected_items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    Counts { unconnected, track_dangling: by_type.get("track_dangling").copied().unwrap_or(0), via_dangling: by_type.get("via_dangling").copied().unwrap_or(0) }
}

fn route_best_effort(design: &Design, model: &ConstraintModel) -> Design {
    let mut out = design.clone();
    if let Ok(routed) = eda_freeroute::design::route_design(design, model, &model.board, 20, 10) {
        if !routed.unrouted.is_empty() {
            eprintln!("  (best-effort route left unrouted: {:?})", routed.unrouted);
        }
        out.routing = Some(routed.routing);
    }
    out
}

#[derive(Debug, Clone, serde::Serialize)]
struct BoardResult {
    board: String,
    source: &'static str,
    error: Option<String>,
    ours: Counts,
    oracle: Counts,
}

fn example_boards(repo: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(repo.join("examples")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")).collect();
    v.extend(std::fs::read_dir(repo.join("examples/ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")));
    v.sort();
    v
}

fn process_example(cli: &Path, yaml_path: &Path, source: &'static str) -> BoardResult {
    let name = yaml_path.file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let result = catch(AssertUnwindSafe(|| {
        let text = std::fs::read_to_string(yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
        let mut model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", yaml_path.display()));
        let fp_root = eda_kicad::default_footprint_library_root();
        for w in eda_kicad::resolve_library_footprints(&mut model, &fp_root) {
            eprintln!("  footprint library: {w}");
        }
        let opts = EngineOptions { seed: 0, intent_hash: format!("parity_conn_{name}"), ..Default::default() };
        let design = derive_schematic(&model, &opts).unwrap_or_else(|e| panic!("derive_schematic: {e:?}"));
        let placed = place(&design, &model, &PlaceOptions { seed: 0, ..Default::default() }).unwrap_or_else(|e| panic!("place: {e:?}"));
        let routed = route_best_effort(&placed, &model);
        let meta = ExportMeta { date: "2026-01-01", title: &name };
        let pcb_text = export_kicad_pcb(&routed, &model, &meta).unwrap_or_else(|e| panic!("export: {e:?}"));
        let dir = std::env::temp_dir().join("eda_parity_conn").join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        let pcb_path = dir.join(format!("{name}.kicad_pcb"));
        std::fs::write(&pcb_path, &pcb_text).unwrap();
        (our_counts(&routed, &model), oracle_counts(cli, &pcb_path, true))
    }));
    match result {
        Ok((ours, oracle)) => BoardResult { board: name, source, error: None, ours, oracle },
        Err(e) => BoardResult { board: name, source, error: Some(e), ours: Counts::default(), oracle: Counts::default() },
    }
}

fn process_prebuilt(cli: &Path, name: &'static str, design_json: &Path, intent_yaml: &Path, pcb: &Path) -> BoardResult {
    let result = catch(AssertUnwindSafe(|| {
        let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(intent_yaml).unwrap_or_else(|e| panic!("read {}: {e}", intent_yaml.display())))
            .unwrap_or_else(|e| panic!("parse {}: {e}", intent_yaml.display()));
        let design: Design = serde_json::from_str(&std::fs::read_to_string(design_json).unwrap_or_else(|e| panic!("read {}: {e}", design_json.display())))
            .unwrap_or_else(|e| panic!("parse {}: {e}", design_json.display()));
        (our_counts(&design, &model), oracle_counts(cli, pcb, true))
    }));
    match result {
        Ok((ours, oracle)) => BoardResult { board: name.into(), source: "work", error: None, ours, oracle },
        Err(e) => BoardResult { board: name.into(), source: "work", error: Some(e), ours: Counts::default(), oracle: Counts::default() },
    }
}

fn process_qa_board(cli: &Path, pcb: &Path) -> BoardResult {
    let name = pcb.strip_prefix(pcb.ancestors().nth(2).unwrap_or(pcb)).unwrap_or(pcb).display().to_string();
    let result = catch(AssertUnwindSafe(|| {
        let text = std::fs::read_to_string(pcb).unwrap_or_else(|e| panic!("read {}: {e}", pcb.display()));
        let (design, model, _notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import_kicad_pcb failed: {e:?}"));
        (our_counts(&design, &model), oracle_counts(cli, pcb, true))
    }));
    match result {
        Ok((ours, oracle)) => BoardResult { board: name, source: "qa", error: None, ours, oracle },
        Err(e) => BoardResult { board: name, source: "qa", error: Some(e), ours: Counts::default(), oracle: Counts::default() },
    }
}

#[test]
#[ignore]
fn parity_connectivity_harness() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping parity_connectivity_harness");
        return;
    };
    let repo = repo_root();
    let mut boards = Vec::new();

    for yaml in example_boards(&repo) {
        let source = if yaml.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some("ladder") { "ladder" } else { "example" };
        println!("== example: {} ==", yaml.display());
        boards.push(process_example(&cli, &yaml, source));
    }

    let work_root = repo.join("work");
    let mcu30_design = work_root.join("mcu30/design.json");
    let mcu30_pcb = work_root.join("mcu30/export/mcu_board_30plus.kicad_pcb");
    if mcu30_design.exists() && mcu30_pcb.exists() {
        boards.push(process_prebuilt(&cli, "work/mcu30", &mcu30_design, &repo.join("examples/mcu_board_30plus.yaml"), &mcu30_pcb));
    }
    let l1_design = work_root.join("l1-order/_pipeline/design.json");
    let l1_pcb = work_root.join("l1-order/_pipeline/l1_usb_mcu.kicad_pcb");
    if l1_design.exists() && l1_pcb.exists() {
        boards.push(process_prebuilt(&cli, "work/l1-order", &l1_design, &repo.join("examples/ladder/l1_usb_mcu.yaml"), &l1_pcb));
    }

    if let Some(root) = qa_root() {
        let qa_sample = select_qa_pcb_boards(&root, 40);
        println!("\nQA corpus sample ({} boards, root={})", qa_sample.len(), root.display());
        for pcb in &qa_sample {
            boards.push(process_qa_board(&cli, pcb));
        }
    } else {
        eprintln!("KiCad QA corpus not found; skipping QA-corpus boards");
    }

    let mut totals_ours = Counts::default();
    let mut totals_oracle = Counts::default();
    let mut exact_matches = 0usize;
    let mut total_checks = 0usize;
    let mut import_failures = Vec::new();
    for b in &boards {
        if let Some(e) = &b.error {
            import_failures.push(format!("{} [{}]: {e}", b.board, b.source));
            continue;
        }
        totals_ours.unconnected += b.ours.unconnected;
        totals_ours.track_dangling += b.ours.track_dangling;
        totals_ours.via_dangling += b.ours.via_dangling;
        totals_oracle.unconnected += b.oracle.unconnected;
        totals_oracle.track_dangling += b.oracle.track_dangling;
        totals_oracle.via_dangling += b.oracle.via_dangling;
        for (ours, oracle) in [(b.ours.unconnected, b.oracle.unconnected), (b.ours.track_dangling, b.oracle.track_dangling), (b.ours.via_dangling, b.oracle.via_dangling)] {
            total_checks += 1;
            if ours == oracle {
                exact_matches += 1;
            }
        }
        println!(
            "{:28} [{}] unconnected ours={:<4} oracle={:<4} | track_dangling ours={:<4} oracle={:<4} | via_dangling ours={:<4} oracle={:<4}",
            b.board, b.source, b.ours.unconnected, b.oracle.unconnected, b.ours.track_dangling, b.oracle.track_dangling, b.ours.via_dangling, b.oracle.via_dangling
        );
    }
    let match_rate = if total_checks > 0 { exact_matches as f64 / total_checks as f64 } else { 1.0 };
    println!("\ntotals: ours={totals_ours:?} oracle={totals_oracle:?}");
    println!("exact per-board-per-field match rate: {exact_matches}/{total_checks} ({:.1}%)", match_rate * 100.0);
    if !import_failures.is_empty() {
        println!("\nboards that errored (gaps, not folded into totals):");
        for f in &import_failures {
            println!("  {f}");
        }
    }

    #[derive(serde::Serialize)]
    struct Report<'a> {
        boards: &'a [BoardResult],
        totals_ours: Counts,
        totals_oracle: Counts,
        exact_match_rate: f64,
        exact_matches: usize,
        total_checks: usize,
        import_failures: &'a [String],
    }
    let report = Report { boards: &boards, totals_ours, totals_oracle, exact_match_rate: match_rate, exact_matches, total_checks, import_failures: &import_failures };
    let out_dir = repo.join("docs/parity/raw");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("connectivity.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("\nwrote {}", out_dir.join("connectivity.json").display());

    assert!(!boards.is_empty(), "parity harness processed zero boards -- that's a harness bug, not a measurement");
}
