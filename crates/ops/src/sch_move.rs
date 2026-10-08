//! Move, Drag, Rotate, Mirror and Align to Grid for every kind of schematic item, as undoable verbs (`Cmd::SchMove`).
//!
//! Ported from eeschema at 8303b2ad: `SCH_MOVE_TOOL` (`Main`, `doMoveSelection`, `AlignToGrid`; the drag half is `sch_drag.rs`),
//! `SCH_EDIT_TOOL::Rotate` and `Mirror`, `sch_item_alignment.cpp` (`AlignSchematicItemsToGrid`, `MoveSchematicItem`) and
//! `SCH_ALIGN_TOOL`'s grid rule. One verb moves, turns or mirrors any set of items together, so the studio's M, G, R, X, Y and a
//! click-drag are one `/api/cmd` each and one undo step, on the sheet in view (`Cmd::OnSheet` wraps it like any schematic verb).
//!
//! Differences from the C++, all of them from there being no event loop here:
//! - the move arrives as one offset, not as a stream of mouse positions, so it is applied once (an `x` move, then a `y` move);
//! - a text keeps its anchor side as an angle (the IR has no justification), so a text turned twice is upside down where KiCad
//!   would keep it upright and flip its justification;
//! - `AutoRotateItem` (a placed label picking a spin from the wire) needs the label's "auto rotate on placement" flag, which the IR
//!   does not hold, so it is not run;
//! - a symbol's box for the centre of a selection is its body and pins, without its fields.

use crate::sch_drag::{constrain_on_edge, Drag};
use crate::sch_scene::{inferred_spin, mirror_coord, rotate_point, rotate_vec, Item, Matrix, Scene, F_END, F_SELECTED, F_START};
use crate::{Board, Cmd};
use eda_model::ir::{Point, SchematicSection, SheetInstance, Um};
use eda_model::sch_extras::{LabelSpin, SchGraphic, SchGraphicKind, SchHAlign};
use eda_model::{CheckResult, ConstraintModel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The schematic's connection grid, 50 mil (`GRID_CONNECTABLE`).
pub const GRID_UM: Um = 1_270;

fn d_true() -> bool {
    true
}

/// One item's offset for [`SchMoveCmd::Align`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignMove {
    pub id: String,
    pub dx: Um,
    pub dy: Um,
}

/// Move, drag, turn or mirror schematic items. `ids` are the items' ids: a reference for a symbol (every placed unit; `U1#2` names one),
/// the id of a wire, label, power symbol, text, no-connect, bus entry, junction, graphic line, drawn shape, sheet or sheet pin.
/// On the wire: `{"op": "sch_move", "verb": "drag", "ids": [...], "dx": 2540, "dy": 0}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum SchMoveCmd {
    /// `M` (`SCH_MOVE_TOOL::Main`, `MOVE`): every item moves rigidly by (`dx`, `dy`). A wire attached to a moved pin is not stretched: it
    /// stays where it is (`moveItem` moves both ends of a line in this mode).
    Move {
        ids: Vec<String>,
        dx: Um,
        dy: Um,
        /// R, Shift+R, X and Y pressed while the items were held, in order (`SCH_EDIT_TOOL::Rotate` / `Mirror` during a move): done
        /// to the held items after the move, about `about` (the point they are held at) for a turn, as one step with it.
        #[serde(default)]
        turns: Vec<Turn>,
        #[serde(default)]
        about: Option<Point>,
    },
    /// `G` and a click-drag (`DRAG`): the items move and the wires, labels, junctions and no-connects attached to them follow, a wire
    /// stretching from the item it is on to where the item has gone, with right angles kept (`ortho`, KiCad's 90 and 45 degree line modes;
    /// off is the free angle line mode). `vertices` names, for a wire, which of its points are picked, as a click near one end of a wire
    /// picks that end (`STARTPOINT` / `ENDPOINT`): a wire not in the map is picked whole. `grid` is the distance bends are put at
    /// (0 is the connection grid).
    Drag {
        ids: Vec<String>,
        #[serde(default)]
        vertices: BTreeMap<String, Vec<usize>>,
        dx: Um,
        dy: Um,
        #[serde(default = "d_true")]
        ortho: bool,
        #[serde(default)]
        grid: Um,
        /// As for `Move`: turns made while the items were held; they turn the wires dragged along too.
        #[serde(default)]
        turns: Vec<Turn>,
        #[serde(default)]
        about: Option<Point>,
    },
    /// `R` (counter-clockwise) and Shift+`R` (`ccw` false): one item turns about its own anchor (or, for a shape or sheet, the half-grid
    /// point nearest its centre), several about the half-grid point nearest the centre of the selection; a lone wire turns about its far
    /// end. `about` overrides that point (the cursor, while the selection is being moved).
    Rotate {
        ids: Vec<String>,
        #[serde(default)]
        vertices: BTreeMap<String, Vec<usize>>,
        #[serde(default = "d_true")]
        ccw: bool,
        #[serde(default)]
        about: Option<Point>,
        #[serde(default)]
        grid: Um,
    },
    /// `X` (`vertical` false, mirror left-right) and `Y` (top-bottom): one item about its own anchor (a symbol in place), several about the
    /// half-grid point nearest the centre of the selection.
    Mirror {
        ids: Vec<String>,
        #[serde(default)]
        vertices: BTreeMap<String, Vec<usize>>,
        #[serde(default)]
        vertical: bool,
        #[serde(default)]
        about: Option<Point>,
        #[serde(default)]
        grid: Um,
    },
    /// Align to Grid (`SCH_MOVE_TOOL::AlignToGrid`): each item moves to the nearest grid point (a symbol by where most of its pins are),
    /// taking the wires attached to it along.
    AlignToGrid {
        ids: Vec<String>,
        #[serde(default)]
        grid: Um,
    },
    /// Align Left / Right / Top / Bottom / Center (`SCH_ALIGN_TOOL`): each item moves by its own offset (the caller measured the boxes), snapped
    /// so an item on the connection grid stays on it (`adjustDeltaForGrid`), the wires attached to it stretching.
    Align {
        moves: Vec<AlignMove>,
        #[serde(default)]
        grid: Um,
    },
}

impl SchMoveCmd {
    pub fn ids(&self) -> Vec<&str> {
        match self {
            SchMoveCmd::Move { ids, .. } | SchMoveCmd::Drag { ids, .. } | SchMoveCmd::Rotate { ids, .. } | SchMoveCmd::Mirror { ids, .. } | SchMoveCmd::AlignToGrid { ids, .. } => ids.iter().map(String::as_str).collect(),
            SchMoveCmd::Align { moves, .. } => moves.iter().map(|m| m.id.as_str()).collect(),
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        let mm = |v: Um| format!("{:.2}", v as f64 / 1000.0);
        let list = |ids: &[String]| ids.join(" ");
        match self {
            SchMoveCmd::Move { ids, dx, dy, .. } => format!("schematic move {} --by {},{}", list(ids), mm(*dx), mm(*dy)),
            SchMoveCmd::Drag { ids, dx, dy, .. } => format!("schematic drag {} --by {},{}", list(ids), mm(*dx), mm(*dy)),
            SchMoveCmd::Rotate { ids, ccw, .. } => format!("schematic rotate {} {}", list(ids), if *ccw { "--ccw" } else { "--cw" }),
            SchMoveCmd::Mirror { ids, vertical, .. } => format!("schematic mirror {} {}", list(ids), if *vertical { "--vertical" } else { "--horizontal" }),
            SchMoveCmd::AlignToGrid { ids, .. } => format!("schematic align-to-grid {}", list(ids)),
            SchMoveCmd::Align { moves, .. } => format!("schematic align {}", moves.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(" ")),
        }
    }

    /// The activity kind this verb files under.
    pub fn kind(&self) -> &'static str {
        "schematic-move"
    }

    /// Can this edit change what is connected? Symbols, text and drawn shapes only change place (layout, which never rewrites the net
    /// list: `Cmd::edits_connectivity`); a wire, label, power symbol, junction, no-connect, bus entry or sheet carries connectivity by
    /// itself, so moving one is read from the drawing like drawing it was.
    pub fn edits_connectivity(&self) -> bool {
        self.ids().iter().any(|id| !is_layout_id(id))
    }
}

/// Is `id` the id of an item whose place does not decide what is connected: a symbol reference, a text, a drawn shape or note line?
fn is_layout_id(id: &str) -> bool {
    const CONNECTING: [&str; 8] = ["wire_", "lbl_", "nc_", "bent_", "jct_", "sheet", "shpin_", "dirl_"];
    !id.starts_with('#') && !CONNECTING.iter().any(|p| id.starts_with(p))
}

impl<'a> Board<'a> {
    /// Apply one [`SchMoveCmd`] to the schematic in view.
    pub(crate) fn apply_sch_move(&mut self, cmd: &SchMoveCmd) -> Result<(), Vec<CheckResult>> {
        let model = self.model;
        let out = {
            let sch = self.schematic()?;
            run(sch, model, cmd)?
        };
        *self.schematic_mut()? = out;
        Ok(())
    }
}

fn fail(check: &str, subject: &str, why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, why)]
}

/// The edited section.
pub(crate) fn run(sch: &SchematicSection, model: &ConstraintModel, cmd: &SchMoveCmd) -> Result<SchematicSection, Vec<CheckResult>> {
    let mut scene = Scene::new(sch, model);
    match cmd {
        SchMoveCmd::Move { ids, dx, dy, turns, about } => {
            select(&mut scene, ids, &BTreeMap::new())?;
            move_selection(&mut scene, Point { x: *dx, y: *dy }, turns, *about);
        }
        SchMoveCmd::Drag { ids, vertices, dx, dy, ortho, grid, turns, about } => {
            select(&mut scene, ids, vertices)?;
            drag_selection(&mut scene, Point { x: *dx, y: *dy }, *ortho, grid_of(*grid), turns, *about);
        }
        SchMoveCmd::Rotate { ids, vertices, ccw, about, grid } => {
            select(&mut scene, ids, vertices)?;
            turn_selection(&mut scene, if *ccw { Turn::RotCcw } else { Turn::RotCw }, *about, grid_of(*grid), false);
            finish_turn(&mut scene);
        }
        SchMoveCmd::Mirror { ids, vertices, vertical, about, grid } => {
            select(&mut scene, ids, vertices)?;
            turn_selection(&mut scene, if *vertical { Turn::MirrorV } else { Turn::MirrorH }, *about, grid_of(*grid), false);
            finish_turn(&mut scene);
        }
        SchMoveCmd::AlignToGrid { ids, grid } => {
            select(&mut scene, ids, &BTreeMap::new())?;
            align_to_grid(&mut scene, grid_of(*grid));
        }
        SchMoveCmd::Align { moves, grid } => align(&mut scene, moves, grid_of(*grid))?,
    }
    Ok(scene.finish())
}

fn grid_of(g: Um) -> Um {
    if g > 0 {
        g
    } else {
        GRID_UM
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// the selection

/// Resolve `ids` (and which points of a wire are picked) to the scene's items. A locked item is left out
/// (`FilterSelectionForLockedItems`); no item left is an error, so an edit that would change nothing leaves no undo step.
pub(crate) fn select(scene: &mut Scene, ids: &[String], vertices: &BTreeMap<String, Vec<usize>>) -> Result<(), Vec<CheckResult>> {
    if ids.is_empty() {
        return Err(fail("ops_nothing_selected", "selection", "nothing is selected"));
    }
    let mut locked_skipped = 0usize;
    for id in ids {
        let items = find_items(scene, id);
        if items.is_empty() {
            return Err(fail("ops_unknown_item", id, "no item with this id on the sheet"));
        }
        if scene.sch.extras.is_locked(id) || scene.sch.extras.is_locked(id.split('#').next().unwrap_or(id)) {
            locked_skipped += 1;
            continue;
        }
        for it in items {
            match it {
                Item::Seg(i) => {
                    let Some((wi, k)) = scene.segs[i].from else { continue };
                    let (start, end) = match vertices.get(&scene.meta[wi].id) {
                        Some(vs) => (vs.contains(&k), vs.contains(&(k + 1))),
                        None => (true, true),
                    };
                    if start || end {
                        scene.segs[i].flags |= F_SELECTED | if start { F_START } else { 0 } | if end { F_END } else { 0 };
                    }
                }
                other => {
                    scene.selected.insert(other);
                }
            }
        }
    }
    if scene.in_selection().is_empty() {
        if locked_skipped > 0 {
            return Err(fail("ops_locked", &ids[0], "the selected items are locked"));
        }
        return Err(fail("ops_nothing_selected", "selection", "nothing is selected"));
    }
    Ok(())
}

fn find_items(scene: &Scene, id: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for (wi, m) in scene.meta.iter().enumerate() {
        if m.id == id {
            out.extend(scene.segs.iter().enumerate().filter(|(_, s)| !s.dead && s.from.map(|f| f.0) == Some(wi)).map(|(i, _)| Item::Seg(i)));
        }
    }
    let (reference, unit) = match id.rsplit_once('#') {
        Some((r, u)) if u.parse::<u32>().is_ok() => (r, u.parse::<u32>().ok()),
        _ => (id, None),
    };
    for (i, s) in scene.sch.symbols.iter().enumerate() {
        if (s.id == id) || (s.id == reference && Some(s.unit) == unit) {
            out.push(Item::Symbol(i));
        }
    }
    out.extend(scene.sch.power_symbols.iter().enumerate().filter(|(_, p)| p.id == id).map(|(i, _)| Item::Power(i)));
    out.extend(scene.sch.labels.iter().enumerate().filter(|(_, l)| l.id == id).map(|(i, _)| Item::Label(i)));
    out.extend(scene.sch.texts.iter().enumerate().filter(|(_, t)| t.id == id).map(|(i, _)| Item::Text(i)));
    out.extend(scene.sch.no_connects.iter().enumerate().filter(|(_, n)| n.id == id).map(|(i, _)| Item::NoConnect(i)));
    out.extend(scene.sch.bus_entries.iter().enumerate().filter(|(_, b)| b.id == id).map(|(i, _)| Item::BusEntry(i)));
    out.extend(scene.sch.junctions.iter().enumerate().filter(|(_, j)| j.id == id).map(|(i, _)| Item::Junction(i)));
    out.extend(scene.sch.lines.iter().enumerate().filter(|(_, l)| l.id == id).map(|(i, _)| Item::NoteLine(i)));
    out.extend(scene.sch.extras.graphics.iter().enumerate().filter(|(_, g)| g.id == id).map(|(i, _)| Item::Graphic(i)));
    for (si, s) in scene.sch.sheets.iter().enumerate() {
        if s.id == id {
            out.push(Item::Sheet(si));
        }
        out.extend(s.pins.iter().enumerate().filter(|(_, p)| p.id == id).map(|(pi, _)| Item::SheetPin(si, pi)));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Move

/// Where the connections of the selection are, to look for a junction left behind (`setupItemsForMove`'s `aInternalPoints`).
fn internal_points(scene: &Scene) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::new();
    for it in scene.in_selection() {
        for p in scene.connection_points(it) {
            if !pts.contains(&p) {
                pts.push(p);
            }
        }
    }
    pts
}

/// `initializeMoveOperation`: a junction on a point a selected line ends at that nothing but the line made one is removed ("hidden" while
/// the line is away, and gone for good if it stays away).
fn remove_orphaned_junctions(scene: &mut Scene) {
    let selection: BTreeSet<Item> = scene.in_selection().into_iter().collect();
    let mut remove: Vec<usize> = Vec::new();
    for it in &selection {
        let Item::Seg(i) = it else { continue };
        for p in [scene.segs[*i].a, scene.segs[*i].b] {
            for (ji, j) in scene.sch.junctions.iter().enumerate() {
                if j.at == p && !selection.contains(&Item::Junction(ji)) && !remove.contains(&ji) && !scene.analyze_point(p, false, &selection).is_junction {
                    remove.push(ji);
                }
            }
        }
    }
    if !remove.is_empty() {
        let keep: Vec<bool> = (0..scene.sch.junctions.len()).map(|i| !remove.contains(&i)).collect();
        scene.retain_junctions(&keep);
    }
}

fn move_selection(scene: &mut Scene, delta: Point, turns: &[Turn], about: Option<Point>) {
    let internal = internal_points(scene);
    remove_orphaned_junctions(scene);
    for it in scene.in_selection() {
        scene.move_item(it, delta);
    }
    for &t in turns {
        turn_selection(scene, t, about, GRID_UM, true);
    }
    scene.finalize(None, &internal);
}

fn drag_selection(scene: &mut Scene, delta: Point, ortho: bool, grid: Um, turns: &[Turn], about: Option<Point>) {
    let mut st = Drag { ortho, grid, ..Default::default() };
    scene.setup_items_for_drag(&mut st);
    let internal = internal_points(scene);
    remove_orphaned_junctions(scene);
    scene.perform_item_move(&mut st, delta, true);
    for &t in turns {
        turn_selection(scene, t, about, GRID_UM, true);
    }
    scene.finalize(Some(&st), &internal);
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Rotate and Mirror

/// One quarter turn or mirror of a held selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    /// `R`: counter-clockwise.
    RotCcw,
    /// Shift+`R`: clockwise.
    RotCw,
    /// `X`: mirror left-right.
    MirrorH,
    /// `Y`: mirror top-bottom.
    MirrorV,
}

/// `GetNearestHalfGridPosition`.
fn half_grid(p: Point, grid: Um) -> Point {
    let half = (grid as f64) / 2.0;
    let snap = |v: Um| ((v as f64 / half).round() * half).round() as Um;
    Point { x: snap(p.x), y: snap(p.y) }
}

fn box_center(b: (Point, Point)) -> Point {
    Point { x: b.0.x + (b.1.x - b.0.x) / 2, y: b.0.y + (b.1.y - b.0.y) / 2 }
}

/// `SELECTION::GetCenter`: the middle of the selection's box, text and labels left out; of a selection of only those, the mean of their
/// places (so turning it does not shift it).
fn selection_center(scene: &Scene, items: &[Item]) -> Point {
    let text_like = |it: &Item| matches!(it, Item::Text(_)) || matches!(it, Item::Label(_)) || scene.is_label_like(*it);
    if items.iter().all(text_like) {
        let n = items.len().max(1) as i64;
        let (sx, sy) = items.iter().fold((0, 0), |(x, y), it| {
            let p = scene.position(*it);
            (x + p.x, y + p.y)
        });
        return Point { x: sx / n, y: sy / n };
    }
    let mut lo: Option<Point> = None;
    let mut hi: Option<Point> = None;
    for it in items.iter().filter(|it| !text_like(it)) {
        if let Some((a, b)) = scene.item_box(*it) {
            lo = Some(match lo {
                None => a,
                Some(l) => Point { x: l.x.min(a.x), y: l.y.min(a.y) },
            });
            hi = Some(match hi {
                None => b,
                Some(h) => Point { x: h.x.max(b.x), y: h.y.max(b.y) },
            });
        }
    }
    match (lo, hi) {
        (Some(l), Some(h)) => box_center((l, h)),
        _ => Point::default(),
    }
}

/// How many items `SCH_EDIT_TOOL::Rotate` counts: a wire is a line per segment.
fn principal_count(scene: &Scene, items: &[Item]) -> usize {
    items.iter().map(|it| match it {
        Item::NoteLine(i) => scene.sch.lines[*i].pts.len().saturating_sub(1).max(1),
        _ => 1,
    }).sum()
}

/// `SCH_EDIT_TOOL::Rotate` and `Mirror` for the selection as it stands (the user's items, and what a drag added). `moving` is true while
/// the items are held (`moving && selection.HasReferencePoint()`), when `about` is where they are held.
fn turn_selection(scene: &mut Scene, turn: Turn, about: Option<Point>, grid: Um, moving: bool) {
    let all = scene.in_selection();
    if all.is_empty() {
        return;
    }
    // what the user picked: not "SELECTED_BY_DRAG" (the lines and labels a drag added are turned with them, not counted)
    let principal: Vec<Item> = all.iter().copied().filter(|it| !scene.is_dragged(*it)).collect();
    // sheet pins follow their sheet when it is selected too
    let sheets: BTreeSet<usize> = all.iter().filter_map(|it| if let Item::Sheet(s) = it { Some(*s) } else { None }).collect();
    // a no-connect sitting on a sheet pin follows the pin (`sheet->GetNoConnects()`)
    let mut nc_on_pins: Vec<(usize, usize, usize)> = Vec::new();
    for it in &all {
        let sheet_ids: Vec<usize> = match it {
            Item::Sheet(s) => vec![*s],
            Item::SheetPin(s, _) => vec![*s],
            _ => Vec::new(),
        };
        for s in sheet_ids {
            for (pi, pin) in scene.sch.sheets[s].pins.iter().enumerate() {
                if let Some(ni) = scene.sch.no_connects.iter().position(|n| n.at == pin.at) {
                    nc_on_pins.push((s, pi, ni));
                }
            }
        }
    }
    let reference = if moving { about } else { None };

    match turn {
        Turn::RotCcw | Turn::RotCw => {
            let ccw = turn == Turn::RotCcw;
            if principal_count(scene, &principal) == 1 {
                let head = principal[0];
                let pivot = rotate_single(scene, head, reference, ccw, grid);
                // "We've already rotated the user selected item if there was only one. We're just here to rotate the ends of wires that were attached to it."
                let dragged: Vec<Item> = all.iter().copied().filter(|it| scene.is_dragged(*it)).collect();
                for it in dragged {
                    rotate_item(scene, it, pivot, ccw, grid);
                }
            } else {
                let center = reference.unwrap_or_else(|| half_grid(selection_center(scene, &all), grid));
                for &it in &all {
                    if let Item::SheetPin(s, _) = it {
                        if sheets.contains(&s) {
                            continue; // the sheet turns its pins
                        }
                    }
                    rotate_item(scene, it, center, ccw, grid);
                }
            }
        }
        Turn::MirrorH | Turn::MirrorV => {
            let vertical = turn == Turn::MirrorV;
            if all.len() == 1 {
                mirror_single(scene, all[0], vertical, about, grid);
            } else {
                let center = about.unwrap_or_else(|| half_grid(selection_center(scene, &all), grid));
                for &it in &all {
                    if let Item::SheetPin(s, _) = it {
                        if sheets.contains(&s) {
                            continue;
                        }
                    }
                    mirror_item(scene, it, center, vertical, grid);
                }
            }
        }
    }

    // the no-connects follow their pins
    for (s, pi, ni) in nc_on_pins {
        scene.sch.no_connects[ni].at = scene.sch.sheets[s].pins[pi].at;
    }
}

/// The tail of Rotate and Mirror when nothing is being held (`TrimOverLappingWires`, `AddJunctionsIfNeeded`, `CleanUp`).
fn finish_turn(scene: &mut Scene) {
    let items = scene.in_selection();
    scene.trim_overlapping_wires(&items);
    scene.add_junctions_if_needed(&items);
    scene.clean_up();
}

/// `SCH_EDIT_TOOL::Rotate`'s single-item switch. Returns the point the item was turned about, which the lines a drag brought along turn
/// about too.
fn rotate_single(scene: &mut Scene, head: Item, reference: Option<Point>, ccw: bool, grid: Um) -> Point {
    // "if( moving && selection.HasReferencePoint() ) rotPoint = selection.GetReferencePoint(); else if( head->IsConnectable() ) rotPoint =
    // head->GetPosition(); else rotPoint = GetNearestHalfGridPosition( head->GetBoundingBox().GetCenter() )"
    let default_pivot = |scene: &Scene| {
        reference.unwrap_or_else(|| {
            if scene.is_connectable(head) {
                scene.position(head)
            } else {
                half_grid(scene.item_box(head).map(box_center).unwrap_or_else(|| scene.position(head)), grid)
            }
        })
    };
    match head {
        // text and every kind of label turn about their own anchor
        Item::Text(_) | Item::Label(_) => {
            let pivot = default_pivot(scene);
            let at = scene.position(head);
            rotate_item(scene, head, at, ccw, grid);
            pivot
        }
        Item::Graphic(i) if matches!(scene.graphic(i).shape, SchGraphicKind::Directive { .. }) => {
            let pivot = default_pivot(scene);
            let at = scene.position(head);
            rotate_item(scene, head, at, ccw, grid);
            pivot
        }
        Item::SheetPin(s, p) => {
            // "Rotate pin within parent sheet"
            let pivot = default_pivot(scene);
            let sheet = scene.sch.sheets[s].clone();
            let center = box_center((sheet.at, Point { x: sheet.at.x + sheet.size.0, y: sheet.at.y + sheet.size.1 }));
            rotate_sheet_pin(&mut scene.sch.sheets[s], p, &sheet, center, ccw);
            pivot
        }
        Item::Seg(i) => {
            // "Equal checks for both and neither" -- a line selected at neither end, or both, turns whole, about its far end
            let s = &mut scene.segs[i];
            if s.has(F_START) == s.has(F_END) {
                s.flags |= F_START | F_END;
            }
            let pivot = if scene.segs[i].has(F_START) { scene.segs[i].b } else { scene.segs[i].a };
            rotate_item(scene, head, pivot, ccw, grid);
            pivot
        }
        Item::NoteLine(i) => {
            // a graphic line is a `SCH_LINE` too: a lone one turns about its end
            let pts = scene.sch.lines[i].pts.clone();
            let pivot = if pts.len() == 2 { pts[1] } else { default_pivot(scene) };
            rotate_item(scene, head, pivot, ccw, grid);
            pivot
        }
        Item::Sheet(i) => {
            // "Rotate the sheet on itself. Sheets do not have an anchor point."
            let s = &scene.sch.sheets[i];
            let c = half_grid(box_center((s.at, Point { x: s.at.x + s.size.0, y: s.at.y + s.size.1 })), grid);
            rotate_item(scene, head, c, ccw, grid);
            c
        }
        other => {
            let pivot = default_pivot(scene);
            rotate_item(scene, other, pivot, ccw, grid);
            pivot
        }
    }
}

/// `item->Rotate( center, ccw )` for every kind.
fn rotate_item(scene: &mut Scene, it: Item, c: Point, ccw: bool, grid: Um) {
    let _ = grid;
    match it {
        Item::Seg(i) => {
            let s = &mut scene.segs[i];
            if s.has(F_START) {
                s.a = rotate_point(s.a, c, ccw);
            }
            if s.has(F_END) {
                s.b = rotate_point(s.b, c, ccw);
            }
        }
        Item::NoteLine(i) => {
            for p in scene.sch.lines[i].pts.iter_mut() {
                *p = rotate_point(*p, c, ccw);
            }
        }
        Item::Junction(i) => scene.sch.junctions[i].at = rotate_point(scene.sch.junctions[i].at, c, ccw),
        Item::NoConnect(i) => scene.sch.no_connects[i].at = rotate_point(scene.sch.no_connects[i].at, c, ccw),
        Item::BusEntry(i) => {
            // `SCH_BUS_ENTRY_BASE::Rotate`: the anchor about the centre, the size as a vector
            let b = &mut scene.sch.bus_entries[i];
            b.at = rotate_point(b.at, c, ccw);
            b.size = rotate_vec(b.size, ccw);
        }
        Item::Symbol(i) => {
            let s = &mut scene.sch.symbols[i];
            s.at = rotate_point(s.at, c, ccw);
            let m = Matrix::of(s.rot, s.mirrored, s.mirror_y).then(if ccw { Matrix::ROT_CCW } else { Matrix::ROT_CW });
            (s.rot, s.mirrored, s.mirror_y) = m.orientation();
        }
        Item::Power(i) => {
            let p = &mut scene.sch.power_symbols[i];
            p.at = rotate_point(p.at, c, ccw);
            let m = Matrix::of(p.rot, false, false).then(if ccw { Matrix::ROT_CCW } else { Matrix::ROT_CW });
            p.rot = m.orientation().0;
        }
        Item::Label(i) => {
            // `SCH_LABEL_BASE::Rotate`: the anchor about the centre, then a quarter turn of the spin
            let at = scene.sch.labels[i].at;
            let spin = effective_spin(scene, i);
            scene.sch.labels[i].at = rotate_point(at, c, ccw);
            let id = scene.sch.labels[i].id.clone();
            scene.sch.extras.label_spins.insert(id, spin.rotated(ccw));
        }
        Item::Text(i) => {
            let t = &mut scene.sch.texts[i];
            t.at = rotate_point(t.at, c, ccw);
            t.angle = ((t.angle as i64 + if ccw { 90_000 } else { 270_000 }) % 360_000) as u32;
        }
        Item::Graphic(i) => rotate_graphic(&mut scene.sch.extras.graphics[i], c, ccw),
        Item::Sheet(i) => {
            let old = scene.sch.sheets[i].clone();
            rotate_sheet(&mut scene.sch.sheets[i], &old, c, ccw);
        }
        Item::SheetPin(s, p) => {
            let sheet = scene.sch.sheets[s].clone();
            let center = box_center((sheet.at, Point { x: sheet.at.x + sheet.size.0, y: sheet.at.y + sheet.size.1 }));
            rotate_sheet_pin(&mut scene.sch.sheets[s], p, &sheet, center, ccw);
        }
    }
}

fn effective_spin(scene: &Scene, label: usize) -> LabelSpin {
    let l = &scene.sch.labels[label];
    scene.sch.extras.label_spins.get(&l.id).copied().unwrap_or_else(|| inferred_spin(&scene.segs, l.at))
}

fn mirror_single(scene: &mut Scene, head: Item, vertical: bool, about: Option<Point>, grid: Um) {
    match head {
        Item::Symbol(i) => {
            // `SetOrientation( SYM_MIRROR_x )`, nothing else
            let s = &mut scene.sch.symbols[i];
            let m = Matrix::of(s.rot, s.mirrored, s.mirror_y).then(if vertical { Matrix::MIRROR_X } else { Matrix::MIRROR_Y });
            (s.rot, s.mirrored, s.mirror_y) = m.orientation();
        }
        Item::Text(_) => {}
        Item::Label(i) => {
            let spin = effective_spin(scene, i);
            let id = scene.sch.labels[i].id.clone();
            scene.sch.extras.label_spins.insert(id, spin.mirrored(!vertical));
        }
        Item::Graphic(i) if matches!(scene.graphic(i).shape, SchGraphicKind::Directive { .. }) => mirror_item_in_place(scene, head, vertical),
        Item::SheetPin(s, p) => {
            let sheet = scene.sch.sheets[s].clone();
            let center = box_center((sheet.at, Point { x: sheet.at.x + sheet.size.0, y: sheet.at.y + sheet.size.1 }));
            mirror_sheet_pin(&mut scene.sch.sheets[s].pins[p].at, &sheet, center, vertical);
        }
        Item::Sheet(i) => {
            // "Mirror the sheet on itself. Sheets do not have a anchor point."
            let s = &scene.sch.sheets[i];
            let c = about.unwrap_or_else(|| half_grid(box_center((s.at, Point { x: s.at.x + s.size.0, y: s.at.y + s.size.1 })), grid));
            mirror_item(scene, head, c, vertical, grid);
        }
        other => {
            // a line mirrors about its first point, anything else about its own anchor
            let p = about.unwrap_or_else(|| {
                if let Item::Seg(i) = other {
                    let s = &mut scene.segs[i];
                    if s.has(F_START) == s.has(F_END) {
                        s.flags |= F_START | F_END;
                    }
                }
                scene.position(other)
            });
            mirror_item(scene, other, p, vertical, grid);
        }
    }
}

fn mirror_item_in_place(scene: &mut Scene, it: Item, vertical: bool) {
    if let Item::Graphic(i) = it {
        let g = &mut scene.sch.extras.graphics[i];
        if let SchGraphicKind::Directive { orientation, .. } = &mut g.shape {
            *orientation = mirror_orientation(*orientation, !vertical);
        }
    }
}

/// A directive label's pole direction mirrored (`MirrorSpinStyle`): left-right swaps right and left, top-bottom swaps up and down.
fn mirror_orientation(o: u32, left_right: bool) -> u32 {
    match (o % 360_000, left_right) {
        (0, true) => 180_000,
        (180_000, true) => 0,
        (90_000, false) => 270_000,
        (270_000, false) => 90_000,
        (o, _) => o,
    }
}

/// `item->MirrorHorizontally( centre.x )` / `MirrorVertically( centre.y )` for every kind.
fn mirror_item(scene: &mut Scene, it: Item, c: Point, vertical: bool, grid: Um) {
    let _ = grid;
    let flip = |p: Point| if vertical { Point { x: p.x, y: mirror_coord(p.y, c.y) } } else { Point { x: mirror_coord(p.x, c.x), y: p.y } };
    match it {
        Item::Seg(i) => {
            let s = &mut scene.segs[i];
            if s.has(F_START) {
                s.a = flip(s.a);
            }
            if s.has(F_END) {
                s.b = flip(s.b);
            }
        }
        Item::NoteLine(i) => {
            for p in scene.sch.lines[i].pts.iter_mut() {
                *p = flip(*p);
            }
        }
        Item::Junction(i) => scene.sch.junctions[i].at = flip(scene.sch.junctions[i].at),
        Item::NoConnect(i) => scene.sch.no_connects[i].at = flip(scene.sch.no_connects[i].at),
        Item::BusEntry(i) => {
            let b = &mut scene.sch.bus_entries[i];
            b.at = flip(b.at);
            if vertical {
                b.size.y = -b.size.y;
            } else {
                b.size.x = -b.size.x;
            }
        }
        Item::Symbol(i) => {
            // `SCH_SYMBOL::MirrorHorizontally( aCenter )`: the orientation, then the anchor about the axis
            let s = &mut scene.sch.symbols[i];
            let m = Matrix::of(s.rot, s.mirrored, s.mirror_y).then(if vertical { Matrix::MIRROR_X } else { Matrix::MIRROR_Y });
            (s.rot, s.mirrored, s.mirror_y) = m.orientation();
            s.at = flip(s.at);
        }
        Item::Power(i) => {
            // a power symbol has no mirror flag here: a power symbol is symmetric about its own vertical axis, so the mirror is a
            // rotation of the picture (a flip of x leaves it, a flip of y turns it half a turn)
            let p = &mut scene.sch.power_symbols[i];
            let mut m = Matrix::of(p.rot, false, false).then(if vertical { Matrix::MIRROR_X } else { Matrix::MIRROR_Y });
            if m.x1 * m.y2 - m.y1 * m.x2 < 0 {
                m = mul(m, Matrix::MIRROR_Y);
            }
            p.rot = m.orientation().0;
            p.at = flip(p.at);
        }
        Item::Label(i) => {
            let spin = effective_spin(scene, i);
            let id = scene.sch.labels[i].id.clone();
            scene.sch.extras.label_spins.insert(id, spin.mirrored(!vertical));
            scene.sch.labels[i].at = flip(scene.sch.labels[i].at);
        }
        Item::Text(i) => scene.sch.texts[i].at = flip(scene.sch.texts[i].at),
        Item::Graphic(i) => mirror_graphic(&mut scene.sch.extras.graphics[i], c, vertical),
        Item::Sheet(i) => {
            let old = scene.sch.sheets[i].clone();
            let s = &mut scene.sch.sheets[i];
            if vertical {
                s.at.y = mirror_coord(s.at.y, c.y) - s.size.1;
            } else {
                s.at.x = mirror_coord(s.at.x, c.x) - s.size.0;
            }
            for p in 0..s.pins.len() {
                mirror_sheet_pin(&mut s.pins[p].at, &old, c, vertical);
            }
        }
        Item::SheetPin(s, p) => {
            let sheet = scene.sch.sheets[s].clone();
            mirror_sheet_pin(&mut scene.sch.sheets[s].pins[p].at, &sheet, c, vertical);
        }
    }
}

/// Matrix product `a(b(p))`.
fn mul(a: Matrix, b: Matrix) -> Matrix {
    Matrix { x1: a.x1 * b.x1 + a.y1 * b.x2, y1: a.x1 * b.y1 + a.y1 * b.y2, x2: a.x2 * b.x1 + a.y2 * b.x2, y2: a.x2 * b.y1 + a.y2 * b.y2 }
}

/// `SCH_SHEET::Rotate`: the corner and size turn, a size that comes out negative moves the corner, and every pin turns with it.
fn rotate_sheet(sheet: &mut SheetInstance, old: &SheetInstance, c: Point, ccw: bool) {
    let mut at = rotate_point(old.at, c, ccw);
    let size = rotate_vec(Point { x: old.size.0, y: old.size.1 }, ccw);
    let (mut w, mut h) = (size.x, size.y);
    if w < 0 {
        at.x += w;
        w = -w;
    }
    if h < 0 {
        at.y += h;
        h = -h;
    }
    sheet.at = at;
    sheet.size = (w, h);
    for p in sheet.pins.iter_mut() {
        let pt = rotate_point(p.at, c, ccw);
        // a pin turned with its sheet lies on the new border; `ConstrainOnEdge` keeps it exactly there
        p.at = constrain_on_edge(&SheetInstance { pins: Vec::new(), ..sheet_geometry(at, (w, h)) }, pt, true, pt);
    }
}

fn sheet_geometry(at: Point, size: (Um, Um)) -> SheetInstance {
    SheetInstance { id: String::new(), name: String::new(), file: String::new(), at, size, pins: Vec::new(), page: String::new() }
}

/// `SCH_SHEET_PIN::Rotate` for one pin of a sheet that stays: the pin turns about `center`, lands on the nearest edge, and when that is
/// the edge it started on, or the opposite one, it is mirrored across the middle of that side instead.
fn rotate_sheet_pin(sheet: &mut SheetInstance, pin: usize, before: &SheetInstance, center: Point, ccw: bool) {
    let start = sheet.pins[pin].at;
    let delta = Point { x: start.x - center.x, y: start.y - center.y };
    let pt = rotate_point(start, center, ccw);
    let old_side = side_of(before, start);
    let new_pos = constrain_on_edge(before, pt, true, pt);
    let new_side = side_of(before, new_pos);
    let horizontal_side = |s: u8| s == 0 || s == 2;
    let mut at = new_pos;
    if new_side == old_side {
        // "If the new side is the same as the old side, instead mirror across the center of that side."
        at = if horizontal_side(new_side) { Point { x: center.x - delta.x, y: new_pos.y } } else { Point { x: new_pos.x, y: center.y - delta.y } };
    } else if new_side == (old_side + 2) % 4 {
        // "If the new side is opposite to the old side, instead mirror across the center of an adjacent side."
        at = if horizontal_side(new_side) { Point { x: center.x + delta.x, y: new_pos.y } } else { Point { x: new_pos.x, y: center.y + delta.y } };
    }
    sheet.pins[pin].at = constrain_on_edge(before, at, false, at);
}

/// 0 top, 1 right, 2 bottom, 3 left.
fn side_of(sheet: &SheetInstance, p: Point) -> u8 {
    let (l, t, r, b) = (sheet.at.x, sheet.at.y, sheet.at.x + sheet.size.0, sheet.at.y + sheet.size.1);
    if p.x <= l {
        3
    } else if p.x >= r {
        1
    } else if p.y <= t {
        0
    } else if p.y >= b {
        2
    } else {
        3
    }
}

/// `SCH_SHEET_PIN::MirrorHorizontally / MirrorVertically`: the pin's coordinate mirrored about the axis, which puts it on the opposite side.
fn mirror_sheet_pin(at: &mut Point, _sheet: &SheetInstance, c: Point, vertical: bool) {
    if vertical {
        at.y = mirror_coord(at.y, c.y);
    } else {
        at.x = mirror_coord(at.x, c.x);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// drawn graphics

/// `SCH_SHAPE::Move`.
pub(crate) fn move_graphic(g: &mut SchGraphic, d: Point) {
    map_graphic_points(g, &|p| Point { x: p.x + d.x, y: p.y + d.y });
}

/// Where a graphic is anchored (`GetPosition`) set to `p`.
pub(crate) fn set_graphic_position(g: &mut SchGraphic, p: Point) {
    let at = crate::sch_scene::graphic_position(g);
    move_graphic(g, Point { x: p.x - at.x, y: p.y - at.y });
}

fn map_graphic_points(g: &mut SchGraphic, f: &dyn Fn(Point) -> Point) {
    match &mut g.shape {
        SchGraphicKind::Rectangle { start, end, .. } | SchGraphicKind::TextBox { start, end, .. } => {
            *start = f(*start);
            *end = f(*end);
        }
        SchGraphicKind::Circle { center, .. } => *center = f(*center),
        SchGraphicKind::Arc { start, mid, end } => {
            *start = f(*start);
            *mid = f(*mid);
            *end = f(*end);
        }
        SchGraphicKind::Bezier { start, c1, c2, end } => {
            *start = f(*start);
            *c1 = f(*c1);
            *c2 = f(*c2);
            *end = f(*end);
        }
        SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => {
            for p in pts.iter_mut() {
                *p = f(*p);
            }
        }
        SchGraphicKind::Directive { at, .. } => *at = f(*at),
    }
}

/// `SCH_SHAPE::Rotate` (`EDA_SHAPE::rotate`), and for a text box and a directive label the turn of the text or the pole.
fn rotate_graphic(g: &mut SchGraphic, c: Point, ccw: bool) {
    map_graphic_points(g, &|p| rotate_point(p, c, ccw));
    match &mut g.shape {
        // `SCH_TEXTBOX::Rotate`: "SetTextAngle( vertical ? horizontal : vertical )"
        SchGraphicKind::TextBox { angle, .. } => *angle = if *angle % 180_000 == 90_000 { 0 } else { 90_000 },
        SchGraphicKind::Directive { orientation, .. } => *orientation = ((*orientation as i64 + if ccw { 90_000 } else { 270_000 }) % 360_000) as u32,
        _ => {}
    }
}

fn mirror_graphic(g: &mut SchGraphic, c: Point, vertical: bool) {
    let flip = |p: Point| if vertical { Point { x: p.x, y: mirror_coord(p.y, c.y) } } else { Point { x: mirror_coord(p.x, c.x), y: p.y } };
    map_graphic_points(g, &flip);
    match &mut g.shape {
        // "Text is NOT really mirrored; it just has its justification flipped"
        SchGraphicKind::TextBox { angle, h_align, .. } => {
            let flips = if vertical { *angle % 180_000 == 90_000 } else { *angle % 180_000 == 0 };
            if flips {
                *h_align = match *h_align {
                    SchHAlign::Left => SchHAlign::Right,
                    SchHAlign::Right => SchHAlign::Left,
                    other => other,
                };
            }
        }
        SchGraphicKind::Directive { orientation, .. } => *orientation = mirror_orientation(*orientation, !vertical),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Align to Grid and Align

/// `GRID_HELPER::AlignGrid`.
fn align_grid(p: Point, grid: Um) -> Point {
    let snap = |v: Um| ((v as f64 / grid as f64).round() as Um) * grid;
    Point { x: snap(p.x), y: snap(p.y) }
}

/// `AlignSchematicItemsToGrid`: a line's two ends are aligned one after the other with what is attached to each; a text by its anchor; a
/// sheet by its two corners with its pins; anything else by the shift most of its connection points need, with what is attached to it.
fn align_to_grid(scene: &mut Scene, grid: Um) {
    let items = scene.in_selection();
    let mut st = Drag { ortho: false, grid, ..Default::default() };
    let internal = internal_points(scene);
    for it in items {
        match it {
            Item::Seg(i) => {
                for end in 0..2 {
                    let (flag, p) = if end == 0 { (F_START, scene.segs[i].a) } else { (F_END, scene.segs[i].b) };
                    scene.segs[i].flags = F_SELECTED | flag;
                    let mut drag_items = vec![it];
                    scene.get_connected_drag_items(&mut st, it, p, &mut drag_items);
                    let delta = diff(align_grid(p, grid), p);
                    if delta != (Point { x: 0, y: 0 }) {
                        let unique: BTreeSet<Item> = drag_items.into_iter().collect();
                        for d in unique {
                            scene.move_item_drag(&st, d, delta);
                        }
                    }
                }
                scene.segs[i].flags |= F_START | F_END;
            }
            Item::Text(_) | Item::NoteLine(_) | Item::Graphic(_) if !scene.is_connectable(it) => {
                let p = scene.position(it);
                let delta = diff(align_grid(p, grid), p);
                if delta != (Point { x: 0, y: 0 }) {
                    scene.move_item(it, delta);
                }
            }
            Item::Sheet(i) => {
                let s = scene.sch.sheets[i].clone();
                let tl = s.at;
                let br = Point { x: tl.x + s.size.0, y: tl.y + s.size.1 };
                let tl_delta = diff(align_grid(tl, grid), tl);
                let br_delta = diff(align_grid(br, grid), br);
                // the pins' neighbours, found before the sheet moves
                let mut pin_drag: Vec<(usize, Point, Vec<Item>)> = Vec::new();
                for (pi, pin) in s.pins.iter().enumerate() {
                    let mut list = Vec::new();
                    scene.get_connected_drag_items(&mut st, Item::SheetPin(i, pi), pin.at, &mut list);
                    pin_drag.push((pi, pin.at, list));
                }
                if tl_delta != (Point { x: 0, y: 0 }) || br_delta != (Point { x: 0, y: 0 }) {
                    scene.move_item(Item::Sheet(i), tl_delta);
                    let sh = &mut scene.sch.sheets[i];
                    sh.size = (sh.size.0 - tl_delta.x + br_delta.x, sh.size.1 - tl_delta.y + br_delta.y);
                }
                for (pi, original, list) in pin_drag {
                    let pos = scene.sch.sheets[i].pins[pi].at;
                    let target = if list.is_empty() { align_grid(pos, grid) } else { Point { x: pos.x, y: original.y } };
                    scene.sch.sheets[i].pins[pi].at = constrain_on_edge(&scene.sch.sheets[i], target, false, pos);
                    let total = diff(scene.sch.sheets[i].pins[pi].at, original);
                    if total != (Point { x: 0, y: 0 }) {
                        for d in list {
                            if !matches!(d, Item::Sheet(_)) {
                                scene.move_item_drag(&st, d, total);
                            }
                        }
                    }
                }
            }
            other => {
                let conns = scene.connection_points(other);
                let mut drag_items = Vec::new();
                for &p in &conns {
                    scene.get_connected_drag_items(&mut st, other, p, &mut drag_items);
                }
                let mut shifts: BTreeMap<(Um, Um), usize> = BTreeMap::new();
                let mut most_common = Point { x: 0, y: 0 };
                let mut max = 0usize;
                for &p in &conns {
                    let g = diff(align_grid(p, grid), p);
                    let n = shifts.entry((g.x, g.y)).or_default();
                    *n += 1;
                    if *n > max {
                        max = *n;
                        most_common = g;
                    }
                }
                if conns.is_empty() {
                    let p = scene.position(other);
                    most_common = diff(align_grid(p, grid), p);
                }
                if most_common != (Point { x: 0, y: 0 }) {
                    scene.move_item(other, most_common);
                    let unique: BTreeSet<Item> = drag_items.into_iter().collect();
                    for d in unique {
                        scene.move_item_drag(&st, d, most_common);
                    }
                }
            }
        }
    }
    scene.finalize(Some(&st), &internal);
}

fn diff(a: Point, b: Point) -> Point {
    Point { x: a.x - b.x, y: a.y - b.y }
}

/// Align Left / Right / Top / Bottom / Center (`SCH_ALIGN_TOOL::moveItem`): each item's offset, snapped so an item on the connection grid
/// stays on it (`adjustDeltaForGrid`), and the wires attached stretch (a drag of one item at a time).
fn align(scene: &mut Scene, moves: &[AlignMove], grid: Um) -> Result<(), Vec<CheckResult>> {
    if moves.is_empty() {
        return Err(fail("ops_nothing_selected", "selection", "nothing to align"));
    }
    let mut moved: Vec<Item> = Vec::new();
    let mut internal: Vec<Point> = Vec::new();
    let mut any = false;
    for m in moves {
        let items = find_items(scene, &m.id);
        if items.is_empty() {
            return Err(fail("ops_unknown_item", &m.id, "no item with this id on the sheet"));
        }
        if scene.sch.extras.is_locked(&m.id) {
            continue;
        }
        for it in items {
            let delta = Point { x: m.dx, y: m.dy };
            if delta == (Point { x: 0, y: 0 }) {
                continue;
            }
            // `adjustDeltaForGrid`: only for items on the connection grid
            let delta = if grid_item(scene, it) {
                let p = scene.position(it);
                let snapped = align_grid(Point { x: p.x + delta.x, y: p.y + delta.y }, grid);
                diff(snapped, p)
            } else {
                delta
            };
            if delta == (Point { x: 0, y: 0 }) {
                continue;
            }
            any = true;
            // one item: reset what the last one left, select it, drag it
            scene.selected.clear();
            scene.by_drag.clear();
            for s in scene.segs.iter_mut() {
                s.flags = 0;
            }
            match it {
                Item::Seg(i) => scene.segs[i].flags = F_SELECTED | F_START | F_END,
                other => {
                    scene.selected.insert(other);
                }
            }
            let mut st = Drag { ortho: true, grid, ..Default::default() };
            scene.setup_items_for_drag(&mut st);
            internal.extend(internal_points(scene));
            scene.perform_item_move(&mut st, delta, true);
            moved.extend(scene.in_selection());
        }
    }
    if !any {
        return Err(fail("ops_nothing_to_move", "selection", "nothing moves: the items are locked or already aligned"));
    }
    // clean up once for everything that moved
    scene.selected = moved.iter().copied().filter(|i| !matches!(i, Item::Seg(_))).collect();
    for it in &moved {
        if let Item::Seg(i) = it {
            if *i < scene.segs.len() {
                scene.segs[*i].flags |= F_SELECTED;
            }
        }
    }
    internal.sort();
    internal.dedup();
    scene.finalize(None, &internal);
    Ok(())
}

/// Is this an item on the connection grid (`GRID_CONNECTABLE`)?
fn grid_item(scene: &Scene, it: Item) -> bool {
    match it {
        Item::Symbol(_) | Item::Power(_) | Item::Label(_) | Item::Sheet(_) | Item::SheetPin(..) | Item::NoConnect(_) => true,
        Item::Graphic(i) => matches!(scene.graphic(i).shape, SchGraphicKind::Directive { .. } | SchGraphicKind::RuleArea { .. }),
        _ => false,
    }
}

impl Cmd {
    /// A `Cmd` for one [`SchMoveCmd`].
    pub fn sch_move(cmd: SchMoveCmd) -> Cmd {
        Cmd::SchMove(cmd)
    }
}
