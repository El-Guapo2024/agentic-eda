//! Parity harness, section 2 of the task's measurement plan: `check_erc` vs
//! real `kicad-cli sch erc`, per violation type, across:
//!   - every `examples/*.yaml` + `examples/ladder/*.yaml` board, derived
//!     into a schematic and exported the same way `eda pipeline` does
//!     (library symbols resolved first, exactly like `kicad_cli_erc.rs`'s
//!     `export_example` helper, which this mirrors);
//!   - `work/mcu30` and `work/l1-order`'s already-exported `.kicad_sch`;
//!   - every `.kicad_sch` in KiCad's own QA board corpus (33 files, small
//!     enough to run in full rather than sample), read through
//!     `import_kicad_sch` -- exactly the path a user's own schematic would
//!     take. Import failures are recorded as gaps, not silently dropped.
//!
//! Counts only, per type (the task's wording for this section doesn't ask
//! for position-level matching the way the DRC section does, and
//! `CheckResult` carries no structured position to match on anyway).
//!
//! Writes `docs/parity/raw/erc.json`; `tools/parity_report.py` folds every
//! `raw/*.json` into `docs/parity/REPORT.md` + `docs/parity/scores.json`.
//! `#[ignore]`d -- the one command:
//!   cargo test -p eda-kicad --test parity_erc -- --ignored --nocapture
//! Skips cleanly without kicad-cli; the QA-corpus portion additionally
//! skips cleanly if its directory isn't present (see `EDA_KICAD_QA_BOARDS`).

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{check_erc, export_kicad_sch, import_kicad_sch, import_kicad_sch_tree, ExportMeta};
use eda_model::ir::Design;
use eda_model::{CheckStatus, ConstraintModel};

fn find_kicad_cli() -> Option<PathBuf> {
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

fn find_qa_sch_boards(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "kicad_sch") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
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

#[derive(Default, Debug, Clone, serde::Serialize)]
struct TypeStat {
    kicad: usize,
    ours: usize,
    matched: usize,
    missing: usize,
    extra: usize,
}

fn kicad_counts_by_type(report: &serde_json::Value) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    if let Some(sheets) = report.get("sheets").and_then(|s| s.as_array()) {
        for sheet in sheets {
            if let Some(vs) = sheet.get("violations").and_then(|v| v.as_array()) {
                for v in vs {
                    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
                    *m.entry(ty).or_insert(0) += 1;
                }
            }
        }
    }
    m
}

fn our_counts_by_type(results: &[eda_model::CheckResult]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for r in results {
        if r.status == CheckStatus::Pass {
            continue;
        }
        *m.entry(r.check.clone()).or_insert(0) += 1;
    }
    m
}

/// `check_erc` emits two families under one `Vec<CheckResult>`: KiCad's own
/// electrical checks (ERC proper, `erc.rs`) and this project's own
/// readability/style checks folded in from the former
/// `eda-gates::check_schematic` (`erc_style.rs`, every `schematic_*`-named
/// check -- see `crates/kicad/tests/erc_check_registry.rs`'s own doc
/// comment). The style family has no KiCad counterpart *by design*: it's
/// never meant to match `kicad-cli sch erc`, so counting its firings as
/// "extra" (false positives) against KiCad would just be measurement
/// noise, not a real precision number. Excluded from the KiCad-comparison
/// set; reported separately (`style_only` below) because it's still a
/// real, meaningful count of this project's own schematic-quality checks.
const STYLE_ONLY_PREFIX: &str = "schematic_";

fn split_style_only(counts: BTreeMap<String, usize>) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
    let (mut electrical, mut style_only) = (BTreeMap::new(), BTreeMap::new());
    for (ty, n) in counts {
        if ty.starts_with(STYLE_ONLY_PREFIX) {
            style_only.insert(ty, n);
        } else {
            electrical.insert(ty, n);
        }
    }
    (electrical, style_only)
}

fn compare(kicad_report: &serde_json::Value, ours: &[eda_model::CheckResult]) -> (BTreeMap<String, TypeStat>, BTreeMap<String, usize>) {
    let k = kicad_counts_by_type(kicad_report);
    let (o, style_only) = split_style_only(our_counts_by_type(ours));
    let mut types = BTreeMap::new();
    for ty in k.keys().chain(o.keys()).cloned().collect::<std::collections::BTreeSet<_>>() {
        let kc = k.get(&ty).copied().unwrap_or(0);
        let oc = o.get(&ty).copied().unwrap_or(0);
        let matched = kc.min(oc);
        types.insert(ty, TypeStat { kicad: kc, ours: oc, matched, missing: kc - matched, extra: oc - matched });
    }
    (types, style_only)
}

fn run_kicad_erc(cli: &Path, sch: &Path) -> serde_json::Value {
    let report_path = sch.with_extension("erc.json");
    let out = Command::new(cli).args(["sch", "erc", "--format", "json", "--severity-all", "--output"]).arg(&report_path).arg(sch).output().expect("failed to run kicad-cli sch erc");
    if !report_path.exists() {
        eprintln!("kicad-cli produced no ERC report for {}: stdout={} stderr={}", sch.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        return serde_json::json!({});
    }
    serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap_or_default()).unwrap_or_default()
}

#[derive(Debug, Clone, serde::Serialize)]
struct BoardResult {
    board: String,
    source: &'static str,
    error: Option<String>,
    types: BTreeMap<String, TypeStat>,
    style_only: BTreeMap<String, usize>,
}

fn example_boards(repo: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(repo.join("examples")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")).collect();
    v.extend(std::fs::read_dir(repo.join("examples/ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")));
    v.sort();
    v
}

/// Mirrors `kicad_cli_erc.rs`'s `export_example`: real installed KiCad
/// symbol libraries resolved first, falling back to a synthesized generic
/// symbol -- the same path `eda board`/`eda schematic` take.
fn process_example(cli: &Path, yaml_path: &Path, source: &'static str) -> BoardResult {
    let name = yaml_path.file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    let result = catch(AssertUnwindSafe(|| {
        let text = std::fs::read_to_string(yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
        let mut model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", yaml_path.display()));
        let root = eda_kicad::default_symbol_library_root();
        for w in eda_kicad::resolve_library_symbols(&mut model, &root) {
            eprintln!("  symbol library: {w}");
        }
        let opts = EngineOptions { seed: 0, intent_hash: format!("parity_erc_{name}"), ..Default::default() };
        let design: Design = derive_schematic(&model, &opts).unwrap_or_else(|e| panic!("derive_schematic: {e:?}"));
        let meta = ExportMeta { date: "2026-01-01", title: &name };
        let text = export_kicad_sch(&design, &model, &meta).unwrap_or_else(|e| panic!("export_kicad_sch: {e:?}"));
        let dir = std::env::temp_dir().join("eda_parity_erc").join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        let sch_path = dir.join(format!("{name}.kicad_sch"));
        std::fs::write(&sch_path, &text).unwrap();

        let kicad_report = run_kicad_erc(cli, &sch_path);
        let ours = check_erc(&design, &model);
        compare(&kicad_report, &ours)
    }));
    match result {
        Ok((types, style_only)) => BoardResult { board: name, source, error: None, types, style_only },
        Err(e) => BoardResult { board: name, source, error: Some(e), types: BTreeMap::new(), style_only: BTreeMap::new() },
    }
}

fn process_prebuilt_sch(cli: &Path, name: &'static str, sch: &Path) -> BoardResult {
    let result = catch(AssertUnwindSafe(|| {
        let text = std::fs::read_to_string(sch).unwrap_or_else(|e| panic!("read {}: {e}", sch.display()));
        let (design, model, notes) = import_kicad_sch(&text).unwrap_or_else(|e| panic!("import_kicad_sch failed: {e:?}"));
        eprintln!("  import notes: {notes:?}");
        let kicad_report = run_kicad_erc(cli, sch);
        let ours = check_erc(&design, &model);
        compare(&kicad_report, &ours)
    }));
    match result {
        Ok((types, style_only)) => BoardResult { board: name.into(), source: "work", error: None, types, style_only },
        Err(e) => BoardResult { board: name.into(), source: "work", error: Some(e), types: BTreeMap::new(), style_only: BTreeMap::new() },
    }
}

fn process_qa_sch(cli: &Path, sch: &Path) -> BoardResult {
    let name = sch.strip_prefix(sch.ancestors().nth(2).unwrap_or(sch)).unwrap_or(sch).display().to_string();
    let result = catch(AssertUnwindSafe(|| {
        // GAPS.md #6/#20: `import_kicad_sch_tree` (not bare `import_kicad_sch`)
        // so a root sheet with its own `(sheet ...)` placements actually
        // descends into them -- a no-op for every leaf-only board (the
        // overwhelming majority of the corpus), since a board with no
        // sheets of its own tree-walks to nothing beyond itself either way.
        // `check_erc` itself flattens the result before judging it (see
        // `check_erc`'s own doc), so this one swap is the whole change
        // needed for the QA-corpus side of this measurement to see real
        // multi-sheet connectivity instead of a single, un-descended sheet.
        let (design, model, _notes) = import_kicad_sch_tree(sch).unwrap_or_else(|e| panic!("import_kicad_sch_tree failed: {e:?}"));
        let kicad_report = run_kicad_erc(cli, sch);
        let ours = check_erc(&design, &model);
        compare(&kicad_report, &ours)
    }));
    match result {
        Ok((types, style_only)) => BoardResult { board: name, source: "qa", error: None, types, style_only },
        Err(e) => BoardResult { board: name, source: "qa", error: Some(e), types: BTreeMap::new(), style_only: BTreeMap::new() },
    }
}

#[test]
#[ignore]
fn parity_erc_harness() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping parity_erc_harness");
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
    let mcu30_sch = work_root.join("mcu30/export/mcu_board_30plus.kicad_sch");
    if mcu30_sch.exists() {
        println!("== work: mcu30 ==");
        boards.push(process_prebuilt_sch(&cli, "work/mcu30", &mcu30_sch));
    }
    let l1_sch = work_root.join("l1-order/_pipeline/l1_usb_mcu.kicad_sch");
    if l1_sch.exists() {
        println!("== work: l1-order ==");
        boards.push(process_prebuilt_sch(&cli, "work/l1-order", &l1_sch));
    }

    if let Some(root) = qa_root() {
        let qa_sample = find_qa_sch_boards(&root);
        println!("\nQA corpus schematics ({} files, root={}):", qa_sample.len(), root.display());
        for p in &qa_sample {
            println!("  {}", p.display());
        }
        for sch in &qa_sample {
            boards.push(process_qa_sch(&cli, sch));
        }
    } else {
        eprintln!("KiCad QA corpus not found; skipping QA-corpus schematics");
    }

    let mut totals: BTreeMap<String, TypeStat> = BTreeMap::new();
    let mut style_only_totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut import_failures = Vec::new();
    for b in &boards {
        if let Some(e) = &b.error {
            import_failures.push(format!("{} [{}]: {e}", b.board, b.source));
            continue;
        }
        for (ty, s) in &b.types {
            let t = totals.entry(ty.clone()).or_default();
            t.kicad += s.kicad;
            t.ours += s.ours;
            t.matched += s.matched;
            t.missing += s.missing;
            t.extra += s.extra;
        }
        for (ty, n) in &b.style_only {
            *style_only_totals.entry(ty.clone()).or_default() += n;
        }
    }
    println!("\n=== ERC parity totals by type ===");
    let (mut sum_matched, mut sum_kicad, mut sum_ours) = (0usize, 0usize, 0usize);
    for (ty, s) in &totals {
        println!("  {ty:28} kicad={:<5} ours={:<5} matched={:<5} missing={:<5} extra={:<5}", s.kicad, s.ours, s.matched, s.missing, s.extra);
        sum_matched += s.matched;
        sum_kicad += s.kicad;
        sum_ours += s.ours;
    }
    let precision = if sum_ours > 0 { sum_matched as f64 / sum_ours as f64 } else { 1.0 };
    let recall = if sum_kicad > 0 { sum_matched as f64 / sum_kicad as f64 } else { 1.0 };
    println!("\noverall: matched={sum_matched} kicad_total={sum_kicad} our_total={sum_ours} precision={precision:.3} recall={recall:.3}");
    println!("\n(style-only checks, no KiCad counterpart by design, excluded from the above -- {} occurrences total):", style_only_totals.values().sum::<usize>());
    for (ty, n) in &style_only_totals {
        println!("  {ty:28} {n}");
    }
    if !import_failures.is_empty() {
        println!("\nboards that errored (gaps, not folded into totals):");
        for f in &import_failures {
            println!("  {f}");
        }
    }

    #[derive(serde::Serialize)]
    struct Report<'a> {
        boards: &'a [BoardResult],
        totals: &'a BTreeMap<String, TypeStat>,
        style_only_totals: &'a BTreeMap<String, usize>,
        precision: f64,
        recall: f64,
        import_failures: &'a [String],
    }
    let report = Report { boards: &boards, totals: &totals, style_only_totals: &style_only_totals, precision, recall, import_failures: &import_failures };
    let out_dir = repo.join("docs/parity/raw");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("erc.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("\nwrote {}", out_dir.join("erc.json").display());

    assert!(!boards.is_empty(), "parity harness processed zero boards -- that's a harness bug, not a measurement");
}
