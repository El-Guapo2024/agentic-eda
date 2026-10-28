//! Parity harness, section 4 of the task's measurement plan: round-trip
//! fidelity. Three independent measurements, written to
//! `docs/parity/raw/roundtrip.json`:
//!
//! 1. **Full-corpus import notes** (pure Rust, no kicad-cli -- runs
//!    whenever the QA corpus is present, even without kicad-cli installed):
//!    every `.kicad_pcb` (185) and `.kicad_sch` (33) in KiCad's own QA board
//!    corpus, read through `import_kicad_pcb`/`import_kicad_sch`. The
//!    importers already track exactly what they couldn't faithfully
//!    represent (`ImportNotes`/`SchImportNotes`); this aggregates that
//!    across the whole corpus instead of one board at a time, plus every
//!    outright import failure (panic or `Err`) as a reportable gap.
//! 2. **Our own pipeline's round trip**: for every `examples/*.yaml` +
//!    `examples/ladder/*.yaml` board, `design.json`'s own shape ->
//!    `export_kicad_pcb` -> `import_kicad_pcb` -> which top-level fields
//!    survive exactly (extends `kicad_cli_import.rs`'s
//!    `round_trips_own_pipeline_output`, which only covers 2 boards, to all
//!    17, and adds drawings/zones/modules counts it doesn't check).
//! 3. **Re-export fidelity on real boards** (needs kicad-cli): for a small,
//!    deterministic subset of the QA corpus, import the real board, export
//!    it back out through our writer, and ask kicad-cli to DRC *our*
//!    export -- comparing its violation-type counts (and, more basically,
//!    whether kicad-cli can parse the file at all) against kicad-cli's
//!    verdict on the original. This is "anything KiCad reports on load" for
//!    files this project writes, on boards we didn't construct ourselves.
//!
//! `#[ignore]`d -- the one command:
//!   cargo test -p eda-kicad --test parity_roundtrip -- --ignored --nocapture
//! Part 1 and 2 need no kicad-cli at all (only part 3 is skipped without
//! it). Everything here skips cleanly if the QA corpus directory isn't
//! present (see `EDA_KICAD_QA_BOARDS`); part 2 (our own examples) always
//! runs regardless.
//!
//! Knobs (all optional):
//! - `EDA_KICAD_QA_BOARDS` / `KICAD_QA_DATA`: the corpus root (`<root>/pcbnew/*.kicad_pcb`, each with an
//!   optional `.kicad_pro` and `.kicad_dru` beside it). Default: `qa/data` of the KiCad sources at
//!   `/Users/juanantonioluera/ws/kicad-src-8303b2ad`.
//! - `EDA_PARITY_PARTS`: which parts to run, a comma list of `own`, `corpus`, `reexport`
//!   (default all). A partial run keeps the other parts' results from the `roundtrip.json`
//!   already on disk instead of erasing them.
//! - `EDA_PARITY_REEXPORT_CAP`: how many boards part 3 takes (default 12).
//! - `EDA_PARITY_REEXPORT_ONLY`: part 3 on these boards only (a comma list of file names without `.kicad_pcb`), for working on one
//!   difference; such a run does not write `roundtrip.json`. The files it makes are in `<temp dir>/eda_parity_reexport/`.
//! - `EDA_PARITY_REEXPORT_UPGRADE`: part 3 on the boards as kicad-cli writes them in the newest file format (`pcb upgrade --force` on the staged copy), original and re-export
//!   judged alike (files in `<temp dir>/eda_parity_reexport_upgraded/`); it does not write `roundtrip.json` either (the measurement is the older formats').
//! - `EDA_PARITY_DRC_TIMEOUT_SECS`: how long one kicad-cli DRC may run (default 600).

use eda_model::units::Nm;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, import_kicad_sch, ExportMeta};
use eda_model::ir::{Track, Via};
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
    manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root").to_path_buf()
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
    let d = PathBuf::from(QA_ROOT_DEFAULT);
    d.exists().then_some(d)
}

fn find_all(root: &Path, ext: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, ext, out);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, ext, &mut out);
    out.sort();
    out
}

/// Every top-level named board plus one per `issue*/` dir, same rule as the
/// DRC/connectivity parity files -- capped small here since this section
/// also runs a kicad-cli DRC pair per board (part 3), unlike part 1's
/// unconditional full-corpus sweep.
fn select_subset(root: &Path, cap: usize) -> Vec<PathBuf> {
    let pcbnew = root.join("pcbnew");
    let mut named: Vec<PathBuf> = std::fs::read_dir(&pcbnew).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).collect();
    named.sort();
    named.truncate(cap);
    named
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

// ---------------------------------------------------------------------
// Part 1: full-corpus import notes
// ---------------------------------------------------------------------

#[derive(Default, Debug, serde::Serialize)]
struct PcbCorpusStats {
    total: usize,
    imported_ok: usize,
    import_failed: usize,
    zones_skipped: usize,
    track_arcs_approximated: usize,
    track_arcs_kept: usize,
    non_rect_pad_shapes_approximated: usize,
    outline_open: usize,
    outline_source_counts: BTreeMap<String, usize>,
    /// Edge.Cuts items kept as shapes (arcs, circles, rectangles, polygons, curves, and the lines of a board that is not one plain loop).
    outline_shapes: usize,
    /// Boards whose Edge.Cuts are malformed by the live port of `testOutline` (`kicad-cli`'s `invalid_outline`), and which.
    outline_malformed: usize,
    outline_malformed_boards: Vec<String>,
    failures: Vec<String>,
}

fn pcb_corpus_notes(root: &Path) -> PcbCorpusStats {
    let mut s = PcbCorpusStats::default();
    for pcb in find_all(&root.join("pcbnew"), "kicad_pcb") {
        s.total += 1;
        let name = pcb.strip_prefix(root).unwrap_or(&pcb).display().to_string();
        let result = catch(AssertUnwindSafe(|| {
            let text = std::fs::read_to_string(&pcb).unwrap_or_else(|e| panic!("read: {e}"));
            import_kicad_pcb(&text).map_err(|checks| format!("{checks:?}"))
        }));
        match result {
            Ok(Ok((_, _, notes))) => {
                s.imported_ok += 1;
                s.zones_skipped += notes.zones_skipped;
                s.track_arcs_approximated += notes.track_arcs_approximated;
                s.track_arcs_kept += notes.track_arcs_kept;
                s.non_rect_pad_shapes_approximated += notes.non_rect_pad_shapes_approximated;
                if notes.outline_open {
                    s.outline_open += 1;
                }
                *s.outline_source_counts.entry(notes.outline_source.to_string()).or_default() += 1;
                s.outline_shapes += notes.outline_shapes;
                if !notes.outline_errors.is_empty() {
                    s.outline_malformed += 1;
                    s.outline_malformed_boards.push(name.clone());
                }
            }
            Ok(Err(e)) => {
                s.import_failed += 1;
                s.failures.push(format!("{name}: {e}"));
            }
            Err(e) => {
                s.import_failed += 1;
                s.failures.push(format!("{name}: PANIC: {e}"));
            }
        }
    }
    s
}

#[derive(Default, Debug, serde::Serialize)]
struct SchCorpusStats {
    total: usize,
    imported_ok: usize,
    import_failed: usize,
    sheets_not_descended: usize,
    unresolved_symbols: usize,
    failures: Vec<String>,
}

fn sch_corpus_notes(root: &Path) -> SchCorpusStats {
    let mut s = SchCorpusStats::default();
    for sch in find_all(&root.join("pcbnew"), "kicad_sch") {
        s.total += 1;
        let name = sch.strip_prefix(root).unwrap_or(&sch).display().to_string();
        let result = catch(AssertUnwindSafe(|| {
            let text = std::fs::read_to_string(&sch).unwrap_or_else(|e| panic!("read: {e}"));
            import_kicad_sch(&text).map_err(|checks| format!("{checks:?}"))
        }));
        match result {
            Ok(Ok((_, _, notes))) => {
                s.imported_ok += 1;
                s.sheets_not_descended += notes.sheets_not_descended;
                s.unresolved_symbols += notes.unresolved_symbols;
            }
            Ok(Err(e)) => {
                s.import_failed += 1;
                s.failures.push(format!("{name}: {e}"));
            }
            Err(e) => {
                s.import_failed += 1;
                s.failures.push(format!("{name}: PANIC: {e}"));
            }
        }
    }
    s
}

// ---------------------------------------------------------------------
// Part 2: our own pipeline's round trip, field-by-field
// ---------------------------------------------------------------------

type Segment = (String, String, Nm, ((Nm, Nm), (Nm, Nm)));
fn segment_set(tracks: &[Track]) -> BTreeSet<Segment> {
    let mut out = BTreeSet::new();
    for t in tracks {
        for w in t.pts.windows(2) {
            let (mut a, mut b) = ((w[0].x, w[0].y), (w[1].x, w[1].y));
            if b < a {
                std::mem::swap(&mut a, &mut b);
            }
            out.insert((t.net.clone(), t.layer.clone(), t.width, (a, b)));
        }
    }
    out
}
fn via_set(vias: &[Via]) -> BTreeSet<(String, Nm, Nm, Nm, Nm, String, String)> {
    vias.iter().map(|v| (v.net.clone(), v.at.x, v.at.y, v.drill, v.diameter, v.from_layer.clone(), v.to_layer.clone())).collect()
}

#[derive(Debug, Default, Clone, serde::Serialize)]
struct FieldSurvival {
    footprints_before: usize,
    footprints_after: usize,
    footprint_pose_exact_matches: usize,
    segments_before: usize,
    segments_after_exact_matches: usize,
    vias_before: usize,
    vias_after_exact_matches: usize,
    zones_before: usize,
    zones_after: usize,
    modules_before: usize,
    modules_after: usize,
}

#[derive(Debug, serde::Serialize)]
struct OwnRoundTrip {
    board: String,
    error: Option<String>,
    survival: FieldSurvival,
}

fn own_round_trip(yaml: &Path) -> OwnRoundTrip {
    let name = yaml.file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let result = catch(AssertUnwindSafe(|| {
        let text = std::fs::read_to_string(yaml).unwrap_or_else(|e| panic!("read {}: {e}", yaml.display()));
        let mut model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse: {e}"));
        let fp_root = eda_kicad::default_footprint_library_root();
        for w in eda_kicad::resolve_library_footprints(&mut model, &fp_root) {
            eprintln!("  footprint library: {w}");
        }
        let opts = EngineOptions { seed: 0, intent_hash: format!("parity_rt_{name}"), ..Default::default() };
        let design = derive_schematic(&model, &opts).unwrap_or_else(|e| panic!("derive_schematic: {e:?}"));
        let placed = place(&design, &model, &PlaceOptions { seed: 0, ..Default::default() }).unwrap_or_else(|e| panic!("place: {e:?}"));
        let mut routed = placed.clone();
        if let Ok(r) = eda_freeroute::design::route_design(&placed, &model, &model.board, 20, 10) {
            routed.routing = Some(r.routing);
        }

        let meta = ExportMeta { date: "2026-01-01", title: &name };
        let pcb_text = export_kicad_pcb(&routed, &model, &meta).unwrap_or_else(|e| panic!("export: {e:?}"));
        let (back, _model2, _notes) = import_kicad_pcb(&pcb_text).unwrap_or_else(|e| panic!("import: {e:?}"));

        let orig_pl = routed.placement.as_ref();
        let back_pl = back.placement.as_ref();
        let mut survival = FieldSurvival::default();
        if let (Some(op), Some(bp)) = (orig_pl, back_pl) {
            survival.footprints_before = op.footprints.len();
            survival.footprints_after = bp.footprints.len();
            survival.modules_before = op.modules.len();
            survival.modules_after = bp.modules.len();
            let by_id: std::collections::HashMap<&str, _> = bp.footprints.iter().map(|f| (f.id.as_str(), f)).collect();
            survival.footprint_pose_exact_matches = op.footprints.iter().filter(|fp| by_id.get(fp.id.as_str()).is_some_and(|g| g.at == fp.at && g.rot == fp.rot && g.side == fp.side)).count();
        }
        let orig_rt = routed.routing.as_ref();
        let back_rt = back.routing.as_ref();
        if let Some(rt) = orig_rt {
            let before_segs = segment_set(&rt.tracks);
            let before_vias = via_set(&rt.vias);
            survival.segments_before = before_segs.len();
            survival.vias_before = before_vias.len();
            survival.zones_before = rt.zones.len();
            if let Some(brt) = back_rt {
                let after_segs = segment_set(&brt.tracks);
                let after_vias = via_set(&brt.vias);
                survival.segments_after_exact_matches = before_segs.intersection(&after_segs).count();
                survival.vias_after_exact_matches = before_vias.intersection(&after_vias).count();
                survival.zones_after = brt.zones.len();
            }
        }
        survival
    }));
    match result {
        Ok(survival) => OwnRoundTrip { board: name, error: None, survival },
        Err(e) => OwnRoundTrip { board: name, error: Some(e), survival: FieldSurvival::default() },
    }
}

// ---------------------------------------------------------------------
// Part 3: re-export fidelity on a subset of real QA boards
// ---------------------------------------------------------------------

/// A command with a clock on it: a run still going after `limit` is killed.
fn output_within(mut cmd: Command, limit: std::time::Duration) -> Option<std::process::Output> {
    use std::process::Stdio;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
}

fn drc_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(std::env::var("EDA_PARITY_DRC_TIMEOUT_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(600))
}

/// kicad-cli's DRC report of `pcb` (`--refill-zones --severity-all`, as JSON), `None` when it did not load the board or did not finish.
fn kicad_drc_report(cli: &Path, pcb: &Path, tag: &str) -> Option<serde_json::Value> {
    let report_path = pcb.with_file_name(format!("{}.{tag}.drc.json", pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("b")));
    let _ = std::fs::remove_file(&report_path);
    let mut cmd = Command::new(cli);
    cmd.args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--output"]).arg(&report_path).arg(pcb);
    let out = output_within(cmd, drc_timeout());
    if !report_path.exists() {
        match out {
            Some(out) => eprintln!("kicad-cli failed to load {} ({}): stdout={} stderr={}", pcb.display(), tag, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
            None => eprintln!("kicad-cli DRC on {} ({}) did not finish in time", pcb.display(), tag),
        }
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap_or_default()).unwrap_or_default())
}

/// The violations of a report by type.
fn type_counts(report: &serde_json::Value) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for v in report.get("violations").and_then(|v| v.as_array()).into_iter().flatten() {
        *m.entry(v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string()).or_insert(0) += 1;
    }
    m
}

/// A description with its lengths taken out (`3.6322 mm` and `3.632 mm` are one finding of two boards whose coordinates differ in the fourth decimal: the importer keeps
/// micrometres, an inch board's grid has tenths of one).
fn without_lengths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            let rest: String = chars[j..].iter().take(3).collect();
            if rest.starts_with(" mm") || rest.starts_with("mm") {
                out.push('#');
            } else {
                out.extend(&chars[i..j]);
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// One finding of a report: its kind, severity, description (lengths out) and, per item, the description (lengths out) and the position in mm.
type Finding = (String, String, Vec<(String, f64, f64)>);

fn findings(report: &serde_json::Value, key: &str) -> Vec<Finding> {
    report
        .get(key)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|v| {
            let text = |k: &str| v.get(k).and_then(|t| t.as_str()).unwrap_or("").to_string();
            let mut items: Vec<(String, f64, f64)> = v
                .get("items")
                .and_then(|i| i.as_array())
                .into_iter()
                .flatten()
                .map(|i| (without_lengths(i.get("description").and_then(|d| d.as_str()).unwrap_or("")), i.pointer("/pos/x").and_then(|x| x.as_f64()).unwrap_or(0.0), i.pointer("/pos/y").and_then(|y| y.as_f64()).unwrap_or(0.0)))
                .collect();
            items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
            (format!("{}|{}|{}", text("type"), text("severity"), without_lengths(&text("description"))), key.to_string(), items)
        })
        .collect()
}

/// How many findings (violations and unconnected items) one report has that the other does not: a finding is matched by its type, severity, description and the descriptions
/// of its items, with every item's position within `tolerance_mm`. 0 is the same findings; the counts of each type being equal does not say so.
fn finding_differences(a: &serde_json::Value, b: &serde_json::Value, tolerance_mm: f64) -> usize {
    let mut differing = 0;
    for key in ["violations", "unconnected_items"] {
        let (fa, mut fb) = (findings(a, key), findings(b, key));
        for x in &fa {
            let hit = fb.iter().position(|y| x.0 == y.0 && x.2.len() == y.2.len() && x.2.iter().zip(&y.2).all(|(p, q)| p.0 == q.0 && (p.1 - q.1).abs() <= tolerance_mm && (p.2 - q.2).abs() <= tolerance_mm));
            match hit {
                Some(i) => {
                    fb.swap_remove(i);
                }
                None => differing += 1,
            }
        }
        differing += fb.len();
    }
    differing
}

#[test]
fn findings_are_compared_by_type_items_and_place_and_not_by_lengths() {
    let report = |violations: serde_json::Value| serde_json::json!({ "violations": violations, "unconnected_items": [] });
    let finding = |ty: &str, description: &str, item: &str, x: f64| serde_json::json!({ "type": ty, "severity": "error", "description": description, "items": [{ "description": item, "pos": { "x": x, "y": 2.0 } }] });
    let a = report(serde_json::json!([finding("connection_width", "Copper connection too narrow (actual 0.1014 mm)", "Track [net_3] on F.Cu, length 3.6322 mm", 112.7756)]));
    let b = report(serde_json::json!([finding("connection_width", "Copper connection too narrow (actual 0.1004 mm)", "Track [net_3] on F.Cu, length 3.6320 mm", 112.776)]));
    assert_eq!(finding_differences(&a, &b, 0.005), 0, "the lengths and a place a few micrometres off are the same finding");
    let moved = report(serde_json::json!([finding("connection_width", "Copper connection too narrow (actual 0.1004 mm)", "Track [net_3] on F.Cu, length 3.6320 mm", 112.9)]));
    assert_eq!(finding_differences(&a, &moved, 0.005), 2, "a finding elsewhere is one missing and one extra");
    let other = report(serde_json::json!([finding("clearance", "Clearance violation", "Track [net_3] on F.Cu, length 3.6320 mm", 112.776)]));
    assert_eq!(finding_differences(&a, &other, 0.005), 2);
    let none = report(serde_json::json!([]));
    assert_eq!((finding_differences(&a, &none, 0.005), finding_differences(&none, &none, 0.005)), (1, 0));
    assert_eq!(without_lengths("Pad 1 [GND] of U1, 0.25 mm; actual 12 mm"), "Pad 1 [GND] of U1, # mm; actual # mm");
}

#[derive(Debug, serde::Serialize)]
struct ReexportFidelity {
    board: String,
    error: Option<String>,
    original_parses: bool,
    reexport_parses: bool,
    original_types: BTreeMap<String, usize>,
    reexport_types: BTreeMap<String, usize>,
    identical_type_counts: bool,
    /// The two reports list the same findings, not only as many of each type (`finding_differences` is 0); the number that differ.
    #[serde(default)]
    finding_differences: Option<usize>,
}

/// The file format of the board both sides of the measurement start from: as the QA corpus ships it (the KiCad 6 to 9 formats), or as the installed kicad-cli writes it
/// today (`pcb upgrade --force`: the KiCad 10 nightly's format, which the importer reads differently in places).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Shipped,
    Upgraded,
}

/// Where a format's files go: `<temp dir>/eda_parity_reexport/` (`orig/` has the staged originals), and `eda_parity_reexport_upgraded/` for the upgraded ones.
fn work_dir(format: Format) -> PathBuf {
    std::env::temp_dir().join(match format {
        Format::Shipped => "eda_parity_reexport",
        Format::Upgraded => "eda_parity_reexport_upgraded",
    })
}

/// The original board with its project and custom rules, copied to `<work dir>/orig/`: kicad-cli writes the report (and a
/// `.kicad_prl`) beside the board it loads, and the KiCad sources' `qa/data` is read-only.
fn stage_original(cli: &Path, pcb: &Path, format: Format) -> PathBuf {
    let dir = work_dir(format).join("orig");
    std::fs::create_dir_all(&dir).expect("temp dir for the original board");
    for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
        let from = pcb.with_extension(ext);
        if from.exists() {
            let _ = std::fs::copy(&from, dir.join(from.file_name().expect("file name")));
        }
    }
    let staged = dir.join(pcb.file_name().expect("file name"));
    // The same measurement on the board in the newest file format: the original and our re-export of it are both judged by what kicad-cli makes of them, and the importer
    // reads KiCad 10's format instead of the QA corpus's older ones.
    if format == Format::Upgraded {
        let ok = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&staged).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "kicad-cli pcb upgrade failed on {}", staged.display());
    }
    staged
}

fn reexport_fidelity(cli: &Path, pcb: &Path, format: Format) -> ReexportFidelity {
    let name = pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let result = catch(AssertUnwindSafe(|| {
        let staged = stage_original(cli, pcb, format);
        let original_report = kicad_drc_report(cli, &staged, "orig");
        let text = std::fs::read_to_string(&staged).unwrap_or_else(|e| panic!("read: {e}"));
        let (design, mut model, notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import_kicad_pcb: {e:?}"));
        // The project the board came from, as `eda import-kicad` reads it: net classes, severities and the
        // design-rule minimums from the sidecar `.kicad_pro` (less what a legacy board keeps of its own), custom rules from the `.kicad_dru`.
        if let Ok(pro) = std::fs::read_to_string(pcb.with_extension("kicad_pro")) {
            let pro = eda_kicad::project_json_for_board(&pro, &notes);
            eda_kicad::merge_project_net_classes(&mut model, &pro);
            eda_kicad::merge_project_rule_severities(&mut model, &pro);
            eda_kicad::merge_project_design_rules(&mut model, &pro);
            // Board Setup's smaller pages: Formatting's footprint boxes, the Length-tuning Patterns, the Tuning Profiles and the profile each net class names.
            eda_kicad::merge_project_setup_pages(&mut model, &pro);
        }
        if let Ok(dru) = std::fs::read_to_string(pcb.with_extension("kicad_dru")) {
            eda_kicad::merge_custom_rules(&mut model, &dru);
        }
        let meta = ExportMeta { date: "2026-01-01", title: &name };
        let reexported = export_kicad_pcb(&design, &model, &meta).unwrap_or_else(|e| panic!("export_kicad_pcb: {e:?}"));
        let dir = work_dir(format);
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join(format!("{name}.kicad_pcb"));
        std::fs::write(&out_path, &reexported).unwrap();
        // The derived project kicad-cli reads next to the board, as the studio's engine writes it: our design
        // rules and the imported custom rules.
        std::fs::write(dir.join(format!("{name}.kicad_pro")), eda_kicad::export_kicad_pro_for(&design, &model)).unwrap();
        let dru = dir.join(format!("{name}.kicad_dru"));
        match model.board.custom_rules_text.as_deref() {
            Some(rules) => std::fs::write(&dru, rules).unwrap(),
            None => {
                let _ = std::fs::remove_file(&dru);
            }
        }
        let reexport_report = kicad_drc_report(cli, &out_path, "reexport");
        (original_report, reexport_report)
    }));
    match result {
        Ok((original, reexport)) => {
            let (original_parses, reexport_parses) = (original.is_some(), reexport.is_some());
            let (original_types, reexport_types) = (original.as_ref().map(type_counts).unwrap_or_default(), reexport.as_ref().map(type_counts).unwrap_or_default());
            let finding_differences = match (&original, &reexport) {
                (Some(a), Some(b)) => Some(finding_differences(a, b, 0.005)),
                _ => None,
            };
            let identical = original_types == reexport_types;
            ReexportFidelity { board: name, error: None, original_parses, reexport_parses, original_types, reexport_types, identical_type_counts: identical, finding_differences }
        }
        Err(e) => ReexportFidelity { board: name, error: Some(e), original_parses: false, reexport_parses: false, original_types: BTreeMap::new(), reexport_types: BTreeMap::new(), identical_type_counts: false, finding_differences: None },
    }
}

/// The slow-tier proof of the re-export: every one of the 12 boards, as the corpus ships it and as the installed kicad-cli writes it in the newest format, is judged by
/// kicad-cli's DRC after the round trip as it is before it -- the same number of violations of each type, and the same findings (type, items, places to a few
/// micrometres). Needs kicad-cli and KiCad's QA boards; `EDA_SLOW_TESTS=1` runs it (`EDA_PARITY_DRC_TIMEOUT_SECS` bounds a run). A board that fails here has lost design data
/// in `import_kicad_pcb` or `export_kicad_pcb`: read the two reports with `EDA_PARITY_REEXPORT_ONLY=<board> EDA_PARITY_PARTS=reexport` and diff them.
#[test]
fn a_real_boards_reexport_is_judged_by_kicad_cli_as_the_original_is() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let (Some(cli), Some(root)) = (find_kicad_cli(), qa_root()) else {
        eprintln!("skipped: needs kicad-cli (9 or later) and KiCad's QA boards");
        return;
    };
    let mut failures = Vec::new();
    for format in [Format::Shipped, Format::Upgraded] {
        for pcb in select_subset(&root, 12) {
            let r = reexport_fidelity(&cli, &pcb, format);
            let ok = r.error.is_none() && r.original_parses && r.reexport_parses && r.identical_type_counts && r.finding_differences == Some(0);
            eprintln!("{format:?} {}: {}", r.board, if ok { "same".to_string() } else { format!("{:?} {:?} {:?}", r.error, r.finding_differences, r.original_types.iter().filter(|(k, v)| r.reexport_types.get(*k) != Some(v)).collect::<Vec<_>>()) });
            if !ok {
                failures.push(format!("{format:?} {}: error {:?}, finding differences {:?}, original {:?}, re-export {:?}", r.board, r.error, r.finding_differences, r.original_types, r.reexport_types));
            }
        }
    }
    assert!(failures.is_empty(), "the re-export is judged differently from the original on:\n{}", failures.join("\n"));
}

#[test]
#[ignore]
fn parity_roundtrip_harness() {
    let repo = repo_root();

    let parts = std::env::var("EDA_PARITY_PARTS").unwrap_or_else(|_| "own,corpus,reexport".into());
    let wants = |part: &str| parts.split(',').any(|p| p.trim() == part);

    // ---- Part 2: no kicad-cli needed ----
    let mut own: Vec<OwnRoundTrip> = Vec::new();
    let mut examples: Vec<PathBuf> = std::fs::read_dir(repo.join("examples")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")).collect();
    examples.extend(std::fs::read_dir(repo.join("examples/ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")));
    examples.sort();
    if wants("own") {
        println!("=== Part 2: own-pipeline round trip ({} boards) ===", examples.len());
    }
    for yaml in examples.iter().filter(|_| wants("own")) {
        let rt = own_round_trip(yaml);
        println!("  {}: {:?}", rt.board, rt.survival);
        if let Some(e) = &rt.error {
            println!("    ERROR: {e}");
        }
        own.push(rt);
    }

    // ---- Part 1: full corpus import notes, no kicad-cli needed ----
    let (mut pcb_stats, mut sch_stats) = (None, None);
    if let Some(root) = qa_root().filter(|_| wants("corpus")) {
        println!("\n=== Part 1: full-corpus import notes (root={}) ===", root.display());
        let p = pcb_corpus_notes(&root);
        println!("pcb: {p:?}");
        let s = sch_corpus_notes(&root);
        println!("sch: {s:?}");
        pcb_stats = Some(p);
        sch_stats = Some(s);
    } else if wants("corpus") {
        eprintln!("KiCad QA corpus not found; skipping Part 1 (full-corpus import notes) and Part 3 (re-export fidelity)");
    }

    // ---- Part 3: re-export fidelity subset, needs kicad-cli ----
    let mut reexport: Vec<ReexportFidelity> = Vec::new();
    if let (Some(cli), Some(root)) = (find_kicad_cli().filter(|_| wants("reexport")), qa_root()) {
        let cap = std::env::var("EDA_PARITY_REEXPORT_CAP").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
        let format = if std::env::var_os("EDA_PARITY_REEXPORT_UPGRADE").is_some() { Format::Upgraded } else { Format::Shipped };
        let mut subset = select_subset(&root, cap);
        // A run on some of the boards only (a comma list of file names without `.kicad_pcb`): for working on one difference; it leaves `roundtrip.json` alone.
        let only: Vec<String> = std::env::var("EDA_PARITY_REEXPORT_ONLY").ok().map(|s| s.split(',').map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).collect()).unwrap_or_default();
        if !only.is_empty() {
            subset.retain(|p| p.file_stem().and_then(|s| s.to_str()).is_some_and(|n| only.iter().any(|o| o == n)));
        }
        println!("\n=== Part 3: re-export fidelity on {} boards ===", subset.len());
        for pcb in &subset {
            let r = reexport_fidelity(&cli, pcb, format);
            println!("  {}: orig_parses={} reexport_parses={} identical_type_counts={} finding_differences={:?}", r.board, r.original_parses, r.reexport_parses, r.identical_type_counts, r.finding_differences);
            reexport.push(r);
        }
    } else if wants("reexport") {
        eprintln!("kicad-cli and/or QA corpus not found; skipping Part 3 (re-export fidelity)");
    }

    #[derive(serde::Serialize)]
    struct Report {
        own_pipeline_roundtrip: Vec<OwnRoundTrip>,
        qa_corpus_pcb_import_notes: Option<PcbCorpusStats>,
        qa_corpus_sch_import_notes: Option<SchCorpusStats>,
        reexport_fidelity_subset: Vec<ReexportFidelity>,
    }
    if std::env::var("EDA_PARITY_REEXPORT_ONLY").is_ok_and(|s| !s.trim().is_empty()) || std::env::var_os("EDA_PARITY_REEXPORT_UPGRADE").is_some() {
        println!("\nEDA_PARITY_REEXPORT_ONLY or EDA_PARITY_REEXPORT_UPGRADE is set: a partial run, {} left alone", repo.join("docs/parity/raw/roundtrip.json").display());
        return;
    }
    let report = Report { own_pipeline_roundtrip: own, qa_corpus_pcb_import_notes: pcb_stats, qa_corpus_sch_import_notes: sch_stats, reexport_fidelity_subset: reexport };
    let out_dir = repo.join("docs/parity/raw");
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut json = serde_json::to_value(&report).unwrap();
    // A partial run keeps what the parts it did not run measured last time.
    let old = std::fs::read_to_string(out_dir.join("roundtrip.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    if let Some(old) = &old {
        for (part, keys) in [("own", &["own_pipeline_roundtrip"][..]), ("corpus", &["qa_corpus_pcb_import_notes", "qa_corpus_sch_import_notes"][..]), ("reexport", &["reexport_fidelity_subset"][..])] {
            if !wants(part) {
                for k in keys {
                    if let Some(v) = old.get(*k) {
                        json[*k] = v.clone();
                    }
                }
            }
        }
    }
    // The day each part was last measured: a partial run keeps the dates of the parts it kept, so the report can say which numbers are old.
    let today = Command::new("date").args(["-u", "+%Y-%m-%d"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let mut measured = serde_json::Map::new();
    for part in ["own", "corpus", "reexport"] {
        if wants(part) {
            measured.insert(part.into(), today.clone().into());
        } else if let Some(v) = old.as_ref().and_then(|o| o.get("measured_at")).and_then(|m| m.get(part)) {
            measured.insert(part.into(), v.clone());
        }
    }
    json["measured_at"] = measured.into();
    std::fs::write(out_dir.join("roundtrip.json"), serde_json::to_string_pretty(&json).unwrap()).unwrap();
    println!("\nwrote {}", out_dir.join("roundtrip.json").display());
}
