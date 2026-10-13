# crates/pns vs KiCad PNS — parity tracker

Gap #7 in `docs/parity/GAPS.md`: port KiCad's interactive push-and-shove
router (`pcbnew/router/*`) to Rust. This file tracks, per KiCad source
file, what's ported, what's deliberately simplified, and what's not done
yet. Read this before touching a module -- it says why each simplification
exists rather than letting you rediscover it.

## Architectural decisions that apply across the whole crate

1. **`NODE::Branch()` is a full clone, not KiCad's two-tier copy-on-write
   overlay.** Real KiCad branches cheaply off the root (empty
   joints/index, fall through to root on miss) but deep-clones
   joints/index/override-set the moment you branch off a non-root node,
   because every lookup (`FindJoint`, `QueryColliding`, ...) only ever
   consults "this node" or "the root," never a chain of ancestors. That
   machinery exists for interactive, thousands-of-ticks-per-route
   performance on boards with tens of thousands of items. This project's
   boards are small test boards and the frontend drives the backend over
   HTTP per mouse-move (request-granularity, not tick-granularity), so
   `Node::branch` (`src/node.rs`) is simply `self.clone()`. Semantically
   identical for every caller (branch, try an edit, keep or discard);
   asymptotically worse (O(board size) per branch instead of O(delta)).
   If this crate is ever pointed at much larger boards, this is the first
   thing to revisit.
2. **Collision is exact shape-vs-shape (`eda_drc::kimath::Shape::collides`),
   never hull-vs-hull.** This matches real KiCad too: `ITEM::Collide`
   dispatches to the real shapes; `ITEM::Hull()` is a separate construct
   used only by WALKAROUND/OPTIMIZER to shape a path around an obstacle.
   Reusing `eda_drc`'s geometry/constraint-resolution keeps the router's
   idea of clearance the rules the exported board carries (kicad-cli's
   DRC, the only DRC engine, judges the result).
3. **No `ARC_T`.** This project's IR represents every track as a
   polyline (`eda_model::ir::Track::pts`), an arc track (`PCB_ARC`) as the
   32 chords of its tessellation (`Track::arc`). The router's world has no arc
   item: `from_ir.rs` puts an arc track's chords in the world (since P5c, one
   chord from end to end made the world's length of a rounded meander 10 % short
   and left the arc's bulge unseen as an obstacle), and every KiCad pass that
   branches on "does this line contain an arc" takes the non-arc path
   unconditionally here. The length tuner (Stage 8) is the one place that
   *makes* arcs: `chain.rs` is a `SHAPE_LINE_CHAIN` that holds them, and
   `tuning_commit.rs` hands them to the IR as arc tracks.
4. **No component dragger, no multi-selection drag.** Single-item
   (segment/via) drag only. Diff pairs (Stage 7) and length tuning
   (Stage 8: single track, pair and skew, KiCad's placers and tool) are
   ported; KiCad's dragger has no pair mode, so there is nothing to port for a
   pair drag.
   **Status (2026-10-11):** the diff pair is KiCad's `DIFF_PAIR_PLACER` now
   (Stage 7): coupled in all three modes, walked and shoved as one unit, with
   vias.
5. **Units**: integer micrometers throughout (`eda_model::ir::Um`), like
   the rest of this workspace, not KiCad's internal nanometers. The length
   tuner is the exception inside the crate: its geometry (`kigeom.rs`,
   `chain.rs`, `meander*.rs`) is nanometres, as KiCad's meanders are, and is
   rounded to micrometres where it becomes tracks (`tuning_commit.rs`).

## Stage 1 -- core (done)

| KiCad file | Port | Status |
|---|---|---|
| `pns_item.h`, `pns_solid.h`, `pns_segment.h`, `pns_via.h`, `pns_linked_item.h`, `pns_link_holder.h` | `src/item.rs` | Done, as a closed `Item` enum (`Solid`/`Segment`/`Via`) instead of a class hierarchy. `HOLE_T`/`DIFF_PAIR_T` not modeled (hole-to-copper clearance is out of scope; see `eda_drc`'s own fidelity notes for that check). |
| `pns_layerset.h` | `src/layer.rs` (`LayerRange`) + `LayerMap` (board layer-name <-> index, not in KiCad -- this project names layers by string, KiCad by a global enum) | Done. |
| `pns_joint.h` | `src/joint.rs` (data) + `Node::next_segment`/`assemble_line` (traversal, since it needs the item table `JOINT` doesn't own in this port) | Done. `JOINT::NextSegment`'s exact rule ported: a `Solid`/`Via` at a joint is *always* a hard stop, even if a same-net segment candidate also exists there. |
| `pns_itemset.h` | Not ported as its own type -- callers just use `Vec<ItemId>`/`Vec<Item>` directly; the filtering helpers (`FilterNet` etc.) weren't needed yet. | Skipped (YAGNI so far). |
| `pns_index.h` | `src/index.rs` -- a uniform grid (not a literal R-tree, same tradeoff `eda_drc::rtree::DrcRTree` already documents), supporting `remove`, which `DrcRTree` doesn't (DRC builds its index once per run; this router mutates constantly). A new small index rather than extending `DrcRTree`, to keep `crates/drc` untouched. | Done. |
| `pns_node.{h,cpp}` | `src/node.rs` | Done, modulo the branching simplification (#1 above) and: no `m_override`/root-fallback machinery (unneeded once branching is a full clone); dangling joints *are* pruned on empty (KiCad leaves them, per its own `// fixme: remove dangling joints`) since pruning is free with a plain `HashMap`; no edge exclusions, no `RULE_RESOLVER` net-tie/keepout hooks (nothing in this project's model to resolve yet); `NearestObstacle`'s true path-length-global nearest search is narrowed to "first leg that collides, nearest hit on that leg" (see `walkaround.rs`/`shove.rs`'s own notes) -- cheaper and almost always the same answer at these board sizes. |
| `pns_utils.cpp` (`OctagonalHull`/`SegmentHull`/`ConvexHull`) | `src/hull.rs` | Done, via a different construction (circumscribing-octagon samples + a shared convex-hull routine) that is always *at least* as generous as KiCad's own chamfer -- see that file's doc comment for the exact argument. |
| `pns_routing_settings.h`, `pns_sizes_settings.h` | `src/settings.rs` | Done. Defaults cross-checked against `pns_routing_settings.cpp`'s real constructor (including the non-obvious default `RM_Walkaround`, not `RM_Shove`). `SizesSettings::for_net` pulls from this project's own `BoardRules` net-class resolution instead of a live KiCad dialog. `free_angle_mode` is live since 2026-10-08 (Highlight collisions only: `build_head` draws the head as one free-angle segment, as `buildInitialLine` does). |
| `geometry/direction45.h` | `src/direction45.rs` | `CORNER_MODE` narrowed to `Mitered45`/`Mitered90` (no arcs to fillet a rounded corner with -- see decision #3). `BuildInitialTrace`'s "only honor `start_diagonal` when direction is `Undefined`" subtlety preserved. |

## Stage 2 -- LINE_PLACER + WALKAROUND

| KiCad file | Port | Status |
|---|---|---|
| `pns_walkaround.{h,cpp}` | `src/walkaround.rs` | `Walker` is the faithful `WALKAROUND`: a whole `AssembleCluster` hugged per step, `WP_CW`/`WP_CCW`/`WP_SHORTEST` (with the check-back against the clusters already hugged), `SetItemMask`, `RestrictToCluster`, the length-expansion cut-off and the per-cluster iteration limit. `walk_base` is `LINE_PLACER::rhWalkBase` on it (Walk around mode and the solids-only pre-pass of the shove). Not ported: `m_PNSProcessClusterTimeout` (the wall-clock bound; the iteration limit is the only one). |
| `pns_line_placer.{h,cpp}`, `pns_mouse_trail_tracer.{h,cpp}` | `src/line_placer.rs` | See that file's own doc comment for the detailed list; headline simplification is posture: this port keeps the explicit direction state and the `/` toggle (`Direction45::right()`) and continues from the last fixed segment's direction, but does not implement `MOUSE_TRAIL_TRACER`'s continuous mouse-trail-area heuristic (automatic posture guessing from how the cursor swept toward the target) -- not meaningful without a continuous mouse-move stream to measure. |

**Status (2026-10-08):** `walkaround::Walker` is the faithful `WALKAROUND`
(`pns_walkaround.cpp`): policies `WP_CW`/`WP_CCW`/`WP_SHORTEST` (with the
check-back against clusters already hugged), a whole `AssembleCluster` per
step, `SetItemMask` (solids-only), `RestrictToCluster`, the 10x length
cut-off and the iteration limit per cluster. The shove (`onCollidingSolid`)
and the solids-only pre-pass of `rhShoveOnly` use it, and so does plain Walkaround
mode (`walkaround::walk_base`, `rhWalkBase`): the per-obstacle `walkaround::route`
that walked one item per iteration is gone, and D15 is closed.

Two real bugs surfaced while getting the stage 2 tests to pass on a
realistic multi-footprint board, both fixed rather than worked around:

- `eda_drc::kimath::Seg::intersect` returned a point along *the wrong
  segment's own direction* for an asymmetric crossing (correct only when
  the two segments happen to be geometrically symmetric, which is why the
  crate's own existing test never caught it -- every consumer inside
  `eda_drc` only ever used `.is_some()`, never the point). Fixed in
  `crates/drc/src/kimath.rs` with a new regression test,
  `asymmetric_crossing_point_is_on_both_segments`; see that function's own
  updated doc comment for the derivation. `walkaround.rs` is the first
  caller in this workspace that actually consumes the returned point.
- [`hull::hull_of`]'s octagon vertices are each rounded to the nearest
  integer micrometer independently, which can shift a vertex up to ~0.7um
  closer to centre than the exact construction -- enough to put a walked
  leg a single micrometer inside the clearance boundary it was supposed to
  stay outside of. Fixed with a small (2um) safety margin baked into the
  circumradius; see `circle_pts`'s doc comment.

## Stage 3 -- SHOVE

**Status (2026-10-08): `shove.rs` is a port of `pns_shove.cpp`'s control flow.**
The first version of this stage was a flat worklist that gave up at the
first pad, over- and under-shot a pushed via, never optimized what it
shoved, and normalised widths (`CODE-COMPARE-router.md` D5, D6, D10, D12).
It was replaced; the rest of this section describes what is there now. Read
`shove.rs`'s module doc comment for the same list next to the code.

What is ported, function for function (`pns_shove.cpp`):

| KiCad | Here |
|---|---|
| `SHOVE::Run`, `shoveMainLoop`, `shoveIteration` | `Shove::run`, `shove_main_loop`, `shove_iteration`: a stack of lines, the head at the bottom with rank 100000, every line it pushes rank - 1; each iteration takes the line on top and resolves its nearest obstacle, searching **pads, then vias, then tracks** (D12). Ends when the stack is empty, a push fails, or `ShoveIterationLimit` (250) is reached -- including KiCad's off-by-one (`m_iter >= limit` after the iteration). |
| `NODE::NearestObstacle` | `Node::nearest_obstacle`: the obstacle whose clearance hull the line enters first, by **path length along the line** (`HullIntersection` + `PathLength`), not by smallest gap. Kind mask and filter as `COLLISION_SEARCH_OPTIONS`. |
| `SHOVE::onCollidingSegment`, `ShoveObstacleLine`, `shoveLineToHullSet`, `checkShoveDirection` | unchanged algorithm (one hull per pusher segment, 3 hull sizes x 4 traversal/winding attempts), now also with the pusher's **end via** hull. |
| `SHOVE::onCollidingSolid` | `on_colliding_solid`: a pad cannot move, so the *current line* is walked around the cluster of items that touch it (`TOPOLOGY::AssembleCluster` with the 10x area limit, `WALKAROUND` restricted to that cluster with `WP_SHORTEST`), keeps its place on the stack with rank + 10000, and replaces itself in the node. Also the fallback for a locked track (`SH_TRY_WALK`) and for a via that may not move. A line whose end via collides with the pad pushes the via instead. |
| `SHOVE::onCollidingVia`, `pushOrShoveVia` | `on_colliding_via`, `push_or_shove_via`: the via moves by the **minimum translation vector** of its shape against the pusher (`pushoutForce`, epsilon-free), "do not land on an existing joint" included; the tracks attached to it are re-shaped with `LINE::DragCorner` at 45 degrees (`Line::drag_corner45`, `dragCornerInternal`), a via with no track becomes a lone-via line on the stack. `ShoveVias() == false` or a locked via is `SH_TRY_WALK`. (D6) |
| `onCollidingLine`, `onReverseCollidingVia`, `patchTadpoleVia`, `fixupViaCollisions`, `shoveLineFromLoneVia`, `unwindLineStack`, `replaceLine` | ported: the **rank** system (a line that runs into something already shoved with a higher rank does not shove it back -- it is pushed itself), tadpole vias, fan-out width fix-up, the root-line history. |
| `SHOVE::runOptimizer` | `run_optimizer`: every shoved line is optimized in the shoved world after the loop (`OE_MEDIUM`/`OE_FULL`: `MERGE_SEGMENTS` x 2 passes, `OE_LOW`: `MERGE_OBTUSE` x 1, plus `SMART_PADS`), restricted to the area the shove changed inflated by the widest line (`AREA_CONSTRAINT`, `optimizer::optimize_in_area`). The head is not optimized here; `LinePlacer` optimizes it afterwards against the world the shove left (`ShoveOutcome::world`), like `OPTIMIZER::Optimize( &aNewHead, effort, m_currentNode )`. (D10/D11) |
| `LINE_PLACER::rhShoveOnly` | `LinePlacer::build_head`: the head is first walked around the **pads only** (`walkaround::walk_masked`, `rhWalkBase( .., ITEM::SOLID_T, RM_Shove )`: both windings, each merged with `MERGE_SEGMENTS`, the shorter taken), then shoved. A pad it cannot be walked around, or a failed shove, falls back to the full walkaround. (D5) |

Where it differs, on purpose:

- **No springback.** Every call branches fresh from the committed world
  (decision 1, and the HTTP-per-sample driver). The shove main loop is bounded
  as KiCad's is: 250 iterations (`ShoveIterationLimit`) and one second
  (`ShoveTimeLimit`, `time_limit.rs`), checked after every iteration; either
  one ends the shove as `SH_INCOMPLETE`.
- **Widths survive (D10).** `SHOVE::assembleLine` passes the default
  `aAllowSegmentSizeMismatch = true`, so KiCad assembles *through* a change of
  width and writes the whole chain back at the width of the segment it hit.
  Here `Node::assemble_line_with` stops at the width change: each width is
  its own line, pinned at the joint they share.
- **The commit names every IR track a line stood on.** An imported board has
  one IR track per segment; a line over five of them used to replace the first
  and leave the other four behind. `DisplacedLine` is now one per (track,
  line): the first track gets the new polyline, the others an empty line
  ("remove, nothing replaces it"), and the part of a track no shoved line
  covered (a junction cut it) is handed back as it was.
  `LinePlacer::displaced_tracks` keeps a list of lines per track.
- **A via exactly on the pusher's centreline.** KiCad's MTV is zero there,
  the via is not moved, and the loop runs into its iteration limit. Here it is
  pushed along the normal of the nearest pusher segment.
- Not modelled: `SHP_REVERSED` (drag by the first vertex), `SHP_IGNORE`,
  `LockJoint`, arcs, holes, a head that ends in a via (`placing_via` places
  the via after the shove, with `place_via`).

Verified against KiCad's own router regressions
(`qa/data/pcbnew/pns_regressions`, replayed in `tests/qa_regressions.rs`,
skipped when the corpus is absent):

| Case | KiCad recorded | This port |
|---|---|---|
| `simple-shove-1` (a free-hand head through `simple.kicad_pcb`, Shove) | 13 tracks pushed, 28 segments | the same **13** tracks; 12 of the 28 segments within 3 um (the others differ where the optimizer cuts a corner; head width 250 as the replay used), 0 new violations |
| `backspace1` (9 fixes undone by Backspace, one redone) | one run, 2 segments | the same 2 segments (needed the posture to return to the initial one after the last undo) |
| `issue22749-shove-weird-drag-track-end` (a route from a track end, Shove) | 5 tracks pushed, 11 segments | the same **5** tracks; 4 of 11 within 3 um (the wrap around the head's diagonal is ~70 um further out in KiCad's), 0 new violations, also through the `RouteCommit` |
| `issue23449` (lone via drag), `walk_drag_seg_against_board_edge`, `simple-drag-shove-singlelayer` (drags) | no violation recorded | replayed with this port's corner drag (KiCad slides the segment; D7): no panic, 0 new violations on every accepted preview. `video-v10` is a slow-tier test (about a minute in a debug build). |
| a scratch copy of `work/mcu30` (133 tracks, 12 vias) driven through the studio's own `POST /api/route/{start,move,finish}` in Shove mode | -- | 60 random routes from track ends; 12 of them push something (up to 7 IR tracks and a via). The studio's `--strict` gates accepted 2 of the 12 and refused 10 (`routing_clearance` measured a pad by its bounding rectangle, so a track hugging a round pad's real outline at the clearance was "too close" -- **fixed 2026-10-08: the gate measures the exact outline now** (`eda_drc::board::placed_pad_copper`, `Shape::gap_to`), and on `mcu30`, `pic_programmer` and `backspace1` every pair it fails is a pair kicad-cli reports; also `routing_pass_through_pad`, `routing_over_refdes`, `routing_via_in_pad`, workmanship gates that are not clearance and are unchanged apart from `routing_via_in_pad` measuring the exact outline too). The same gates refuse the Walkaround route for one of the two moves compared, and a refused LED5 shove committed with `strict: false` has no clearance violation in **kicad-cli's** DRC. The 2 accepted commits were one undo step each (a line on 3 IR tracks became 1 track + 2 removals, no duplicated segment), `Undo` restored the board exactly, and kicad-cli reported no copper clearance violation; what it did report is `track_dangling` (the free-hand end I chose) and `copper_edge_clearance` on a shoved track pushed toward the board edge (**fixed 2026-10-08: the board outline is a router obstacle**, see "Known gaps" below). |
| random routes from random pads (`simple`, `pic_programmer`, `backspace1`, `dp_test`) and random corner drags in Shove mode | -- | 0 new violations: 250 routes per board while writing it, 50 per board in the committed test; Shove never failed where Walkaround succeeded. Dragging a *via* was the exception: the dragged via itself was never shoved against, so a via dropped onto a track left a violation (6 of 200 random via drags on `simple`). **Fixed 2026-10-08 (D7):** `random_via_drags_never_leave_a_new_violation`, 60 drags per board and mode on `simple` and `pic_programmer`, Shove and Walk around, leaves none. **P5a (2026-10-10):** `random_segment_and_corner_drags_never_leave_a_new_violation` drags 120 grabs per board and mode (half mid-segment, `DM_SEGMENT`; half at an end, `DM_CORNER`) through the stateful `Dragger::drag`: the position it shows is legal every time, with 0 new violations, and 3 to 33 of the 360 cursor positions per run fell back to the last valid one. |

Fixtures that do not need the corpus: `tests/shove_scenarios.rs` (a track
pushed past a pad row, a via pushed just far enough and between two tracks,
widths kept, a line on two IR tracks, the iteration limit, `ShoveVias` off),
`tests/shove_footprint.rs` (a track beside a real SOIC-8's pads) and the unit
tests next to each module.

`shove.rs` operates on its own scratch branch internally (it must -- shoving
genuinely mutates the board) and returns the diff (`ShoveOutcome`'s
`displaced_lines`/`displaced_vias`, plus the head as it ended up and the world
left behind); `LinePlacer` stays stateless for `preview()` (every call re-runs
the shove fresh from the real world), and only `fix()`/`finish()` absorb an
accepted call's displacement into the session's running
`displaced_tracks()`/`displaced_vias()` for the eventual commit.

## Stage 4 -- API + frontend

`pns_router.{h,cpp}` -> `src/router.rs`. `Router` owns the committed
`Node` and, while a route is in progress, a `LinePlacer` session -- no
persistent working/preview `Node` the way upstream keeps one, since
neither `LinePlacer` nor `shove` need one between calls (see those
modules' own doc comments). Commit granularity is **per finished line**,
not KiCad's per-segment `AddItem`/`RemoveItem`/`UpdateItem`: this
project's own `Track` IR already holds a whole polyline, so a finished
route becomes one `Track` (or two, split at a placed via, plus the `Via`)
instead of one `Cmd` per segment -- a deliberate adaptation to this
project's own data model (see the task's "our JSON is the only source of
truth" rule), not a fidelity gap. A newly added `Cmd::CommitRoute`
(`crates/ops/src/lib.rs`) removes whatever existing tracks/vias a shove
displaced and adds the session's final geometry in one undo step,
generalizing the existing `PasteItems` (pure-add) the same way KiCad's own
`NODE::Commit` generalizes a pure add into "remove the overridden set, add
the branch's own items."

HTTP endpoints: `crates/cli/src/route_api.rs`, wired into `crates/cli/src/
studio.rs`'s existing single-threaded dispatch. One `Mutex<Option<Router>>`
(`route_api::RouteCell`) holds the in-progress session across the whole
gesture -- `start`/`drag_start` build a fresh `Router` from the board as it
stands (same as every other endpoint re-reading `design.json`);
`move`/`fix`/`undo_segment`/`via`/`drag_move` just borrow it back out and
forward into its methods (no re-flattening the board per request, which is
the one thing "keep per-request work small" is actually worried about);
`finish`/`drag_finish`/`cancel` take the session back out, ending it
either way. Preview state never touches `design.json` -- only `finish`/
`drag_finish` do, through `Cmd::CommitRoute`.

Frontend: `web/studio/src/kicad-port/routeTool.ts` (pure, dependency-free
glue -- `DrawState` patch construction, the request-ordering guard and
move throttle every async call needs) + `api/client.ts`'s `route*`
functions + `components/canvas/routing.ts` (the async start/fix/finish/
cancel flows, replacing the old client-only `commitRoute`/
`dropViaAndSwitchLayer`) + `Canvas.tsx`/`useActionRunner.ts` wiring:

| Gesture | Wired as |
|---|---|
| X (start/toggle tool) | `pcbnew.InteractiveRouter.SingleTrack` (pre-existing) |
| click (start) | `Canvas.tsx` pointerdown -> `startInteractiveRoute` |
| click (fix) | `Canvas.tsx` pointerdown -> `fixInteractiveRoute` |
| mouse move (preview) | `Canvas.tsx` pointermove, throttled 50ms + request-guarded -> `routeMove` |
| Enter / double-click / F | `finishDraw`/`pcbnew.InteractiveRouter.AttemptFinish` -> `finishInteractiveRoute` |
| Esc | `common.Interactive.cancel` -> `cancelInteractiveRoute` |
| Backspace | `pcbnew.InteractiveRouter.UndoLastSegment` -> `routeUndoSegment` |
| V (via + layer switch) | `pcbnew.Control.layerToggle` -> `routeToggleVia` (arms the *next* fix, doesn't commit immediately) |
| `/` (posture) | `Canvas.tsx` keydown (no separate upstream `TOOL_ACTION` for this one -- see that handler's own comment) -> `routeMove(..., flipPosture: true)` |
| W / Shift+W (width) | `pcbnew.EditorControl.trackWidthInc`/`trackWidthDec` -> a small fixed preset ladder (this project has no per-board "preferred widths" list the way real pcbnew's dialog does) |

Verified end to end: `cargo test`/`clippy` on the Rust side (including
`tests/real_board.rs`), and `npm run typecheck && npm run test:unit &&
npm run build` on the TS side -- but **not visually**: the task's own
instructions note the in-app browser's tools don't work in this
environment, so the click-through was never watched happen in a live
browser. Treat the frontend wiring as "compiles, typechecks, and the pure
logic it's built from is unit-tested," not as "confirmed to look/feel
right."

## Stage 5 -- DRAGGER

`pns_dragger.{h,cpp}` -> `src/dragger.rs`. Reuses `shove`/`walkaround`
exactly as KiCad's own `DRAGGER` does, rather than reimplementing
displacement; `Router` gained `drag_start`/`drag_preview`/`drag_cancel`/
`drag_finish` (mutually exclusive with a route session) and
`crates/cli/src/route_api.rs`/`studio.rs` gained the matching `POST
/api/route/drag_{start,move,finish}` endpoints (sharing `RouteCell` with
the route session, since a `Router` is only ever doing one or the other).

**Updated 2026-10-10 (P5a, D7): the segment slide, `dragCorner45`, `SHP_REVERSED` and the
last-valid-point restore are ported** -- `Dragger::start_with` is `startDragSegment` (within half
the track's width of an end: that corner; anywhere else: `DM_SEGMENT`, the whole segment), the line
is `Line::drag_segment45` / `drag_corner45_snapped` (`pns_line.cpp`, checked against KiCad's own
functions: 80 000 random lines agree point for point; `seg_ops.rs` has the `SEG` pieces), the
snap thresholds are `SmoothDraggedSegments() ? width / 4 : 0` (`width / 2` in Shove), and
`Dragger::drag` / `finish` are `DRAGGER::Drag` / `FixRoute` with their memory: a position the mode
cannot resolve shows the last valid solution again, a release over it commits the last valid
point, a failing first position makes the drag mark obstacles only and Ctrl+click commits it.
The shove takes the dragged line out of its world first (`Shove::dragged_away`) and reverses its
direction test for vertex 0. Replays of KiCad's `walk_drag_seg_against_board_edge` (the three
segments it recorded) and `simple-drag-shove-singlelayer` (116 of 134, the same 139 tracks) are in
`tests/qa_regressions.rs`. What the original scoping list below says about "corner drag only" and
"free-angle" is superseded; what stays out: `OptimizeEntireDraggedTrack`, springback, arcs,
virtual vias, locked tracks.

Scoped down further than `shove`/`walkaround` already are (see
`dragger.rs`'s own doc comment for the full reasoning):
- ~~**Corner drag only**~~ **Fixed (2026-10-10)**: a grab in the middle of a segment slides it
  (`DM_SEGMENT`); within half its width of an end it is that corner.
- ~~**Free-angle corner relocation**~~ **Fixed (2026-10-10)**: `dragCorner45`; free angle only for
  `G` (`dragCornerFree`).
- ~~`Mode::Walkaround` while dragging behaves like `Mode::MarkObstacles`~~
  **Fixed (D7, 2026-10-08).** Walk around mode walks a dragged corner around
  what it lands on (`dragWalkaround`/`tryWalkaround`), and a dragged **via** is a
  pusher in Shove mode (`shove::shove_via`: the via is the head of the shove,
  `pushOrShoveVia` drags the tracks attached to it, the main loop pushes aside
  what they hit; it ends where the shove left it, which a pad or the board edge
  can make further than the cursor) and in Walk around mode `dragViaWalkaround`
  (`VIA::PushoutForce`, `ViaForcePropIterationLimit` steps, then the attached
  tracks walked around what they hit). A via that may not be shoved (`ShoveVias()`
  off, locked) or whose shove fails is dragged the Walk around way, as `dragShove`
  falls back. The attached tracks are re-solved at 45 degrees (`DragCorner`).

## Stage 5b -- routes that start and end in the middle of a track, Route From Other End

**2026-10-10 (P5a, D7/D18).** `LINE_PLACER::SplitAdjacentSegments` is `Node::split_segment`
(`segment_to_split` is its test: nothing is done where a joint already is or the point is not on the
segment; the IR track's later segments are renumbered so it stays one track). `Router::start` takes a
pad, a via, a track end or -- a point on a track's centre line -- the middle of a track, and cuts it in
the *session's own copy* of the world (`Session::world`), so a cancelled session leaves the router's world
whole; the commit gives the IR track a vertex there (`apply_splits`: the track out, itself with the point
in, in the same `Cmd::CommitRoute`), and nothing is committed or cut if the route placed nothing
(`CommitRouting`: `!HasPlacedAnything()`). A route ends on a same-net track (`LinePlacer::snap_end`,
setting `snap_to_tracks`, the magnetic setting `checkSnap` hands the router, default on) that the
cursor is exactly on, cut where it ends, and a head that runs along that track ends where it first
meets it (`Move`: `Collinear` and `Overlaps`). The track a route started on is never where it ends.
`SplitAdjacentArcs` is not ported: a track that is an arc (`arc_mid_offset`) is not split.

`Router::continue_from_end` is `ROUTER::ContinueFromEnd` + `GetNearestRatnestAnchor`:
`Node::nearest_unconnected_item` / `connected_joints` are `TOPOLOGY::NearestUnconnectedItem` /
`ConnectedJoints`, run over a node that holds what the session has fixed and the head. It says where
to restart and what to commit; `POST /api/route/continue_from_end` commits the fixed part, builds a
fresh `Router` from the board and starts it there (the commits are one undo step: `APPEND_UNDO`,
`board::append_to_previous_undo`).

A real bug surfaced and fixed while wiring the via-drag commit path: a via
displaced by a drag's own shove was being removed from the board but never
re-added at its new position (its diameter/drill were only ever carried as
`0, 0` placeholders meant to be re-resolved by `Router::drag_finish`, which
wasn't actually doing that resolution) -- `Router::drag_finish` now looks
up the real via before building its `Cmd::CommitRoute` entry; see
`router.rs`'s `shove_mode_drag_displaces_a_via_and_the_commit_carries_its_real_size`
test, which fails without the fix.

A second real gap surfaced while wiring the frontend's live preview (below):
`DragPreview` never carried the via-drag case's own attached tracks (its
"fanout" -- the ones stretching to follow the via), only the via's own new
point. `Dragger::candidate` always computed that fanout (it needs it for
the mode's own collision check), but `preview()` dropped it on the floor
rather than returning it, so a frontend drawing only `pts` would show the
via jumping across the board with nothing visibly following it until the
drag actually committed. Fixed: `DragPreview` gained a `fanout: Vec<Line>`
field (always empty for a corner drag), covered by
`dragging_a_via_drags_its_connected_track_with_it`'s new assertions. While
in there: `crates/cli/src/route_api.rs`'s `preview_json`/`drag_preview_json`
also never serialized `Preview`/`DragPreview`'s existing `displaced_vias`
field into JSON at all (route AND drag alike) -- a shove-mode preview that
would push a *via* out of the way never showed it moving until commit,
even though the Rust-side data already existed. Both now send
`displaced_vias: [{source_via, x, y}]`; the frontend looks up each one's
real diameter from the board's own still-there via by id rather than this
repeating it over the wire.

**Frontend: done.** `D` (`pcbnew.InteractiveRouter.Drag45Degree`) is wired
into Canvas.tsx's tool system, as its own one-shot action rather than
through the uniform-`(dx,dy)` move/drag system `MovePreview` already
handles for parts/vias/shapes/text (a drag session's shape is an arbitrary
reshape, not a translation -- it needed its own preview state and render
path, the same shape of work the route tool's own frontend wiring was):
see `web/studio/PARITY-pcb.md` section 4 for the click-through. Still not
done, same as upstream's own scope split (`InlineDrag` vs. a plain
footprint `Move`): dragging a *footprint* through the router (so its
attached tracks follow) -- this port's `Dragger` only ever drags a track
segment/corner or a lone via (see the scope list above), matching real
pcbnew's own `CanInlineDrag` rejecting a free-angle footprint drag too
(`DM_FREE_ANGLE`, this port's only mode -- see "Free-angle corner
relocation" above). A plain (non-router) footprint `Move` was already
confirmed, by reading `edit_tool_move_fct.cpp` directly, to **not** drag
attached tracks in real KiCad either -- see `PARITY-pcb.md` section 4's
"Move: connected track ends follow the dragged footprint" row -- so there
is no gap here to close, just a premise the task brief got wrong.

**Free-angle drag (`G`, `pcbnew.InteractiveRouter.DragFreeAngle`): done.**
`DRAGGER` runs `DM_FREE_ANGLE` when the drag is started with `G` instead of
`D`: this port's corner drag was already free-angle (see above), so the
only behavioral difference is the one `DRAGGER::Drag` itself has -- a free-
angle drag always uses `dragMarkObstacles` (a drag is never shoved, it just
reports what it would hit). `Dragger::free_angle` (set by
`Router::drag_start_with`, `POST /api/route/drag_start`'s `free_angle`)
makes `Dragger::preview` mark obstacles even in `Mode::Shove`; the
studio's `useActionRunner.ts` `G` handler shares `D`'s grab-and-go
(`startDragAtCursor( freeAngle )`). Test:
`free_angle_drag_never_shoves_it_only_reports_the_collision`.

## Stage 6 -- router fidelity gaps (settings dialog, loop removal, highlight-collisions mode)

**"Highlight collisions" mode needed no new backend code at all.** It's
not a distinct concept from what this port already calls
`Mode::MarkObstacles` -- `router_tool.cpp`'s own status-bar summary
literally labels `RM_MarkObstacles` as `"Highlight collisions"`. That mode
was already fully implemented (Stage 1) and already accepted over the
wire (`POST /api/route/start`'s `mode: "mark_obstacles"`); the actual gap
was that **nothing in the frontend ever let a person choose it** --
`startInteractiveRoute` hardcoded `"walkaround"` and `drag_start` never
read a mode from its request body at all. Fixed below, alongside the
settings dialog the task asked for.

**Loop removal (`RemoveLoops`/`FindLinesBetweenJoints`) -- now
implemented.** `RoutingSettings::remove_loops` already existed as a field
(defaulting `true`, matching upstream) but nothing read it; the real
behavior was entirely missing. `Node::find_lines_between_joints` (new,
`src/node.rs`) is `NODE::FindLinesBetweenJoints` narrowed to this port's
one caller: every distinct pre-existing same-net line already in the node
whose two endpoints exactly match a given pair of points.
`LinePlacer::finish` (new `remove_loops` method, called when
`settings.remove_loops` and the route reached a real same-net end -- same
gate as upstream's own `reachesEnd && aEndItem`) checks whether the whole
just-finished connection's two outer endpoints are also joined by some
*other* pre-existing path, and if so marks every one of that path's
source tracks for removal (skipping any with a locked segment, matching
upstream's own rule) via the same `displaced_tracks` map shove already
uses for "remove and don't replace" (an empty `Line` as the sentinel --
`point_count() < 2` already means "no replacement" to
`Router::build_commit`). Deliberately coarser than upstream in one way:
checked once for the whole finished connection (its true start to its
real end), not per internal segment/joint the way upstream's own
per-`JOINT` loop does -- matching this crate's existing "one commit per
whole finished line" adaptation (Stage 4's own doc comment) rather than
upstream's per-segment model. The new route's own runs are never
inserted into `node` while routing at all (module doc comment, same
reason same-net items never need collision exclusion here), so every
match this finds is guaranteed to be a genuinely different, already-
committed line -- never the one just placed. 3 new tests in
`line_placer.rs` (redundant path removed, locked path left alone, the
setting turned off is a no-op). Drag sessions don't get this treatment --
upstream's own `DRAGGER` never calls `removeLoops` either.

**Frontend: `RouterSettingsDialog.tsx` (`Ctrl+<`,
`pcbnew.InteractiveRouter.SettingsDialog`)**, backing `state.routerSettings`
(`kicad-port/routerSettings.ts`). **Updated 2026-10-08 (D16):** every
`RoutingSettings` field is read by the router now, and the dialog has
`dialog_pns_settings.cpp`'s rows: Highlight collisions (Free angle mode,
Allow DRC violations), Shove (Shove vias, Jump over obstacles), Walk around;
Remove redundant tracks, Optimize pad connections, Fix all segments on click
(labels and tool tips as `dialog_pns_settings_base.cpp`; enabled as
`onModeChange` does), plus an Optimizer effort select -- KiCad keeps
`OptimizerEffort` in its settings file only. Smooth dragged segments, Optimize
entire track being dragged and Use mouse path to set track posture have
nothing behind them here and are shown disabled with the reason in their tool
tip (**2026-10-10: Smooth dragged segments works now**, `smooth_dragged_segments`). `POST /api/route/{start,drag_start,dp_start}` take a `settings` object
(`route_api.rs` `settings_of`: those switches and `shove_iteration_limit`,
`shove_time_limit` (ms), `walkaround_iteration_limit`, `via_force_prop_iteration_limit`,
`walkaround_hug_length_threshold`, which KiCad has no row for); `mode` and
`remove_loops` at the top level still work. **OK in the dialog also applies
the settings to the route, drag or diff pair in progress**
(`POST /api/route/settings`), so a change takes effect from the next move as
upstream's does; the session itself is still a fresh backend `Router` per
route or drag.

## Stage 7 -- differential pairs

**Rewritten 2026-10-11 (P5b, branch `task-diff-pairs`).** The first port was a
spine and its offsets that only reported a collision; this is KiCad's pair
placer. `pns_diff_pair.{h,cpp}` -> `src/diff_pair.rs`,
`pns_diff_pair_placer.{h,cpp}` -> `src/dp_placer.rs`, plus the pair halves of
`src/shove.rs`, `src/walkaround.rs` and `src/optimizer.rs`. The two modules'
header comments say what differs from KiCad branch by branch; the row-by-row
account is `docs/parity/CODE-COMPARE-router.md` section I and the DP optimizer
row of section D. The short version:

- **Pair detection**: `dp_coupled_net_name` is `BOARD::MatchDpSuffix`
  (`pcbnew/board.cpp`) ported verbatim -- walk a net name backward past
  trailing digits/underscores, swap a trailing `+`/`-`/`P`/`N` for its
  complement (`"LVDS_P0"` -> `"LVDS_N0"`); `dp_net_polarity` says which net is
  the pair's P (`DpNetPair`).
- **Sizing**: `BoardRules::diff_pair_width_of`/`diff_pair_gap_of`/
  `diff_pair_via_gap_of` (`crates/model/src/lib.rs`) resolve the net class's
  `diff_pair_*` fields, falling back to upstream's `SIZES_SETTINGS` defaults
  (125 um width, 180 um gap). `dp_placer::DpSizes::resolve` is `ImportSizes` for
  a pair: the width is at least the board's minimum track width, and a gap or
  via gap from the rules is at least the clearance between the two nets (the
  fallback gap is under the default clearance, which no pair can be clean at);
  a gap typed in the Differential Pair Dimensions dialog is kept.
- **Start** (`DiffPairPlacer::start`, `find_dp_primitive_pair`,
  `dangling_anchor`): the item under the click and the item of the coupled net
  nearest to it make `DP_PRIMITIVE_PAIR` -- two pads or two vias (on the same
  layers), or two track ends nothing else joins. KiCad's refusals are
  returned as they are worded.
- **The pair** (`diff_pair.rs`): `DpGateways` builds the gateways of the start
  (`BuildFromPrimitivePair`: a rectangular pad pair fans out at 45 and 90
  degrees through entry lines, a pair of tracks continues at 22.5 degrees),
  of the coupled pair under the cursor or of the cursor (`BuildForCursor`, with
  `FilterByOrientation` for a cursor near the extension of the pair), and
  `FitGateways` tries every start x end x posture and keeps the best that
  `DiffPair::build_initial` accepts: the gap kept (`checkGap`), no crossing, no
  self-intersection, only obtuse turns (`CheckConnectionAngle`). Units: KiCad's
  nm tolerances are restated in um (`GAP_CHECK_SLACK` is `checkGap`'s 100 nm).
- **The three modes** (`DiffPairPlacer::route_head`, `rh_mark_obstacles`,
  `rh_walk_only`, `rh_shove_only`): Highlight collisions reports the pair as
  fitted and flags it when either line collides. Walk around walks it round
  what is in its way as one unit (`attempt_walk`: one line walked
  round the obstacle by `Walker`, the other shoved to the gap from it by
  `shove::shove_obstacle_line` with `ForceClearance`, roles swapping for up to
  three rounds; `try_walk_dp` takes the better of the walks that start with P
  and with N and runs `optimizer::optimize_pair` on it). Shove walks the pair
  round the pads and then shoves both lines as the heads of one shove
  (`shove::shove_heads`: several heads, each with the via it ends in), so the
  tracks and vias in the way are pushed aside with the pair's gap kept between
  its own lines. `propagate_dp_head_forces` first pushes the cursor off what a
  circle of the pair's size would sit on.
- **Vias** (`toggle_via`, `make_via`): `V` makes the head end in two vias a
  `viaGap()` apart (`BuildGeneric` in via mode fans the lines to the via
  centres); a fix places both and the route goes on from them on the other
  layer (`SetLayer`), the start pair taken from the vias.
- **Fix, Backspace, posture** (`fix_head`, `undo_last_segment`,
  `flip_posture`): a fix makes the head a leg and the next leg starts from
  `EndingPrimitives` (`m_initialDiagonal` carries the posture on); a head that
  reaches the coupled pair of pads finishes the route. Backspace is an
  extension (`DIFF_PAIR_PLACER` has no `UnfixRoute`): it puts back the world,
  the posture and the pair the route went on from. `/` toggles the posture.
- **Commit**: the whole route is one `Cmd::CommitRoute` -- the legs as tracks
  on their own nets, the vias, and the tracks and vias a shove moved as
  removals and replacements (`Router::finish_diff_pair`, `build_dp_commit`;
  the shoved lines keep their IR track by `Node::add_line_tagged`) -- so it is
  one undo step. `Router` keeps a pair, a route and a drag mutually exclusive.
- **HTTP**: `POST /api/route/dp_{start,dims,move,via,fix,undo_segment,finish}`
  (`crates/cli/src/route_api.rs`), sharing `RouteCell`/`Router` with the
  route and drag sessions; `/api/route/cancel` ends a pair too, and
  `/api/route/mode` and `/api/route/settings` reach the running pair. `dp_move`'s
  reply is the head (`head_a`, `head_b`), the fixed legs (`runs_*`), the vias
  (`vias`, `fixed_vias`), what the shove moves (`displaced`, `displaced_vias`,
  which also keeps the tracks an earlier leg pushed until the commit),
  `colliding` and `snapped_end`.
- **Frontend**: `6` (`pcbnew.InteractiveRouter.DiffPair`) arms the tool;
  `components/canvas/diffPairRouting.ts` + `kicad-port/dpTool.ts` mirror
  `routing.ts`/`routeTool.ts`; the mode actions (Highlight collisions, Walk
  around, Shove, Cycle Router Mode) and Differential Pair Dimensions reach a
  running pair; `V` is `pcbnew.Control.layerToggle`; `/` flips the posture,
  Backspace is `UndoLastSegment`, `AttemptFinish` finishes. `painter.ts`
  draws both lines (the violation colour if either collides), the pair of vias
  and what the shove moves.
- **Differences from KiCad, on purpose** (the module header of `dp_placer.rs`
  has them, with the reasoning): no springback (every move resolves from the
  world the last fix left); a walk that fails keeps the pair as drawn and flags
  it, where KiCad's `tryWalkDp` ends with `if( bestScore > 0.0 )`, which is
  always true, and so empties the pair; two walk attempts where KiCad makes
  four (`attempt & 2` is a winding `attemptWalk` never reads, so 2 and 3 repeat
  0 and 1); Backspace; the gap and via gap raised to the clearance. Not
  ported because KiCad never calls it: `DP_GATEWAYS::BuildOrthoProjections` and
  the pair's ortho mode.
- **Not done**: Route From Other End for a pair (`ROUTER::ContinueFromEnd`
  restarts the placer of the same mode; here only `Router::continue_from_end`,
  for a single track), a drag of a pair (KiCad's `DRAGGER` has no pair mode:
  `m_draggedLine` is one `LINE`). Length and skew tuning of a pair is
  Stage 8's.
- **Tests**: unit tests in `diff_pair.rs` (the gateways, the pair, the
  coupled length), `dp_placer.rs` (start, a pad pair at 45 degrees, tracks,
  walk, shove, vias, Backspace, the posture, the gap), `optimizer.rs`
  (`optimize_pair`, the memo's answer for a candidate that shares segments
  with its base) and `shove.rs`; `crates/pns/tests/dp_boards.rs` on KiCad's own
  pair board (`qa/data/pcbnew/pns_regressions/boards/dp_test.kicad_pcb`:
  no recorded session in the QA corpus routes a pair, so these are not
  replays) and a slow sweep of random cursors (`EDA_SLOW_TESTS=1`) that checks
  every accepted pair leaves the board clean and keeps its gap; the HTTP tests
  in `route_api.rs` (a routed pair is one undo step, `V` places vias, a net that
  is not half of a pair is refused); the slow
  `gate_vs_kicad::a_placed_pair_passes_kicad_clis_clearance_and_diff_pair_gap_checks`
  (kicad-cli's DRC on four placed pairs: no clearance, track width or
  diff-pair gap violation added); `kicad-port/dpTool.test.ts`; and the browser
  check `web/studio/e2e/diff-pairs.check.js` (the tool from the canvas
  through `window.__eda`: the three modes, a pair routed and undone, `V`, `/`,
  Backspace, Escape).

## Stage 8 -- length tuning (P5c)

`pns_meander.cpp`, `pns_meander_placer_base.cpp`, `pns_meander_placer.cpp`,
`pns_meander_skew_placer.cpp`, `pns_dp_meander_placer.cpp`, the tuning half of
`pns_topology.cpp` and `LENGTH_DELAY_CALCULATION`, and -- on the other side of
the HTTP boundary -- `PCB_TUNING_PATTERN` / `GENERATOR_TOOL` / `DRAWING_TOOL::PlaceTuningPattern`.
KiCad's tool, not a dialog: the first version of this stage was a one-shot dialog for a
straight, axis-aligned track with a 45-degree accordion; it is gone.

| Module | KiCad | What it is |
|---|---|---|
| `kigeom.rs` | `libs/kimath`: `SEG`, `SHAPE_ARC`, `VECTOR2`, `EDA_ANGLE`, `CalcArcCenter`, `trigo` | nanometre geometry with KiCad's own evaluation order and roundings (`KiROUND`, the truncating casts, `EDA_ANGLE`'s special-cased sin/cos) |
| `chain.rs` | `SHAPE_LINE_CHAIN` | a polyline whose runs of points can be arcs (`m_shapes`), `Append` of a point / an arc / a chain, `Split`, `Slice`, `Simplify`, `Length`, `Mirror`, `Reverse`, `NextShape` |
| `meander.rs` | `MEANDER_SETTINGS`, `MEANDER_SHAPE`, `MEANDERED_LINE`, `MEANDER_PLACER_BASE::tuneLineLength` | the shapes (single, start, finish, turn, corner, arc, the dual ones of a pair), `Fit` / `Resize` / `Recalculate`, `MeanderSegment`, `CheckSelfIntersections`, the amplitude search |
| `tuning_path.rs` | `TOPOLOGY::AssembleTuningPath`, `walkTuningPath`, `findLinesFromVia`, `LENGTH_DELAY_CALCULATION` | the lines of a net a pad to a pad (through its vias) and their length: the end of a line on a pad clipped to the pad (`OptimiseTraceInPad`), in a via to its centre, the stackup height a via spans, the via inferred in a pad on the other side |
| `meander_placer.rs` | `MEANDER_PLACER`, `MEANDER_SKEW_PLACER` | `Start`, `Move` (`doMove`), `CheckFit`, the amplitude and spacing steps, the status (`TUNED` / `TOO_SHORT` / `TOO_LONG`); the skew placer lengthens the clicked line to the partner's length plus the target skew |
| `dp_meander_placer.rs` | `DP_MEANDER_PLACER`, `TOPOLOGY::AssembleDiffPair`, `DIFF_PAIR::CoupledSegmentPairs` | the pair assembled from a segment, its coupled stretches, `baselineSegment`, `pairOrientation`, `CheckFit` at four widths |
| `tuning_commit.rs` | `PCB_TUNING_PATTERN::EditFinish`, `resetToBaseline`, `Remove` | what a tuned line becomes in the IR: the tracks with a segment of the stretch are replaced, the runs of them outside it handed back, the line up to the pattern, the pattern (a polyline for each run of segments, an arc for each arc, flagged as members) and the line after it |

**Checked against KiCad's own code.** `crates/pns/oracle/` compiles
`pns_meander.cpp` and the tuning functions of `pns_meander_placer_base.cpp`
verbatim over the real `libs/kimath` (clang++, stand-in headers for the
rest, `KICAD_SRC_DIR=<8303b2ad checkout> crates/pns/oracle/build.sh <dir>`, a
minute) into a program that answers one line per case; `tests/meander_oracle.rs`
makes the same cases (arcs, chains, shapes, single and dual lines, coupled
segment pairs), runs them through this crate and compares the answers line by
line: 100 000 random cases agreed when it was written (`EDA_PNS_ORACLE=<binary>
EDA_PNS_ORACLE_CASES=<n>` runs them again; 6 000 arcs, 6 000 chains, 15 000
shapes and 3 000 lines at `1500` ran green on 2026-10-13), and the answers of 118 of them are
baked into `tests/data/meander_kicad_cases.txt`, which the always-on test
`kicad_answers_baked_in` reads. The one disagreement the oracle found (a dual
shape's baseline midpoint used the truncating `/ 2` where `VECTOR2I::operator/`
rounds) is fixed.

**The tool** (`crates/cli/src/tune_api.rs`, `tune_edit.rs`; the studio's
`components/canvas/tuneRouting.ts`): `7` / `8` / `9` arm it; hovering a track
reads its length against the target (`current length | min | max`, red where it is
outside), the first click starts a pattern on the track at the point nearest the
click, the cursor sets where it ends and which side the first meander is on
(`UpdateSideFromEnd`) and the placer fits the meander, `1`-`4` step the spacing
and the amplitude, Ctrl+L is the Tuning Pattern dialog (Board Setup's
Length-tuning Patterns are what it starts from), the second click commits one
`Cmd::CommitTuning` (one undo step), Escape drops the pattern and then leaves the
tool. The target is a design rule's `length` (`skew`) constraint for the net
(`.kicad_dru`, the last rule that applies, `Opt()` being its minimum when it has no
optimum) unless the pattern overrides the custom rules.

**A pattern is an item** (`eda_model::tuning::TuningPattern`, `DrawingsSection::tuning_patterns`;
`crates/kicad/src/pcb_generated.rs`): the tracks of the meander are its members,
it keeps the settings it was made with, the baseline it replaced and (for a pair) the
partner's, and it is written to the `.kicad_pcb` as `(generated (type tuning_pattern) ..)`
and read back (KiCad's own `tuning_generators_load_save.kicad_pcb` and `issue22536.kicad_pcb`
load with their members and write again). It selects as one (the studio lists it
with the groups, `generator: tuning_pattern`), E / double-click is its dialog and tunes it again
(`resetToBaseline`, then the placer, one undo step), Delete puts the track back
(`Remove`), its four edit points (start, end, the width of the meanders and the
side, their spacing; `MakeEditPoints` / `UpdateFromEditPoints`) can be dragged,
`Generator.regenerate*` tune patterns again on the board as it is, and the
Generators Manager lists them.

**Proof.** kicad-cli is the judge (`tune_api::tests::kicad_cli_finds_the_tuned_nets_within_the_length_and_skew_rules_they_were_tuned_to`,
`EDA_SLOW_TESTS=1`): a track tuned to 22 mm is within 21.99..22.01 mm for a `length` rule,
a pair tuned to 26 mm within 25.9..26.1 mm per net and 0.3 mm of skew, the skew tuner
takes a 1.4 mm skew under a 0.15 mm rule -- and the same rules flag the nets before
they were tuned. `web/studio/e2e/length-tuning.check.js` drives it all through the
page (93 checks).

**Differences from upstream** (each listed in `docs/parity/GAPS.md` item 7 where it is not final):

- the world has no arc item (decision 3): an arc track is its chords, a track that
  is an arc cannot be picked to start a pattern on, and a line through an arc is
  tuned along its chords;
- the world is cloned once at `Start` (the line taken out of it), not branched at every
  `Move`: nothing in `Move` writes to it;
- the line a placer works on stops where the track width changes (KiCad rewrites the
  whole of it at the first segment's width);
- no time domain (a target in picoseconds, tuning profiles' delays) and no pad-to-die length:
  the model has neither;
- the status box is a box at the cursor in the canvas, not a `ROUTER_PREVIEW_ITEM` list; the
  tuned path is drawn as the new tracks over the board with the replaced ones hidden;
- a pattern's outline is the baseline offset round (a capsule); KiCad's merged polygon for a pair
  is two of them;
- vertices are micrometres on the board (a meander of 60 segments is within 0.01 mm of the
  length the placer reports; the proof above is at 10 um);
- a pattern is moved, turned and flipped as locked (it and its tracks): KiCad lets them move
  and leaves the baseline behind.

## Known gaps vs. upstream (won't-fix for this task, tracked for later)

- `SMART_PADS`, `FANOUT_CLEANUP` optimizer passes -- `MERGE_OBTUSE` is now
  ported too (`src/optimizer.rs`'s `merge_obtuse`, run in upstream's own
  `mergeFull -> mergeObtuse -> mergeColinear` order; see that module's
  header comment for exactly how it differs from `MERGE_SEGMENTS`: it
  extends two existing obtuse segments' own directions to their natural
  meeting point rather than hunting for a fresh lower-cost bypass, which
  can collapse a long obtuse "staircase" (common after a walkaround) in
  one step). `SMART_PADS`/`FANOUT_CLEANUP` remain unported.
  **Status (2026-10-07): closed** -- both are ported in `src/optimizer.rs`
  (`smart_pads_single`, `run_smart_pads`, `fanout_cleanup`), and
  `RoutingSettings::smart_pads` (default on) now gates smart pads in
  `line_placer.rs`. The placer never passes the `FANOUT_CLEANUP` flag
  (`docs/parity/CODE-COMPARE-router.md` section D). The other settings
  fields named below are still never read.
  **Status (2026-10-08):** closed -- every `RoutingSettings` field is read:
  `shove_vias`, `jump_over_obstacles`, `optimizer_effort`, `smart_pads`,
  `shove_iteration_limit` and `walkaround_iteration_limit` by the shove and the
  placer, and now `fix_all_segments` (`LinePlacer::fix`),
  `walkaround_hug_length_threshold` (`walkaround::walk_base`) and
  `via_force_prop_iteration_limit` (`shove::via_pushout_force`), plus the new
  `free_angle_mode`. The settings dialog has a row for each KiCad has.
- ~~The board outline is not an obstacle.~~ **Fixed 2026-10-08.** The outline
  is one `Solid` per segment on every copper layer (`from_ir::add_board_outline`,
  `Solid::edge`), asked only for the board's copper-to-edge clearance
  (`Node::clearance_to`), as `syncGraphicalItem` and `Clearance( .., CT_EDGE_CLEARANCE )`
  do. A shove toward the edge stops at the clearance
  (`tests/board_edge.rs`; eight shoves toward `mcu30`'s top edge leave kicad-cli no
  `copper_edge_clearance`). **Every Edge.Cuts item is an obstacle (2026-10-09):**
  `from_ir::add_board_outline` takes the board's Edge.Cuts items (`eda_model::outline::edge_cuts_shapes`:
  the polygon's sides and every shape on Edge.Cuts, a footprint's own graphic included, which the
  importer makes a board-level shape) and puts a zero-width `Solid` on every side of each one's
  effective shape (an arc's chords at KiCad's 0.005 mm, a circle's or a rectangle's ring, a curve's
  flattening), as `syncGraphicalItem` does with `MakeEffectiveShapes()`; a cutout's rim keeps the
  clearance like the outer edge (`tests/board_edge.rs`,
  `an_arc_edge_and_a_cutout_are_obstacles_like_the_outer_edge`). Not modelled: castellated pads'
  edge exclusions. A walk around an edge in a
  shove may not grow past 2 x `WalkaroundHugLengthThreshold` of the line's own
  length (KiCad has no bound; with no room left the shortest clear walk is the long
  way round the whole outline).
- `KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` optimizer constraints
  (every candidate is still collision-checked, which is the one
  constraint that must never be skipped; the others are refinements).
- Rounded (arc-filleted) corners and arcs as router items -- no arc geometry
  in this port's `Item` (decision #3): the tuner makes arcs (Stage 8) and the router
  sees them as chords.
- ~~`VIA::PushoutForce`~~ is ported (2026-10-08, `shove::via_pushout_force`); the
  second lead KiCad tries (`m_last_p_end`) is not kept.
- `MOUSE_TRAIL_TRACER` (posture guessed from the swept mouse trail) and
  springback (an incremental undo stack so backing the cursor up reverts
  shove/walkaround decisions instead of recomputing from scratch) --
  still not implemented; see `line_placer.rs`'s own module doc comment
  (point 1) and `shove.rs`'s Stage 3 doc comment above for why, and
  `docs/parity/GAPS.md` for this task's own priority ranking of what's
  left in gap #7.
