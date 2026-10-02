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
    non_rect_pad_shapes_approximated: usize,
    outline_open: usize,
    outline_source_counts: BTreeMap<String, usize>,
    failures: Vec<String>,
}

fn pcb_corpus_notes(root: &Path) -> PcbCorpusStats {
    let mut s = PcbCorpusStats::default();
    for pcb in find_all(root, "kicad_pcb") {
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
                s.non_rect_pad_shapes_approximated += notes.non_rect_pad_shapes_approximated;
                if notes.outline_open {
                    s.outline_open += 1;
                }
                *s.outline_source_counts.entry(notes.outline_source.to_string()).or_default() += 1;
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
    for sch in find_all(root, "kicad_sch") {
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

type Segment = (String, String, i64, ((i64, i64), (i64, i64)));
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
fn via_set(vias: &[Via]) -> BTreeSet<(String, i64, i64, i64, i64, String, String)> {
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

fn kicad_type_counts(cli: &Path, pcb: &Path, tag: &str) -> (BTreeMap<String, usize>, bool) {
    let report_path = pcb.with_file_name(format!("{}.{tag}.drc.json", pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("b")));
    let out = Command::new(cli)
        .args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--output"])
        .arg(&report_path)
        .arg(pcb)
        .output()
        .expect("run kicad-cli");
    if !report_path.exists() {
        eprintln!("kicad-cli failed to load {} ({}): stdout={} stderr={}", pcb.display(), tag, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        return (BTreeMap::new(), false);
    }
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap_or_default()).unwrap_or_default();
    let mut m = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            *m.entry(ty).or_insert(0) += 1;
        }
    }
    (m, true)
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
}

fn reexport_fidelity(cli: &Path, pcb: &Path) -> ReexportFidelity {
    let name = pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let result = catch(AssertUnwindSafe(|| {
        let (original_types, original_parses) = kicad_type_counts(cli, pcb, "orig");
        let text = std::fs::read_to_string(pcb).unwrap_or_else(|e| panic!("read: {e}"));
        let (design, model, _notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import_kicad_pcb: {e:?}"));
        let meta = ExportMeta { date: "2026-01-01", title: &name };
        let reexported = export_kicad_pcb(&design, &model, &meta).unwrap_or_else(|e| panic!("export_kicad_pcb: {e:?}"));
        let dir = std::env::temp_dir().join("eda_parity_reexport");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join(format!("{name}.kicad_pcb"));
        std::fs::write(&out_path, &reexported).unwrap();
        let (reexport_types, reexport_parses) = kicad_type_counts(cli, &out_path, "reexport");
        (original_types, original_parses, reexport_types, reexport_parses)
    }));
    match result {
        Ok((original_types, original_parses, reexport_types, reexport_parses)) => {
            let identical = original_types == reexport_types;
            ReexportFidelity { board: name, error: None, original_parses, reexport_parses, original_types, reexport_types, identical_type_counts: identical }
        }
        Err(e) => ReexportFidelity { board: name, error: Some(e), original_parses: false, reexport_parses: false, original_types: BTreeMap::new(), reexport_types: BTreeMap::new(), identical_type_counts: false },
    }
}

#[test]
#[ignore]
fn parity_roundtrip_harness() {
    let repo = repo_root();

    // ---- Part 2: always runs, no kicad-cli needed ----
    let mut own: Vec<OwnRoundTrip> = Vec::new();
    let mut examples: Vec<PathBuf> = std::fs::read_dir(repo.join("examples")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")).collect();
    examples.extend(std::fs::read_dir(repo.join("examples/ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")));
    examples.sort();
    println!("=== Part 2: own-pipeline round trip ({} boards) ===", examples.len());
    for yaml in &examples {
        let rt = own_round_trip(yaml);
        println!("  {}: {:?}", rt.board, rt.survival);
        if let Some(e) = &rt.error {
            println!("    ERROR: {e}");
        }
        own.push(rt);
    }

    // ---- Part 1: full corpus import notes, no kicad-cli needed ----
    let (mut pcb_stats, mut sch_stats) = (None, None);
    if let Some(root) = qa_root() {
        println!("\n=== Part 1: full-corpus import notes (root={}) ===", root.display());
        let p = pcb_corpus_notes(&root);
        println!("pcb: {p:?}");
        let s = sch_corpus_notes(&root);
        println!("sch: {s:?}");
        pcb_stats = Some(p);
        sch_stats = Some(s);
    } else {
        eprintln!("KiCad QA corpus not found; skipping Part 1 (full-corpus import notes) and Part 3 (re-export fidelity)");
    }

    // ---- Part 3: re-export fidelity subset, needs kicad-cli ----
    let mut reexport: Vec<ReexportFidelity> = Vec::new();
    if let (Some(cli), Some(root)) = (find_kicad_cli(), qa_root()) {
        let subset = select_subset(&root, 12);
        println!("\n=== Part 3: re-export fidelity on {} boards ===", subset.len());
        for pcb in &subset {
            let r = reexport_fidelity(&cli, pcb);
            println!("  {}: orig_parses={} reexport_parses={} identical_type_counts={}", r.board, r.original_parses, r.reexport_parses, r.identical_type_counts);
            reexport.push(r);
        }
    } else {
        eprintln!("kicad-cli and/or QA corpus not found; skipping Part 3 (re-export fidelity)");
    }

    #[derive(serde::Serialize)]
    struct Report {
        own_pipeline_roundtrip: Vec<OwnRoundTrip>,
        qa_corpus_pcb_import_notes: Option<PcbCorpusStats>,
        qa_corpus_sch_import_notes: Option<SchCorpusStats>,
        reexport_fidelity_subset: Vec<ReexportFidelity>,
    }
    let report = Report { own_pipeline_roundtrip: own, qa_corpus_pcb_import_notes: pcb_stats, qa_corpus_sch_import_notes: sch_stats, reexport_fidelity_subset: reexport };
    let out_dir = repo.join("docs/parity/raw");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("roundtrip.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("\nwrote {}", out_dir.join("roundtrip.json").display());
}
