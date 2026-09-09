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
}

fn stage_schematic(cx: &mut Ctx) -> Result<Design, Vec<CheckResult>> {
    let opts = EngineOptions { seed: cx.args.seed, intent_hash: cx.ihash.clone(), ..Default::default() };
    let design = derive_schematic(&cx.model, &opts)?;
    let checks = check_schematic(&design, &cx.model);
    cx.log.candidate(Stage::Schematic, 0, cx.args.seed, &design, Tier::Geometry, &checks, serde_json::Value::Null).ok();
    // Always persist the candidate: a failed one is what review reads.
    save_design(&cx.args.out, &design)?;
    let ok = print_checks("schematic gates", &checks);
    let svg = render_schematic(&design, &cx.model)?;
    write(&cx.args.out.join("schematic.svg"), svg.as_bytes())?;
    if !ok {
        return Err(checks.into_iter().filter(|c| c.status == CheckStatus::Fail).collect());
    }
    Ok(design)
}

fn stage_place(cx: &mut Ctx, design: &Design) -> Result<Design, Vec<CheckResult>> {
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
                cx.log.note(line.clone()).ok();
                table.push(line);
            }
        }
    }
    cx.model.board = base_board;
    cx.args.placer = base_placer;
    write(&cx.args.out.join("strategy.txt"), table.join("
").as_bytes())?;
    Err(vec![CheckResult::fail("solve", "design", format!("no strategy inside the allowances passed every gate ({} tried; see strategy.txt)", ladder.len()))])
}

fn stage_route(cx: &mut Ctx, design: &Design) -> Result<Design, Vec<CheckResult>> {
    let routed = match eda::route_partial(design, &cx.model, &cx.model.board, cx.args.seed) {
        (Some(d), fails) if fails.is_empty() => d,
        (partial, fails) => {
            // Persist the partial result for review, then fail the stage.
            if let Some(d) = partial {
                save_design(&cx.args.out, &d)?;
                export(cx, &d).ok();
            }
            return Err(fails);
        }
    };
    // Always persist the candidate: a gate-failed one is what review reads.
    save_design(&cx.args.out, &routed)?;
    let checks = check_routing(&routed, &cx.model);
    let r = routed.routing.as_ref().unwrap();
    let metrics = serde_json::json!({ "tracks": r.tracks.len(), "vias": r.vias.len() });
    cx.log.candidate(Stage::Routing, 0, cx.args.seed, &routed, Tier::Geometry, &checks, metrics).ok();
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
    let mut cx = Ctx { args, model, log, ihash };

    let lint_checks = lint(&cx.model);
    if !print_checks("lint", &lint_checks) {
        return Err(lint_checks);
    }

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
        "schematic" => stage_schematic(&mut cx)?,
        "place" => {
            let base = prior.unwrap_or_else(blank);
            stage_place(&mut cx, &base)?
        }
        "route" => {
            let base = prior.ok_or_else(|| vec![CheckResult::fail("cli", "route", "route needs --design with a placement")])?;
            stage_route(&mut cx, &base)?
        }
        "solve" => {
            let d = stage_schematic(&mut cx)?;
            if cx.args.judge { run_judge(&mut cx, &d, eda_judge::Stage::Schematic)?; }
            stage_solve(&mut cx, &d)?
        }
        "pipeline" => {
            let d = stage_schematic(&mut cx)?;
            if cx.args.judge { run_judge(&mut cx, &d, eda_judge::Stage::Schematic)?; }
            let d = stage_place(&mut cx, &d)?;
            if cx.args.judge { run_judge(&mut cx, &d, eda_judge::Stage::Placement)?; }
            let d = stage_route(&mut cx, &d)?;
            if cx.args.judge { run_judge(&mut cx, &d, eda_judge::Stage::Routing)?; }
            d
        }
        "judge" => {
            let d = prior.ok_or_else(|| vec![CheckResult::fail("cli", "judge", "judge needs --design")])?;
            let mut fails = Vec::new();
            for (present, stage) in [(d.schematic.is_some(), eda_judge::Stage::Schematic), (d.placement.is_some(), eda_judge::Stage::Placement), (d.routing.is_some(), eda_judge::Stage::Routing)] {
                if present {
                    if let Err(f) = run_judge(&mut cx, &d, stage) { fails.extend(f); }
                }
            }
            return if fails.is_empty() { Ok(()) } else { Err(fails) };
        }
        "export" => {
            let d = prior.ok_or_else(|| vec![CheckResult::fail("cli", "export", "export needs --design")])?;
            export(&cx, &d)?;
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
    export(&cx, &design)?;
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
