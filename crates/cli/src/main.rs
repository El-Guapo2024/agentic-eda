//! `eda` — the loop runner. Drives intent -> schematic -> placement ->
//! routing through the gates, logging every candidate to JSONL, and
//! writing `design.json` plus renders/exports next to it.
//!
//! No argument-parsing crate: the surface is small and stable.
//!
//! ```text
//! eda lint      <intent.zen|model.yaml>
//! eda schematic <intent> [-o out_dir] [--seed N]
//! eda place     <intent> [-o out_dir] [--seed N] [--design design.json]
//! eda route     <intent> [-o out_dir] [--seed N] [--design design.json]
//! eda pipeline  <intent> [-o out_dir] [--seed N]        # all three, gated
//! eda export    <intent> --design design.json [-o out_dir]   # kicad_sch + circuit json
//! eda check     <intent> --design design.json          # run every gate, print scorecard
//! eda import-pl <intent> --design design.json --pl x.gp.pl [-o out]  # Bookshelf placement (Cypress) -> design.json
//! eda import-kicad <board.kicad_pcb> -o <dir>           # KiCad board -> an `eda board` directory
//! ```

mod board;
mod import_kicad;
mod studio;

use eda::prelude::*;
use eda::{export_kicad_pcb, export_kicad_sch, hpwl, lint, place, render_schematic, to_circuit_json, PlaceOptions, Placer};
use eda::ExportMeta;
use eda_model::ir::Stage;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use eda::model::ir::Point;


struct Args {
    cmd: String,
    intent: PathBuf,
    out: PathBuf,
    seed: u64,
    design: Option<PathBuf>,
    use_pcb_cli: bool,
    pl: Option<PathBuf>,
    /// Placement generator: "anneal" (eda-place, default) or "cypress".
    placer: String,
    /// Run the VLM judge after each stage (pipeline) — needs ANTHROPIC_API_KEY.
    judge: bool,
    /// Also write a fabrication package (gerbers, drill, BOM, placement).
    fab: bool,
    /// Also run the ported KiCad design-rule checker (`eda-drc`) as part of
    /// `check`.
    drc: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or("missing command")?;
    let mut intent: Option<PathBuf> = None;
    let mut out = PathBuf::from("out");
    let mut seed = 0u64;
    let mut design = None;
    let mut use_pcb_cli = false;
    let mut pl = None;
    let mut placer = String::new(); // empty = take the intent's solver.placer
    let mut judge = false;
    let mut fab = false;
    let mut drc = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--out" => out = PathBuf::from(it.next().ok_or("-o needs a path")?),
            "--seed" => seed = it.next().ok_or("--seed needs a number")?.parse().map_err(|_| "bad --seed")?,
            "--design" => design = Some(PathBuf::from(it.next().ok_or("--design needs a path")?)),
            "--pcb-cli" => use_pcb_cli = true,
            "--pl" => pl = Some(PathBuf::from(it.next().ok_or("--pl needs a path")?)),
            "--placer" => placer = it.next().ok_or("--placer needs anneal|cypress|build|flash")?,
            "--judge" => judge = true,
            "--fab" => fab = true,
            "--drc" => drc = true,
            s if s.starts_with('-') => return Err(format!("unknown flag {s}")),
            s => intent = Some(PathBuf::from(s)),
        }
    }
    Ok(Args { cmd, intent: intent.ok_or("missing intent path")?, out, seed, design, use_pcb_cli, pl, placer, judge, fab, drc })
}

fn usage() -> ExitCode {
    eprintln!("usage: eda <lint|schematic|place|route|pipeline|solve|export|check|import-pl|judge> <intent> [-o out] [--seed N] [--design design.json] [--pl x.pl] [--placer anneal|cypress|build|flash] [--judge] [--fab] [--drc] [--pcb-cli]");
    ExitCode::from(2)
}

fn print_checks(title: &str, checks: &[CheckResult]) -> bool {
    let fails = checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
    let warns = checks.iter().filter(|c| c.status == CheckStatus::Warn).count();
    println!("{title}: {} checks, {fails} fail, {warns} warn", checks.len());
    for c in checks {
        if c.status != CheckStatus::Pass {
            println!("  [{:?}] {} @ {}: {}", c.status, c.check, c.location.as_deref().unwrap_or("-"), c.hint.as_deref().unwrap_or(""));
            if let Some(sg) = c.detail.as_ref().and_then(|d| d.get("suggest")).and_then(|v| v.as_str()) {
                println!("      suggest: {sg}");
            }
        }
    }
    fails == 0
}

/// Run the ported KiCad DRC engine (`eda-drc`) and print a scorecard the
/// same shape `print_checks` uses. A `warning`-severity item does not fail
/// the run (matching KiCad's own default severities -- see
/// `eda_drc::item`); an `error` does.
fn print_drc(d: &eda_model::ir::Design, model: &ConstraintModel) -> bool {
    let violations = eda_drc::run(d, model);
    let errors = violations.iter().filter(|v| v.severity == eda_drc::Severity::Error).count();
    let warnings = violations.len() - errors;
    println!("drc: {} violations, {errors} error, {warnings} warning", violations.len());
    for v in &violations {
        let refs: Vec<String> = v.items.iter().map(|it| it.description.clone()).collect();
        println!("  [{:?}] {}: {}", v.severity, v.error_type, v.description);
        if !refs.is_empty() {
            println!("      {}", refs.join(" <-> "));
        }
    }
    errors == 0
}

fn load_model(args: &Args) -> Result<ConstraintModel, Vec<CheckResult>> {
    let m = load_model_unchecked(args)?;
    // Validate at the door, once, so nothing downstream has to invent a
    // value for a board that never made sense. Every reader that used to
    // cope with a zero grid or an empty stackup was quietly deciding what
    // the board meant; this is where that stops being their problem.
    let mut bad = m.board.validate();
    // Validate the footprint each part actually *resolves* to, not just
    // the intent's inline list: `footprint_of` falls back to the built-in
    // library, which is where most parts get their pads, so checking only
    // `m.footprints` would have left the library unchecked and made this
    // gate a near no-op.
    let mut checked: std::collections::HashSet<String> = std::collections::HashSet::new();
    for part in &m.parts {
        if let Some(fp) = m.footprint_of(part) {
            if checked.insert(fp.name.clone()) {
                bad.extend(fp.validate());
            }
        }
    }
    for fp in &m.footprints {
        if checked.insert(fp.name.clone()) {
            bad.extend(fp.validate());
        }
    }
    if !bad.is_empty() {
        return Err(bad);
    }
    Ok(m)
}

fn load_model_unchecked(args: &Args) -> Result<ConstraintModel, Vec<CheckResult>> {
    let ext = args.intent.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut model: ConstraintModel = match ext {
        "yaml" | "yml" => {
            let text = std::fs::read_to_string(&args.intent).map_err(|e| vec![CheckResult::fail("io", args.intent.display().to_string(), e.to_string())])?;
            serde_yaml::from_str(&text).map_err(|e| vec![CheckResult::fail("yaml", args.intent.display().to_string(), e.to_string())])?
        }
        "json" => {
            let text = std::fs::read_to_string(&args.intent).map_err(|e| vec![CheckResult::fail("io", args.intent.display().to_string(), e.to_string())])?;
            serde_json::from_str(&text).map_err(|e| vec![CheckResult::fail("json", args.intent.display().to_string(), e.to_string())])?
        }
        _ => {
            if args.use_pcb_cli {
                import_zen_cli(&args.intent)?
            } else {
                import_zen(&args.intent)?
            }
        }
    };
    resolve_footprint_libraries(&mut model);
    resolve_symbol_libraries(&mut model);
    Ok(model)
}

/// For every part naming a `"Library:Footprint"` footprint not already in
/// `model.footprints`, try to load it from the KiCad footprint libraries
/// (see `eda::default_footprint_library_root`, overridable with
/// `EDA_KICAD_FOOTPRINTS`) and add it there -- so `model.footprint_of`
/// resolves it exactly as if the intent had defined it inline. Best-effort:
/// a name that does not resolve is left for `footprint_of` to fail on
/// downstream, the same as a typo in a built-in package name already does.
/// The one place every intent this binary loads -- `.yaml`/`.json`/`.zen`,
/// and `eda board new`/`eda board <verb>`'s own intent reads -- passes
/// through, so a part only has to name its footprint once.
pub(crate) fn resolve_footprint_libraries(model: &mut ConstraintModel) {
    let root = eda::default_footprint_library_root();
    for w in eda::resolve_library_footprints(model, &root) {
        eprintln!("footprint library: {w}");
    }
}

/// For every part whose resolved `lib_id` (`symbol:` in the intent, or a
/// sensible default by kind -- see `eda_model::symbol::resolve_lib_id`)
/// names a real KiCad symbol library, try to load it from the installed
/// libraries (`eda::default_symbol_library_root`, overridable with
/// `EDA_KICAD_SYMBOLS`) and add it to `model.symbols` -- so the schematic
/// exporter and the ERC port both see the part's real pin electrical types
/// and real graphics instead of falling all the way back to a synthesized
/// generic box. Best-effort, the same as `resolve_footprint_libraries`: a
/// part that resolves to no real library (most ICs) is untouched here and
/// stays a synthesized box at export time.
pub(crate) fn resolve_symbol_libraries(model: &mut ConstraintModel) {
    let root = eda::default_symbol_library_root();
    for w in eda::resolve_library_symbols(model, &root) {
        eprintln!("symbol library: {w}");
    }
}

fn intent_hash(path: &Path) -> String {
    std::fs::read(path).map(|b| blake3::hash(&b).to_hex().to_string()).unwrap_or_default()
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Vec<CheckResult>> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| vec![CheckResult::fail("io", p.display().to_string(), e.to_string())])?;
    }
    std::fs::write(path, bytes).map_err(|e| vec![CheckResult::fail("io", path.display().to_string(), e.to_string())])
}

fn load_design(path: &Path) -> Result<Design, Vec<CheckResult>> {
    let text = std::fs::read_to_string(path).map_err(|e| vec![CheckResult::fail("io", path.display().to_string(), e.to_string())])?;
    serde_json::from_str(&text).map_err(|e| vec![CheckResult::fail("design_json", path.display().to_string(), e.to_string())])
}

fn save_design(out: &Path, design: &Design) -> Result<(), Vec<CheckResult>> {
    let bytes = design.canonical_bytes().map_err(|e| vec![CheckResult::fail("design_json", "design", e.to_string())])?;
    write(&out.join("design.json"), &bytes)
}

struct Ctx {
    args: Args,
    model: ConstraintModel,
    log: RunLog,
    ihash: String,
    /// What result.json is built from: one entry per gate stage run.
    report: Report,
}

#[derive(Default)]
struct Report {
    stages: Vec<serde_json::Value>,
    strategy: Option<serde_json::Value>,
    started: Option<Instant>,
}

impl Ctx {
    fn record(&mut self, stage: &str, checks: &[CheckResult], t0: Instant) {
        let fails = checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
        let warns = checks.iter().filter(|c| c.status == CheckStatus::Warn).count();
        self.report.stages.push(serde_json::json!({
            "stage": stage, "checks": checks.len(), "fail": fails, "warn": warns,
            "wall_s": (t0.elapsed().as_secs_f64() * 100.0).round() / 100.0,
            "placer": if stage == "placement" { Some(if self.args.placer.is_empty() { self.model.solver.placer.clone() } else { self.args.placer.clone() }) } else { None },
            "layers": if stage == "routing" { Some(self.model.board.layers.len()) } else { None },
        }));
    }
}

/// Per-run score: the numbers an agent ranks passing runs by, and the
/// numbers that say how far a failing one got. All from design.json.
fn score(design: &Design, model: &ConstraintModel) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    if let Some(pl) = &design.placement {
        let outline_area = polygon_area(&pl.outline);
        let mut used: i64 = 0;
        for fp in &pl.footprints {
            if let Some(part) = model.part(&fp.id) {
                if let Some((x0, y0, x1, y1)) = eda_model::footprint::placed_courtyard(model, part, fp) {
                    used += (x1 - x0) * (y1 - y0);
                }
            }
        }
        out.insert("parts".into(), pl.footprints.len().into());
        if outline_area > 0 {
            out.insert("board_use".into(), serde_json::json!((used as f64 / outline_area as f64 * 1000.0).round() / 1000.0));
        }
        if let Some(h) = hpwl(design, model) {
            out.insert("hpwl_um".into(), h.into());
        }
    }
    if let Some(r) = &design.routing {
        let mut len_by_net: std::collections::BTreeMap<&str, i64> = Default::default();
        let mut layers: std::collections::BTreeSet<&str> = Default::default();
        for t in &r.tracks {
            layers.insert(&t.layer);
            let l: i64 = t.pts.windows(2).map(|w| (w[1].x - w[0].x).abs() + (w[1].y - w[0].y).abs()).sum();
            *len_by_net.entry(&t.net).or_default() += l;
        }
        let total: i64 = len_by_net.values().sum();
        out.insert("tracks".into(), r.tracks.len().into());
        out.insert("vias".into(), r.vias.len().into());
        out.insert("layers_used".into(), layers.len().into());
        out.insert("track_len_um".into(), total.into());
        out.insert("routed_nets".into(), len_by_net.len().into());
        // Detour: routed length over the net's own half-perimeter bbox of
        // footprint centres. 1.0 is a straight run; big values are the
        // nets an agent should look at first.
        if let Some(pl) = &design.placement {
            let at: std::collections::HashMap<&str, Point> = pl.footprints.iter().map(|f| (f.id.as_str(), f.at)).collect();
            let mut detours: Vec<(f64, &str)> = Vec::new();
            for net in &model.nets {
                let Some(&len) = len_by_net.get(net.name.as_str()) else { continue };
                let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
                for pin in &net.pins {
                    let refdes = pin.split('.').next().unwrap_or("");
                    if let Some(p) = at.get(refdes) {
                        x0 = x0.min(p.x); y0 = y0.min(p.y); x1 = x1.max(p.x); y1 = y1.max(p.y);
                    }
                }
                if x0 == i64::MAX { continue }
                let hp = ((x1 - x0) + (y1 - y0)).max(2000) as f64;
                detours.push((len as f64 / hp, net.name.as_str()));
            }
            detours.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            if !detours.is_empty() {
                let mean = detours.iter().map(|d| d.0).sum::<f64>() / detours.len() as f64;
                out.insert("detour_mean".into(), serde_json::json!((mean * 100.0).round() / 100.0));
                out.insert("detour_max".into(), serde_json::json!((detours[0].0 * 100.0).round() / 100.0));
                out.insert("worst_nets".into(), serde_json::json!(detours.iter().take(3).map(|d| serde_json::json!({"net": d.1, "detour": (d.0 * 100.0).round() / 100.0})).collect::<Vec<_>>()));
            }
        }
    }
    serde_json::Value::Object(out)
}

fn polygon_area(pts: &[Point]) -> i64 {
    if pts.len() < 3 { return 0 }
    let mut a: i64 = 0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.x * q.y - q.x * p.y;
    }
    a.abs() / 2
}

/// result.json: one machine-readable verdict per run. Written on every
/// exit path so a failed run is as readable as a passed one.
fn write_result(cx: &Ctx, res: &Result<(), Vec<CheckResult>>) {
    // Only commands that produce or judge a design candidate own
    // result.json; `judge`/`export`/`check` are read-only passes that
    // must not clobber the verdict of the run they inspect.
    if matches!(cx.args.cmd.as_str(), "judge" | "export" | "check" | "lint") {
        return;
    }
    let design = load_design(&cx.args.out.join("design.json")).ok();
    let failures: Vec<&CheckResult> = res.as_ref().err().map(|f| f.iter().collect()).unwrap_or_default();
    let v = serde_json::json!({
        "run_id": cx.log.run_id,
        "intent": cx.args.intent.display().to_string(),
        "seed": cx.args.seed,
        "cmd": cx.args.cmd,
        "engine_version": env!("CARGO_PKG_VERSION"),
        "status": if res.is_ok() { "pass" } else { "fail" },
        "wall_s": cx.report.started.map(|t| (t.elapsed().as_secs_f64() * 100.0).round() / 100.0),
        "stages": cx.report.stages,
        "strategy": cx.report.strategy,
        "score": design.as_ref().map(|d| score(d, &cx.model)),
        "failures": failures,
    });
    let _ = write(&cx.args.out.join("result.json"), serde_json::to_string_pretty(&v).unwrap_or_default().as_bytes());
}

fn stage_schematic(cx: &mut Ctx) -> Result<Design, Vec<CheckResult>> {
    let t0 = Instant::now();
    let opts = EngineOptions { seed: cx.args.seed, intent_hash: cx.ihash.clone(), ..Default::default() };
    let design = derive_schematic(&cx.model, &opts)?;
    // One engine: `check_erc` is the ported KiCad ERC's electrical checks
    // *and* this generator's own readability/tidiness checks (grid,
    // overlap, wire length, ...) folded in as additional tests -- the same
    // way KiCad's own ERC runs non-electrical checks (similar labels,
    // off-grid pins) alongside electrical ones, rather than as a second
    // tool a caller has to remember to also run.
    let checks = check_erc(&design, &cx.model);
    cx.log.candidate(Stage::Schematic, 0, cx.args.seed, &design, Tier::Geometry, &checks, serde_json::Value::Null).ok();
    // Always persist the candidate: a failed one is what review reads.
    save_design(&cx.args.out, &design)?;
    let ok = print_checks("schematic gates", &checks);
    cx.record("schematic", &checks, t0);
    let svg = render_schematic(&design, &cx.model)?;
    write(&cx.args.out.join("schematic.svg"), svg.as_bytes())?;
    if !ok {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    Ok(design)
}

/// Give the constructive placer an outline to build into.
///
/// The annealer gets one from `fit_outline` as part of its own run; the
/// command-driven placer needs it up front, because every command is
/// resolved against the board's extent. A design that already carries a
/// placement keeps its outline.
fn seed_outline(design: &Design, model: &ConstraintModel) -> Result<Design, Vec<CheckResult>> {
    if design.placement.as_ref().is_some_and(|p| p.outline.len() >= 3) {
        return Ok(design.clone());
    }
    let mut d = design.clone();
    // Size the board for the parts before placing, in both directions.
    //
    // A declared outline states the shape and origin; it is not
    // necessarily the right size. Placing into one that is too large
    // spreads connectors to all four edges, after which nothing can be
    // trimmed -- L2 stayed at its declared 80x55 where the annealer
    // reached 65x45, and both failed placement_board_use for being
    // mostly empty. One too small leaves the placer nowhere to work.
    //
    // The target is just past the gate's floor rather than the full
    // utilisation: board_use fails below fit_target * 0.5, and a
    // constructive placer packs less tightly than an annealer, so
    // fitting to the full target leaves its hub part with no room.
    let want_density = model.solver.fit_board_utilization.max(0.25)
        * eda_gates::pcb::BOARD_USE_MIN_DENSITY_FRACTION
        * 1.3;
    let outline = match model.board.outline.clone() {
        Some(o) if o.len() >= 3 => {
            eda_model::board::sized_for_parts(model, &o, want_density).unwrap_or(o)
        }
        // No declared outline: a square, sized the same way. This used to
        // ask fit_outline, which only shrinks an outline that exists --
        // given none it returned none, and every intent without an outline
        // failed here from the day this placer became the default.
        _ => eda_model::board::square_for_parts(model, want_density).ok_or_else(|| {
            vec![CheckResult::fail("build_precondition", "board.outline", "no board outline in the intent, and no part resolves to a footprint to size one from")]
        })?,
    };
    d.placement = Some(eda_model::ir::PlacementSection { outline, footprints: Vec::new(), modules: Vec::new() });
    Ok(d)
}

fn stage_place(cx: &mut Ctx, design: &Design, seed: u64, board_floor: f64) -> Result<Design, Vec<CheckResult>> {
    let t0 = Instant::now();
    // The intent's `solver` block sets the placer and its tuning; an
    // explicit --placer on the command line overrides the choice only.
    let sv = &cx.model.solver;
    let placer = if cx.args.placer.is_empty() { sv.placer.clone() } else { cx.args.placer.clone() };
    let placed = match placer.as_str() {
        "cypress" => {
            // No fallback: Cypress needs a board extent from the intent.
            if design.placement.is_none() && cx.model.board.outline.is_none() {
                return Err(vec![CheckResult::fail("cypress_precondition", "board.outline", "Cypress needs `board.outline` in the intent (or an existing placement); no auto-sizing")]);
            }
            let mut o = eda::CypressOptions::default();
            o.proximity_weight = sv.cypress_proximity_weight;
            o.fit_board_utilization = sv.fit_board_utilization;
            eda::Cypress(o).place(design, &cx.model, seed)?
        }
        "anneal" => place(
            design,
            &cx.model,
            &PlaceOptions {
                seed,
                spacing: sv.place_spacing_um,
                moves_per_part: sv.place_moves_per_part,
                snap: sv.place_snap_um,
                fit_floor: board_floor,
                ..Default::default()
            },
        )?,
        // Constructive placement: parts go down one at a time, each
        // beside one already placed, with the gates run after every
        // step. `build` chooses by gate outcome; `ai` asks the
        // evaluation model which neighbour and side, knowing what the
        // circuit is. They share all their geometry, so a difference
        // between them is a difference in judgement and nothing else.
        "build" | "flash" => {
            let seeded = seed_outline(design, &cx.model)?;
            let mut greedy = eda_ops::build::Greedy;
            let mut flash = eda_ops::flash::Flash::new();
            let chooser: &mut dyn eda_ops::build::Chooser = match placer.as_str() {
                "flash" => &mut flash,
                _ => &mut greedy,
            };
            let (d, report) = eda_ops::build::build(seeded, &cx.model, sv.place_snap_um, sv.place_spacing_um, chooser)?;
            eprintln!(
                "place {}: {} of {} parts in {} steps, {} rip(s), {} gate failure(s)",
                report.chooser, report.placed, report.total, report.steps, report.ripped, report.failures
            );
            if placer == "flash" {
                eprintln!("place flash: {} part(s) scored in {} request(s)", flash.scored, flash.requests);
            }
            d
        }
        other => return Err(vec![CheckResult::fail("cli", other, "unknown placer (anneal|cypress|build|flash)")]),
    };
    save_design(&cx.args.out, &placed)?;
    let mut checks = check_placement(&placed, &cx.model);
    checks.extend(eda::preflight(&placed, &cx.model, &cx.model.board));
    let metrics = serde_json::json!({
        "hpwl_um": hpwl(&placed, &cx.model),
        "features": placement_features(&placed, &cx.model),
    });
    cx.log.candidate(Stage::Placement, 0, seed, &placed, Tier::Geometry, &checks, metrics).ok();
    cx.record("placement", &checks, t0);
    if !print_checks("placement gates", &checks) {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    Ok(placed)
}

/// Whether another placement seed could plausibly clear these failures.
///
/// A near miss on a distance rule, a stub crossing or a compactness
/// complaint is a *different arrangement* problem, and a different seed is
/// a different arrangement. A part that will not fit, connectors that
/// overflow the edges, or Cypress refusing to run are not: those need a
/// different rung of the ladder, and re-seeding only burns placements.
///
/// The "far miss" test reads the suggestion the proximity gate already
/// writes into its detail. Those hints existed and nothing acted on them.
fn reseed_may_help(fails: &[CheckResult]) -> bool {
    const STRUCTURAL: &[&str] = &[
        "cypress_precondition", "cypress_unavailable", "cypress_io", "cypress_failed", "cypress_diverged",
        "edge_connectors_overflow", "placement_outline", "placement_present", "placement_within_outline", "cli",
    ];
    !fails.iter().any(|c| {
        STRUCTURAL.contains(&c.check.as_str())
            || c.detail
                .as_ref()
                .and_then(|d| d.get("suggest"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.starts_with("far miss"))
    })
}

/// Failures no amount of retrying can fix.
///
/// The placement loop has two recovery moves: a new seed, and a bigger
/// board. Both are useless against a part that has no land pattern -- no
/// seed conjures geometry and no amount of space helps -- yet
/// `place_precondition` used to fall through to both, so an impossible
/// board burned three full placement attempts before reporting the thing
/// it knew on the first. On one board that is a slow failure; across a
/// fleet it is three times the work spent proving the same impossibility.
///
/// These are failures about the *input*, not the search. The right
/// response is to stop and say so.
fn hopeless(fails: &[CheckResult]) -> bool {
    const HOPELESS: &[&str] = &[
        // No footprint geometry: the part is not physically realisable.
        "place_precondition",
        // The intent itself is wrong; every stage would fail the same way.
        "board_rules", "footprint", "source_pin_has_no_pad", "source_footprint_body_mismatch",
        // The tool is missing, not the answer.
        "cypress_unavailable",
    ];
    fails.iter().any(|c| HOPELESS.contains(&c.check.as_str()))
}

/// Placement as its own feedback loop, inside one rung of the ladder.
///
/// Placement is the cheap stage to repeat and the one whose failures are
/// most often a near miss: on L4 the annealer misses a proximity rule by
/// 1.5 mm on seed 0 and clears every gate on seed 2. Escalating to the
/// next rung throws the rung away and re-places anyway under coarser
/// settings, so try a few seeds here first and only escalate when the
/// failure says another arrangement will not help.
fn stage_place_looping(cx: &mut Ctx, schematic: &Design) -> Result<Design, Vec<CheckResult>> {
    // `build` takes no seed and no board floor (seed_outline sizes from
    // the parts), so a second attempt is the first one again: three
    // identical boards and three identical failures, reported as if
    // something different had been tried.
    let placer = if cx.args.placer.is_empty() { &cx.model.solver.placer } else { &cx.args.placer };
    let attempts = if placer == "build" { 1 } else { cx.model.solver.place_attempts.max(1) };
    let mut last: Vec<CheckResult> = Vec::new();
    // Lower bound on the fitted board as a fraction of the intent outline.
    // Starts unbounded (shrink as far as the utilisation target wants) and
    // ratchets up when the parts turn out not to fit.
    let mut board_floor = 0.0f64;
    for k in 0..attempts {
        // Spread the derived seeds far apart so attempt 1 is not attempt 0
        // with one part nudged; still a pure function of --seed.
        let seed = cx.args.seed.wrapping_add(k as u64 * 1_000_003);
        if k > 0 {
            let why: Vec<&str> = last.iter().map(|c| c.check.as_str()).collect();
            let grew = if board_floor > 0.0 { format!(", board >= {:.0}% of the intent outline", board_floor * 100.0) } else { String::new() };
            let line = format!("placement attempt {} of {attempts}, seed {seed}{grew} (previous: {})", k + 1, why.join(", "));
            println!("{line}");
            cx.log.note(line).ok();
        }
        match stage_place(cx, schematic, seed, board_floor) {
            Ok(d) => return Ok(d),
            Err(f) => {
                if hopeless(&f) {
                    return Err(f);
                }
                if !reseed_may_help(&f) {
                    // A board fitted too tightly reads as structural -- the
                    // parts genuinely do not fit what we shrank it to. Grow
                    // it and try again rather than escalating the ladder,
                    // which would only re-place under coarser rules.
                    if board_floor < 1.0 {
                        board_floor = (board_floor + 0.25).min(1.0);
                        last = f;
                        continue;
                    }
                    return Err(f);
                }
                last = f;
            }
        }
    }
    Err(last)
}

/// Bounding box of the intent's declared outline. The floorplan stage
/// needs a board before any placement exists, so an intent with no outline
/// has nothing to plan on and says so rather than inventing one.
fn board_bb(model: &eda_model::ConstraintModel) -> (eda_model::ir::Um, eda_model::ir::Um, eda_model::ir::Um, eda_model::ir::Um) {
    match model.board.outline.as_ref().filter(|o| !o.is_empty()) {
        Some(o) => {
            let (xs, ys): (Vec<_>, Vec<_>) = o.iter().map(|p| (p.x, p.y)).unzip();
            (*xs.iter().min().unwrap(), *ys.iter().min().unwrap(), *xs.iter().max().unwrap(), *ys.iter().max().unwrap())
        }
        None => (0, 0, 0, 0),
    }
}

/// One rung of the solver's ladder: a board-rule variant plus a placer.
#[derive(Clone, Debug)]
struct Strategy {
    name: String,
    placer: String,
    board: eda_model::BoardRules,
}

/// Cheapest-first ladder inside the intent's allowances: as written, then
/// the other permitted placer, then tighter track/clearance (free at the
/// fab if the user allowed them), then more layers (not free), then both.
fn strategies(model: &ConstraintModel, base_placer: &str) -> Vec<Strategy> {
    let a = &model.allow;
    let base = model.board.clone();
    let mut placers: Vec<String> = vec![base_placer.to_string()];
    for p in &a.placers {
        if !placers.contains(p) {
            placers.push(p.clone());
        }
    }
    let mut rule_variants: Vec<(String, eda_model::BoardRules)> = vec![("as written".into(), base.clone())];
    let tight = {
        let mut b = base.clone();
        let mut changed = false;
        if let Some(t) = a.min_track {
            if t < b.track_width { b.track_width = t; changed = true; }
        }
        if let Some(c) = a.min_clearance {
            if c < b.clearance { b.clearance = c; changed = true; }
        }
        changed.then_some(b)
    };
    if let Some(b) = &tight {
        rule_variants.push((format!("track {} / clearance {}", b.track_width, b.clearance), b.clone()));
    }
    if let Some(ml) = a.max_layers {
        if ml >= 4 && base.layers.len() < 4 {
            let four = vec!["F.Cu".to_string(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
            let mut b = base.clone();
            b.layers = four.clone();
            rule_variants.push(("4 layers".into(), b));
            if let Some(t) = &tight {
                let mut b = t.clone();
                b.layers = four;
                rule_variants.push((format!("4 layers, track {} / clearance {}", b.track_width, b.clearance), b));
            }
        }
    }
    let mut out = Vec::new();
    for (rname, board) in rule_variants {
        for p in &placers {
            out.push(Strategy { name: format!("{rname}, placer {p}"), placer: p.clone(), board: board.clone() });
        }
    }
    out
}

/// Walk the ladder: place + route under each strategy until one passes
/// every gate. No fallback inside a rung — each is a full hard-gated run.
/// Every attempt is logged; none passing is a hard fail with the table.
fn stage_solve(cx: &mut Ctx, schematic: &Design) -> Result<Design, Vec<CheckResult>> {
    let base = if cx.args.placer.is_empty() { cx.model.solver.placer.clone() } else { cx.args.placer.clone() };
    let ladder = strategies(&cx.model, &base);
    let base_board = cx.model.board.clone();
    let base_placer = cx.args.placer.clone();
    let mut table: Vec<String> = Vec::new();
    let mut last_fails: Vec<CheckResult> = Vec::new();
    for (i, st) in ladder.iter().enumerate() {
        cx.model.board = st.board.clone();
        cx.args.placer = st.placer.clone();
        println!("== strategy {i}: {}", st.name);
        cx.log.note(format!("strategy {i} start: {}", st.name)).ok();
        let t = std::time::Instant::now();
        let result = stage_place_looping(cx, schematic).and_then(|d| stage_route(cx, &d));
        match result {
            Ok(d) => {
                let line = format!("strategy {i}: {} -> PASS in {:.1}s", st.name, t.elapsed().as_secs_f64());
                cx.report.strategy = Some(serde_json::json!({ "index": i, "name": st.name, "tried": i + 1, "layers": st.board.layers.len(), "placer": st.placer }));
                println!("{line}");
                cx.log.note(line.clone()).ok();
                table.push(line);
                write(&cx.args.out.join("strategy.txt"), table.join("
").as_bytes())?;
                return Ok(d);
            }
            Err(fails) => {
                let why: Vec<String> = fails.iter().take(3).map(|c| format!("{}@{}", c.check, c.location.as_deref().unwrap_or("-"))).collect();
                let line = format!("strategy {i}: {} -> FAIL ({} fails: {}) in {:.1}s", st.name, fails.len(), why.join(", "), t.elapsed().as_secs_f64());
                println!("{line}");
                // Router failures come back from route_partial without
                // passing print_checks; show them (and their suggestion)
                // so the log carries the same facts as result.json.
                print_checks(&format!("strategy {i} failures"), &fails);
                cx.log.note(line.clone()).ok();
                table.push(line);
                last_fails = fails;
            }
        }
    }
    cx.report.strategy = Some(serde_json::json!({ "index": null, "tried": ladder.len() }));
    cx.model.board = base_board;
    cx.args.placer = base_placer;
    write(&cx.args.out.join("strategy.txt"), table.join("
").as_bytes())?;
    // The last rung's failures keep their detail: that is what the agent
    // acts on. The solve summary goes first so the headline stays clear.
    let mut out = vec![CheckResult::fail("solve", "design", format!("no strategy inside the allowances passed every gate ({} tried; see strategy.txt)", ladder.len()))];
    out.extend(last_fails);
    Err(out)
}

fn stage_route(cx: &mut Ctx, design: &Design) -> Result<Design, Vec<CheckResult>> {
    let t0 = Instant::now();
    let routed = match eda::route_checked(design, &cx.model, &cx.model.board, cx.args.seed) {
        (Some(d), fails) if fails.is_empty() => d,
        (partial, fails) => {
            // Persist the partial result for review, then fail the stage.
            if let Some(d) = partial {
                save_design(&cx.args.out, &d)?;
                export(cx, &d).ok();
            }
            cx.record("routing", &fails, t0);
            return Err(fails);
        }
    };
    // Always persist the candidate: a gate-failed one is what review reads.
    save_design(&cx.args.out, &routed)?;
    let checks = check_routing(&routed, &cx.model);
    let r = routed.routing.as_ref().unwrap();
    let metrics = serde_json::json!({ "tracks": r.tracks.len(), "vias": r.vias.len() });
    cx.log.candidate(Stage::Routing, 0, cx.args.seed, &routed, Tier::Geometry, &checks, metrics).ok();
    cx.record("routing", &checks, t0);
    if !print_checks("routing gates", &checks) {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    Ok(routed)
}

/// Bookshelf integer unit used for Cypress export/import (100 µm).
const BOOKSHELF_UNIT_UM: i64 = 100;

/// A placement's shape, as numbers, for every candidate we log.
///
/// The run log already records which gates passed, which is a row of
/// booleans -- and a board that *barely* passes placement is exactly the
/// one that fails routing, so the booleans throw away the signal worth
/// having. These are the continuous quantities behind them.
///
/// Deliberately computed from the finished `Design` and never from the
/// annealer's internals: a feature only the annealer can produce is
/// useless for judging a Cypress placement, and a predictor that cannot
/// compare the two placers is not worth training.
///
/// Nothing consumes these yet. They are here so that every run a fleet
/// does accumulates (features -> outcome) rows for free; the model comes
/// after there is data, not before.
fn placement_features(design: &Design, model: &ConstraintModel) -> serde_json::Value {
    let Some(pl) = design.placement.as_ref() else { return serde_json::Value::Null };
    let (xs, ys): (Vec<_>, Vec<_>) = pl.outline.iter().map(|p| (p.x, p.y)).unzip();
    let (bx0, bx1) = (xs.iter().min().copied().unwrap_or(0), xs.iter().max().copied().unwrap_or(0));
    let (by0, by1) = (ys.iter().min().copied().unwrap_or(0), ys.iter().max().copied().unwrap_or(0));
    let board_area = ((bx1 - bx0) as f64 / 1000.0) * ((by1 - by0) as f64 / 1000.0); // mm^2

    let mut court_area = 0.0f64;
    let mut pads = 0usize;
    let mut worst_edge_gap = 0i64;
    let mut rects: Vec<(String, (i64, i64, i64, i64))> = Vec::new();
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        pads += part.pins.len();
        if let Some(r) = eda_model::footprint::placed_courtyard(model, part, fp) {
            court_area += ((r.2 - r.0) as f64 / 1000.0) * ((r.3 - r.1) as f64 / 1000.0);
            rects.push((fp.id.clone(), r));
            if eda_model::footprint::is_edge_connector(part) {
                worst_edge_gap = worst_edge_gap.max(eda_model::footprint::edge_connector_gap(r, (bx0, by0, bx1, by1)));
            }
        }
    }

    // Tightest courtyard gap on the board: how much room legalisation had
    // left over, which is the thing that decides whether the router can
    // get a track between two parts at all.
    let mut min_gap = i64::MAX;
    for a in 0..rects.len() {
        for b in a + 1..rects.len() {
            let (ra, rb) = (rects[a].1, rects[b].1);
            let dx = (rb.0 - ra.2).max(ra.0 - rb.2).max(0);
            let dy = (rb.1 - ra.3).max(ra.1 - rb.3).max(0);
            min_gap = min_gap.min(if dx == 0 && dy == 0 { 0 } else { dx.max(dy) });
        }
    }

    // Worst per-net stretch against its packed bound -- the same quantity
    // `placement_net_compactness` thresholds at 1.6x, kept as the ratio.
    let mut worst_compact = 0.0f64;
    for net in &model.nets {
        let mut refs: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for pin in &net.pins {
            if let Some((r, _)) = pin.split_once('.') {
                refs.insert(r);
            }
        }
        if refs.len() < 2 || refs.len() > 6 {
            continue;
        }
        let mut area = 0.0f64;
        let (mut nx0, mut ny0, mut nx1, mut ny1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        let mut ok = true;
        for r in &refs {
            match rects.iter().find(|(id, _)| id == r) {
                Some((_, c)) => {
                    area += ((c.2 - c.0) as f64) * ((c.3 - c.1) as f64);
                    nx0 = nx0.min(c.0); ny0 = ny0.min(c.1); nx1 = nx1.max(c.2); ny1 = ny1.max(c.3);
                }
                None => { ok = false; break; }
            }
        }
        if !ok || area <= 0.0 {
            continue;
        }
        let bound = (2.0 * area.sqrt()).max(6000.0);
        let span = ((nx1 - nx0) + (ny1 - ny0)) as f64;
        worst_compact = worst_compact.max(span / bound);
    }

    serde_json::json!({
        "parts": pl.footprints.len(),
        "nets": model.nets.len(),
        "pads": pads,
        "board_mm2": (board_area * 100.0).round() / 100.0,
        "courtyard_fill": if board_area > 0.0 { (court_area / board_area * 1000.0).round() / 1000.0 } else { 0.0 },
        "pad_density_per_mm2": if board_area > 0.0 { (pads as f64 / board_area * 1000.0).round() / 1000.0 } else { 0.0 },
        "min_courtyard_gap_um": if min_gap == i64::MAX { serde_json::Value::Null } else { min_gap.into() },
        "worst_compact_ratio": (worst_compact * 1000.0).round() / 1000.0,
        "worst_edge_gap_um": worst_edge_gap,
    })
}

fn export(cx: &Ctx, design: &Design) -> Result<(), Vec<CheckResult>> {
    if design.schematic.is_some() {
        let title = cx.args.intent.file_stem().and_then(|s| s.to_str()).unwrap_or("design");
        let date = eda::now_rfc3339();
        let sch = export_kicad_sch(design, &cx.model, &ExportMeta { date: &date[..10], title })?;
        write(&cx.args.out.join(format!("{title}.kicad_sch")), sch.as_bytes())?;
    }
    if design.placement.is_some() {
        let title = cx.args.intent.file_stem().and_then(|s| s.to_str()).unwrap_or("design");
        let date = eda::now_rfc3339();
        let pcb = export_kicad_pcb(design, &cx.model, &ExportMeta { date: &date[..10], title })?;
        write(&cx.args.out.join(format!("{title}.kicad_pcb")), pcb.as_bytes())?;
        // Without a sibling project file, `kicad-cli pcb drc` has no
        // project to load and checks the board against its own hard-coded
        // design-rule floors instead of this board's own -- see
        // `export_kicad_pro`.
        write(&cx.args.out.join(format!("{title}.kicad_pro")), eda::export_kicad_pro(&cx.model).as_bytes())?;
    }
    if design.placement.is_some() {
        let title = cx.args.intent.file_stem().and_then(|s| s.to_str()).unwrap_or("design");
        let bs = eda::to_bookshelf(design, &cx.model, title, BOOKSHELF_UNIT_UM)?;
        for (name, content) in bs.files(title) {
            write(&cx.args.out.join("bookshelf").join(name), content.as_bytes())?;
        }
    }
    let cj = to_circuit_json(design, &cx.model)?;
    write(&cx.args.out.join("circuit.json"), serde_json::to_string_pretty(&cj).unwrap_or_default().as_bytes())?;
    if cx.args.fab {
        export_fab(cx, design)?;
    }
    Ok(())
}

/// Locate `kicad-cli`. Same lookup the DRC test uses.
fn find_kicad_cli() -> Option<std::path::PathBuf> {
    if let Ok(out) = std::process::Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(std::path::PathBuf::from(p));
            }
        }
    }
    let mac = std::path::PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

/// Write the files a board house reads, into `<out>/fab/`.
///
/// Gated first, and the gate fails rather than warns: a package missing a
/// part number is not a good order with a note on it, it is an order the
/// assembler cannot fill. Getting that back from the factory costs days;
/// getting it back from here costs nothing.
fn export_fab(cx: &Ctx, design: &Design) -> Result<(), Vec<CheckResult>> {
    let checks = eda_fab::check_fab(design, &cx.model);
    if !print_checks("fab gates", &checks) {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    let title = cx.args.intent.file_stem().and_then(|s| s.to_str()).unwrap_or("design");
    let dir = cx.args.out.join("fab");
    write(&dir.join(format!("{title}-bom.csv")), eda_fab::bom_csv(&cx.model).as_bytes())?;
    write(&dir.join(format!("{title}-positions.csv")), eda_fab::cpl_csv(design)?.as_bytes())?;

    // Gerbers and the drill file are plotted from the .kicad_pcb we just
    // wrote, so the copper shipped is the copper `kicad-cli pcb drc`
    // checks. Missing kicad-cli is a hard failure: silently shipping a
    // BOM and calling it a fab package is exactly the half-done output
    // that gets discovered at the factory.
    let pcb = cx.args.out.join(format!("{title}.kicad_pcb"));
    if !pcb.exists() {
        return Err(vec![CheckResult::fail("fab_no_board", title, "no .kicad_pcb to plot: a fab package without copper is not a package")]);
    }
    let cli = find_kicad_cli().ok_or_else(|| {
        vec![CheckResult::fail(
            "fab_no_kicad_cli",
            "kicad-cli",
            "kicad-cli was not found, so the gerbers and drill file cannot be plotted.              The BOM and positions alone are not a fabrication package; install KiCad or drop --fab.",
        )]
    })?;
    for (what, args) in [
        ("gerbers", vec!["pcb", "export", "gerbers"]),
        ("drill", vec!["pcb", "export", "drill"]),
    ] {
        let out = std::process::Command::new(&cli)
            .args(&args)
            .arg("-o")
            .arg(format!("{}/", dir.display()))
            .arg(&pcb)
            .output()
            .map_err(|e| vec![CheckResult::fail("fab_plot", what, format!("could not run kicad-cli: {e}"))])?;
        if !out.status.success() {
            return Err(vec![CheckResult::fail(
                "fab_plot",
                what,
                format!("kicad-cli {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()),
            )]);
        }
    }
    println!("fab package written to {}", dir.display());
    Ok(())
}

fn run(args: Args) -> Result<(), Vec<CheckResult>> {
    let model = load_model(&args)?;
    if args.cmd == "lint" {
        let checks = lint(&model);
        return if print_checks("lint", &checks) { Ok(()) } else { Err(checks) };
    }
    let ihash = intent_hash(&args.intent);
    let run_id = format!("{}-{}", &ihash[..8.min(ihash.len())], args.seed);
    let mut log = RunLog::open(&args.out.join("runs.jsonl"), run_id.clone())
        .map_err(|e| vec![CheckResult::fail("io", args.out.display().to_string(), e.to_string())])?;
    log.write(&Event::RunStarted {
        run_id,
        ts: eda::now_rfc3339(),
        intent_path: args.intent.display().to_string(),
        intent_hash: ihash.clone(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
    })
    .ok();
    let mut cx = Ctx { args, model, log, ihash, report: Report { started: Some(Instant::now()), ..Default::default() } };

    let lint_checks = lint(&cx.model);
    if !print_checks("lint", &lint_checks) {
        write_result(&cx, &Err(lint_checks.clone()));
        return Err(lint_checks);
    }
    let res = run_cmd(&mut cx);
    write_result(&cx, &res);
    res
}

fn run_cmd(cx: &mut Ctx) -> Result<(), Vec<CheckResult>> {
    let prior = match &cx.args.design {
        Some(p) => Some(load_design(p)?),
        None => None,
    };
    let blank = || Design {
        schema: 1,
        provenance: eda_model::ir::Provenance { engine_version: env!("CARGO_PKG_VERSION").into(), intent_hash: cx.ihash.clone(), seed: cx.args.seed, stage_hashes: vec![] },
        schematic: None,
        placement: None,
        routing: None,
        drawings: None,
    };

    let design = match cx.args.cmd.as_str() {
        "schematic" => stage_schematic(cx)?,
        "place" => {
            let base = prior.unwrap_or_else(blank);
            stage_place_looping(cx, &base)?
        }
        "floorplan" => {
            let bb = board_bb(&cx.model);
            let fp = eda_model::floorplan::plan(&cx.model, bb, 500, cx.args.seed)
                .map_err(|e| vec![CheckResult::fail("floorplan", "arrange", e)])?;
            println!("{} modules, {} free parts, board {:.1} x {:.1} mm",
                fp.modules.len(), fp.free.len(), (bb.2 - bb.0) as f64 / 1000.0, (bb.3 - bb.1) as f64 / 1000.0);
            for m in &fp.modules {
                println!("  {:<10} {:>5.1} x {:>5.1} mm at ({:>5.1}, {:>5.1})  {} parts: {}",
                    m.name, (m.rect.2 - m.rect.0) as f64 / 1000.0, (m.rect.3 - m.rect.1) as f64 / 1000.0,
                    m.rect.0 as f64 / 1000.0, m.rect.1 as f64 / 1000.0, m.refs.len(), m.refs.join(" "));
            }
            if !fp.free.is_empty() {
                println!("  free (no proximity rule ties them anywhere): {}", fp.free.join(" "));
            }
            return Ok(());
        }
        "route" => {
            let base = prior.ok_or_else(|| vec![CheckResult::fail("cli", "route", "route needs --design with a placement")])?;
            stage_route(cx, &base)?
        }
        "solve" => {
            let d = stage_schematic(cx)?;
            if cx.args.judge { run_judge(cx, &d, eda_judge::Stage::Schematic)?; }
            stage_solve(cx, &d)?
        }
        "pipeline" => {
            let d = stage_schematic(cx)?;
            if cx.args.judge { run_judge(cx, &d, eda_judge::Stage::Schematic)?; }
            // An intent that declares `allow:` is stating what the fab may
            // do -- four layers, a tighter track -- if the board as written
            // will not route. Running place+route straight through here
            // ignored all of it, so `pipeline` failed boards that `solve`
            // routed, and the difference was invisible: nothing said the
            // allowance had been skipped. The ladder is the same single
            // as-written run when no allowance widens it, so this changes
            // nothing for an intent that declares none.
            let ladder = strategies(&cx.model, &if cx.args.placer.is_empty() { cx.model.solver.placer.clone() } else { cx.args.placer.clone() });
            let d = if ladder.len() > 1 {
                stage_solve(cx, &d)?
            } else {
                let d = stage_place_looping(cx, &d)?;
                if cx.args.judge { run_judge(cx, &d, eda_judge::Stage::Placement)?; }
                stage_route(cx, &d)?
            };
            if cx.args.judge {
                // stage_solve judges nothing inside the ladder: a rung that
                // fails its gates is not a candidate worth an opinion. The
                // winner gets both passes here.
                if ladder.len() > 1 { run_judge(cx, &d, eda_judge::Stage::Placement)?; }
                run_judge(cx, &d, eda_judge::Stage::Routing)?;
            }
            d
        }
        "judge" => {
            let d = prior.ok_or_else(|| vec![CheckResult::fail("cli", "judge", "judge needs --design")])?;
            let mut fails = Vec::new();
            for (present, stage) in [(d.schematic.is_some(), eda_judge::Stage::Schematic), (d.placement.is_some(), eda_judge::Stage::Placement), (d.routing.is_some(), eda_judge::Stage::Routing)] {
                if present {
                    if let Err(f) = run_judge(cx, &d, stage) { fails.extend(f); }
                }
            }
            return if fails.is_empty() { Ok(()) } else { Err(fails) };
        }
        "export" => {
            let d = prior.ok_or_else(|| vec![CheckResult::fail("cli", "export", "export needs --design")])?;
            export(cx, &d)?;
            return Ok(());
        }
        "import-pl" => {
            let d = prior.ok_or_else(|| vec![CheckResult::fail("cli", "import-pl", "import-pl needs --design (for the outline)")])?;
            let pl_path = cx.args.pl.clone().ok_or_else(|| vec![CheckResult::fail("cli", "import-pl", "import-pl needs --pl")])?;
            let pl = std::fs::read_to_string(&pl_path).map_err(|e| vec![CheckResult::fail("io", pl_path.display().to_string(), e.to_string())])?;
            let placed = eda::from_bookshelf_pl(&pl, &d, &cx.model, BOOKSHELF_UNIT_UM)?;
            save_design(&cx.args.out, &placed)?;
            let mut checks = check_placement(&placed, &cx.model);
    checks.extend(eda::preflight(&placed, &cx.model, &cx.model.board));
            let metrics = serde_json::json!({ "hpwl_um": hpwl(&placed, &cx.model), "source": "bookshelf" });
            cx.log.candidate(Stage::Placement, 0, cx.args.seed, &placed, Tier::Geometry, &checks, metrics).ok();
            println!("hpwl_um {}", hpwl(&placed, &cx.model).unwrap_or(-1));
            if !print_checks("placement gates", &checks) {
                return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
            }
            placed
        }
        "check" => {
            let d = prior.ok_or_else(|| vec![CheckResult::fail("cli", "check", "check needs --design")])?;
            let mut ok = true;
            if d.schematic.is_some() {
                ok &= print_checks("schematic gates", &check_erc(&d, &cx.model));
            }
            if d.placement.is_some() {
                ok &= print_checks("placement gates", &{ let mut c = check_placement(&d, &cx.model); c.extend(eda::preflight(&d, &cx.model, &cx.model.board)); c });
            }
            if d.routing.is_some() {
                ok &= print_checks("routing gates", &check_routing(&d, &cx.model));
                // KiCad's own DRC type names (`unconnected_items`,
                // `track_dangling`, `via_dangling`) -- see
                // `eda_connectivity::check`, ported from
                // `DRC_TEST_PROVIDER_CONNECTIVITY::Run`.
                ok &= print_checks("connectivity", &eda_connectivity::check(&d, &cx.model));
            }
            if cx.args.drc {
                ok &= print_drc(&d, &cx.model);
            }
            return if ok { Ok(()) } else { Err(vec![CheckResult::fail("check", "design", "gate failures above")]) };
        }
        other => return Err(vec![CheckResult::fail("cli", other, "unknown command")]),
    };
    save_design(&cx.args.out, &design)?;
    export(cx, &design)?;
    println!("wrote {}", cx.args.out.join("design.json").display());
    Ok(())
}

/// VLM judge on a gate-clean candidate: verdict → CheckResults, logged at
/// Tier::Critic. Fails the run on judge_unavailable or judge fails.
fn run_judge(cx: &mut Ctx, d: &Design, stage: eda_judge::Stage) -> Result<(), Vec<CheckResult>> {
    let opts = eda_judge::JudgeOptions::default();
    let (verdict, checks) = eda_judge::judge_stage(stage, d, &cx.model, &cx.args.out, &opts)?;
    let log_stage = match stage { eda_judge::Stage::Schematic => Stage::Schematic, eda_judge::Stage::Placement => Stage::Placement, eda_judge::Stage::Routing => Stage::Routing };
    let metrics = serde_json::json!({ "judge_score": verdict.score, "rubric_version": verdict.rubric_version, "judge_model": verdict.model, "summary": verdict.summary });
    cx.log.candidate(log_stage, 0, cx.args.seed, d, Tier::Critic, &checks, metrics).ok();
    let label = format!("{} judge ({} {:.1}/10)", stage.name(), verdict.model, verdict.score);
    if !print_checks(&label, &checks) {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    Ok(())
}

fn main() -> ExitCode {
    // `eda board ...` is its own surface: a board held in a directory
    // and advanced one command at a time, rather than a pipeline stage.
    // It is dispatched before the usual argument parsing because its
    // verbs and flags are its own.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("import-kicad") {
        return match import_kicad::run(&argv[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(fails) => {
                for f in &fails {
                    eprintln!("FAIL {} @ {}: {}", f.check, f.location.as_deref().unwrap_or("-"), f.hint.as_deref().unwrap_or(""));
                }
                ExitCode::FAILURE
            }
        };
    }
    if argv.first().map(String::as_str) == Some("board") {
        let r = board::run(&argv[1..], |model| {
            let empty = Design {
                schema: 1,
                provenance: eda_model::ir::Provenance {
                    engine_version: env!("CARGO_PKG_VERSION").into(),
                    intent_hash: String::new(),
                    seed: 0,
                    stage_hashes: vec![],
                },
                schematic: None,
                placement: None,
                routing: None,
                drawings: None,
            };
            seed_outline(&empty, model)
        });
        return match r {
            Ok(()) => ExitCode::SUCCESS,
            Err(fails) => {
                for f in &fails {
                    eprintln!("FAIL {} @ {}: {}", f.check, f.location.as_deref().unwrap_or("-"), f.hint.as_deref().unwrap_or(""));
                }
                ExitCode::FAILURE
            }
        };
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return usage();
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fails) => {
            for f in &fails {
                eprintln!("FAIL {} @ {}: {}", f.check, f.location.as_deref().unwrap_or("-"), f.hint.as_deref().unwrap_or(""));
            }
            ExitCode::FAILURE
        }
    }
}
