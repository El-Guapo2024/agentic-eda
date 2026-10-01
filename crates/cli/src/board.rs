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
use eda_ops::{Board, Cmd, Dir, Domain, Region};
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
    // Once a schematic `Cmd` has run on this board, its own derived net
    // list (see `Design::nets`'s doc) is the one netlist -- it overrides
    // whatever the intent file says, exactly the way a real KiCad project
    // only ever trusts its *current* schematic, not the spec it started
    // from. `None` (every board nothing has ever hand-edited, which is
    // every board in this project's own test/parity corpus) leaves
    // `model.nets` exactly as the intent declared it, unchanged from
    // before this field existed.
    if let Some(nets) = design.nets.clone() {
        model.nets = nets;
    }
    // GAPS.md #8: a *published* footprint-library entry overrides whatever
    // the intent/real-library resolution would otherwise give that name --
    // the same "design.json wins over the frozen intent" precedent `nets`
    // above already sets. Un-published entries (the overwhelming common
    // case while a footprint is still being edited) are deliberately left
    // out of `model.footprints` entirely: `Cmd::UpdateFootprintOnBoard` is
    // the one explicit switch that makes an edit visible to anything
    // already on the board -- see `LibraryFootprint::published`'s own doc.
    if let Some(lib) = &design.footprint_library {
        for lib_fp in lib.footprints.iter().filter(|f| f.published) {
            model.footprints.retain(|f| f.name != lib_fp.name);
            model.footprints.push(lib_fp.to_engine_footprint());
        }
    }
    Ok((meta, design, model))
}

/// Recompute `design.schematic`'s own derived connectivity (`Wire::net`/
/// `pins`, `PowerSymbol::pin`, `NoConnect::pin`) from its current drawn
/// geometry, and the net list that implies -- the same union-find
/// reconciliation `import_kicad_sch` runs on a real `.kicad_sch` file
/// (`eda_kicad::reconcile`), run here instead on *this* design's own
/// schematic every time a schematic-domain `Cmd` lands (see `step_quiet`,
/// gated on `Cmd::domain`). A no-op if the design has no schematic at all.
///
/// Also folds a synthesized `Part` into `model` for any symbol instance it
/// does not already know about -- `AddSymbol` placing a part with no
/// intent counterpart, which must still "show up unplaced on the PCB"
/// (the task's own words): pins come from a real resolved library symbol
/// when `lib_id` names one, the same generic-box fallback `schematic_json`
/// already renders with otherwise, same resolution order either way so
/// the pins a wire can land on always match what the frontend drew.
///
/// Writes `design.nets = Some(nets)` -- see that field's own doc comment
/// on why this never touches a board nothing has hand-edited yet (every
/// board in this project's own test/parity corpus included: none of them
/// has ever executed a schematic `Cmd`, so `design.nets` stays `None` for
/// every one of them exactly as before this function existed, and
/// `load`'s `model.nets` override above never fires).
fn reconcile_schematic(design: &mut eda_model::ir::Design, model: &mut ConstraintModel) {
    let Some(sch) = design.schematic.as_mut() else { return };

    let resolve = |lib_id: &str, model: &ConstraintModel| -> Option<eda_model::LibSymbol> {
        if lib_id.is_empty() || eda_model::is_synthetic_lib_id(lib_id) {
            None
        } else {
            model.symbol_of(lib_id)
        }
    };

    for sym in sch.symbols.clone() {
        if model.part(&sym.id).is_some() {
            continue;
        }
        let pins = resolve(&sym.lib_id, model)
            .map(|lib| {
                lib.pins
                    .iter()
                    .map(|p| eda_model::Pin { number: p.number.clone(), name: (!p.name.is_empty()).then(|| p.name.clone()), kind: eda_kicad::pin_kind_from_electrical_type(&p.electrical_type, &p.name) })
                    .collect()
            })
            .unwrap_or_default();
        model.parts.push(eda_model::Part {
            reference: sym.id.clone(),
            mpn: None,
            lcsc: None,
            value: (!sym.value.is_empty()).then(|| sym.value.clone()),
            package: None,
            footprint: (!sym.footprint.is_empty()).then(|| sym.footprint.clone()),
            symbol: (!sym.lib_id.is_empty()).then(|| sym.lib_id.clone()),
            datasheet: (!sym.datasheet.is_empty()).then(|| sym.datasheet.clone()),
            pins,
            body_um: None,
            edge: None,
        });
    }

    let mut pin_world: std::collections::BTreeMap<String, Point> = std::collections::BTreeMap::new();
    for sym in &sch.symbols {
        let lib = resolve(&sym.lib_id, model).unwrap_or_else(|| crate::studio::synthesize_generic_symbol(&format!("eda:{}", sym.id), model));
        let angle_deg = sym.rot as f64 / 1000.0;
        // Multi-unit: this placed instance only seeds `pin_world` for the
        // pins that are actually drawn on its own unit (plus any `unit ==
        // 0` pin, common to every unit) -- a pin belonging to a *different*
        // unit of the same reference is positioned when *that* unit's own
        // `SymbolInstance` is visited, not here. Without this filter, two
        // instances sharing one reference would both claim every pin
        // number the whole part has, and whichever was iterated last would
        // silently win for every pin the other unit actually owns.
        for p in lib.pins.iter().filter(|p| p.unit == 0 || p.unit == sym.unit) {
            let world = eda_kicad::transform_local_point(p.at, angle_deg, sym.mirrored, sym.mirror_y);
            pin_world.insert(format!("{}.{}", sym.id, p.number), Point { x: sym.at.x + eda_kicad::mm_to_um(world.x), y: sym.at.y + eda_kicad::mm_to_um(world.y) });
        }
    }

    let nets = eda_kicad::reconcile(&pin_world, &mut sch.wires, &sch.labels, &mut sch.power_symbols, &mut sch.no_connects);
    design.nets = Some(nets);
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
    let domain = cmd.domain();
    let r = step_quiet(dir, &cmd, strict);
    match &r {
        Ok(summary) => {
            if let Some(before) = before {
                let _ = push_snapshot(&undo_dir(dir), &before, domain);
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
/// lexical (and numeric) max with no separate counter to maintain. Each
/// file's name also carries the `Domain` the command that produced it
/// belonged to (`<timestamp>.<pcb|schematic>.json`) -- a purely additive
/// tag on top of the same one-file-per-edit scheme, not a second stack:
/// see `undo`/`redo`'s own doc for what it is used for and GAPS.md #15
/// for the bug this exists to fix (Ctrl+Z on the Schematic tab silently
/// undoing the last *PCB* edit).
fn history_root(dir: &Path) -> PathBuf {
    dir.join(".history")
}
fn undo_dir(dir: &Path) -> PathBuf {
    history_root(dir).join("undo")
}
fn redo_dir(dir: &Path) -> PathBuf {
    history_root(dir).join("redo")
}

fn domain_tag(d: Domain) -> &'static str {
    match d {
        Domain::Pcb => "pcb",
        Domain::Schematic => "schematic",
        Domain::FootprintEditor => "footprint_editor",
    }
}

fn push_snapshot(stack_dir: &Path, design: &eda_model::ir::Design, domain: Domain) -> std::io::Result<()> {
    std::fs::create_dir_all(stack_dir)?;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let s = serde_json::to_string(design).unwrap_or_default();
    std::fs::write(stack_dir.join(format!("{t:020}.{}.json", domain_tag(domain))), s)
}

/// The most recent snapshot on the stack, removed from it -- the most
/// recent *overall* when `scope` is `None` (every CLI call site: `eda
/// board undo`/`redo` have no notion of "tab" to scope by, so they keep
/// exactly the single-timeline behavior this had before `Domain` existed),
/// or the most recent whose own tag matches `scope` otherwise (every
/// studio HTTP call site, scoped to whichever tab issued the request).
/// Scoping never removes a *different*-domain entry sitting more recently
/// on the stack -- it is left exactly where it is, for a later undo of
/// *that* domain to find.
fn pop_snapshot(stack_dir: &Path, scope: Option<Domain>) -> Option<(Domain, eda_model::ir::Design)> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(stack_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    entries.sort();
    let matches = |p: &Path| -> Option<Domain> {
        let name = p.file_stem()?.to_str()?; // "<timestamp>.<tag>"
        let tag = name.rsplit('.').next()?;
        let domain = match tag {
            "schematic" => Domain::Schematic,
            "footprint_editor" => Domain::FootprintEditor,
            _ => Domain::Pcb,
        };
        (scope.is_none() || scope == Some(domain)).then_some(domain)
    };
    let (idx, domain) = entries.iter().enumerate().rev().find_map(|(i, p)| matches(p).map(|d| (i, d)))?;
    let path = entries.remove(idx);
    let design = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    let _ = std::fs::remove_file(&path);
    Some((domain, design))
}

fn clear_dir(stack_dir: &Path) {
    let _ = std::fs::remove_dir_all(stack_dir);
}

/// Overlay `scope`'s own half of `snapshot` onto `current`, leaving
/// everything else untouched -- `None` is a full restore (the original,
/// single-timeline undo/redo behavior: `snapshot` replaces `current`
/// outright), `Some(Schematic)` touches only `schematic`, `Some(Pcb)`
/// touches everything *except* `schematic`. Symmetric: the exact same
/// function reconstructs the pre-undo state in `redo`, by swapping which
/// side is "current" and which is "the snapshot" -- see that pair's own
/// doc for why this is the whole fix for GAPS.md #15 rather than two
/// independent undo stacks.
fn restore_domain(current: eda_model::ir::Design, snapshot: eda_model::ir::Design, scope: Option<Domain>) -> eda_model::ir::Design {
    match scope {
        None => snapshot,
        Some(Domain::Schematic) => eda_model::ir::Design { schematic: snapshot.schematic, nets: snapshot.nets, ..current },
        // GAPS.md #8: the Footprint Editor's own scope touches only
        // `footprint_library`, same reasoning `Schematic`'s own arm
        // documents -- an undo on that tab must never revert a PCB edit
        // (or vice versa), which is also why `Pcb`'s arm below now
        // excludes `footprint_library` from what it restores too.
        Some(Domain::FootprintEditor) => eda_model::ir::Design { footprint_library: snapshot.footprint_library, ..current },
        Some(Domain::Pcb) => eda_model::ir::Design { schematic: current.schematic, nets: current.nets, footprint_library: current.footprint_library, ..snapshot },
    }
}

/// `eda board undo` (CLI, `scope: None`) or `POST /api/undo` (studio UI,
/// `scope: Some(state.tab)`): back to the state before the last edit --
/// the last edit *of any kind* for the CLI (unchanged from before
/// `Domain` existed), or the last edit *in that tab's own editor* for the
/// UI, so Ctrl+Z on the Schematic tab with nothing drawn there yet is
/// correctly "nothing to undo" instead of reverting whatever was last
/// done on the PCB tab (GAPS.md #15). A `Some(Pcb)` undo still reverts
/// routing the same way an unscoped one always did -- if the edit being
/// undone was itself a route, undo removes the routing along with it.
pub(crate) fn undo(dir: &Path, by: &str, scope: Option<Domain>) -> Result<String, Vec<CheckResult>> {
    let (_, current, _) = load(dir)?;
    let Some((domain, previous)) = pop_snapshot(&undo_dir(dir), scope) else {
        let msg = "nothing to undo".to_string();
        log_activity(dir, by, "undo", false, &msg);
        return Err(fail("board_no_undo", "undo", msg));
    };
    let _ = push_snapshot(&redo_dir(dir), &current, domain);
    let restored = restore_domain(current, previous, scope);
    save(dir, &restored)?;
    let msg = "undo: reverted the last edit".to_string();
    log_activity(dir, by, "undo", true, &msg);
    Ok(msg)
}

/// `eda board redo` / `POST /api/redo` -- see `undo`'s own doc; `scope`
/// means the same thing here. Anything undone is invalidated the moment a
/// new edit happens (`step` clears the whole redo stack, not just one
/// domain's -- see its own doc comment), same as any other undo/redo
/// stack.
pub(crate) fn redo(dir: &Path, by: &str, scope: Option<Domain>) -> Result<String, Vec<CheckResult>> {
    let (_, current, _) = load(dir)?;
    let Some((domain, next)) = pop_snapshot(&redo_dir(dir), scope) else {
        let msg = "nothing to redo".to_string();
        log_activity(dir, by, "redo", false, &msg);
        return Err(fail("board_no_redo", "redo", msg));
    };
    let _ = push_snapshot(&undo_dir(dir), &current, domain);
    let restored = restore_domain(current, next, scope);
    save(dir, &restored)?;
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
    let (meta, design, mut model) = load(dir)?;
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
    // `board` borrows `model` for its whole lifetime (pad/pin lookups,
    // decoupling pairs); `reconcile_schematic` below needs `model` back
    // mutably (a brand-new symbol can add a `Part` nothing else has heard
    // of yet), so this drop makes that borrow's end explicit rather than
    // relying on NLL to notice `board` is never read again.
    drop(board);
    // Adding copper or graphics by hand must not clear the routing the way
    // a part edit does -- a part edit can move a footprint out from under
    // a track, a copper/drawing edit cannot invalidate anything by
    // construction. See `Cmd::clears_routing`.
    let stale = if cmd.clears_routing() { design.routing.take().is_some() } else { false };
    // Schematic connectivity only needs retracing after a schematic edit
    // -- see `reconcile_schematic`'s own doc on why this is the entire
    // "one netlist" mechanism and why it never touches a PCB-only board.
    if cmd.domain() == Domain::Schematic {
        reconcile_schematic(&mut design, &mut model);
    }
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
        Cmd::SetLabelSide { part, side } => format!("label-side {part} --side {side:?}"),

        Cmd::AddTrack { net, layer, width, pts: p } => format!("track add --net {net} --layer {layer} --width {} --pts \"{}\"", mm(*width), pts(p)),
        Cmd::DeleteTrack { id } => format!("track delete {id}"),
        Cmd::SetTrackWidth { id, width } => format!("track width {id} --width {}", mm(*width)),

        Cmd::AddVia { net, x, y, drill, diameter, from_layer, to_layer } => {
            format!("via add --net {net} --at {},{} --drill {} --dia {} --from {from_layer} --to-layer {to_layer}", mm(*x), mm(*y), mm(*drill), mm(*diameter))
        }
        Cmd::DeleteVia { id } => format!("via delete {id}"),
        Cmd::MoveVia { id, x, y } => format!("via move {id} --to {},{}", mm(*x), mm(*y)),
        Cmd::EditVia { id, diameter, drill } => format!("via edit {id} --dia {} --drill {}", mm(*diameter), mm(*drill)),
        Cmd::SetTrackWidthPresets { widths } => format!("board-setup track-widths \"{}\"", widths.iter().map(|w| mm(*w)).collect::<Vec<_>>().join(" ")),
        Cmd::SetViaPresets { presets } => format!("board-setup via-sizes \"{}\"", presets.iter().map(|p| format!("{}/{}", mm(p.diameter), mm(p.drill))).collect::<Vec<_>>().join(" ")),
        Cmd::EditTracksAndVias { ids, .. } => format!("global-edit tracks-and-vias {}", ids.join(" ")),

        Cmd::AddZone { net, layer, outline } => format!("zone add --net {net} --layer {layer} --pts \"{}\"", pts(outline)),
        Cmd::DeleteZone { id } => format!("zone delete {id}"),
        Cmd::EditZone { id, net, layer, clearance, min_thickness, priority, .. } => {
            format!("zone edit {id} --net {net} --layer {layer} --clearance {} --min-width {} --priority {priority}", mm(*clearance), mm(*min_thickness))
        }
        Cmd::SetZoneOutline { id, outline } => format!("zone outline {id} --pts \"{}\"", pts(outline)),

        Cmd::AddShape { shape } => format!("shape add --kind {} --layer {}", shape_kind(shape), shape.layer()),
        Cmd::DeleteShape { id } => format!("shape delete {id}"),
        Cmd::MoveShape { id, dx, dy } => format!("shape move {id} --dx {} --dy {}", mm(*dx), mm(*dy)),
        Cmd::EditShape { id, layer, stroke_width, filled } => format!("shape edit {id} --layer {layer} --width {} --filled {filled}", mm(*stroke_width)),

        Cmd::AddText { text } => format!("text add --content {:?} --at {},{} --layer {}", text.content, mm(text.at.x), mm(text.at.y), text.layer),
        Cmd::EditText { id, content, layer, .. } => format!("text edit {id} --content {content:?} --layer {layer}"),
        Cmd::DeleteText { id } => format!("text delete {id}"),
        Cmd::MoveText { id, x, y } => format!("text move {id} --to {},{}", mm(*x), mm(*y)),
        Cmd::EditTextAndGraphics { shape_ids, text_ids, .. } => format!("global-edit text-and-graphics --shapes {} --texts {}", shape_ids.len(), text_ids.len()),
        Cmd::SetTeardropSettings { .. } => "board-setup teardrops".to_string(),
        Cmd::AddAllTeardrops => "teardrops add-all".to_string(),
        Cmd::RemoveAllTeardrops => "teardrops remove-all".to_string(),
        Cmd::Group { ids } => format!("group {}", ids.join(" ")),
        Cmd::Ungroup { ids } => format!("ungroup {}", ids.join(" ")),
        Cmd::AddToGroup { group_id, ids } => format!("group add-to {group_id} {}", ids.join(" ")),
        Cmd::RemoveFromGroup { ids } => format!("group remove-from {}", ids.join(" ")),
        Cmd::CreateArray { ids, arrange, .. } => format!("array{} {}", if *arrange { " --arrange" } else { "" }, ids.join(" ")),
        Cmd::AddDimension { .. } => "dimension add".to_string(),
        Cmd::DeleteDimension { id } => format!("dimension delete {id}"),
        Cmd::MoveDimension { id, dx, dy } => format!("dimension move {id} --by {},{}", mm(*dx), mm(*dy)),
        Cmd::EditDimension { id, .. } => format!("dimension edit {id}"),
        Cmd::SetDimensionSettings { .. } => "board-setup dimensions".to_string(),

        Cmd::Duplicate { ids } => format!("duplicate {}", ids.join(" ")),
        Cmd::PasteItems { tracks, vias, zones, shapes, texts } => {
            format!("paste --tracks {} --vias {} --zones {} --shapes {} --texts {}", tracks.len(), vias.len(), zones.len(), shapes.len(), texts.len())
        }
        Cmd::MoveExact { parts, dx, dy, rotate_millideg, pivot } => format!(
            "move-exact {} --by {},{} --rotate {:.3} --pivot {}",
            parts.join(" "),
            mm(*dx),
            mm(*dy),
            *rotate_millideg as f64 / 1000.0,
            pivot.map_or("self".to_string(), |p| format!("{},{}", mm(p.x), mm(p.y)))
        ),

        // No real `eda board` CLI subcommand parses these yet (the studio
        // UI is their only caller so far) -- this text exists purely for
        // activity.jsonl's own human-readable log line, same role `mm`/
        // `pts` already play above for the PCB verbs, not a promise that
        // typing it back in works.
        Cmd::MoveSymbol { id, x, y, .. } => format!("schematic move {id} --to {},{}", mm(*x), mm(*y)),
        Cmd::DragSymbol { id, x, y, .. } => format!("schematic drag {id} --to {},{}", mm(*x), mm(*y)),
        Cmd::RotateSymbol { id, quarter_turns, .. } => format!("schematic rotate {id} --quarters {quarter_turns}"),
        Cmd::MirrorSymbol { id, .. } => format!("schematic mirror {id}"),
        Cmd::MirrorSymbolVertical { id, .. } => format!("schematic mirror-vertical {id}"),
        Cmd::DeleteSymbol { id, .. } => format!("schematic delete-symbol {id}"),
        Cmd::AddWire { pts: p, bus } => format!("schematic {} --pts \"{}\"", if *bus { "bus" } else { "wire" }, pts(p)),
        Cmd::DeleteWire { id } => format!("schematic delete-wire {id}"),
        Cmd::AddNoConnect { at } => format!("schematic no-connect --at {},{}", mm(at.x), mm(at.y)),
        Cmd::DeleteNoConnect { id } => format!("schematic delete-no-connect {id}"),
        Cmd::AddBusEntry { at, size } => format!("schematic bus-entry --at {},{} --size {},{}", mm(at.x), mm(at.y), mm(size.x), mm(size.y)),
        Cmd::DeleteBusEntry { id } => format!("schematic delete-bus-entry {id}"),
        Cmd::AddErcExclusion { check, location } => format!("schematic erc-exclude {check:?} {location:?}"),
        Cmd::DeleteErcExclusion { check, location } => format!("schematic erc-unexclude {check:?} {location:?}"),
        Cmd::AddLabel { net, at, .. } => format!("schematic label {net} --at {},{}", mm(at.x), mm(at.y)),
        Cmd::DeleteLabel { id } => format!("schematic delete-label {id}"),
        Cmd::AddSchText { content, at, .. } => format!("schematic text add --content {content:?} --at {},{}", mm(at.x), mm(at.y)),
        Cmd::DeleteSchText { id } => format!("schematic text delete {id}"),
        Cmd::EditSymbolFields { id, value, footprint, datasheet } => format!(
            "schematic edit-fields {id}{}{}{}",
            value.as_ref().map(|v| format!(" --value {v:?}")).unwrap_or_default(),
            footprint.as_ref().map(|f| format!(" --footprint {f:?}")).unwrap_or_default(),
            datasheet.as_ref().map(|d| format!(" --datasheet {d:?}")).unwrap_or_default(),
        ),
        Cmd::RenameSymbol { id, new_id } => format!("schematic rename {id} {new_id}"),
        Cmd::AddPowerSymbol { lib_id, at, net, rot_millideg, .. } => format!("schematic power {lib_id} --net {net} --at {},{} --rot {:.3}", mm(at.x), mm(at.y), *rot_millideg as f64 / 1000.0),
        Cmd::DeletePowerSymbol { id } => format!("schematic delete-power {id}"),
        Cmd::AddSymbol { id, lib_id, at, .. } => format!("schematic place {id} --lib {lib_id} --at {},{}", mm(at.x), mm(at.y)),
        Cmd::Annotate { reset_existing, order, ids } => format!(
            "schematic annotate{}{}{}",
            if *reset_existing { " --reset" } else { "" },
            match order {
                eda_ops::AnnotateOrder::XThenY => " --order x-then-y",
                eda_ops::AnnotateOrder::YThenX => "",
            },
            ids.as_ref().map(|v| format!(" --selection {}", v.join(","))).unwrap_or_default(),
        ),
        Cmd::CommitRoute { tracks, vias, remove_track_ids, remove_via_ids } => {
            format!("route: +{} track(s) +{} via(s), -{} track(s) -{} via(s)", tracks.len(), vias.len(), remove_track_ids.len(), remove_via_ids.len())
        }

        // GAPS.md #8. No real `eda board` CLI subcommand parses these
        // either (same reasoning the schematic verbs' own comment above
        // gives) -- activity.jsonl's human-readable line only.
        Cmd::OpenFootprintForEdit { name } => format!("footprint open {name:?}"),
        Cmd::DeleteLibraryFootprint { name } => format!("footprint delete {name:?}"),
        Cmd::EditFootprintProperties { name, description, .. } => format!("footprint properties {name:?} --description {description:?}"),
        Cmd::SetFootprintAnchor { name, at } => format!("footprint anchor {name:?} --at {},{}", mm(at.x), mm(at.y)),
        Cmd::UpdateFootprintOnBoard { name } => format!("footprint update-on-board {name:?}"),
        Cmd::AddPad { footprint, pad } => format!("pad add {footprint:?} --number {:?} --at {},{}", pad.number, mm(pad.at.x), mm(pad.at.y)),
        Cmd::MovePad { footprint, id, x, y } => format!("pad move {footprint:?} {id} --to {},{}", mm(*x), mm(*y)),
        Cmd::RotatePad { footprint, id, quarter_turns } => format!("pad rotate {footprint:?} {id} --quarters {quarter_turns}"),
        Cmd::DeletePad { footprint, id } => format!("pad delete {footprint:?} {id}"),
        Cmd::EditPad { footprint, id, pad } => format!("pad edit {footprint:?} {id} --number {:?}", pad.number),
        Cmd::PushPadProperties { footprint, source_pad_id, .. } => format!("pad push-properties {footprint:?} {source_pad_id}"),
        Cmd::RenumberPads { footprint, start, prefix, step } => format!("pad renumber {footprint:?} --start {start} --prefix {prefix:?} --step {step}"),
        Cmd::AddFootprintGraphic { footprint, shape } => format!("footprint-shape add {footprint:?} --kind {} --layer {}", shape_kind(shape), shape.layer()),
        Cmd::DeleteFootprintGraphic { footprint, id } => format!("footprint-shape delete {footprint:?} {id}"),
        Cmd::MoveFootprintGraphic { footprint, id, dx, dy } => format!("footprint-shape move {footprint:?} {id} --dx {} --dy {}", mm(*dx), mm(*dy)),
        Cmd::EditFootprintGraphic { footprint, id, layer, stroke_width, filled } => format!("footprint-shape edit {footprint:?} {id} --layer {layer} --width {} --filled {filled}", mm(*stroke_width)),
        Cmd::AddFootprintText { footprint, text } => format!("footprint-text add {footprint:?} --content {:?} --at {},{}", text.content, mm(text.at.x), mm(text.at.y)),
        Cmd::EditFootprintText { footprint, id, content, layer, .. } => format!("footprint-text edit {footprint:?} {id} --content {content:?} --layer {layer}"),
        Cmd::DeleteFootprintText { footprint, id } => format!("footprint-text delete {footprint:?} {id}"),
        Cmd::MoveFootprintText { footprint, id, x, y } => format!("footprint-text move {footprint:?} {id} --to {},{}", mm(*x), mm(*y)),
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
        Cmd::SetLabelSide { .. } => "label-side",
        Cmd::AddTrack { .. } | Cmd::DeleteTrack { .. } | Cmd::SetTrackWidth { .. } => "track",
        Cmd::AddVia { .. } | Cmd::DeleteVia { .. } | Cmd::MoveVia { .. } | Cmd::EditVia { .. } => "via",
        Cmd::SetTrackWidthPresets { .. } | Cmd::SetViaPresets { .. } => "board-setup",
        Cmd::EditTracksAndVias { .. } => "global-edit-tracks-and-vias",
        Cmd::AddZone { .. } | Cmd::DeleteZone { .. } | Cmd::EditZone { .. } | Cmd::SetZoneOutline { .. } => "zone",
        Cmd::AddShape { .. } | Cmd::DeleteShape { .. } | Cmd::MoveShape { .. } | Cmd::EditShape { .. } => "shape",
        Cmd::AddText { .. } | Cmd::EditText { .. } | Cmd::DeleteText { .. } | Cmd::MoveText { .. } => "text",
        Cmd::EditTextAndGraphics { .. } => "global-edit-text-and-graphics",
        Cmd::SetTeardropSettings { .. } => "board-setup-teardrops",
        Cmd::AddAllTeardrops => "teardrops-add-all",
        Cmd::RemoveAllTeardrops => "teardrops-remove-all",
        Cmd::Group { .. } => "group",
        Cmd::Ungroup { .. } => "ungroup",
        Cmd::AddToGroup { .. } => "group-add-to",
        Cmd::RemoveFromGroup { .. } => "group-remove-from",
        Cmd::CreateArray { .. } => "create-array",
        Cmd::AddDimension { .. } => "dimension-add",
        Cmd::DeleteDimension { .. } => "dimension-delete",
        Cmd::MoveDimension { .. } => "dimension-move",
        Cmd::EditDimension { .. } => "dimension-edit",
        Cmd::SetDimensionSettings { .. } => "dimension-settings",
        Cmd::Duplicate { .. } | Cmd::PasteItems { .. } => "duplicate",
        Cmd::CommitRoute { .. } => "route",
        Cmd::MoveExact { .. } => "move-exact",

        Cmd::MoveSymbol { .. } | Cmd::DragSymbol { .. } => "schematic-move",
        Cmd::RotateSymbol { .. } => "schematic-rotate",
        Cmd::MirrorSymbol { .. } => "schematic-mirror",
        Cmd::MirrorSymbolVertical { .. } => "schematic-mirror-vertical",
        Cmd::DeleteSymbol { .. } => "schematic-delete-symbol",
        Cmd::AddWire { .. } | Cmd::DeleteWire { .. } => "schematic-wire",
        Cmd::AddNoConnect { .. } | Cmd::DeleteNoConnect { .. } => "schematic-no-connect",
        Cmd::AddBusEntry { .. } | Cmd::DeleteBusEntry { .. } => "schematic-bus-entry",
        Cmd::AddErcExclusion { .. } | Cmd::DeleteErcExclusion { .. } => "schematic-erc-exclusion",
        Cmd::AddLabel { .. } | Cmd::DeleteLabel { .. } => "schematic-label",
        Cmd::AddSchText { .. } | Cmd::DeleteSchText { .. } => "schematic-text",
        Cmd::AddPowerSymbol { .. } | Cmd::DeletePowerSymbol { .. } => "schematic-power",
        Cmd::AddSymbol { .. } => "schematic-place",
        Cmd::EditSymbolFields { .. } => "schematic-edit-fields",
        Cmd::RenameSymbol { .. } => "schematic-rename",
        Cmd::Annotate { .. } => "schematic-annotate",

        Cmd::OpenFootprintForEdit { .. } | Cmd::DeleteLibraryFootprint { .. } | Cmd::EditFootprintProperties { .. } | Cmd::SetFootprintAnchor { .. } | Cmd::UpdateFootprintOnBoard { .. } => "footprint",
        Cmd::AddPad { .. } | Cmd::MovePad { .. } | Cmd::RotatePad { .. } | Cmd::DeletePad { .. } | Cmd::EditPad { .. } | Cmd::PushPadProperties { .. } | Cmd::RenumberPads { .. } => "pad",
        Cmd::AddFootprintGraphic { .. } | Cmd::DeleteFootprintGraphic { .. } | Cmd::MoveFootprintGraphic { .. } | Cmd::EditFootprintGraphic { .. } => "footprint-shape",
        Cmd::AddFootprintText { .. } | Cmd::EditFootprintText { .. } | Cmd::DeleteFootprintText { .. } | Cmd::MoveFootprintText { .. } => "footprint-text",
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
        // CLI undo/redo has no "tab" to scope by -- `None` is the original,
        // single-timeline behavior (the most recent edit of any kind),
        // unchanged from before `Domain`-scoped undo existed for the
        // studio UI's own `/api/undo`/`/api/redo` (see their own doc).
        "undo" => undo(&dir, &actor(), None).map(|s| eprintln!("{s}")),
        "redo" => redo(&dir, &actor(), None).map(|s| eprintln!("{s}")),
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
    use eda_model::ir::{Design, FootprintInstance, PlacementSection, Provenance, Side, Um};
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
            footprint_library: None, sheet_contents: None, bus_aliases: vec![],
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
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

    // ---------------------------------------------------------- eeschema

    /// A single-pin `LibSymbol` whose one pin sits at local (0,0) -- so a
    /// placed instance's pin lands at *exactly* the instance's own `at`,
    /// with no rotation/pitch arithmetic for a test to get right
    /// independently of `reconcile_schematic`'s own. Two references, R1
    /// and R2, each starting on their own singleton net (so a later merge
    /// is unambiguous), no placement/routing section.
    fn setup_schematic(dir: &Path) {
        let part = |r: &str| Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: None,
            footprint: None,
            symbol: Some("TEST:R".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }],
            body_um: None,
            datasheet: None,
            edge: None,
        };
        let lib = eda_model::LibSymbol {
            lib_id: "TEST:R".into(),
            graphics: vec![],
            pins: vec![eda_model::LibPin { number: "1".into(), name: String::new(), electrical_type: "passive".into(), shape: "line".into(), at: eda_model::symbol::SPoint::new(0.0, 0.0), angle_deg: 0.0, length_mm: 2.54, unit: 1 }],
            power: false,
            in_bom: true,
            on_board: true,
            datasheet: String::new(),
            description: String::new(),
            reference_prefix: "R".into(),
            unit_count: 1,
        };
        let model = ConstraintModel {
            parts: vec![part("R1"), part("R2")],
            nets: vec![Net { name: "N1".into(), pins: vec!["R1.1".into()] }, Net { name: "N2".into(), pins: vec!["R2.1".into()] }],
            symbols: vec![lib],
            ..Default::default()
        };
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();

        let sym = |id: &str, x: Um, y: Um| eda_model::ir::SymbolInstance { id: id.into(), at: Point { x, y }, rot: 0, mirrored: false, mirror_y: false, lib_id: "TEST:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![],
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(eda_model::ir::SchematicSection {
                symbols: vec![sym("R1", 10_000, 10_000), sym("R2", 20_000, 10_000)],
                wires: vec![],
                labels: vec![],
                texts: vec![],
                power_symbols: vec![],
                no_connects: vec![], bus_entries: vec![],
                erc_exclusions: vec![], imported_from_kicad: false,
                title_block: None,
                sheets: vec![],
                instance_overrides: vec![],
            }),
            nets: None,
            // `Board` (crates/ops) always expects a placement section to
            // exist, even an empty one -- the same intermediate state a
            // real `eda board new` leaves a board in before anything is
            // placed -- so a schematic-only `Cmd` through `step()` (which
            // always builds a `Board`, PCB-only or not) does not panic in
            // `Board::placement()`.
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: None,
            drawings: None,
        };
        save(dir, &design).unwrap();
        let meta = Meta { intent: intent_path.display().to_string(), snap_um: 100, spacing_um: 300 };
        std::fs::write(meta_path(dir), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    }

    /// Same two symbols as `setup_schematic`, plus a placement section
    /// that puts R1 on the PCB too -- for the undo/redo domain-scoping
    /// test, which needs one real edit of each kind.
    fn setup_both(dir: &Path) {
        setup_schematic(dir);
        let (_, mut design, _) = load(dir).unwrap();
        design.placement = Some(PlacementSection {
            outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
            footprints: vec![FootprintInstance { id: "R1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 0, side: Side::Top, label: Default::default() }],
            modules: vec![],
        });
        save(dir, &design).unwrap();
    }

    fn net_of<'a>(nets: &'a [Net], pin: &str) -> Option<&'a Net> {
        nets.iter().find(|n| n.pins.iter().any(|p| p == pin))
    }

    /// The hard rule this task was built around: "there must be one
    /// netlist." Drawing a wire between two previously-unconnected pins
    /// must merge them into the same net *in the model `check_erc`/the PCB
    /// ratsnest read* -- not just in the schematic's own drawing -- and
    /// deleting that wire must split them back apart. Exercises `AddWire`
    /// and `DeleteWire` end to end: `step` -> `reconcile_schematic` (fired
    /// because both are `Domain::Schematic`) -> `design.nets` -> `load`'s
    /// override into `model.nets`.
    #[test]
    fn schematic_wire_connects_and_disconnects_pins_on_one_netlist() {
        let dir = scratch("sch_wire_nets");
        setup_schematic(&dir);

        let (_, _, model) = load(&dir).unwrap();
        assert_ne!(net_of(&model.nets, "R1.1").unwrap().name, net_of(&model.nets, "R2.1").unwrap().name, "sanity: R1 and R2 start on separate nets");

        step(&dir, Cmd::AddWire { pts: vec![Point { x: 10_000, y: 10_000 }, Point { x: 20_000, y: 10_000 }], bus: false }, false, "test").expect("R1.1 and R2.1 both sit exactly on this wire's endpoints");

        let (_, design, model) = load(&dir).unwrap();
        assert!(design.nets.is_some(), "a schematic Cmd must persist the reconciled net list onto the design, not just compute it in-memory for this one request");
        let merged = net_of(&model.nets, "R1.1").expect("R1.1 must still be on a net");
        assert_eq!(merged.name, net_of(&model.nets, "R2.1").unwrap().name, "the wire must merge R1 and R2 onto the same net -- this is what 'the PCB ratsnest then follows' depends on");
        assert!(merged.pins.contains(&"R1.1".to_string()) && merged.pins.contains(&"R2.1".to_string()));

        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(sch.wires.len(), 1);
        let wire = &sch.wires[0];
        assert!(!wire.id.is_empty(), "AddWire's id must be backfilled, same as every other addressable item");
        assert_eq!(wire.net, merged.name, "the wire's own `net` field must be filled in by reconciliation, not left blank");
        assert!(wire.pins.contains(&"R1.1".to_string()) && wire.pins.contains(&"R2.1".to_string()));

        // Deleting the wire must split the net back apart -- the "delete
        // splits" half of the same hard rule.
        let wire_id = wire.id.clone();
        step(&dir, Cmd::DeleteWire { id: wire_id }, false, "test").unwrap();
        let (_, design, model) = load(&dir).unwrap();
        assert!(design.schematic.as_ref().unwrap().wires.is_empty());
        assert_ne!(net_of(&model.nets, "R1.1").unwrap().name, net_of(&model.nets, "R2.1").unwrap().name, "deleting the connecting wire must split R1 and R2 back onto separate nets");
    }

    /// GAPS.md #20: `AddWire { bus: true }` and `AddBusEntry` both survive
    /// `reconcile_schematic`'s own in-place mutation of `sch.wires`
    /// (`Wire::bus`/`SchematicSection::bus_entries` are never touched by
    /// `eda_kicad::reconcile` -- it only ever writes `.net`/`.pins`), and
    /// `DeleteBusEntry` removes exactly the one entry by id, the same shape
    /// `DeleteWire`'s own test above already covers for plain wires.
    #[test]
    fn schematic_bus_wire_and_entry_persist_through_reconcile() {
        let dir = scratch("sch_bus_entry");
        setup_schematic(&dir);

        step(&dir, Cmd::AddWire { pts: vec![Point { x: 0, y: 30_000 }, Point { x: 30_000, y: 30_000 }], bus: true }, false, "test").unwrap();
        step(&dir, Cmd::AddBusEntry { at: Point { x: 10_000, y: 30_000 }, size: Point { x: 2_540, y: 2_540 } }, false, "test").unwrap();

        let (_, design, _model) = load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(sch.wires.len(), 1);
        assert!(sch.wires[0].bus, "the wire's own bus flag must survive reconcile_schematic's in-place mutation");
        assert_eq!(sch.bus_entries.len(), 1);
        assert_eq!(sch.bus_entries[0].at, Point { x: 10_000, y: 30_000 });
        assert_eq!(sch.bus_entries[0].size, Point { x: 2_540, y: 2_540 });
        let entry_id = sch.bus_entries[0].id.clone();
        assert!(!entry_id.is_empty(), "AddBusEntry's id must be backfilled, same as every other addressable item");

        step(&dir, Cmd::DeleteBusEntry { id: entry_id }, false, "test").unwrap();
        let (_, design, _model) = load(&dir).unwrap();
        assert!(design.schematic.as_ref().unwrap().bus_entries.is_empty());
    }

    /// The other half of the "one netlist" hard rule `schematic_wire_
    /// connects_and_disconnects_pins_on_one_netlist` doesn't cover: two
    /// same-named labels merge their nets with *no wire at all* between
    /// them (`eda_kicad::sch_import::reconcile`'s own doc: "two same-
    /// named local labels anywhere on the sheet are the same net"),
    /// exactly how a real, spread-out schematic usually ties a rail
    /// together. Deleting one label must not merely remove that one net
    /// name -- once neither label exists, R1 and R2 have no shared
    /// identity left at all and must split back onto two independent
    /// (synthesized) nets, same as the wire-delete case.
    #[test]
    fn schematic_label_merges_nets_with_no_wire_between_them() {
        let dir = scratch("sch_label_nets");
        setup_schematic(&dir);

        let (_, _, model) = load(&dir).unwrap();
        assert_ne!(net_of(&model.nets, "R1.1").unwrap().name, net_of(&model.nets, "R2.1").unwrap().name, "sanity: starts on separate nets");

        // One label, exactly on each pin (no wire needed -- see reconcile's own doc).
        step(&dir, Cmd::AddLabel { net: "SIG".into(), at: Point { x: 10_000, y: 10_000 }, kind: eda_model::ir::LabelKind::Local }, false, "test").unwrap();
        step(&dir, Cmd::AddLabel { net: "SIG".into(), at: Point { x: 20_000, y: 10_000 }, kind: eda_model::ir::LabelKind::Local }, false, "test").unwrap();

        let (_, design, model) = load(&dir).unwrap();
        let merged = net_of(&model.nets, "R1.1").expect("R1.1 must still be on a net");
        assert_eq!(merged.name, "SIG", "the merged net must take the label's own name, not a synthesized one");
        assert_eq!(merged.name, net_of(&model.nets, "R2.1").unwrap().name, "two same-named labels must merge R1 and R2 onto one net with no wire drawn at all");

        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(sch.labels.len(), 2);
        assert!(sch.labels.iter().all(|l| !l.id.is_empty()), "AddLabel's id must be backfilled");

        // Delete one label: the two pins no longer share any identity, so
        // the net must split back apart (not just rename itself).
        let first_label_id = sch.labels[0].id.clone();
        step(&dir, Cmd::DeleteLabel { id: first_label_id }, false, "test").unwrap();
        let (_, design, model) = load(&dir).unwrap();
        assert_eq!(design.schematic.as_ref().unwrap().labels.len(), 1);
        assert_ne!(net_of(&model.nets, "R1.1").unwrap().name, net_of(&model.nets, "R2.1").unwrap().name, "removing one of the two labels must split R1 and R2 back onto separate nets");
    }

    /// `T`: free-standing text is purely cosmetic -- unlike `AddLabel`
    /// above, it must have *no* effect on connectivity at all (it names no
    /// net), while still getting the same id-backfill/undo treatment every
    /// other schematic item gets.
    #[test]
    fn schematic_text_is_added_and_deleted_without_touching_connectivity() {
        let dir = scratch("sch_text");
        setup_schematic(&dir);

        let (_, _, before) = load(&dir).unwrap();
        assert_ne!(net_of(&before.nets, "R1.1").unwrap().name, net_of(&before.nets, "R2.1").unwrap().name, "sanity: starts on separate nets");

        step(&dir, Cmd::AddSchText { content: "Power supply section".into(), at: Point { x: 15_000, y: 5_000 }, angle_millideg: 0, size_um: 1270 }, false, "test").unwrap();

        // Reconciling at all (triggered by *any* Domain::Schematic Cmd, not
        // just this one) is free to rename an unlabeled net to its own
        // synthesized `NET_<n>` -- same as `schematic_wire_connects_and_
        // disconnects_pins_on_one_netlist` above only ever asserts the two
        // pins' nets differ, never a specific name. What free text must
        // never do is *merge* them onto the same net.
        let (_, design, model) = load(&dir).unwrap();
        assert_ne!(net_of(&model.nets, "R1.1").unwrap().name, net_of(&model.nets, "R2.1").unwrap().name, "free text must not merge R1 and R2 onto the same net");

        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(sch.texts.len(), 1);
        let text = &sch.texts[0];
        assert!(!text.id.is_empty(), "AddSchText's id must be backfilled, same as every other addressable schematic item");
        assert_eq!(text.content, "Power supply section");
        assert_eq!(text.at, Point { x: 15_000, y: 5_000 });

        let text_id = text.id.clone();
        step(&dir, Cmd::DeleteSchText { id: text_id }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert!(design.schematic.as_ref().unwrap().texts.is_empty());
    }

    /// `U`: a rename must not silently strand a power-symbol/no-connect
    /// that named the old reference -- every `"REF.PIN"` string anywhere
    /// on the sheet has to follow it. (The wire case is covered
    /// separately, below, with real connected geometry -- a power symbol
    /// or no-connect placed at the exact position of a *wired* pin would
    /// make reconcile's own, unrelated "a no-connect point never
    /// contributes a pin" rule suppress it regardless of what this method
    /// does, so this fixture deliberately uses its own unconnected point
    /// for each, to isolate the rename cascade itself from reconcile's
    /// separate geometric re-derivation.)
    #[test]
    fn rename_symbol_cascades_through_power_symbol_and_no_connect_pin_references() {
        let dir = scratch("sch_rename_cascade");
        setup_schematic(&dir);

        let (_, mut design, _) = load(&dir).unwrap();
        {
            let sch = design.schematic.as_mut().unwrap();
            sch.power_symbols.push(eda_model::ir::PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: Point { x: 50_000, y: 50_000 }, rot: 0, net: "GND".into(), pin: "R1.1".into() });
            sch.no_connects.push(eda_model::ir::NoConnect { id: String::new(), at: Point { x: 60_000, y: 60_000 }, pin: "R1.1".into() });
        }
        save(&dir, &design).unwrap();

        step(&dir, Cmd::RenameSymbol { id: "R1".into(), new_id: "R9".into() }, false, "test").unwrap();

        let (_, design, _) = load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        assert_eq!(sch.symbols.iter().find(|s| s.id == "R9").unwrap().id, "R9");
        assert!(sch.symbols.iter().all(|s| s.id != "R1"), "the old reference must not linger as a second symbol");
        assert_eq!(sch.power_symbols[0].pin, "R9.1");
        assert_eq!(sch.no_connects[0].pin, "R9.1");
    }

    /// The wire half of the same rule, through real connected geometry
    /// instead of a hand-set `pins` string: renaming a symbol one end of
    /// an existing wire lands on must not break that connection.
    #[test]
    fn rename_symbol_keeps_a_connected_wire_connected() {
        let dir = scratch("sch_rename_wire");
        setup_schematic(&dir);
        step(&dir, Cmd::AddWire { pts: vec![Point { x: 10_000, y: 10_000 }, Point { x: 20_000, y: 10_000 }], bus: false }, false, "test").unwrap();

        let (_, design, model) = load(&dir).unwrap();
        let before_net = net_of(&model.nets, "R2.1").unwrap().name.clone();
        assert_eq!(design.schematic.as_ref().unwrap().wires[0].pins.len(), 2, "sanity: the wire starts connected to both pins");

        step(&dir, Cmd::RenameSymbol { id: "R1".into(), new_id: "R9".into() }, false, "test").unwrap();

        let (_, design, model) = load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        assert!(sch.wires[0].pins.contains(&"R9.1".to_string()), "reconcile re-derives the wire's own pins from geometry using the renamed id -- the connection must survive the rename, not just the string");
        assert!(sch.wires[0].pins.contains(&"R2.1".to_string()));
        assert_eq!(net_of(&model.nets, "R9.1").unwrap().name, net_of(&model.nets, "R2.1").unwrap().name, "still the same merged net");
        assert_eq!(net_of(&model.nets, "R2.1").unwrap().name, before_net, "renaming one end must not even need to rename the net itself");
    }

    #[test]
    fn rename_symbol_refuses_a_duplicate_id_and_an_unknown_source() {
        let dir = scratch("sch_rename_refuse");
        setup_schematic(&dir);
        assert!(step(&dir, Cmd::RenameSymbol { id: "R1".into(), new_id: "R2".into() }, false, "test").is_err(), "R2 already names another symbol on the sheet");
        assert!(step(&dir, Cmd::RenameSymbol { id: "R1".into(), new_id: "".into() }, false, "test").is_err(), "a blank reference is refused");
        assert!(step(&dir, Cmd::RenameSymbol { id: "R404".into(), new_id: "R9".into() }, false, "test").is_err(), "no symbol named R404 exists");
        // Renaming to its own current id is a harmless no-op, not an error.
        step(&dir, Cmd::RenameSymbol { id: "R1".into(), new_id: "R1".into() }, false, "test").unwrap();
    }

    /// `X`/`Y`: KiCad's own symbols never carry both mirror flags at once
    /// (only 3 states: none, X, Y) -- turning one axis on must turn the
    /// other off, the same way a real `SetOrientation` call replaces the
    /// whole orientation rather than adding a flag.
    #[test]
    fn mirror_x_and_mirror_y_are_mutually_exclusive() {
        let dir = scratch("sch_mirror_exclusive");
        setup_schematic(&dir);

        step(&dir, Cmd::MirrorSymbol { id: "R1".into(), unit: None }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        let r1 = |d: &eda_model::ir::Design| d.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "R1").unwrap().clone();
        assert!(r1(&design).mirrored && !r1(&design).mirror_y, "X alone sets mirrored");

        step(&dir, Cmd::MirrorSymbolVertical { id: "R1".into(), unit: None }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert!(!r1(&design).mirrored && r1(&design).mirror_y, "Y must clear the X flag it replaces, not add to it");

        step(&dir, Cmd::MirrorSymbol { id: "R1".into(), unit: None }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert!(r1(&design).mirrored && !r1(&design).mirror_y, "and X must clear Y back, symmetrically");

        // Each hotkey is still its own toggle: pressing the same one twice
        // returns to "no mirror", not a stuck state.
        step(&dir, Cmd::MirrorSymbol { id: "R1".into(), unit: None }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert!(!r1(&design).mirrored && !r1(&design).mirror_y);
    }

    /// `dialog_erc.cpp`'s "Exclude this violation"/un-exclude: refuses a
    /// blank location (nothing to key on, same limitation `Exclusions`
    /// itself has), is idempotent (excluding the same finding twice does
    /// not duplicate it), refuses un-excluding something never excluded,
    /// and persists in `design.schematic.erc_exclusions` sorted by
    /// (check, location).
    #[test]
    fn erc_exclusion_add_and_delete() {
        let dir = scratch("sch_erc_exclusion");
        setup_schematic(&dir);

        assert!(
            step(&dir, Cmd::AddErcExclusion { check: "pin_not_connected".into(), location: "".into() }, false, "test").is_err(),
            "a blank location is refused -- nothing to key an exclusion on"
        );

        step(&dir, Cmd::AddErcExclusion { check: "pin_not_connected".into(), location: "R2.1".into() }, false, "test").unwrap();
        step(&dir, Cmd::AddErcExclusion { check: "pin_not_connected".into(), location: "R1.1".into() }, false, "test").unwrap();
        // Adding the same one again is a harmless no-op, not a duplicate.
        step(&dir, Cmd::AddErcExclusion { check: "pin_not_connected".into(), location: "R1.1".into() }, false, "test").unwrap();

        let (_, design, _) = load(&dir).unwrap();
        let exclusions = &design.schematic.as_ref().unwrap().erc_exclusions;
        assert_eq!(exclusions.len(), 2, "the repeated add must not duplicate");
        assert_eq!(
            exclusions,
            &vec![
                eda_model::ir::ErcExclusion { check: "pin_not_connected".into(), location: "R1.1".into() },
                eda_model::ir::ErcExclusion { check: "pin_not_connected".into(), location: "R2.1".into() },
            ],
            "sorted by (check, location), not insertion order"
        );

        assert!(
            step(&dir, Cmd::DeleteErcExclusion { check: "pin_not_connected".into(), location: "R404.1".into() }, false, "test").is_err(),
            "nothing excluded under this key"
        );
        step(&dir, Cmd::DeleteErcExclusion { check: "pin_not_connected".into(), location: "R1.1".into() }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        let exclusions = &design.schematic.as_ref().unwrap().erc_exclusions;
        assert_eq!(exclusions, &vec![eda_model::ir::ErcExclusion { check: "pin_not_connected".into(), location: "R2.1".into() }]);
    }

    /// `E`/`V`/`F`: each field is independently settable -- editing just
    /// the footprint must not reset a value set by an earlier, separate
    /// edit back to blank.
    #[test]
    fn edit_symbol_fields_updates_only_the_fields_given() {
        let dir = scratch("sch_edit_fields");
        setup_schematic(&dir);

        step(&dir, Cmd::EditSymbolFields { id: "R1".into(), value: Some("10k".into()), footprint: None, datasheet: None }, false, "test").unwrap();
        step(&dir, Cmd::EditSymbolFields { id: "R1".into(), value: None, footprint: Some("Resistor_SMD:R_0603".into()), datasheet: None }, false, "test").unwrap();

        let (_, design, _) = load(&dir).unwrap();
        let r1 = design.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "R1").unwrap();
        assert_eq!(r1.value, "10k", "the second edit (footprint only) must not have reset the first edit's value");
        assert_eq!(r1.footprint, "Resistor_SMD:R_0603");
        assert_eq!(r1.datasheet, "", "never touched, stays at its default");
    }

    /// `dialog_annotate.cpp`'s "Order Options": Y-then-X is the default
    /// (top to bottom, left to right as the tiebreak) -- same numbering
    /// this project always did before `AnnotateOrder` existed, confirmed
    /// with two points chosen so the two orders actually disagree (A is
    /// left-of and below B: Y-then-X puts B first, X-then-Y puts A
    /// first).
    #[test]
    fn annotate_order_default_is_y_then_x_and_x_then_y_is_the_opposite_here() {
        let dir = scratch("sch_annotate_order");
        setup_schematic(&dir);
        let (_, mut design, _) = load(&dir).unwrap();
        {
            let sch = design.schematic.as_mut().unwrap();
            // Neither point may collide with setup_schematic's own R1
            // (10_000,10_000)/R2 (20_000,10_000) -- `.find(|s| s.at == ...)`
            // below would silently match the wrong symbol otherwise.
            sch.symbols.push(eda_model::ir::SymbolInstance { id: "C?".into(), at: Point { x: 5_000, y: 50_000 }, rot: 0, mirrored: false, mirror_y: false, lib_id: "TEST:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() }); // "A": further left, further down
            sch.symbols.push(eda_model::ir::SymbolInstance { id: "C?".into(), at: Point { x: 60_000, y: 40_000 }, rot: 0, mirrored: false, mirror_y: false, lib_id: "TEST:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() }); // "B": further right, further up
        }
        save(&dir, &design).unwrap();

        step(&dir, Cmd::Annotate { reset_existing: false, order: eda_ops::AnnotateOrder::YThenX, ids: None }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        let a = sch.symbols.iter().find(|s| s.at == Point { x: 5_000, y: 50_000 }).unwrap();
        let b = sch.symbols.iter().find(|s| s.at == Point { x: 60_000, y: 40_000 }).unwrap();
        assert_eq!(b.id, "C1", "Y-then-X: B (lower Y) is numbered first");
        assert_eq!(a.id, "C2");

        // Reset and redo with X-then-Y -- the order must flip.
        step(&dir, Cmd::Annotate { reset_existing: true, order: eda_ops::AnnotateOrder::XThenY, ids: None }, false, "test").unwrap();
        let (_, design, _) = load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        let a = sch.symbols.iter().find(|s| s.at == Point { x: 5_000, y: 50_000 }).unwrap();
        let b = sch.symbols.iter().find(|s| s.at == Point { x: 60_000, y: 40_000 }).unwrap();
        assert_eq!(a.id, "C1", "X-then-Y: A (lower X) is numbered first -- the opposite of the Y-then-X result above");
        assert_eq!(b.id, "C2");
    }

    /// `dialog_annotate.cpp`'s "Selection" scope: only the named symbols
    /// are reset/renumbered, everything else on the sheet is left alone
    /// even if `reset_existing` is set.
    #[test]
    fn annotate_with_an_explicit_id_list_only_touches_those_symbols() {
        let dir = scratch("sch_annotate_selection");
        setup_schematic(&dir);
        step(&dir, Cmd::AddSymbol { id: "R3".into(), lib_id: "TEST:R".into(), at: Point { x: 30_000, y: 10_000 }, rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 }, false, "test").unwrap();
        step(&dir, Cmd::AddSymbol { id: "R4".into(), lib_id: "TEST:R".into(), at: Point { x: 40_000, y: 10_000 }, rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 }, false, "test").unwrap();

        step(&dir, Cmd::Annotate { reset_existing: true, order: eda_ops::AnnotateOrder::default(), ids: Some(vec!["R3".into()]) }, false, "test").unwrap();

        let (_, design, _) = load(&dir).unwrap();
        let sch = design.schematic.as_ref().unwrap();
        assert!(sch.symbols.iter().any(|s| s.id == "R1") && sch.symbols.iter().any(|s| s.id == "R2"), "R1/R2 were never in scope -- must be untouched");
        assert!(sch.symbols.iter().any(|s| s.id == "R4"), "R4 was never in scope either, even though it's the same prefix as the one being reset");
        // R3 was in scope: reset to "R?", then renumbered to the next free
        // "R" number -- which is 5, not 3, since R4 (number 4, untouched
        // and out of scope) is still on the sheet contributing to "next
        // free" -- `annotate` fills the next free slot, it does not
        // backfill gaps, same as before this field existed.
        assert_eq!(sch.symbols.iter().find(|s| s.at == Point { x: 30_000, y: 10_000 }).unwrap().id, "R5");
        assert!(!sch.symbols.iter().any(|s| s.id.ends_with('?')), "nothing should be left unannotated after the pass");
    }

    /// GAPS.md #15: "pressing Ctrl+Z while viewing the Schematic tab
    /// silently undoes the last PCB edit instead of being a no-op." Proves
    /// the fix -- `board::undo`/`redo`'s `Domain` scope -- the long way:
    /// one real edit of each kind, then check every combination of which
    /// scope's undo/redo touches which edit.
    #[test]
    fn undo_redo_are_scoped_to_the_tab_that_asked() {
        let dir = scratch("domain_scoped_undo");
        setup_both(&dir);

        step(&dir, Cmd::MoveTo { part: "R1".into(), x: 11_000, y: 10_000 }, false, "test").unwrap();
        step(&dir, Cmd::MoveSymbol { id: "R2".into(), x: 25_000, y: 10_000, unit: None }, false, "test").unwrap();

        let pcb_at = |d: &eda_model::ir::Design| d.placement.as_ref().unwrap().footprints[0].at;
        let sch_at = |d: &eda_model::ir::Design| d.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "R2").unwrap().at;

        let (_, design, _) = load(&dir).unwrap();
        assert_eq!(pcb_at(&design), Point { x: 11_000, y: 10_000 });
        assert_eq!(sch_at(&design), Point { x: 25_000, y: 10_000 });

        // The schematic edit is the most recent on the *shared* timeline --
        // before this fix, an unscoped undo (what every call site used)
        // would have reverted it correctly here, but there would be no way
        // to *also* reach the PCB edit without first undoing the
        // schematic one, and no way to undo "nothing more on this tab"
        // without silently reverting the other tab instead. Scoped: a
        // Schematic undo touches only the schematic symbol.
        undo(&dir, "test", Some(Domain::Schematic)).unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert_eq!(pcb_at(&design), Point { x: 11_000, y: 10_000 }, "a Schematic-tab undo must never touch the PCB tab's edit");
        assert_eq!(sch_at(&design), Point { x: 20_000, y: 10_000 }, "a Schematic-tab undo must revert the schematic move");

        // Nothing left to undo on the Schematic tab: a clean no-op/error,
        // *not* a silent fallback to the PCB tab's edit -- the literal bug
        // this gap describes.
        let err = undo(&dir, "test", Some(Domain::Schematic)).unwrap_err();
        assert_eq!(err[0].check, "board_no_undo");
        let (_, design, _) = load(&dir).unwrap();
        assert_eq!(pcb_at(&design), Point { x: 11_000, y: 10_000 }, "a failed Schematic undo must still never reach the PCB tab's edit");

        // The PCB tab's own undo/redo still works, scoped the same way.
        undo(&dir, "test", Some(Domain::Pcb)).unwrap();
        assert_eq!(pcb_at(&load(&dir).unwrap().1), Point { x: 10_000, y: 10_000 });
        redo(&dir, "test", Some(Domain::Pcb)).unwrap();
        assert_eq!(pcb_at(&load(&dir).unwrap().1), Point { x: 11_000, y: 10_000 });

        // And the Schematic tab's own redo brings its move back, still
        // without disturbing the PCB tab.
        redo(&dir, "test", Some(Domain::Schematic)).unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert_eq!(sch_at(&design), Point { x: 25_000, y: 10_000 });
        assert_eq!(pcb_at(&design), Point { x: 11_000, y: 10_000 });
    }

    // ---------------------------------------------------- footprint editor

    /// GAPS.md #8's `Domain::FootprintEditor` gets the exact same
    /// per-tab-undo treatment `undo_redo_are_scoped_to_the_tab_that_asked`
    /// already proves for Pcb/Schematic: a footprint-library edit and a
    /// PCB edit land in the same session, and each tab's undo must touch
    /// only its own.
    #[test]
    fn footprint_editor_undo_is_scoped_independently_of_pcb_and_schematic() {
        let dir = scratch("footprint_editor_domain_scoped_undo");
        setup(&dir);

        step(&dir, Cmd::OpenFootprintForEdit { name: "Test:FP".into() }, false, "test").unwrap();
        step(&dir, Cmd::MoveTo { part: "U1".into(), x: 6_000, y: 5_000 }, false, "test").unwrap();

        let pcb_at = |d: &eda_model::ir::Design| d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().at;
        let lib_has_fp = |d: &eda_model::ir::Design| d.footprint_library.as_ref().is_some_and(|l| l.by_name("Test:FP").is_some());

        let (_, design, _) = load(&dir).unwrap();
        assert_eq!(pcb_at(&design), Point { x: 6_000, y: 5_000 });
        assert!(lib_has_fp(&design));

        // A Pcb-scoped undo must never touch the footprint library.
        undo(&dir, "test", Some(Domain::Pcb)).unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert_eq!(pcb_at(&design), Point { x: 5_000, y: 5_000 }, "the PCB move must revert");
        assert!(lib_has_fp(&design), "a Pcb-tab undo must never touch the Footprint Editor tab's edit");

        step(&dir, Cmd::MoveTo { part: "U1".into(), x: 6_000, y: 5_000 }, false, "test").unwrap();

        // A FootprintEditor-scoped undo must never touch the PCB tab.
        undo(&dir, "test", Some(Domain::FootprintEditor)).unwrap();
        let (_, design, _) = load(&dir).unwrap();
        assert!(!lib_has_fp(&design), "the OpenFootprintForEdit must revert");
        assert_eq!(pcb_at(&design), Point { x: 6_000, y: 5_000 }, "a FootprintEditor-tab undo must never touch the PCB tab's edit");

        // Nothing left to undo on this tab: a clean error, not a silent
        // fallback onto the PCB tab's stack -- the exact GAPS.md #15 bug
        // this scoping exists to prevent, now also for the third tab.
        let err = undo(&dir, "test", Some(Domain::FootprintEditor)).unwrap_err();
        assert_eq!(err[0].check, "board_no_undo");
    }

    /// `board::load`'s overlay (GAPS.md #8): a *published* library
    /// footprint reaches `ConstraintModel::footprints` (so a part naming
    /// it resolves to the edited pads); an unpublished one -- the ordinary
    /// state while still mid-edit -- must not, keeping "Update Footprint
    /// from Library" an explicit, observable step rather than automatic.
    #[test]
    fn loading_overlays_only_published_footprints_onto_the_model() {
        let dir = scratch("footprint_overlay");
        setup(&dir);
        step(&dir, Cmd::OpenFootprintForEdit { name: "2PAD".into() }, false, "test").unwrap();
        let pad = eda_model::ir::LibraryPad {
            id: String::new(),
            number: "3".into(),
            at: Point { x: 0, y: 2000 },
            offset: Point { x: 0, y: 0 },
            size: (800, 800),
            shape: eda_model::ir::LibraryPadShape::Rect,
            kind: PadKind::Smd,
            drill: None,
            drill_slot: None,
            rot: 0,
            roundrect_ratio: None,
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: eda_model::ir::ChamferCorners::default(),
            layers: vec!["F.Cu".into()],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
        };
        step(&dir, Cmd::AddPad { footprint: "2PAD".into(), pad }, false, "test").unwrap();

        // Still unpublished: the board-wide model keeps resolving "2PAD" to
        // its original two-pad definition, exactly as before this editor
        // touched it -- a part's pads on the PCB tab must not shift while
        // someone is mid-edit in the Footprint Editor tab.
        let (_, _, model) = load(&dir).unwrap();
        assert_eq!(model.footprints.iter().find(|f| f.name == "2PAD").unwrap().pads.len(), 2, "unpublished edits must not reach the board's own model");

        step(&dir, Cmd::UpdateFootprintOnBoard { name: "2PAD".into() }, false, "test").unwrap();
        let (_, _, model) = load(&dir).unwrap();
        assert_eq!(model.footprints.iter().find(|f| f.name == "2PAD").unwrap().pads.len(), 3, "an explicit Update Footprint on Board must republish the edited definition");
    }
}
