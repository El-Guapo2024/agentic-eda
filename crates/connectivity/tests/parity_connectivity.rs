//! Parity harness, section 3 of the task's measurement plan:
//! `eda_connectivity::analyze` (ratsnest + dangling copper) vs real
//! `kicad-cli pcb drc`'s `unconnected_items`/`track_dangling`/`via_dangling`,
//! per board. Complements the existing, narrower `kicad_cli_ratsnest.rs`
//! (which holds three hand-picked boards through placement-only / fully
//! routed / damaged-routing *scenarios* -- depth on few boards): this file
//! is breadth -- one best-effort-routed scenario each, across every
//! `examples/*.yaml` + `examples/ladder/*.yaml` board, and every board of
//! KiCad's own QA corpus that [`select_qa_pcb_boards`] names (real,
//! already-routed KiCad boards, read via `import_kicad_pcb`). The set is
//! fixed by the repository and the KiCad source (commit 8303b2ad): never the
//! user's `work/` boards, which only some checkouts have.
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
//! skips cleanly if its directory isn't present (see `EDA_KICAD_QA_BOARDS`,
//! `KICAD_QA_DATA`, `KICAD_SRC_DIR`).
//!
//! What is compared, per board: our count of each check, capped at kicad-cli's
//! error limit ([`capped`]), with kicad-cli's on a copy of the board whose
//! project runs all three checks ([`oracle_copy`]).

use eda_model::units::Nm;
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
/// KiCad's own QA data (`qa/data` of the KiCad source at commit 8303b2ad), as one directory: `KICAD_QA_DATA` names it; the default is the
/// copy kept beside the KiCad sources this port was read from (a persistent place, not a scratch directory).
const QA_DATA_ENV: &str = "KICAD_QA_DATA";
const QA_ROOT_DEFAULT: &str = "/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data";

fn qa_root() -> Option<PathBuf> {
    for var in [QA_ROOT_ENV, QA_DATA_ENV] {
        if let Ok(p) = std::env::var(var) {
            let pb = PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
    }
    // The KiCad sources the port reads (`KICAD_SRC_DIR`) have the same `qa/data`.
    if let Some(pb) = std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")).filter(|p| p.exists()) {
        return Some(pb);
    }
    let d = PathBuf::from(QA_ROOT_DEFAULT);
    d.exists().then_some(d)
}

/// The QA boards measured: every `<root>/pcbnew/*.kicad_pcb`, then the first `.kicad_pcb` (by name) directly inside each subdirectory of `pcbnew/`
/// (`issue*/`, `connect/`, `hatch_thermal_connectivity/`, ...), each list sorted by path. A fixed set for a given KiCad source (commit 8303b2ad): no
/// sample, no cap, nothing from `work/` (the user's boards exist only in some checkouts, so a floor that counted them was a machine difference).
fn select_qa_pcb_boards(root: &Path) -> Vec<PathBuf> {
    let pcbnew = root.join("pcbnew");
    let pcbs_in = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "kicad_pcb")).collect();
        v.sort();
        v
    };
    let mut all = pcbs_in(&pcbnew);
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&pcbnew).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    all.extend(dirs.iter().filter_map(|d| pcbs_in(d).into_iter().next()));
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

/// The copper of the zones as kicad-cli fills them (Fill All Zones: `eda_kicad_engine::refill_zones` on a copy of the derived board): what the connectivity is given, as
/// the studio gives it the fills it has kept. kicad-cli's own DRC below refills the same board, so both sides see the same pours.
fn kicad_copper(design: &Design, model: &ConstraintModel) -> eda_drc::zone_copper::ZoneCopper {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let work = std::env::temp_dir().join(format!("eda_conn_fill_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let mut design = design.clone();
    if let Some(r) = design.routing.as_mut() {
        r.assign_missing_ids();
    }
    let copper = match eda_kicad_engine::refill_zones(&design, model, &work) {
        Ok(fills) => fills.copper(&design),
        Err(e) => {
            eprintln!("  kicad-cli could not fill the zones ({e:?}): the pours have no copper");
            Default::default()
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    copper
}

fn our_counts(design: &Design, model: &ConstraintModel) -> Counts {
    let report = analyze(design, model, &kicad_copper(design, model));
    let track_dangling = report.dangling.iter().filter(|d| d.kind == DanglingKind::Track).count();
    let via_dangling = report.dangling.iter().filter(|d| d.kind == DanglingKind::Via).count();
    if std::env::var_os("EDA_PARITY_CONN_VERBOSE").is_some() {
        for e in &report.ratsnest {
            println!("  ours unconnected {} {:?} {:?} -> {:?} {:?}", e.net, e.from_item, e.from, e.to_item, e.to);
        }
        // `EDA_PARITY_CONN_PROBE=<x>,<y>` (micrometres): every item whose box holds the point, with what it touches.
        if let Some((x, y)) = std::env::var("EDA_PARITY_CONN_PROBE").ok().and_then(|v| v.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<i64>().ok()?, y.trim().parse::<i64>().ok()?)))) {
            let g = &report.graph.items;
            for it in g.iter().filter(|i| i.bbox.0 <= Nm::from_raw_shim(x) && x <= i.bbox.2.raw_shim() && i.bbox.1 <= Nm::from_raw_shim(y) && y <= i.bbox.3.raw_shim()) {
                let touching: Vec<String> = it.connected.iter().map(|&c| format!("{:?}[{}..{}]{}", g[c].item, g[c].layer_lo, g[c].layer_hi, g[c].net)).collect();
                println!("  probe {:?}[{}..{}]{} touching {:?}", it.item, it.layer_lo, it.layer_hi, it.net, touching);
            }
        }
        for d in &report.dangling {
            let it = report.graph.items.iter().find(|i| match &i.item {
                eda_connectivity::ItemRef::Via { via_id } => *via_id == d.id,
                eda_connectivity::ItemRef::TrackSeg { track_id, seg_index } => (if *seg_index == 0 { track_id.clone() } else { format!("{track_id}#{seg_index}") }) == d.id,
                _ => false,
            });
            let touching: Vec<String> = it.map(|i| i.connected.iter().map(|&c| format!("{:?}[{}..{}]{}", report.graph.items[c].item, report.graph.items[c].layer_lo, report.graph.items[c].layer_hi, report.graph.items[c].net)).collect()).unwrap_or_default();
            println!("  ours dangling {:?} {} {} at {:?} touching {:?}", d.kind, d.net, d.id, d.at, touching);
        }
    }
    Counts { unconnected: report.ratsnest.len(), track_dangling, via_dangling }
}

/// The three checks measured, by their kicad-cli keys.
const MEASURED_CHECKS: [&str; 3] = ["unconnected_items", "track_dangling", "via_dangling"];

/// What kicad-cli reports at most of each measured check: `DRC_ENGINE::RunTests` gives every check an error limit, `ERROR_LIMIT` (199) and
/// `EXTENDED_ERROR_LIMIT` (499) for `DRCE_CLEARANCE` and `DRCE_UNCONNECTED_ITEMS`, and `DRC_TEST_PROVIDER_CONNECTIVITY::Run` stops reporting a code once
/// `IsErrorLimitExceeded`. Our count is compared capped the same way: past the limit kicad-cli says nothing more to compare with.
const ERROR_LIMIT: usize = 199;
const EXTENDED_ERROR_LIMIT: usize = 499;

fn capped(c: Counts) -> Counts {
    Counts { unconnected: c.unconnected.min(EXTENDED_ERROR_LIMIT), track_dangling: c.track_dangling.min(ERROR_LIMIT), via_dangling: c.via_dangling.min(ERROR_LIMIT) }
}

/// The board as kicad-cli is given it: a copy in a directory of the harness's own (the KiCad sources are never written to), with the project files of the
/// same stem (`.kicad_pro`, `.kicad_dru`, `.kicad_prl`). A check the project sets to `ignore` is not run at all (`DRC_ENGINE::RunTests` gives it an error
/// limit of 0), so kicad-cli would report none of it whatever the copper says; the copy's project sets each measured check that is ignored back to KiCad's
/// default severity and the board says which (`unignored`). The library checks are set to `ignore` in the copy: they measure nothing here, and
/// [`eda_kicad_engine::cli_command`] then runs kicad-cli without the user's footprint libraries (minutes faster on a large board, the same connectivity).
fn oracle_copy(pcb: &Path, dir: &Path) -> (PathBuf, Option<PathBuf>, Vec<String>) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let stem = pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("board");
    let copy = dir.join(pcb.file_name().unwrap());
    std::fs::copy(pcb, &copy).unwrap_or_else(|e| panic!("copy {}: {e}", pcb.display()));
    for ext in ["kicad_dru", "kicad_prl"] {
        let side = pcb.with_file_name(format!("{stem}.{ext}"));
        if side.exists() {
            let _ = std::fs::copy(&side, dir.join(side.file_name().unwrap()));
        }
    }
    let pro_src = pcb.with_file_name(format!("{stem}.kicad_pro"));
    let mut unignored = Vec::new();
    let Ok(text) = std::fs::read_to_string(&pro_src) else { return (copy, None, unignored) };
    let pro_dst = dir.join(format!("{stem}.kicad_pro"));
    let Ok(mut pro) = serde_json::from_str::<serde_json::Value>(&text) else {
        std::fs::write(&pro_dst, text).unwrap();
        return (copy, Some(pro_dst), unignored);
    };
    if let Some(sev) = pro.pointer_mut("/board/design_settings/rule_severities").and_then(|v| v.as_object_mut()) {
        for key in MEASURED_CHECKS {
            if sev.get(key).and_then(|v| v.as_str()) == Some("ignore") {
                let default = eda_model::rules::DRC_CHECKS.iter().find(|(k, _)| *k == key).map(|(_, s)| *s).unwrap_or("error");
                sev.insert(key.into(), serde_json::Value::String(default.into()));
                unignored.push(key.to_string());
            }
        }
        for key in ["lib_footprint_issues", "lib_footprint_mismatch"] {
            sev.insert(key.into(), serde_json::Value::String("ignore".into()));
        }
    }
    std::fs::write(&pro_dst, serde_json::to_string_pretty(&pro).unwrap()).unwrap();
    (copy, Some(pro_dst), unignored)
}

/// kicad-cli's DRC of `pcb` (with the zones refilled) on a copy ([`oracle_copy`]): the counts, and the measured checks the board's project had ignored.
fn oracle_counts(cli: &Path, pcb: &Path) -> (Counts, Vec<String>) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join("eda_parity_conn_oracle").join(format!("{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let (copy, project, unignored) = oracle_copy(pcb, &dir);
    let report_path = dir.join("drc.json");
    let mut cmd = eda_kicad_engine::cli_command(cli, project.as_deref());
    cmd.args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--exit-code-violations", "--output"]).arg(&report_path).arg(&copy);
    let out = cmd.output().expect("failed to run kicad-cli pcb drc");
    if !report_path.exists() {
        panic!("kicad-cli produced no report for {}: stdout={} stderr={}", pcb.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap_or_default()).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            *by_type.entry(ty).or_default() += 1;
        }
    }
    let unconnected = report.get("unconnected_items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    if std::env::var_os("EDA_PARITY_CONN_VERBOSE").is_some() {
        let vs = report.get("violations").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let us = report.get("unconnected_items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for v in us.iter().chain(vs.iter().filter(|v| v.get("type").and_then(|t| t.as_str()).is_some_and(|t| MEASURED_CHECKS.contains(&t)))) {
            let items: Vec<String> = v.get("items").and_then(|i| i.as_array()).into_iter().flatten().map(|i| format!("{} @ {}", i.get("description").and_then(|d| d.as_str()).unwrap_or(""), i.get("pos").map(|p| p.to_string()).unwrap_or_default())).collect();
            println!("  kicad {} {}", v.get("type").and_then(|t| t.as_str()).unwrap_or(""), items.join(" | "));
        }
    }
    (Counts { unconnected, track_dangling: by_type.get("track_dangling").copied().unwrap_or(0), via_dangling: by_type.get("via_dangling").copied().unwrap_or(0) }, unignored)
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
    /// The measured checks the board's project sets to `ignore`, turned back on for kicad-cli's run (see [`oracle_copy`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unignored: Vec<String>,
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
        let (oracle, unignored) = oracle_counts(cli, &pcb_path);
        (capped(our_counts(&routed, &model)), oracle, unignored)
    }));
    match result {
        Ok((ours, oracle, unignored)) => BoardResult { board: name, source, error: None, ours, oracle, unignored },
        Err(e) => BoardResult { board: name, source, error: Some(e), ours: Counts::default(), oracle: Counts::default(), unignored: Vec::new() },
    }
}

fn process_qa_board(cli: &Path, pcb: &Path) -> BoardResult {
    let name = pcb.strip_prefix(pcb.ancestors().nth(2).unwrap_or(pcb)).unwrap_or(pcb).display().to_string();
    println!("== qa: {name} ==");
    let result = catch(AssertUnwindSafe(|| {
        let text = std::fs::read_to_string(pcb).unwrap_or_else(|e| panic!("read {}: {e}", pcb.display()));
        let (design, model, _notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import_kicad_pcb failed: {e:?}"));
        let (oracle, unignored) = oracle_counts(cli, pcb);
        (capped(our_counts(&design, &model)), oracle, unignored)
    }));
    match result {
        Ok((ours, oracle, unignored)) => BoardResult { board: name, source: "qa", error: None, ours, oracle, unignored },
        Err(e) => BoardResult { board: name, source: "qa", error: Some(e), ours: Counts::default(), oracle: Counts::default(), unignored: Vec::new() },
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
    // `EDA_PARITY_CONN_ONLY=<a>,<b>`: only the boards whose path has one of these in it, for working one miss; such a run prints, does not write the record.
    let only: Vec<String> = std::env::var("EDA_PARITY_CONN_ONLY").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let wanted = |p: &Path| only.is_empty() || only.iter().any(|o| p.display().to_string().contains(o.as_str()));

    for yaml in example_boards(&repo).into_iter().filter(|p| wanted(p)) {
        let source = if yaml.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some("ladder") { "ladder" } else { "example" };
        println!("== example: {} ==", yaml.display());
        boards.push(process_example(&cli, &yaml, source));
    }

    if let Some(root) = qa_root() {
        let qa_sample = select_qa_pcb_boards(&root);
        println!("\nQA corpus ({} boards, root={})", qa_sample.len(), root.display());
        for pcb in qa_sample.iter().filter(|p| wanted(p)) {
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
        /// The day (UTC) this was measured; `tools/parity_report.py` prints it with the numbers.
        measured_at: String,
    }
    let today = std::process::Command::new("date").args(["-u", "+%Y-%m-%d"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let report = Report { boards: &boards, totals_ours, totals_oracle, exact_match_rate: match_rate, exact_matches, total_checks, import_failures: &import_failures, measured_at: today };
    if !only.is_empty() {
        println!("\nEDA_PARITY_CONN_ONLY is set: docs/parity/raw/connectivity.json is left as it was");
        return;
    }
    let out_dir = repo.join("docs/parity/raw");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("connectivity.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("\nwrote {}", out_dir.join("connectivity.json").display());

    assert!(!boards.is_empty(), "parity harness processed zero boards -- that's a harness bug, not a measurement");
}
