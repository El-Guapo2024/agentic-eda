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
//! ```

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
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--out" => out = PathBuf::from(it.next().ok_or("-o needs a path")?),
            "--seed" => seed = it.next().ok_or("--seed needs a number")?.parse().map_err(|_| "bad --seed")?,
            "--design" => design = Some(PathBuf::from(it.next().ok_or("--design needs a path")?)),
            "--pcb-cli" => use_pcb_cli = true,
            "--pl" => pl = Some(PathBuf::from(it.next().ok_or("--pl needs a path")?)),
            "--placer" => placer = it.next().ok_or("--placer needs anneal|cypress")?,
            "--judge" => judge = true,
            s if s.starts_with('-') => return Err(format!("unknown flag {s}")),
            s => intent = Some(PathBuf::from(s)),
        }
    }
    Ok(Args { cmd, intent: intent.ok_or("missing intent path")?, out, seed, design, use_pcb_cli, pl, placer, judge })
}

fn usage() -> ExitCode {
    eprintln!("usage: eda <lint|schematic|place|route|pipeline|solve|export|check|import-pl|judge> <intent> [-o out] [--seed N] [--design design.json] [--pl x.pl] [--placer anneal|cypress] [--judge] [--pcb-cli]");
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

fn load_model(args: &Args) -> Result<ConstraintModel, Vec<CheckResult>> {
    let ext = args.intent.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "yaml" | "yml" => {
            let text = std::fs::read_to_string(&args.intent).map_err(|e| vec![CheckResult::fail("io", args.intent.display().to_string(), e.to_string())])?;
            serde_yaml::from_str(&text).map_err(|e| vec![CheckResult::fail("yaml", args.intent.display().to_string(), e.to_string())])
        }
        "json" => {
            let text = std::fs::read_to_string(&args.intent).map_err(|e| vec![CheckResult::fail("io", args.intent.display().to_string(), e.to_string())])?;
            serde_json::from_str(&text).map_err(|e| vec![CheckResult::fail("json", args.intent.display().to_string(), e.to_string())])
        }
        _ => {
            if args.use_pcb_cli {
                import_zen_cli(&args.intent)
            } else {
                import_zen(&args.intent)
            }
        }
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
    let checks = check_schematic(&design, &cx.model);
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

fn stage_place(cx: &mut Ctx, design: &Design) -> Result<Design, Vec<CheckResult>> {
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
            eda::Cypress(o).place(design, &cx.model, cx.args.seed)?
        }
        "anneal" => place(
            design,
            &cx.model,
            &PlaceOptions { seed: cx.args.seed, spacing: sv.place_spacing_um, moves_per_part: sv.place_moves_per_part, snap: sv.place_snap_um, ..Default::default() },
        )?,
        other => return Err(vec![CheckResult::fail("cli", other, "unknown placer (anneal|cypress)")]),
    };
    save_design(&cx.args.out, &placed)?;
    let mut checks = check_placement(&placed, &cx.model);
    checks.extend(eda::preflight(&placed, &cx.model, &cx.model.board));
    let metrics = serde_json::json!({ "hpwl_um": hpwl(&placed, &cx.model) });
    cx.log.candidate(Stage::Placement, 0, cx.args.seed, &placed, Tier::Geometry, &checks, metrics).ok();
    cx.record("placement", &checks, t0);
    if !print_checks("placement gates", &checks) {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    Ok(placed)
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
        let result = stage_place(cx, schematic).and_then(|d| stage_route(cx, &d));
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
    let routed = match eda::route_partial(design, &cx.model, &cx.model.board, cx.args.seed) {
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
    };

    let design = match cx.args.cmd.as_str() {
        "schematic" => stage_schematic(cx)?,
        "place" => {
            let base = prior.unwrap_or_else(blank);
            stage_place(cx, &base)?
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
            let d = stage_place(cx, &d)?;
            if cx.args.judge { run_judge(cx, &d, eda_judge::Stage::Placement)?; }
            let d = stage_route(cx, &d)?;
            if cx.args.judge { run_judge(cx, &d, eda_judge::Stage::Routing)?; }
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
                ok &= print_checks("schematic gates", &check_schematic(&d, &cx.model));
            }
            if d.placement.is_some() {
                ok &= print_checks("placement gates", &{ let mut c = check_placement(&d, &cx.model); c.extend(eda::preflight(&d, &cx.model, &cx.model.board)); c });
            }
            if d.routing.is_some() {
                ok &= print_checks("routing gates", &check_routing(&d, &cx.model));
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
