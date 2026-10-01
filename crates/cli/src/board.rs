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
//! eda board move U12 --to 42.5,18 -C work
//! eda board check -C work
//! eda board route -C work
//! eda board serve -C work --port 8765
//! ```
//!
//! `serve` shows the board in a browser and lets a person edit it with
//! the same verbs: a drag is `move --to`, a key press `rotate`. Whoever
//! issues a command -- the CLI or the page -- it goes through [`step`]
//! and into `activity.jsonl`, so each sees what the other did.
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

use eda_model::ir::{Point, Shape, Text, TextJustify};
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use eda_ops::{Board, Cmd, Dir, Region};
use std::path::{Path, PathBuf};

/// What a board directory remembers between commands.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Meta {
    /// The intent this board was started from.
    pub(crate) intent: String,
    /// Snap and spacing, kept so every command uses the same grid the
    /// board was created on.
    pub(crate) snap_um: i64,
    pub(crate) spacing_um: i64,
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
pub(crate) fn load(dir: &Path) -> Result<(Meta, eda_model::ir::Design, ConstraintModel), Vec<CheckResult>> {
    let meta: Meta = serde_json::from_str(
        &std::fs::read_to_string(meta_path(dir))
            .map_err(|e| fail("board_no_dir", &dir.display().to_string(), format!("no board here: {e}. Start one with `eda board new`.")))?,
    )
    .map_err(|e| fail("board_bad_meta", "board.json", format!("the board's metadata is unreadable: {e}")))?;
    let mut design: eda_model::ir::Design = serde_json::from_str(
        &std::fs::read_to_string(design_path(dir))
            .map_err(|e| fail("board_no_design", "design.json", format!("the board has no design: {e}")))?,
    )
    .map_err(|e| fail("board_bad_design", "design.json", format!("the design is unreadable: {e}")))?;
    // A design.json written before tracks/vias/zones/shapes/text carried
    // ids has none; back-fill them the same deterministic way a fresh
    // route or a hand-add would get, so every board is addressable from
    // the moment it is opened. A no-op once every id is already set.
    design.assign_missing_ids();
    let mut model: ConstraintModel = serde_yaml::from_str(
        &std::fs::read_to_string(&meta.intent)
            .map_err(|e| fail("board_no_intent", &meta.intent, format!("the intent this board came from is gone: {e}")))?,
    )
    .map_err(|e| fail("board_bad_intent", &meta.intent, format!("the intent does not parse: {e}")))?;
    crate::resolve_footprint_libraries(&mut model);
    crate::resolve_symbol_libraries(&mut model);
    Ok((meta, design, model))
}

pub(crate) fn save(dir: &Path, design: &eda_model::ir::Design) -> Result<(), Vec<CheckResult>> {
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

/// `eda board fill [--zone ID] [--layer L] [--json]`: compute every zone's
/// real fill (`eda_drc::fill::fill_all_zones`, the `eda_zone_filler` port)
/// and report it -- a human-readable per-zone summary by default (net,
/// layer, area, fragment/island count), or the full fragment polygons
/// (mm) with `--json`, the same shape `crates/cli/src/studio.rs`'s
/// `/api/fill` endpoint returns for the UI to draw. Fills are always
/// recomputed here, never read back from a stored "second master".
fn print_fill(design: &eda_model::ir::Design, model: &ConstraintModel, want_zone: Option<&str>, want_layer: Option<&str>, as_json: bool) -> Result<(), Vec<CheckResult>> {
    let zones: &[eda_model::ir::Zone] = design.routing.as_ref().map(|r| r.zones.as_slice()).unwrap_or(&[]);
    let drc_board = eda_drc::board::build(design, model);
    let results = eda_drc::fill::fill_all_zones(&drc_board, &model.board);
    let mm = |v: i64| v as f64 / 1000.0;

    let mut matched = 0usize;
    let mut json_zones = Vec::new();
    for z in zones {
        if want_zone.is_some_and(|id| id != z.id) || want_layer.is_some_and(|l| l != z.layer) {
            continue;
        }
        matched += 1;
        let fill = results.get(&z.id);
        let fragments: Vec<&eda_shape_poly_set::Polygon> = fill.map(|f| f.polys.iter().collect()).unwrap_or_default();
        let area_mm2 = fill.map(|f| f.area() / 1_000_000.0).unwrap_or(0.0);

        if as_json {
            let frags_json: Vec<Vec<[f64; 2]>> = fragments.iter().map(|poly| poly[0].iter().map(|p| [mm(p.x), mm(p.y)]).collect()).collect();
            json_zones.push(serde_json::json!({
                "id": z.id, "net": z.net, "layer": z.layer,
                "area_mm2": area_mm2, "islands": fragments.len(),
                "fragments": frags_json,
            }));
        } else {
            println!("zone {} net={} layer={} area={:.4}mm^2 islands={}", if z.id.is_empty() { "<no-id>" } else { &z.id }, z.net, z.layer, area_mm2, fragments.len());
        }
    }

    if matched == 0 {
        return Err(fail("board_fill", want_zone.or(want_layer).unwrap_or("*"), "no zone matched --zone/--layer"));
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(&json_zones).map_err(|e| fail("fill_encode", "fill", e.to_string()))?);
    }
    Ok(())
}

/// Apply one command and report what it cost, logged as `by` did it.
/// A placement change leaves any routing stale, so it goes. A
/// successful edit also pushes the *pre*-edit design onto the undo
/// stack and clears redo -- the usual "a new edit erases old redos"
/// rule; a refused command changes nothing, so it pushes nothing.
pub(crate) fn step(dir: &Path, cmd: Cmd, strict: bool, by: &str) -> Result<String, Vec<CheckResult>> {
    let line = cmd_line(&cmd);
    let before = std::fs::read_to_string(design_path(dir)).ok().and_then(|s| serde_json::from_str::<eda_model::ir::Design>(&s).ok());
    let r = step_quiet(dir, &cmd, strict);
    match &r {
        Ok(summary) => {
            if let Some(before) = before {
                let _ = push_snapshot(&undo_dir(dir), &before);
                clear_dir(&redo_dir(dir));
            }
            log_activity(dir, by, &line, true, summary)
        }
        Err(e) => log_activity(dir, by, &line, false, &reasons(e)),
    }
    r
}

/// Undo/redo history: two stacks of whole-design snapshots, under the
/// board's own directory. Small on purpose (see the task notes this was
/// written from): no diffing, no branching redo tree, just two stacks of
/// files named by a monotonic nanosecond timestamp so "latest" is a
/// lexical (and numeric) max with no separate counter to maintain.
fn history_root(dir: &Path) -> PathBuf {
    dir.join(".history")
}
fn undo_dir(dir: &Path) -> PathBuf {
    history_root(dir).join("undo")
}
fn redo_dir(dir: &Path) -> PathBuf {
    history_root(dir).join("redo")
}

fn push_snapshot(stack_dir: &Path, design: &eda_model::ir::Design) -> std::io::Result<()> {
    std::fs::create_dir_all(stack_dir)?;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let s = serde_json::to_string(design).unwrap_or_default();
    std::fs::write(stack_dir.join(format!("{t:020}.json")), s)
}

/// The most recent snapshot on the stack, removed from it.
fn pop_snapshot(stack_dir: &Path) -> Option<eda_model::ir::Design> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(stack_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    entries.sort();
    let last = entries.pop()?;
    let design = std::fs::read_to_string(&last).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    let _ = std::fs::remove_file(&last);
    Some(design)
}

fn clear_dir(stack_dir: &Path) {
    let _ = std::fs::remove_dir_all(stack_dir);
}

/// `eda board undo`: back to the state before the last edit. Routing is
/// whatever that snapshot had -- if the edit being undone was itself a
/// route, undo removes the routing along with it, same as any other
/// change.
pub(crate) fn undo(dir: &Path, by: &str) -> Result<String, Vec<CheckResult>> {
    let (_, current, _) = load(dir)?;
    let Some(previous) = pop_snapshot(&undo_dir(dir)) else {
        let msg = "nothing to undo".to_string();
        log_activity(dir, by, "undo", false, &msg);
        return Err(fail("board_no_undo", "undo", msg));
    };
    let _ = push_snapshot(&redo_dir(dir), &current);
    save(dir, &previous)?;
    let msg = "undo: reverted the last edit".to_string();
    log_activity(dir, by, "undo", true, &msg);
    Ok(msg)
}

/// `eda board redo`: re-apply the edit the last undo removed. Anything
/// undone is invalidated the moment a *new* edit happens (see `step`),
/// same as any other undo/redo stack.
pub(crate) fn redo(dir: &Path, by: &str) -> Result<String, Vec<CheckResult>> {
    let (_, current, _) = load(dir)?;
    let Some(next) = pop_snapshot(&redo_dir(dir)) else {
        let msg = "nothing to redo".to_string();
        log_activity(dir, by, "redo", false, &msg);
        return Err(fail("board_no_redo", "redo", msg));
    };
    let _ = push_snapshot(&undo_dir(dir), &current);
    save(dir, &next)?;
    let msg = "redo: re-applied the undone edit".to_string();
    log_activity(dir, by, "redo", true, &msg);
    Ok(msg)
}

/// Every gate result that bears on this board right now: the placement
/// gates `Board::checks` already runs, plus the routing gates when there is
/// any routing to judge. A hand-added track or via goes through
/// `check_routing` exactly like the router's own output does -- same
/// clearance test, same wrong-net test, same function -- so `--strict`
/// refuses a bad one the same way it refuses a bad placement move.
fn all_checks(board: &Board, model: &ConstraintModel) -> Vec<CheckResult> {
    let mut out = board.checks();
    if board.design().routing.is_some() {
        out.extend(eda_gates::check_routing(board.design(), model));
    }
    out
}

fn all_failures(board: &Board, model: &ConstraintModel) -> usize {
    all_checks(board, model).iter().filter(|c| matches!(c.status, CheckStatus::Fail)).count()
}

fn step_quiet(dir: &Path, cmd: &Cmd, strict: bool) -> Result<String, Vec<CheckResult>> {
    let (meta, design, model) = load(dir)?;
    let mut board = Board::new(design, &model, meta.snap_um, meta.spacing_um);
    let before = all_failures(&board, &model);
    let was = board.fork();

    // A refused command is not a crash: it is an answer. The caller
    // asked whether this move is possible and the gates said no, with a
    // reason -- which is exactly the signal a decision layer needs.
    board.apply(cmd)?;

    let after = all_failures(&board, &model);
    eda_ops::episode::record(&was, &model, cmd, before, after, 0);

    if strict && after > before {
        let new: Vec<String> = all_checks(&board, &model)
            .iter()
            .filter(|c| matches!(c.status, eda_model::CheckStatus::Fail))
            .filter(|c| !all_checks(&was, &model).iter().any(|w| w.check == c.check && w.location == c.location && matches!(w.status, eda_model::CheckStatus::Fail)))
            .map(|c| format!("{} @ {}", c.check, c.location.clone().unwrap_or_default()))
            .collect();
        return Err(fail(
            "board_worse",
            &cmd.subjects().join(","),
            format!("that move takes the board from {before} failure(s) to {after} ({}); refused because --strict", new.join("; ")),
        ));
    }

    let (done, placed, failures) = progress(&board, &model);
    let mut design = board.design().clone();
    // Adding copper or graphics by hand must not clear the routing the way
    // a part edit does -- a part edit can move a footprint out from under
    // a track, a copper/drawing edit cannot invalidate anything by
    // construction. See `Cmd::clears_routing`.
    let stale = if cmd.clears_routing() { design.routing.take().is_some() } else { false };
    save(dir, &design)?;
    Ok(format!(
        "{}: {placed}/{} placed ({:.0}%), {failures} failure(s){}{}",
        cmd_name(cmd),
        model.parts.len(),
        done * 100.0,
        match after as i64 - before as i64 {
            0 => String::new(),
            d if d < 0 => format!(", {} closed", -d),
            d => format!(", {d} opened"),
        },
        if stale { "; routing cleared" } else { "" }
    ))
}

/// Who the activity log says issued a CLI command: `EDA_ACTOR`, so a
/// model driving the CLI shows up as itself, or plain "cli".
fn actor() -> String {
    std::env::var("EDA_ACTOR").ok().filter(|a| !a.is_empty()).unwrap_or_else(|| "cli".into())
}

/// The failures' messages, one line.
pub(crate) fn reasons(e: &[CheckResult]) -> String {
    e.iter().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).collect::<Vec<_>>().join("; ")
}

/// Append what was done, by whom, to the board's `activity.jsonl`.
pub(crate) fn log_activity(dir: &Path, by: &str, line: &str, ok: bool, message: &str) {
    use std::io::Write;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let entry = serde_json::json!({ "t": t as u64, "by": by, "cmd": line, "ok": ok, "message": message });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("activity.jsonl")) {
        let _ = writeln!(f, "{entry}");
    }
}

/// The command as it would be typed after `eda board`.
fn cmd_line(c: &Cmd) -> String {
    let mm = |v: i64| format!("{:.2}", v as f64 / 1000.0);
    let pts = |pts: &[Point]| pts.iter().map(|p| format!("{},{}", mm(p.x), mm(p.y))).collect::<Vec<_>>().join(" ");
    match c {
        Cmd::Place { part, anchor, side } => format!("place {part} --near {anchor} --side {}", side.as_str()),
        Cmd::PlaceEdge { part, edge, fraction } => format!("place {part} --edge {} --along {fraction}", edge.as_str()),
        Cmd::PlaceRegion { part, region } => format!("place {part} --region {}", region.as_str()),
        Cmd::PlaceAt { part, x, y } => format!("place {part} --at {},{}", mm(*x), mm(*y)),
        Cmd::MoveTo { part, x, y } => format!("move {part} --to {},{}", mm(*x), mm(*y)),
        Cmd::Nudge { part, dir, steps } => format!("move {part} --dir {} --steps {steps}", dir.as_str()),
        Cmd::Rotate { part, quarter_turns } => format!("rotate {part} --quarters {quarter_turns}"),
        Cmd::Swap { a, b } => format!("swap {a} {b}"),
        Cmd::Rip { part } => format!("rip {part}"),
        Cmd::Flip { part } => format!("flip {part}"),

        Cmd::AddTrack { net, layer, width, pts: p } => format!("track add --net {net} --layer {layer} --width {} --pts \"{}\"", mm(*width), pts(p)),
        Cmd::DeleteTrack { id } => format!("track delete {id}"),
        Cmd::SetTrackWidth { id, width } => format!("track width {id} --width {}", mm(*width)),

        Cmd::AddVia { net, x, y, drill, diameter, from_layer, to_layer } => {
            format!("via add --net {net} --at {},{} --drill {} --dia {} --from {from_layer} --to-layer {to_layer}", mm(*x), mm(*y), mm(*drill), mm(*diameter))
        }
        Cmd::DeleteVia { id } => format!("via delete {id}"),
        Cmd::MoveVia { id, x, y } => format!("via move {id} --to {},{}", mm(*x), mm(*y)),

        Cmd::AddZone { net, layer, outline } => format!("zone add --net {net} --layer {layer} --pts \"{}\"", pts(outline)),
        Cmd::DeleteZone { id } => format!("zone delete {id}"),

        Cmd::AddShape { shape } => format!("shape add --kind {} --layer {}", shape_kind(shape), shape.layer()),
        Cmd::DeleteShape { id } => format!("shape delete {id}"),
        Cmd::MoveShape { id, dx, dy } => format!("shape move {id} --dx {} --dy {}", mm(*dx), mm(*dy)),

        Cmd::AddText { text } => format!("text add --content {:?} --at {},{} --layer {}", text.content, mm(text.at.x), mm(text.at.y), text.layer),
        Cmd::EditText { id, content, layer, .. } => format!("text edit {id} --content {content:?} --layer {layer}"),
        Cmd::DeleteText { id } => format!("text delete {id}"),
        Cmd::MoveText { id, x, y } => format!("text move {id} --to {},{}", mm(*x), mm(*y)),
    }
}

fn shape_kind(shape: &Shape) -> &'static str {
    match shape {
        Shape::Segment { .. } => "segment",
        Shape::Arc { .. } => "arc",
        Shape::Rect { .. } => "rect",
        Shape::Circle { .. } => "circle",
        Shape::Polygon { .. } => "polygon",
    }
}

/// Route the board as it stands and keep the routing, logged as `by`
/// did it.
pub(crate) fn route_board(dir: &Path, by: &str) -> Result<String, Vec<CheckResult>> {
    let r = (|| {
        let (_, design, model) = load(dir)?;
        let t = std::time::Instant::now();
        let (routed, fails) = eda::route_partial(&design, &model, &model.board);
        let Some(routed) = routed else { return Err(fails) };
        let summary = format!(
            "route: {} track(s), {} via(s) in {:.1?}{}",
            routed.routing.as_ref().map_or(0, |r| r.tracks.len()),
            routed.routing.as_ref().map_or(0, |r| r.vias.len()),
            t.elapsed(),
            if fails.is_empty() { String::new() } else { format!("; {} problem(s): {}", fails.len(), reasons(&fails)) }
        );
        save(dir, &routed)?;
        Ok(summary)
    })();
    match &r {
        Ok(s) => log_activity(dir, by, "route", true, s),
        Err(e) => log_activity(dir, by, "route", false, &reasons(e)),
    }
    r
}

fn cmd_name(c: &Cmd) -> &'static str {
    match c {
        Cmd::Place { .. } | Cmd::PlaceAt { .. } | Cmd::PlaceEdge { .. } | Cmd::PlaceRegion { .. } => "place",
        Cmd::MoveTo { .. } | Cmd::Nudge { .. } => "move",
        Cmd::Rotate { .. } => "rotate",
        Cmd::Swap { .. } => "swap",
        Cmd::Rip { .. } => "rip",
        Cmd::Flip { .. } => "flip",
        Cmd::AddTrack { .. } | Cmd::DeleteTrack { .. } | Cmd::SetTrackWidth { .. } => "track",
        Cmd::AddVia { .. } | Cmd::DeleteVia { .. } | Cmd::MoveVia { .. } => "via",
        Cmd::AddZone { .. } | Cmd::DeleteZone { .. } => "zone",
        Cmd::AddShape { .. } | Cmd::DeleteShape { .. } | Cmd::MoveShape { .. } => "shape",
        Cmd::AddText { .. } | Cmd::EditText { .. } | Cmd::DeleteText { .. } | Cmd::MoveText { .. } => "text",
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

/// A millimetre value, as every position/size flag below takes it.
fn mm_arg(s: &str) -> Result<i64, Vec<CheckResult>> {
    s.trim().parse::<f64>().map(|v| (v * 1000.0).round() as i64).map_err(|_| fail("board_usage", s, "expected a number, in millimetres"))
}

/// "x,y" in millimetres.
fn parse_point_mm(s: &str) -> Result<Point, Vec<CheckResult>> {
    let (x, y) = s.split_once(',').ok_or_else(|| fail("board_usage", s, "expected x,y in millimetres"))?;
    Ok(Point { x: mm_arg(x)?, y: mm_arg(y)? })
}

/// A whitespace-separated run of "x,y" pairs, millimetres: `--pts "0,0 10,0 10,10"`.
fn parse_pts_mm(s: &str) -> Result<Vec<Point>, Vec<CheckResult>> {
    s.split_whitespace().map(parse_point_mm).collect()
}

/// Degrees, as typed; stored as millidegrees, wrapped into 0..360000.
fn parse_angle(s: Option<&str>) -> Result<u32, Vec<CheckResult>> {
    let deg: f64 = s.unwrap_or("0").trim().parse().map_err(|_| fail("board_usage", "--angle", "expected a number, in degrees"))?;
    Ok(((deg * 1000.0).round() as i64).rem_euclid(360_000) as u32)
}

fn parse_shape(kind: &str, layer: String, stroke_width: i64, filled: bool, pts: Vec<Point>) -> Result<Shape, Vec<CheckResult>> {
    Ok(match kind {
        "segment" => {
            let [start, end] = two_pts(&pts, "segment", "start,end")?;
            Shape::Segment { id: String::new(), layer, stroke_width, filled, start, end }
        }
        "arc" => {
            if pts.len() != 3 {
                return Err(fail("board_usage", "--pts", "an arc needs exactly 3 points: start,mid,end"));
            }
            Shape::Arc { id: String::new(), layer, stroke_width, filled, start: pts[0], mid: pts[1], end: pts[2] }
        }
        "rect" => {
            let [start, end] = two_pts(&pts, "rect", "start,end")?;
            Shape::Rect { id: String::new(), layer, stroke_width, filled, start, end }
        }
        "circle" => {
            let [center, end] = two_pts(&pts, "circle", "center,end")?;
            Shape::Circle { id: String::new(), layer, stroke_width, filled, center, end }
        }
        "polygon" => {
            if pts.len() < 3 {
                return Err(fail("board_usage", "--pts", "a polygon needs at least 3 points"));
            }
            Shape::Polygon { id: String::new(), layer, stroke_width, filled, pts }
        }
        other => return Err(fail("board_usage", other, "--kind must be segment, arc, rect, circle or polygon")),
    })
}

fn two_pts(pts: &[Point], kind: &str, names: &str) -> Result<[Point; 2], Vec<CheckResult>> {
    if pts.len() != 2 {
        return Err(fail("board_usage", "--pts", format!("a {kind} needs exactly 2 points: {names}")));
    }
    Ok([pts[0], pts[1]])
}

fn parse_justify(s: Option<&str>) -> Result<TextJustify, Vec<CheckResult>> {
    Ok(match s.unwrap_or("center") {
        "left" => TextJustify::Left,
        "center" | "centre" => TextJustify::Center,
        "right" => TextJustify::Right,
        other => return Err(fail("board_usage", other, "--justify must be left, center or right")),
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
            let mut model: ConstraintModel = serde_yaml::from_str(
                &std::fs::read_to_string(intent).map_err(|e| fail("board_no_intent", intent, e.to_string()))?,
            )
            .map_err(|e| fail("board_bad_intent", intent, e.to_string()))?;
            crate::resolve_footprint_libraries(&mut model);
            crate::resolve_symbol_libraries(&mut model);
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
            let checks = all_checks(&board, &model);
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
            step(&dir, cmd, strict, &actor()).map(|s| eprintln!("{s}"))
        }
        "move" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "move", "usage: eda board move <REF> (--dir north|south|east|west [--steps N] | --to x,y)"))?;
            if let Some(to) = flag(rest, "--to") {
                let (x, y) = to.split_once(',').ok_or_else(|| fail("board_usage", "--to", "--to takes x,y in millimetres"))?;
                let mm = |s: &str| -> Result<i64, Vec<CheckResult>> {
                    s.trim().parse::<f64>().map(|v| (v * 1000.0).round() as i64).map_err(|_| fail("board_usage", "--to", "x and y must be numbers, in millimetres"))
                };
                return step(&dir, Cmd::MoveTo { part, x: mm(x)?, y: mm(y)? }, strict, &actor()).map(|s| eprintln!("{s}"));
            }
            let d = flag(rest, "--dir").ok_or_else(|| fail("board_usage", "move", "move needs --dir north|south|east|west, or --to x,y"))?;
            let steps = flag(rest, "--steps").and_then(|s| s.parse().ok()).unwrap_or(1);
            step(&dir, Cmd::Nudge { part, dir: parse_dir(&d)?, steps }, strict, &actor()).map(|s| eprintln!("{s}"))
        }
        "rotate" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "rotate", "usage: eda board rotate <REF> [--quarters N]"))?;
            let q = flag(rest, "--quarters").and_then(|s| s.parse().ok()).unwrap_or(1);
            step(&dir, Cmd::Rotate { part, quarter_turns: q }, strict, &actor()).map(|s| eprintln!("{s}"))
        }
        "swap" => {
            let a = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "swap", "usage: eda board swap <A> <B>"))?;
            let b = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "swap", "usage: eda board swap <A> <B>"))?;
            step(&dir, Cmd::Swap { a, b }, strict, &actor()).map(|s| eprintln!("{s}"))
        }
        "rip" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "rip", "usage: eda board rip <REF>"))?;
            step(&dir, Cmd::Rip { part }, strict, &actor()).map(|s| eprintln!("{s}"))
        }
        "flip" => {
            let part = rest.get(1).cloned().ok_or_else(|| fail("board_usage", "flip", "usage: eda board flip <REF>"))?;
            step(&dir, Cmd::Flip { part }, strict, &actor()).map(|s| eprintln!("{s}"))
        }
        "track" => match rest.get(1).map(String::as_str).unwrap_or("") {
            "add" => {
                let net = flag(rest, "--net").ok_or_else(|| fail("board_usage", "track add", "needs --net N"))?;
                let layer = flag(rest, "--layer").ok_or_else(|| fail("board_usage", "track add", "needs --layer L"))?;
                let width = mm_arg(&flag(rest, "--width").ok_or_else(|| fail("board_usage", "track add", "needs --width, mm"))?)?;
                let pts = parse_pts_mm(&flag(rest, "--pts").ok_or_else(|| fail("board_usage", "track add", "needs --pts \"x1,y1 x2,y2 ...\", mm"))?)?;
                step(&dir, Cmd::AddTrack { net, layer, width, pts }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "delete" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "track delete", "usage: eda board track delete <id>"))?;
                step(&dir, Cmd::DeleteTrack { id }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "width" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "track width", "usage: eda board track width <id> --width mm"))?;
                let width = mm_arg(&flag(rest, "--width").ok_or_else(|| fail("board_usage", "track width", "needs --width, mm"))?)?;
                step(&dir, Cmd::SetTrackWidth { id, width }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            other => Err(fail("board_usage", other, "usage: eda board track <add|delete|width> ...")),
        },
        "via" => match rest.get(1).map(String::as_str).unwrap_or("") {
            "add" => {
                let net = flag(rest, "--net").ok_or_else(|| fail("board_usage", "via add", "needs --net N"))?;
                let Point { x, y } = parse_point_mm(&flag(rest, "--at").ok_or_else(|| fail("board_usage", "via add", "needs --at x,y, mm"))?)?;
                let drill = mm_arg(&flag(rest, "--drill").ok_or_else(|| fail("board_usage", "via add", "needs --drill, mm"))?)?;
                let diameter = mm_arg(&flag(rest, "--dia").ok_or_else(|| fail("board_usage", "via add", "needs --dia, mm"))?)?;
                let from_layer = flag(rest, "--from").ok_or_else(|| fail("board_usage", "via add", "needs --from LAYER"))?;
                let to_layer = flag(rest, "--to-layer").ok_or_else(|| fail("board_usage", "via add", "needs --to-layer LAYER"))?;
                step(&dir, Cmd::AddVia { net, x, y, drill, diameter, from_layer, to_layer }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "delete" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "via delete", "usage: eda board via delete <id>"))?;
                step(&dir, Cmd::DeleteVia { id }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "move" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "via move", "usage: eda board via move <id> --to x,y"))?;
                let Point { x, y } = parse_point_mm(&flag(rest, "--to").ok_or_else(|| fail("board_usage", "via move", "needs --to x,y, mm"))?)?;
                step(&dir, Cmd::MoveVia { id, x, y }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            other => Err(fail("board_usage", other, "usage: eda board via <add|delete|move> ...")),
        },
        "zone" => match rest.get(1).map(String::as_str).unwrap_or("") {
            "add" => {
                let net = flag(rest, "--net").ok_or_else(|| fail("board_usage", "zone add", "needs --net N"))?;
                let layer = flag(rest, "--layer").ok_or_else(|| fail("board_usage", "zone add", "needs --layer L"))?;
                let outline = parse_pts_mm(&flag(rest, "--pts").ok_or_else(|| fail("board_usage", "zone add", "needs --pts \"x1,y1 x2,y2 ...\", mm"))?)?;
                step(&dir, Cmd::AddZone { net, layer, outline }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "delete" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "zone delete", "usage: eda board zone delete <id>"))?;
                step(&dir, Cmd::DeleteZone { id }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            other => Err(fail("board_usage", other, "usage: eda board zone <add|delete> ...")),
        },
        "fill" => {
            let (_meta, design, model) = load(&dir)?;
            let want_zone = flag(rest, "--zone");
            let want_layer = flag(rest, "--layer");
            let as_json = has(rest, "--json");
            print_fill(&design, &model, want_zone.as_deref(), want_layer.as_deref(), as_json)
        }
        "shape" => match rest.get(1).map(String::as_str).unwrap_or("") {
            "add" => {
                let kind = flag(rest, "--kind").ok_or_else(|| fail("board_usage", "shape add", "needs --kind segment|arc|rect|circle|polygon"))?;
                let layer = flag(rest, "--layer").ok_or_else(|| fail("board_usage", "shape add", "needs --layer L"))?;
                let stroke_width = mm_arg(&flag(rest, "--width").unwrap_or_else(|| "0.15".into()))?;
                let filled = has(rest, "--filled");
                let pts = parse_pts_mm(&flag(rest, "--pts").ok_or_else(|| fail("board_usage", "shape add", "needs --pts \"x,y ...\", mm"))?)?;
                let shape = parse_shape(&kind, layer, stroke_width, filled, pts)?;
                step(&dir, Cmd::AddShape { shape }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "delete" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "shape delete", "usage: eda board shape delete <id>"))?;
                step(&dir, Cmd::DeleteShape { id }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "move" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "shape move", "usage: eda board shape move <id> --dx mm --dy mm"))?;
                let dx = mm_arg(&flag(rest, "--dx").ok_or_else(|| fail("board_usage", "shape move", "needs --dx, mm"))?)?;
                let dy = mm_arg(&flag(rest, "--dy").ok_or_else(|| fail("board_usage", "shape move", "needs --dy, mm"))?)?;
                step(&dir, Cmd::MoveShape { id, dx, dy }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            other => Err(fail("board_usage", other, "usage: eda board shape <add|delete|move> ...")),
        },
        "text" => match rest.get(1).map(String::as_str).unwrap_or("") {
            "add" => {
                let content = flag(rest, "--content").ok_or_else(|| fail("board_usage", "text add", "needs --content \"...\""))?;
                let at = parse_point_mm(&flag(rest, "--at").ok_or_else(|| fail("board_usage", "text add", "needs --at x,y, mm"))?)?;
                let angle = parse_angle(flag(rest, "--angle").as_deref())?;
                let layer = flag(rest, "--layer").ok_or_else(|| fail("board_usage", "text add", "needs --layer L"))?;
                let size_um = mm_arg(&flag(rest, "--size").ok_or_else(|| fail("board_usage", "text add", "needs --size, mm"))?)?;
                let stroke_width = mm_arg(&flag(rest, "--width").unwrap_or_else(|| "0.15".into()))?;
                let justify = parse_justify(flag(rest, "--justify").as_deref())?;
                let mirror = has(rest, "--mirror");
                let text = Text { id: String::new(), content, at, angle, layer, size_um, stroke_width, justify, mirror };
                step(&dir, Cmd::AddText { text }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "edit" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "text edit", "usage: eda board text edit <id> ..."))?;
                let content = flag(rest, "--content").ok_or_else(|| fail("board_usage", "text edit", "needs --content \"...\""))?;
                let angle = parse_angle(flag(rest, "--angle").as_deref())?;
                let layer = flag(rest, "--layer").ok_or_else(|| fail("board_usage", "text edit", "needs --layer L"))?;
                let size_um = mm_arg(&flag(rest, "--size").ok_or_else(|| fail("board_usage", "text edit", "needs --size, mm"))?)?;
                let stroke_width = mm_arg(&flag(rest, "--width").unwrap_or_else(|| "0.15".into()))?;
                let justify = parse_justify(flag(rest, "--justify").as_deref())?;
                let mirror = has(rest, "--mirror");
                step(&dir, Cmd::EditText { id, content, angle, layer, size_um, stroke_width, justify, mirror }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "delete" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "text delete", "usage: eda board text delete <id>"))?;
                step(&dir, Cmd::DeleteText { id }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            "move" => {
                let id = rest.get(2).cloned().ok_or_else(|| fail("board_usage", "text move", "usage: eda board text move <id> --to x,y"))?;
                let Point { x, y } = parse_point_mm(&flag(rest, "--to").ok_or_else(|| fail("board_usage", "text move", "needs --to x,y, mm"))?)?;
                step(&dir, Cmd::MoveText { id, x, y }, strict, &actor()).map(|s| eprintln!("{s}"))
            }
            other => Err(fail("board_usage", other, "usage: eda board text <add|edit|delete|move> ...")),
        },
        "route" => route_board(&dir, &actor()).map(|s| eprintln!("{s}")),
        "undo" => undo(&dir, &actor()).map(|s| eprintln!("{s}")),
        "redo" => redo(&dir, &actor()).map(|s| eprintln!("{s}")),
        "serve" => {
            let port = flag(rest, "--port").and_then(|p| p.parse().ok()).unwrap_or(8765);
            let ui = flag(rest, "--ui").map(PathBuf::from);
            crate::studio::serve(&dir, port, ui)
        }
        other => Err(fail(
            "board_usage",
            other,
            "usage: eda board <new|status|check|place|move|rotate|flip|swap|rip|track|via|zone|fill|shape|text|route|undo|redo|serve> [-C dir] [--strict]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Design, FootprintInstance, PlacementSection, Provenance, Side};
    use eda_model::{Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_cli_board_copper_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A two-part, two-net board, footprints placed directly (no placer
    /// involved) at known coordinates, so a hand-added track's endpoints
    /// can be pointed exactly at a pad. U1 and U2 each have pad 1 on GND
    /// and pad 2 on VCC; U1's pads sit at board (4000,5000) and
    /// (6000,5000).
    fn setup(dir: &Path) {
        let fp = Footprint {
            name: "2PAD".into(),
            pads: vec![
                Pad { number: "1".into(), at: (-1000, 0), size: (800, 800), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None },
                Pad { number: "2".into(), at: (1000, 0), size: (800, 800), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None },
            ],
            courtyard: Some((2000, 1000)),
            model: None,
        };
        let part = |r: &str| Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("2PAD".into()),
            footprint: Some("2PAD".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        };
        let model = ConstraintModel {
            parts: vec![part("U1"), part("U2")],
            nets: vec![Net { name: "GND".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, Net { name: "VCC".into(), pins: vec!["U1.2".into(), "U2.2".into()] }],
            footprints: vec![fp],
            ..Default::default()
        };
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();

        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: Some(PlacementSection {
                outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
                footprints: vec![
                    FootprintInstance { id: "U1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
                    FootprintInstance { id: "U2".into(), at: Point { x: 15_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
                ],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        save(dir, &design).unwrap();
        let meta = Meta { intent: intent_path.display().to_string(), snap_um: 100, spacing_um: 300 };
        std::fs::write(meta_path(dir), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    }

    #[test]
    fn a_hand_added_track_on_the_wrong_net_fails_the_same_gate_autorouted_copper_would() {
        let dir = scratch("wrong_net");
        setup(&dir);
        // U1's pad 2 (VCC) sits at board (6000, 5000); a GND track landed
        // on it is copper of two different nets touching -- exactly what
        // `routing_clearance` exists to catch, whoever drew the track.
        let cmd = Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 2_000, y: 5_000 }, Point { x: 6_000, y: 5_000 }] };
        step(&dir, cmd, false, "test").expect("adding the track itself is legal; only the copper it creates is bad");

        let (meta, design, model) = load(&dir).unwrap();
        let board = Board::new(design, &model, meta.snap_um, meta.spacing_um);
        let checks = all_checks(&board, &model);
        assert!(
            checks.iter().any(|c| c.check == "routing_clearance" && matches!(c.status, CheckStatus::Fail)),
            "a track landing on a foreign-net pad must fail routing_clearance, same as the router's own output would: {checks:?}"
        );

        // Same command with --strict: refused up front instead of merely
        // reported, exactly like a placement move that makes things worse.
        let dir2 = scratch("wrong_net_strict");
        setup(&dir2);
        let cmd = Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 2_000, y: 5_000 }, Point { x: 6_000, y: 5_000 }] };
        let err = step(&dir2, cmd, true, "test").unwrap_err();
        assert_eq!(err[0].check, "board_worse");
    }

    #[test]
    fn adding_copper_does_not_clear_existing_routing_but_moving_a_part_does() {
        let dir = scratch("copper_persists");
        setup(&dir);
        // Below both footprints (courtyards end at y=6000): touches no pad.
        step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 5_000, y: 8_000 }, Point { x: 8_000, y: 8_000 }] }, false, "test").unwrap();
        step(&dir, Cmd::AddVia { net: "GND".into(), x: 8_000, y: 8_000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }, false, "test").unwrap();

        let (_, design, _) = load(&dir).unwrap();
        let rt = design.routing.as_ref().expect("adding a via must not have cleared the track added just before it");
        assert_eq!(rt.tracks.len(), 1);
        assert_eq!(rt.vias.len(), 1);

        // A part edit, in contrast, clears routing exactly as it always has.
        step(&dir, Cmd::MoveTo { part: "U2".into(), x: 16_000, y: 5_000 }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert!(design.routing.is_none(), "moving a part must still clear routing -- it can move a footprint out from under a track");
    }

    /// A `design.json` written before tracks/vias carried ids -- no `id`
    /// key on either -- must still load, and `board::load` must back-fill
    /// ids the same deterministic way `RoutingSection::assign_missing_ids`
    /// does anywhere else, so a board created before this change becomes
    /// addressable the moment it is next opened. Re-saving must not move
    /// the id a second load already saw.
    #[test]
    fn an_old_design_without_ids_loads_gets_ids_and_resaves_stably() {
        let dir = scratch("old_design_ids");
        setup(&dir);
        let old_design_json = serde_json::json!({
            "schema": 1,
            "provenance": {"engine_version": "t", "intent_hash": "x", "seed": 0},
            "placement": {
                "outline": [{"x": 0, "y": 0}, {"x": 20000, "y": 0}, {"x": 20000, "y": 20000}, {"x": 0, "y": 20000}],
                "footprints": [
                    {"id": "U1", "at": {"x": 5000, "y": 5000}, "rot": 0, "side": "top"},
                    {"id": "U2", "at": {"x": 15000, "y": 5000}, "rot": 0, "side": "top"}
                ]
            },
            "routing": {
                "tracks": [{"net": "GND", "layer": "F.Cu", "width": 200, "pts": [{"x": 4000, "y": 5000}, {"x": 14000, "y": 5000}]}],
                "vias": [{"net": "GND", "at": {"x": 9000, "y": 5000}, "drill": 300, "diameter": 600, "from_layer": "F.Cu", "to_layer": "B.Cu"}]
            }
        });
        std::fs::write(design_path(&dir), serde_json::to_string_pretty(&old_design_json).unwrap()).unwrap();

        let (_, design, _) = load(&dir).unwrap();
        let rt = design.routing.as_ref().unwrap();
        assert!(!rt.tracks[0].id.is_empty(), "load() must back-fill a missing track id");
        assert!(!rt.vias[0].id.is_empty(), "load() must back-fill a missing via id");
        let (track_id, via_id) = (rt.tracks[0].id.clone(), rt.vias[0].id.clone());

        save(&dir, &design).unwrap();
        let (_, reloaded, _) = load(&dir).unwrap();
        assert_eq!(reloaded.routing.as_ref().unwrap().tracks[0].id, track_id, "re-saving and reloading must not move the id");
        assert_eq!(reloaded.routing.as_ref().unwrap().vias[0].id, via_id);
    }
}
