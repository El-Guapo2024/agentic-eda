//! A board built one command at a time.
//!
//! `eda place` is one shot: an intent goes in and a finished board comes
//! out, and nothing can get inside the loop. These commands open it up.
//! Each one loads the board, applies a single [`eda_ops::Cmd`], lets the
//! gates judge it, and writes it back -- so a caller decides, sees what
//! it cost, and decides again.
//!
//! ```text
//! eda board new examples/ladder/l4_control_hub.yaml -o work
//! eda board status -C work
//! eda board place U4 --region centre -C work
//! eda board place C10 --near U4 --side north -C work
//! eda board move U12 --toward J23 -C work
//! eda board check -C work
//! ```
//!
//! The state lives in the directory: `design.json` is the board and
//! `board.json` remembers which intent it came from, so every command
//! after `new` needs only the directory.
//!
//! # Why this shape
//!
//! It is the action space, exposed. The same verbs the constructive
//! placer issues internally are issued here by whoever is driving, and
//! judged identically -- a command that makes the board worse is
//! reported, and with `--strict` refused. A decision layer driving this
//! CLI and a placer driving `Board` directly are doing the same thing,
//! which means what is learned from one applies to the other.
//!
//! Every command appends to the episode log when `EDA_EPISODE` is set,
//! so a corpus accumulates from ordinary use rather than from a
//! separate harness that would drift from the real thing.

use eda_model::{CheckResult, ConstraintModel};
use eda_ops::{Board, Cmd, Dir, Region};
use std::path::{Path, PathBuf};

/// What a board directory remembers between commands.
#[derive(serde::Serialize, serde::Deserialize)]
struct Meta {
    /// The intent this board was started from.
    intent: String,
    /// Snap and spacing, kept so every command uses the same grid the
    /// board was created on.
    snap_um: i64,
    spacing_um: i64,
}

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join("board.json")
}

fn design_path(dir: &Path) -> PathBuf {
    dir.join("design.json")
}

/// Load the board a directory holds.
fn load(dir: &Path) -> Result<(Meta, eda_model::ir::Design, ConstraintModel), Vec<CheckResult>> {
    let meta: Meta = serde_json::from_str(
        &std::fs::read_to_string(meta_path(dir))
            .map_err(|e| fail("board_no_dir", &dir.display().to_string(), format!("no board here: {e}. Start one with `eda board new`.")))?,
    )
    .map_err(|e| fail("board_bad_meta", "board.json", format!("the board's metadata is unreadable: {e}")))?;
    let design: eda_model::ir::Design = serde_json::from_str(
        &std::fs::read_to_string(design_path(dir))
            .map_err(|e| fail("board_no_design", "design.json", format!("the board has no design: {e}")))?,
    )
    .map_err(|e| fail("board_bad_design", "design.json", format!("the design is unreadable: {e}")))?;
    let model: ConstraintModel = serde_yaml::from_str(
        &std::fs::read_to_string(&meta.intent)
            .map_err(|e| fail("board_no_intent", &meta.intent, format!("the intent this board came from is gone: {e}")))?,
    )
    .map_err(|e| fail("board_bad_intent", &meta.intent, format!("the intent does not parse: {e}")))?;
    Ok((meta, design, model))
}

fn save(dir: &Path, design: &eda_model::ir::Design) -> Result<(), Vec<CheckResult>> {
    let s = serde_json::to_string_pretty(design)
        .map_err(|e| fail("board_encode", "design.json", format!("the design could not be encoded: {e}")))?;
    std::fs::write(design_path(dir), s)
        .map_err(|e| fail("board_write", "design.json", format!("the design could not be written: {e}")))
}

/// How close this board is to finished: everything placed, nothing
/// failing.
///
/// Two things, because either alone misleads. A board 60% placed with
/// every rule satisfied is closer to done than one fully placed with a
/// part stranded across the board, and a wrongness measure alone would
/// score an empty board perfectly.
pub fn progress(board: &Board, model: &ConstraintModel) -> (f64, usize, usize) {
    let placed = board.placed().len();
    let total = model.parts.len().max(1);
    let done = placed as f64 / total as f64;
    (done, placed, board.failures())
}

/// Print the board's status as the view JSON.
fn print_status(board: &Board, model: &ConstraintModel) -> Result<(), Vec<CheckResult>> {
    let v = eda_ops::view::view(board, model)?;
    println!("{}", serde_json::to_string_pretty(&v).map_err(|e| fail("view_encode", "status", e.to_string()))?);
    Ok(())
}

/// Apply one command and report what it cost.
fn step(dir: &Path, cmd: Cmd, strict: bool) -> Result<(), Vec<CheckResult>> {
    let (meta, design, model) = load(dir)?;
    let mut board = Board::new(design, &model, meta.snap_um, meta.spacing_um);
    let before = board.failures();
    let was = board.fork();

    // A refused command is not a crash: it is an answer. The caller
    // asked whether this move is possible and the gates said no, with a
    // reason -- which is exactly the signal a decision layer needs.
    board.apply(&cmd)?;

    let after = board.failures();
    eda_ops::episode::record(&was, &model, &cmd, before, after, 0);

    if strict && after > before {
        return Err(fail(
            "board_worse",
            &cmd.subjects().join(","),
            format!("that move takes the board from {before} failure(s) to {after}; refused because --strict"),
        ));
    }

    let (done, placed, failures) = progress(&board, &model);
    save(dir, board.design())?;
    eprintln!(
        "{}: {placed}/{} placed ({:.0}%), {failures} failure(s){}",
        cmd_name(&cmd),
        model.parts.len(),
        done * 100.0,
        match after as i64 - before as i64 {
            0 => String::new(),
            d if d < 0 => format!(", {} closed", -d),
            d => format!(", {d} opened"),
        }
    );
    Ok(())
}

fn cmd_name(c: &Cmd) -> &'static str {
    match c {
        Cmd::Place { .. } | Cmd::PlaceAt { .. } | Cmd::PlaceEdge { .. } | Cmd::PlaceRegion { .. } => "place",
        Cmd::Nudge { .. } => "move",
        Cmd::Rotate { .. } => "rotate",
        Cmd::Swap { .. } => "swap",
        Cmd::Rip { .. } => "rip",
    }
}

fn dir_arg(rest: &[String]) -> PathBuf {
    flag(rest, "-C").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn flag(rest: &[String], name: &str) -> Option<String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned()
}

fn has(rest: &[String], name: &str) -> bool {
    rest.iter().any(|a| a == name)
}

fn parse_dir(s: &str) -> Result<Dir, Vec<CheckResult>> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "north" | "n" => Dir::North,
        "south" | "s" => Dir::South,
        "east" | "e" => Dir::East,
        "west" | "w" => Dir::West,
        other => return Err(fail("board_bad_side", other, "side must be north, south, east or west")),
    })
}

fn parse_region(s: &str) -> Result<Region, Vec<CheckResult>> {
    Region::ALL
        .into_iter()
        .find(|r| format!("{r:?}").to_ascii_lowercase() == s.to_ascii_lowercase().replace('-', ""))
        .ok_or_else(|| {
            fail(
                "board_bad_region",
                s,
                "region must be one of centre, north, south, east, west, north-east, north-west, south-east, south-west",
            )
        })
}

/// `eda board <verb> ...`
pub fn run(
    rest: &[String],
    seed_outline: impl Fn(&ConstraintModel) -> Result<eda_model::ir::Design, Vec<CheckResult>>,
) -> Result<(), Vec<CheckResult>> {
    let verb = rest.first().map(String::as_str).unwrap_or("");
    let dir = dir_arg(rest);
    let strict = has(rest, "--strict");

    match verb {
        "new" => {
            let intent = rest.get(1).ok_or_else(|| fail("board_usage", "new", "usage: eda board new <intent.yaml> -o <dir>"))?;
            let out = flag(rest, "-o").map(PathBuf::from).ok_or_else(|| fail("board_usage", "new", "eda board new needs -o <dir>"))?;
            let model: ConstraintModel = serde_yaml::from_str(
                &std::fs::read_to_string(intent).map_err(|e| fail("board_no_intent", intent, e.to_string()))?,
            )
            .map_err(|e| fail("board_bad_intent", intent, e.to_string()))?;
            std::fs::create_dir_all(&out).map_err(|e| fail("board_mkdir", &out.display().to_string(), e.to_string()))?;
            let design = seed_outline(&model)?;
            let abs = std::fs::canonicalize(intent).unwrap_or_else(|_| PathBuf::from(intent));
            let meta = Meta {
                intent: abs.display().to_string(),
                snap_um: model.solver.place_snap_um,
                spacing_um: model.solver.place_spacing_um,
            };
            std::fs::write(meta_path(&out), serde_json::to_string_pretty(&meta).expect("meta encodes"))
                .map_err(|e| fail("board_write", "board.json", e.to_string()))?;
            save(&out, &design)?;
            eprintln!("board: {} part(s) to place in {}", model.parts.len(), out.display());
            Ok(())
        }
        "status" => {
            let (meta, design, model) = load(&dir)?;
            let board = Board::new(design, &model, meta.snap_um, meta.spacing_um);
            print_status(&board, &model)
        }
        "check" => {
            let (meta, design, model) = load(&dir)?;
            let board = Board::new(design, &model, meta.snap_um, meta.spacing_um);
            let checks = board.checks();
            let failed: Vec<CheckResult> = checks.iter().filter(|c| matches!(c.status, eda_model::CheckStatus::Fail)).cloned().collect();
            for c in &failed {
                eprintln!("FAIL {} @ {}: {}", c.check, c.location.clone().unwrap_or_default(), c.hint.clone().unwrap_or_default());
            }
            if failed.is_empty() {
                let (done, placed, _) = progress(&board, &model);
                eprintln!("check: {placed}/{} placed ({:.0}%), no failures", model.parts.len(), done * 100.0);
                Ok(())
            } else {
                Err(failed)
            }
        }
        "place" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "place", "usage: eda board place <REF> [--region R | --edge E | --near P --side S | --at x,y]"))?;
            let cmd = if let Some(r) = flag(rest, "--region") {
                Cmd::PlaceRegion { part, region: parse_region(&r)? }
            } else if let Some(e) = flag(rest, "--edge") {
                let fraction = flag(rest, "--along").map(|f| f.parse::<f64>().unwrap_or(0.5)).unwrap_or(0.5);
                Cmd::PlaceEdge { part, edge: parse_dir(&e)?, fraction }
            } else if let Some(n) = flag(rest, "--near") {
                let side = flag(rest, "--side").ok_or_else(|| fail("board_usage", "place", "--near also needs --side north|south|east|west"))?;
                Cmd::Place { part, anchor: n, side: parse_dir(&side)? }
            } else if let Some(at) = flag(rest, "--at") {
                let (x, y) = at.split_once(',').ok_or_else(|| fail("board_usage", "--at", "--at takes x,y in millimetres"))?;
                let mm = |s: &str| -> Result<i64, Vec<CheckResult>> {
                    s.trim().parse::<f64>().map(|v| (v * 1000.0) as i64).map_err(|_| fail("board_usage", "--at", "x and y must be numbers, in millimetres"))
                };
                Cmd::PlaceAt { part, x: mm(x)?, y: mm(y)? }
            } else {
                return Err(fail("board_usage", "place", "say where: --region R, --edge E, --near P --side S, or --at x,y"));
            };
            step(&dir, cmd, strict)
        }
        "move" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "move", "usage: eda board move <REF> --dir north|south|east|west [--steps N]"))?;
            let d = flag(rest, "--dir").ok_or_else(|| fail("board_usage", "move", "move needs --dir north|south|east|west"))?;
            let steps = flag(rest, "--steps").and_then(|s| s.parse().ok()).unwrap_or(1);
            step(&dir, Cmd::Nudge { part, dir: parse_dir(&d)?, steps }, strict)
        }
        "rotate" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "rotate", "usage: eda board rotate <REF> [--quarters N]"))?;
            let q = flag(rest, "--quarters").and_then(|s| s.parse().ok()).unwrap_or(1);
            step(&dir, Cmd::Rotate { part, quarter_turns: q }, strict)
        }
        "swap" => {
            let a = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "swap", "usage: eda board swap <A> <B>"))?;
            let b = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "swap", "usage: eda board swap <A> <B>"))?;
            step(&dir, Cmd::Swap { a, b }, strict)
        }
        "rip" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "rip", "usage: eda board rip <REF>"))?;
            step(&dir, Cmd::Rip { part }, strict)
        }
        other => Err(fail(
            "board_usage",
            other,
            "usage: eda board <new|status|check|place|move|rotate|swap|rip> [-C dir] [--strict]",
        )),
    }
}
