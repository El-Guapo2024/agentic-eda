//! Parity harness, section 1 of the task's measurement plan: `eda_drc::run`
//! vs real `kicad-cli pcb drc`, per violation type, across three board
//! sources:
//!   - every `examples/*.yaml` + `examples/ladder/*.yaml` board, routed
//!     through our own schematic -> place -> route pipeline (the same
//!     stages `eda pipeline <yaml> -o <tmp> --seed 0` runs);
//!   - `work/mcu30` and `work/l1-order`'s already-exported boards (copied
//!     read-only into this worktree's own `work/` -- see the task's
//!     instruction to copy them first), using their pre-existing
//!     `design.json` + the intent YAML their `board.json`/`result.json`
//!     point back to;
//!   - a deterministic sample of KiCad's own QA regression-board corpus
//!     (real KiCad-authored `.kicad_pcb` files), read through
//!     `import_kicad_pcb` -- exactly the path a user's own board would take.
//!
//! "Matched" means same violation type *and* the two violations' primary
//! item position agrees within [`POS_TOLERANCE_UM`] -- not just equal
//! per-type counts, which would hide e.g. two same-count-but-different-spot
//! violations. `unconnected_items` is deliberately excluded from this
//! file's comparison: `eda_drc::run` has no provider for it (that's
//! `eda_connectivity`'s job, measured separately in
//! `crates/connectivity/tests/parity_connectivity.rs`), so counting it here
//! would just show a permanent, uninformative 100% "missing".
//!
//! **Harness independence from `crates/freeroute`** (task item 1): an
//! `examples/*.yaml` board's `place`+`route_best_effort` output is cached
//! to disk under `target/parity_route_cache/` (gitignored, like every
//! other build artifact), keyed by the board's intent YAML content, seed,
//! and a cache-schema version -- see [`cached_place_and_route`]. The first
//! run pays the router's real cost per board (GAPS.md #2 measured
//! `l4_control_hub` at 1850s+); every run after that is a cache hit and
//! costs nothing. This is what lets `drc_boards_evaluated` be a ratcheted
//! metric again instead of hostage to autorouter performance: the
//! per-board watchdog ([`BOARD_TIMEOUT`]) now only has to cover
//! `eda_drc::run` + kicad-cli, not schematic/place/route.
//!
//! Writes `docs/parity/raw/drc.json`; `tools/parity_report.py` folds every
//! `raw/*.json` into `docs/parity/REPORT.md` + `docs/parity/scores.json`.
//! `#[ignore]`d like this workspace's other `kicad_cli_*` oracle tests --
//! the one command:
//!   cargo test -p eda-drc --test parity_drc -- --ignored --nocapture
//! Skips cleanly (prints and returns) if kicad-cli isn't found. The QA
//! corpus sample additionally skips cleanly if its directory (set via the
//! `EDA_KICAD_QA_BOARDS` env var, or a session-scratchpad default that will
//! not exist on another machine) isn't present -- the examples/ladder/work
//! boards still run either way.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_drc::DrcViolation;
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
    manifest_dir.parent().and_then(|p| p.parent()).expect("crates/drc -> repo root").to_path_buf()
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
    let default = PathBuf::from(QA_ROOT_DEFAULT);
    default.exists().then_some(default)
}

/// Deterministic sample, recorded in full by this test's own stdout: every
/// top-level named `*.kicad_pcb` directly under `pcbnew/` (the descriptively
/// named regression boards -- zone_filler, padstacks, via_dangling, ...),
/// interleaved with one board (first, sorted) from every `issue*/`
/// subdirectory, sorted, capped at `cap` total.
fn select_qa_pcb_boards(root: &Path, cap: usize) -> Vec<PathBuf> {
    let pcbnew = root.join("pcbnew");
    let mut named: Vec<PathBuf> = std::fs::read_dir(&pcbnew)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb"))
        .collect();
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

/// Matching tolerance between our `DrcRefItem.pos` (exact micrometres) and
/// kicad-cli's JSON `pos` (millimetres, printed with limited precision):
/// generous enough to absorb that round trip and any benign "which point on
/// the item" convention difference, tight enough that it can't cross-match
/// two genuinely different violations on a crowded board (board features
/// here run from hundreds of micrometres to millimetres apart).
const POS_TOLERANCE_UM: i64 = 50;

/// `kicad-cli`'s DRC type keys this crate's providers have no counterpart
/// for at all -- see `eda_drc::ErrorType`. Kept here (rather than silently
/// folded into "missing" noise on every board) so the report can call out
/// "never implemented" separately from "implemented but under-matching".
const NOT_IN_DRC_SCOPE: &[&str] = &["unconnected_items"];

/// `eda_drc::run` always appends `providers::placement_quality::check` --
/// this project's own proximity/decoupling/board-use/etc. providers with
/// *no* KiCad equivalent (see `eda_drc::item::ErrorType`'s own doc comment:
/// "no KiCad equivalent, ported from eda_gates::pcb") -- plus a netclass
/// track-width conformance check kept under gates' old name. Counting
/// either family as "extra" against kicad-cli would be measurement noise,
/// not a real false positive, exactly like `parity_erc.rs`'s `schematic_*`
/// exclusion. Excluded from the KiCad-comparison set; reported separately.
const OUR_NON_KICAD_TYPES: &[&str] = &[
    "placement_proximity",
    "placement_decoupling",
    "placement_stub_crossings",
    "placement_board_use",
    "placement_net_compactness",
    "placement_edge_connector",
    "placement_refdes_clear",
    "routing_track_width",
];

#[derive(Default, Debug, Clone, serde::Serialize)]
struct TypeStat {
    kicad: usize,
    ours: usize,
    matched: usize,
    missing: usize,
    extra: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct BoardResult {
    board: String,
    source: &'static str,
    error: Option<String>,
    types: BTreeMap<String, TypeStat>,
    non_kicad_counts: BTreeMap<String, usize>,
}

fn catch<F: FnOnce() -> R + panic::UnwindSafe, R>(f: F) -> Result<R, String> {
    panic::catch_unwind(f).map_err(|e| {
        if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic (non-string payload)".to_string()
        }
    })
}

/// Per-board time budget for the part of this harness that's actually
/// under measurement: export + kicad-cli + `eda_drc::run`. Added after a
/// real hang during this harness's own development run: one KiCad
/// QA-corpus board drove `eda_drc::run` into what looked like
/// quadratic-or-worse behavior on real geometry -- 95% CPU, *no* kicad-cli
/// child process (so the hang is inside our own Rust code, not the
/// oracle), running for 35+ minutes before the orchestrator flagged it.
/// Every board now runs under this watchdog instead.
///
/// This budget deliberately does **not** cover schematic/place/route for an
/// `examples/*.yaml` board any more (see [`cached_place_and_route`] and
/// [`ROUTE_TIMEOUT`]) -- GAPS.md #2's follow-up measured `crates/freeroute`
/// alone taking 1850s+ on `l4_control_hub`, which no sane per-board DRC
/// budget can absorb, and a 60s-or-die watchdog around the *whole* pipeline
/// made `drc_boards_evaluated` a referendum on the autorouter's performance
/// instead of this crate's DRC parity. Splitting the budgets is the actual
/// fix (harness independence, task item 1); this one stays tight because
/// `eda_drc::run` + kicad-cli genuinely are fast once routing is out of the
/// way (milliseconds to low seconds -- see `crates/drc/tests/perf_profile.rs`).
const BOARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Runs `f` on a background thread and waits up to `timeout`. A call that
/// blows its budget is reported as a timeout (named in
/// `docs/parity/GAPS.md`/REPORT.md as a gap, same as any other per-board
/// error) and abandoned -- Rust has no portable "kill this thread", so the
/// hung thread is leaked, but since the whole test *process* exits the
/// moment `parity_drc_harness` returns, the leak costs nothing beyond that
/// thread's own memory for the rest of this one run.
fn with_timeout<T: Send + 'static>(timeout: std::time::Duration, f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout).map_err(|_| format!("TIMEOUT after {}s -- likely a hang or quadratic-plus blowup in our own engine on this board's geometry, not kicad-cli (no oracle child process was observed during the hang this was discovered from)", timeout.as_secs()))
}

/// Schema/format version for [`route_cache_dir`]'s cache entries -- bump by
/// hand if a change here (or to `route_best_effort`/`place`'s call shape)
/// should invalidate every entry a prior run wrote, rather than silently
/// reusing a stale routed board that no longer reflects today's pipeline.
const ROUTE_CACHE_SCHEMA: u32 = 1;

/// Generous, one-time-cost budget for the part of the pipeline this harness
/// must still pay for on a cache miss: `place` + `route_best_effort`.
/// Separate from [`BOARD_TIMEOUT`] on purpose -- see that constant's doc
/// comment. GAPS.md #2 measured `l4_control_hub` taking 1850s+ end to end
/// (and still 30+ minutes under a *quiet* machine, ruling out contention as
/// the main cause), so this has to be large enough to let a cold cache
/// entry actually finish at least once; a board that still can't route
/// inside this budget is reported as a genuine per-board error (same as
/// any other), not silently dropped.
const ROUTE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45 * 60);

fn route_cache_dir(repo: &Path) -> PathBuf {
    // `target/` is gitignored workspace-wide already, so this needs no new
    // `.gitignore` entry and no cleanup step: `cargo clean` (or just
    // deleting the directory) costs one slow re-route per board next run,
    // never a correctness difference -- see task item 1's "gitignored or
    // scratch cache" instruction.
    repo.join("target/parity_route_cache")
}

/// Cache key: the board's own name (for a legible filename) plus a hash of
/// everything that can change what a route *should* look like -- the
/// intent YAML's own bytes (so editing an example invalidates its cache
/// entry automatically, no manual bump needed), the seed, and
/// [`ROUTE_CACHE_SCHEMA`]. This is a cache-invalidation key, not a security
/// boundary, so the stdlib's non-cryptographic hasher is the right tool --
/// no new dependency for it.
fn route_cache_key(name: &str, yaml_text: &str, seed: u64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ROUTE_CACHE_SCHEMA.hash(&mut h);
    seed.hash(&mut h);
    yaml_text.hash(&mut h);
    format!("{name}-{:016x}", h.finish())
}

/// `place` then `route_best_effort`, the two pipeline stages whose cost
/// depends on this workspace's own placer/router rather than anything
/// `eda_drc` is being measured on (see [`BOARD_TIMEOUT`]'s doc comment) --
/// cached to disk keyed by [`route_cache_key`] so a repeat run (the normal
/// case once a board's entry exists) pays none of that cost. A cache miss
/// still runs under [`ROUTE_TIMEOUT`] and is cached on success; a
/// placement/routing panic (e.g. `unroutable_tiny_outline`'s intentional
/// failure) propagates uncaught here, exactly as it did before this
/// existed, for the caller's own `catch` to turn into a per-board error.
fn cached_place_and_route(repo: &Path, name: &str, yaml_text: &str, seed: u64, design: &Design, model: &ConstraintModel) -> Design {
    let path = route_cache_dir(repo).join(format!("{}.json", route_cache_key(name, yaml_text, seed)));
    if let Ok(bytes) = std::fs::read(&path) {
        match serde_json::from_slice::<Design>(&bytes) {
            Ok(cached) => return cached,
            Err(e) => eprintln!("  (route cache entry at {} unreadable ({e}); re-routing)", path.display()),
        }
    }

    let (design, model) = (design.clone(), model.clone());
    let routed = with_timeout(ROUTE_TIMEOUT, move || {
        let placed = place(&design, &model, &PlaceOptions { seed, ..Default::default() }).unwrap_or_else(|e| panic!("place: {e:?}"));
        route_best_effort(&placed, &model)
    })
    .unwrap_or_else(|e| panic!("place/route: {e}"));

    if let Err(e) = std::fs::create_dir_all(path.parent().expect("cache path has a parent")) {
        eprintln!("  (could not create route cache dir: {e}; will re-route every run)");
    } else {
        match routed.canonical_bytes() {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    eprintln!("  (could not write route cache entry: {e}; will re-route every run)");
                }
            }
            Err(e) => eprintln!("  (could not encode routed board for caching: {e:?}; will re-route every run)"),
        }
    }
    routed
}

fn kicad_positions_by_type(report: &serde_json::Value) -> BTreeMap<String, Vec<(i64, i64)>> {
    let mut out: BTreeMap<String, Vec<(i64, i64)>> = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            if NOT_IN_DRC_SCOPE.contains(&ty.as_str()) {
                continue;
            }
            let pos = v
                .get("items")
                .and_then(|i| i.as_array())
                .and_then(|a| a.first())
                .and_then(|it| it.get("pos"))
                .and_then(|p| Some((p.get("x")?.as_f64()?, p.get("y")?.as_f64()?)));
            let um = pos.map(|(x, y)| ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)).unwrap_or((0, 0));
            out.entry(ty).or_default().push(um);
        }
    }
    out
}

fn our_positions_by_type(violations: &[DrcViolation]) -> BTreeMap<String, Vec<(i64, i64)>> {
    let mut out: BTreeMap<String, Vec<(i64, i64)>> = BTreeMap::new();
    for v in violations {
        if OUR_NON_KICAD_TYPES.contains(&v.error_type) {
            continue;
        }
        let pos = v.items.first().map(|it| it.pos).unwrap_or((0, 0));
        out.entry(v.error_type.to_string()).or_default().push(pos);
    }
    out
}

fn our_non_kicad_counts(violations: &[DrcViolation]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for v in violations {
        if OUR_NON_KICAD_TYPES.contains(&v.error_type) {
            *out.entry(v.error_type.to_string()).or_insert(0) += 1;
        }
    }
    out
}

/// Greedy nearest-neighbour matching within [`POS_TOLERANCE_UM`]; returns
/// (matched, missing, extra). Good enough at the per-board, per-type
/// cardinalities DRC reports run at (tens, not thousands).
fn match_positions(kicad: &[(i64, i64)], ours: &[(i64, i64)]) -> (usize, usize, usize) {
    let mut used = vec![false; ours.len()];
    let mut matched = 0usize;
    for k in kicad {
        let mut best: Option<(usize, i64)> = None;
        for (i, o) in ours.iter().enumerate() {
            if used[i] {
                continue;
            }
            let d2 = (k.0 - o.0).pow(2) + (k.1 - o.1).pow(2);
            let d = (d2 as f64).sqrt() as i64;
            if d <= POS_TOLERANCE_UM && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        if let Some((i, _)) = best {
            used[i] = true;
            matched += 1;
        }
    }
    (matched, kicad.len() - matched, ours.len() - matched)
}

fn compare(kicad_report: &serde_json::Value, ours: &[DrcViolation]) -> BTreeMap<String, TypeStat> {
    let kicad_by_type = kicad_positions_by_type(kicad_report);
    let our_by_type = our_positions_by_type(ours);
    let mut types: BTreeMap<String, TypeStat> = BTreeMap::new();
    for ty in kicad_by_type.keys().chain(our_by_type.keys()).cloned().collect::<std::collections::BTreeSet<_>>() {
        let k = kicad_by_type.get(&ty).map(|v| v.as_slice()).unwrap_or(&[]);
        let o = our_by_type.get(&ty).map(|v| v.as_slice()).unwrap_or(&[]);
        let (matched, missing, extra) = match_positions(k, o);
        types.insert(ty, TypeStat { kicad: k.len(), ours: o.len(), matched, missing, extra });
    }
    types
}

fn run_kicad_drc(cli: &Path, pcb: &Path, refill: bool) -> serde_json::Value {
    let stem = pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("board");
    let report_path = pcb.with_file_name(format!("{stem}.{}.drc.json", if refill { "refill" } else { "norefill" }));
    let mut args: Vec<String> = vec!["pcb".into(), "drc".into()];
    if refill {
        args.push("--refill-zones".into());
    }
    // `--schematic-parity` (task item 5 / GAPS.md #19): without this flag
    // kicad-cli never runs `duplicate_footprints`/`missing_footprint`/
    // `extra_footprint` at all, which would make every one of our own
    // `schematic_parity` provider's violations count as 100% false
    // positives regardless of whether they're actually right -- a board
    // with no project/schematic context (every synthetic example this
    // harness exports) is unaffected either way (kicad-cli has nothing to
    // cross-check against, so it reports nothing from this group with or
    // without the flag).
    args.push("--schematic-parity".into());
    args.extend(["--format".into(), "json".into(), "--severity-all".into(), "--output".into(), report_path.display().to_string(), pcb.display().to_string()]);
    let out = Command::new(cli).args(&args).output().expect("failed to run kicad-cli pcb drc");
    if !report_path.exists() {
        eprintln!("kicad-cli produced no DRC report for {}\nstdout:\n{}\nstderr:\n{}", pcb.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        return serde_json::json!({});
    }
    serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap_or_default()).unwrap_or_default()
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

fn example_boards(repo: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(repo.join("examples")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")).collect();
    v.extend(std::fs::read_dir(repo.join("examples/ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yaml")));
    v.sort();
    v
}

fn process_example(cli: &Path, repo: &Path, yaml_path: &Path, source: &'static str) -> BoardResult {
    let name = yaml_path.file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();

    // ---- phase 1: schematic -> place -> route, cached, its own budget ----
    // Not wrapped in `with_timeout(BOARD_TIMEOUT, ...)`: that watchdog is
    // for the fast stuff actually under measurement (see its doc comment).
    // `cached_place_and_route` applies its own, much larger, one-time-cost
    // timeout internally.
    let phase1 = catch(AssertUnwindSafe(|| -> (Design, ConstraintModel) {
        let text = std::fs::read_to_string(yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
        let mut model: ConstraintModel = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", yaml_path.display()));
        let fp_root = eda_kicad::default_footprint_library_root();
        for w in eda_kicad::resolve_library_footprints(&mut model, &fp_root) {
            eprintln!("  footprint library: {w}");
        }
        let opts = EngineOptions { seed: 0, intent_hash: format!("parity_drc_{name}"), ..Default::default() };
        let design = derive_schematic(&model, &opts).unwrap_or_else(|e| panic!("derive_schematic: {e:?}"));
        let routed = cached_place_and_route(repo, &name, &text, 0, &design, &model);
        (routed, model)
    }));
    let (routed, model) = match phase1 {
        Ok(v) => v,
        Err(e) => return BoardResult { board: name, source, error: Some(e), types: BTreeMap::new(), non_kicad_counts: BTreeMap::new() },
    };

    // ---- phase 2: export + kicad-cli + eda_drc::run -- the thing under
    // measurement, kept under the normal per-board watchdog. ----
    let (cli, name2) = (cli.to_path_buf(), name.clone());
    let result = with_timeout(BOARD_TIMEOUT, move || {
        catch(AssertUnwindSafe(|| {
            let meta = ExportMeta { date: "2026-01-01", title: &name2 };
            let pcb_text = export_kicad_pcb(&routed, &model, &meta).unwrap_or_else(|e| panic!("export_kicad_pcb: {e:?}"));
            let dir = std::env::temp_dir().join("eda_parity_drc").join(&name2);
            std::fs::create_dir_all(&dir).unwrap();
            let pcb_path = dir.join(format!("{name2}.kicad_pcb"));
            std::fs::write(&pcb_path, &pcb_text).unwrap();

            let kicad_report = run_kicad_drc(&cli, &pcb_path, true);
            let ours = eda_drc::run(&routed, &model);
            (compare(&kicad_report, &ours), our_non_kicad_counts(&ours))
        }))
    })
    .and_then(|r| r);
    match result {
        Ok((types, non_kicad_counts)) => BoardResult { board: name, source, error: None, types, non_kicad_counts },
        Err(e) => BoardResult { board: name, source, error: Some(e), types: BTreeMap::new(), non_kicad_counts: BTreeMap::new() },
    }
}

/// `work/mcu30` and `work/l1-order` (copied read-only into this worktree):
/// a pre-existing `design.json` (real geometry from a past run, not
/// necessarily today's seed-0 derive) paired with the intent YAML its
/// `board.json`/`result.json` points back to, and the already-exported
/// `.kicad_pcb` sitting next to it.
fn process_work_board(cli: &Path, repo: &Path, name: &'static str, design_json: &Path, intent_yaml: &Path, pcb: &Path) -> BoardResult {
    let _ = repo;
    let (cli, design_json, intent_yaml, pcb) = (cli.to_path_buf(), design_json.to_path_buf(), intent_yaml.to_path_buf(), pcb.to_path_buf());
    let result = with_timeout(BOARD_TIMEOUT, move || {
        catch(AssertUnwindSafe(|| {
            let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent_yaml).unwrap_or_else(|e| panic!("read {}: {e}", intent_yaml.display())))
                .unwrap_or_else(|e| panic!("parse {}: {e}", intent_yaml.display()));
            let design: Design = serde_json::from_str(&std::fs::read_to_string(&design_json).unwrap_or_else(|e| panic!("read {}: {e}", design_json.display())))
                .unwrap_or_else(|e| panic!("parse {}: {e}", design_json.display()));
            if !pcb.exists() {
                panic!("expected pre-exported board at {}", pcb.display());
            }
            let kicad_report = run_kicad_drc(&cli, &pcb, true);
            let ours = eda_drc::run(&design, &model);
            (compare(&kicad_report, &ours), our_non_kicad_counts(&ours))
        }))
    })
    .and_then(|r| r);
    match result {
        Ok((types, non_kicad_counts)) => BoardResult { board: name.into(), source: "work", error: None, types, non_kicad_counts },
        Err(e) => BoardResult { board: name.into(), source: "work", error: Some(e), types: BTreeMap::new(), non_kicad_counts: BTreeMap::new() },
    }
}

fn process_qa_board(cli: &Path, pcb: &Path) -> BoardResult {
    let name = pcb.strip_prefix(pcb.ancestors().nth(2).unwrap_or(pcb)).unwrap_or(pcb).display().to_string();
    let (cli, pcb) = (cli.to_path_buf(), pcb.to_path_buf());
    let result = with_timeout(BOARD_TIMEOUT, move || {
        catch(AssertUnwindSafe(|| {
            let text = std::fs::read_to_string(&pcb).unwrap_or_else(|e| panic!("read {}: {e}", pcb.display()));
            let (design, mut model, _notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import_kicad_pcb failed: {e:?}"));
            // Real net classes live in the sidecar `.kicad_pro` (KiCad
            // 7+), not the `.kicad_pcb` itself -- see `eda_kicad::
            // merge_project_net_classes`'s doc comment. A bare single-file
            // QA fixture with no project (common in this corpus) has none
            // to merge, which is a correct no-op, not a gap.
            let pro_path = pcb.with_extension("kicad_pro");
            if let Ok(pro_text) = std::fs::read_to_string(&pro_path) {
                eda_kicad::merge_project_net_classes(&mut model, &pro_text);
                eda_kicad::merge_project_rule_severities(&mut model, &pro_text);
                // Board-wide minimum constraints (`rules.min_*`, task item
                // 1) -- distinct from the net classes merged just above,
                // which only ever carry nominal/default values, never a DRC
                // minimum. See `eda_kicad::merge_project_design_rules`'s
                // doc comment (GAPS.md #10).
                eda_kicad::merge_project_design_rules(&mut model, &pro_text);
            }
            // `.kicad_dru` (task item 4) is its own sibling file, not a
            // `.kicad_pro` section.
            let dru_path = pcb.with_extension("kicad_dru");
            if let Ok(dru_text) = std::fs::read_to_string(&dru_path) {
                eda_kicad::merge_custom_rules(&mut model, &dru_text);
            }
            let kicad_report = run_kicad_drc(&cli, &pcb, true);
            let ours = eda_drc::run(&design, &model);
            (compare(&kicad_report, &ours), our_non_kicad_counts(&ours))
        }))
    })
    .and_then(|r| r);
    match result {
        Ok((types, non_kicad_counts)) => BoardResult { board: name, source: "qa", error: None, types, non_kicad_counts },
        Err(e) => BoardResult { board: name, source: "qa", error: Some(e), types: BTreeMap::new(), non_kicad_counts: BTreeMap::new() },
    }
}

#[derive(serde::Serialize)]
struct ZoneImpact {
    board: String,
    refill: BTreeMap<String, usize>,
    no_refill: BTreeMap<String, usize>,
}

fn counts_by_type(report: &serde_json::Value) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            *m.entry(ty).or_insert(0) += 1;
        }
    }
    if let Some(u) = report.get("unconnected_items").and_then(|v| v.as_array()) {
        *m.entry("unconnected_items".into()).or_insert(0) += u.len();
    }
    m
}

/// Zone fill is "not ported" (task item 5): we export a zone's outline but
/// never compute its fill polygon. This doesn't compare us against KiCad --
/// it compares KiCad against *itself*, with and without `--refill-zones`,
/// on boards that declare a pour, to measure how much the missing fill
/// would otherwise matter to DRC. `examples/ladder/l4_control_hub.yaml`
/// (routed through our pipeline, so zones are outline-only exactly like
/// every other board here) plus any zone-bearing QA boards in the sample.
fn zone_fill_impact(cli: &Path, repo: &Path, qa_sample: &[PathBuf]) -> Vec<ZoneImpact> {
    let mut out = Vec::new();
    let yaml_path = repo.join("examples/ladder/l4_control_hub.yaml");
    if let Ok(text) = std::fs::read_to_string(&yaml_path) {
        if let Ok(model) = serde_yaml::from_str::<ConstraintModel>(&text) {
            let opts = EngineOptions { seed: 0, intent_hash: "parity_zone_impact".into(), ..Default::default() };
            if let Ok(design) = derive_schematic(&model, &opts) {
                // Same cache as the main harness (task item 1): this
                // opt-in sidebar re-derives the identical board, so it
                // should never pay for its own cold route when the main
                // run already has (or will have) a cache entry for it.
                let routed = cached_place_and_route(repo, "l4_control_hub", &text, 0, &design, &model);
                let meta = ExportMeta { date: "2026-01-01", title: "l4_control_hub_zone" };
                if let Ok(pcb_text) = export_kicad_pcb(&routed, &model, &meta) {
                    let dir = std::env::temp_dir().join("eda_parity_drc_zone");
                    std::fs::create_dir_all(&dir).unwrap();
                    let pcb_path = dir.join("l4_control_hub.kicad_pcb");
                    std::fs::write(&pcb_path, &pcb_text).unwrap();
                    let refill = counts_by_type(&run_kicad_drc(cli, &pcb_path, true));
                    let no_refill = counts_by_type(&run_kicad_drc(cli, &pcb_path, false));
                    out.push(ZoneImpact { board: "l4_control_hub (ours, routed)".into(), refill, no_refill });
                }
            }
        }
    }
    // Any QA boards whose filename suggests a zone/pour under test, already
    // in the sample -- no extra kicad-cli calls beyond the refill/no-refill
    // pair for boards we're measuring DRC on anyway.
    for pcb in qa_sample {
        let stem = pcb.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.contains("zone") || stem.contains("thermal") || stem.contains("hatch") {
            let refill = counts_by_type(&run_kicad_drc(cli, pcb, true));
            let no_refill = counts_by_type(&run_kicad_drc(cli, pcb, false));
            out.push(ZoneImpact { board: format!("{stem} (QA corpus, real)"), refill, no_refill });
        }
    }
    out
}

#[test]
#[ignore]
fn parity_drc_harness() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping parity_drc_harness");
        return;
    };
    let repo = repo_root();
    let mut boards = Vec::new();

    // Dev-speed knobs only, both no-ops by default (full board set, cap 40,
    // matching what's committed in docs/parity/*) -- this task's own speed
    // rules ask for iterating on a small fixed sample (QA corpus only, a
    // handful of boards) and paying the full examples/ladder route-cache-cold
    // cost just once, at the end. `PARITY_QA_ONLY=1` skips the
    // examples/ladder/work boards entirely (they need a cold `place`+`route`
    // in a fresh worktree with no `target/parity_route_cache` yet, which is
    // exactly the expensive, not-needed-for-this-task path); `QA_SAMPLE_CAP`
    // overrides `select_qa_pcb_boards`'s board count.
    let qa_only = std::env::var("PARITY_QA_ONLY").as_deref() == Ok("1");
    let qa_cap: usize = std::env::var("QA_SAMPLE_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(40);

    if !qa_only {
        for yaml in example_boards(&repo) {
            let source = if yaml.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some("ladder") { "ladder" } else { "example" };
            println!("== example: {} ==", yaml.display());
            boards.push(process_example(&cli, &repo, &yaml, source));
        }
    }

    let work_root = repo.join("work");
    let mcu30_design = work_root.join("mcu30/design.json");
    let mcu30_pcb = work_root.join("mcu30/export/mcu_board_30plus.kicad_pcb");
    if qa_only {
        // skip -- see the PARITY_QA_ONLY doc comment above.
    } else if mcu30_design.exists() && mcu30_pcb.exists() {
        println!("== work: mcu30 ==");
        boards.push(process_work_board(&cli, &repo, "work/mcu30", &mcu30_design, &repo.join("examples/mcu_board_30plus.yaml"), &mcu30_pcb));
    } else {
        eprintln!("work/mcu30 not found (expected a read-only copy under {}); skipping", work_root.display());
    }
    let l1_design = work_root.join("l1-order/_pipeline/design.json");
    let l1_pcb = work_root.join("l1-order/_pipeline/l1_usb_mcu.kicad_pcb");
    if qa_only {
        // skip -- see the PARITY_QA_ONLY doc comment above.
    } else if l1_design.exists() && l1_pcb.exists() {
        println!("== work: l1-order ==");
        boards.push(process_work_board(&cli, &repo, "work/l1-order", &l1_design, &repo.join("examples/ladder/l1_usb_mcu.yaml"), &l1_pcb));
    } else {
        eprintln!("work/l1-order not found (expected a read-only copy under {}); skipping", work_root.display());
    }

    let mut qa_sample = Vec::new();
    if let Some(root) = qa_root() {
        qa_sample = select_qa_pcb_boards(&root, qa_cap);
        println!("\nQA corpus sample ({} boards, root={}):", qa_sample.len(), root.display());
        for p in &qa_sample {
            println!("  {}", p.display());
        }
        for (i, pcb) in qa_sample.iter().enumerate() {
            // Printed *before* processing (not just on completion): the
            // one piece of information a hang leaves behind is "which
            // board was being printed last" (see `with_timeout`'s doc
            // comment -- this is a direct fix for a real hang that gave no
            // such signal the first time).
            println!("  qa [{}/{}]: {}", i + 1, qa_sample.len(), pcb.display());
            boards.push(process_qa_board(&cli, pcb));
        }
    } else {
        eprintln!("KiCad QA corpus not found (set {QA_ROOT_ENV} or see default path in source); skipping QA-corpus boards");
    }

    // Opt-in only (`PARITY_ZONE_IMPACT=1`): re-derives l4_control_hub's full
    // schematic -> place -> route pipeline from scratch, solely for the
    // informational "zone fill impact" sidebar in REPORT.md -- it does not
    // feed the precision/recall totals below. Now that a real zone filler
    // exists upstream, that sidebar is largely superseded anyway. Left off
    // by default because crates/freeroute's autorouter is measurably very
    // slow on this board regardless of system load (see docs/parity/
    // GAPS.md #2's follow-up note) -- it was turning every plain harness
    // run into a multi-hour wait for one sidebar table.
    let zone_impact = if std::env::var("PARITY_ZONE_IMPACT").as_deref() == Ok("1") {
        zone_fill_impact(&cli, &repo, &qa_sample)
    } else {
        eprintln!("skipping zone_fill_impact (set PARITY_ZONE_IMPACT=1 to include it -- see its doc comment, it re-routes l4_control_hub from scratch and is slow)");
        Vec::new()
    };

    // ---- summarize ----
    let mut totals: BTreeMap<String, TypeStat> = BTreeMap::new();
    let mut non_kicad_totals: BTreeMap<String, usize> = BTreeMap::new();
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
        for (ty, n) in &b.non_kicad_counts {
            *non_kicad_totals.entry(ty.clone()).or_default() += n;
        }
    }
    println!("\n=== DRC parity totals by type ===");
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
    println!("\n(this project's own placement-quality/netclass checks, no KiCad counterpart by design, excluded from the above -- {} occurrences total):", non_kicad_totals.values().sum::<usize>());
    for (ty, n) in &non_kicad_totals {
        println!("  {ty:28} {n}");
    }

    if !import_failures.is_empty() {
        println!("\n=== boards that errored (counted as gaps, not folded into totals) ===");
        for f in &import_failures {
            println!("  {f}");
        }
    }

    #[derive(serde::Serialize)]
    struct Report<'a> {
        generated_at: String,
        tolerance_um: i64,
        not_in_scope: &'a [&'a str],
        boards: &'a [BoardResult],
        totals: &'a BTreeMap<String, TypeStat>,
        non_kicad_totals: &'a BTreeMap<String, usize>,
        precision: f64,
        recall: f64,
        import_failures: &'a [String],
        zone_fill_impact: &'a [ZoneImpact],
    }
    let report = Report {
        generated_at: format!("{:?}", std::time::SystemTime::now()),
        tolerance_um: POS_TOLERANCE_UM,
        not_in_scope: NOT_IN_DRC_SCOPE,
        boards: &boards,
        totals: &totals,
        non_kicad_totals: &non_kicad_totals,
        precision,
        recall,
        import_failures: &import_failures,
        zone_fill_impact: &zone_impact,
    };
    let out_dir = repo.join("docs/parity/raw");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("drc.json"), serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("\nwrote {}", out_dir.join("drc.json").display());

    assert!(!boards.is_empty(), "parity harness processed zero boards -- that's a harness bug, not a measurement");
}
