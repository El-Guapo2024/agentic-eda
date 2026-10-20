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
3. **Arcs are `ARC_T` items (P5d, 2026-10-20; before it, no arc was in the world).**
   The IR keeps an arc track (`PCB_ARC`) as a polyline of the 32 chords of its
   tessellation plus the mid point (`Track::arc`, `arc_mid_offset`), so every
   consumer that only knows polylines keeps working. `from_ir.rs` makes such a
   track one `Item::Arc` (`ArcPts`: start, mid, end; the shape is its 1 um
   polygonisation, the hull is `ArcHull`, `pns_utils.cpp:68`), with its collisions,
   its hull, `NODE::AssembleLine` / `followLine` through it and `NODE::Add( LINE )`
   making an `ARC` of an arc of the line (`arcs.rs`, `item.rs`, `node.rs`). A `Line`
   holds its arcs as `Line::arcs` and finds them again **by their points** (`arc_runs`):
   whatever cuts into an arc (a corner dragged inside it, a route that starts or ends
   in the middle of it, a walk that hugs it) turns that arc into segments, and whatever
   leaves it alone keeps it -- an arc is an arc when it is committed (`line_to_tracks`
   writes polyline runs and `Track::new_arc` arcs), which is what the board file gets as
   `(arc ..)`. KiCad's `SHAPE_LINE_CHAIN` carries arcs in `m_shapes` and loses them at the
   first `Append( vertex )` of `LINE::Walkaround`; here a walked line keeps the arcs it did not
   touch. Where a
   KiCad pass branches on "does this line contain an arc" (the optimizer skips such a
   line, `Simplify` keeps an arc's points) the same branch is taken. Not ported: the
   rounded corners of `CORNER_MODE` (`SwitchRounding*`, `BuildInitialTrace( ROUNDED_45 )`:
   the router makes no arc of its own; the tuner's meanders do, Stage 8).
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
| `pns_line_placer.{h,cpp}`, `pns_mouse_trail_tracer.{h,cpp}` | `src/line_placer.rs`, `src/mouse_trail.rs` | See `line_placer.rs`'s own doc comment for the detailed list. Posture is KiCad's since P5d (2026-10-20): `mouse_trail.rs` is `MOUSE_TRAIL_TRACER` (`AddTrailPoint`, `GetPosture`: the areas of the straight-first and the diagonal-first traces against the trail with its locks and the correction against the last segment, `FlipPosture`, tolerance, `SetMouseDisabled`), and `LinePlacer` builds the head's elbow from it (`buildInitialLine`: `GetPosture( aP )`; `Move` adds the cursor to the trail afterwards; `FixRoute`, a via and `UnfixRoute` restart it; `/` flips from the shown head's first direction; a posture forced by hand leaves Smart Pads off). The setting is "Use mouse path to set track posture" (`RoutingSettings.auto_posture`, default on); off, the posture is the explicit direction state and `/`. It is checked against KiCad's own tracer and `DIRECTION_45` compiled over `libs/kimath` (`oracle/build.sh`, `tests/mouse_trail_oracle.rs`: 100 000 random walks of the cursor, with flips, restarts and the mouse switched off, agree to the nanometre and the octant; 60 of them baked into the always-on test). Not ported: `reduceTail`, `handlePullback`, `handleSelfIntersections`, `mergeHead`, `optimizeTailHeadTransition` (the stateless request model). |

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
- Not modelled: `SHP_IGNORE`, `LockJoint` (the head's end points pinned; nothing
  here shoves a line of the head's own net), holes. (`SHP_REVERSED` is: Stage 5.
  Arcs, locked items and a head that ends in a via are, since P5d: Stage 9.)

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
"free-angle" is superseded; what stayed out then and is ported now (P5d, Stage 9):
`OptimizeEntireDraggedTrack`, arcs (`DM_ARC`), locked tracks; what still stays out: springback,
virtual vias.

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
**2026-10-20 (P5d):** a track that is an arc is split too: `SplitAdjacentArcs` is `Node::split_arc` /
`arc_to_split` over `arcs::ArcPts::split` (the arc cut at the point into two arcs of the same circle), used where `SplitAdjacentSegments` is
(KiCad itself calls it only from Break Track, `ROUTER::BreakSegmentOrArc`; `LINE_PLACER::initPlacement`
and `Move` call `SplitAdjacentSegments`, which returns false for `ARC_T` -- a route starting or ending in
the middle of an arc is an adaptation the P5d brief asked for), and the commit gives the IR the two
`(arc ..)` tracks. See Stage 9.

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
  world the last fix left); two walk attempts where KiCad makes
  four (`attempt & 2` is a winding `attemptWalk` never reads, so 2 and 3 repeat
  0 and 1); Backspace; the gap and via gap raised to the clearance. Not
  ported because KiCad never calls it: `DP_GATEWAYS::BuildOrthoProjections` and
  the pair's ortho mode. (A walk that fails used to keep the pair as drawn and
  flag it, where KiCad's `tryWalkDp` ends with `if( bestScore > 0.0 )`, which is
  always true, and so empties the pair: it empties the pair now, Stage 9 step 5.)
- **Not done**: a drag of a pair (KiCad's `DRAGGER` has no pair mode:
  `m_draggedLine` is one `LINE`). Length and skew tuning of a pair is
  Stage 8's. (Route From Other End for a pair -- `ROUTER::ContinueFromEnd`
  restarts the placer of the same mode -- is done: Stage 9 step 5.)
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

- (arcs and the time domain were here; both are KiCad's since P5d: Stage 9 steps 1 and 6 -- a track
  that is an arc is picked to start a pattern on and a line through an arc is tuned along the arc;
  a target is a delay in picoseconds through the tuning profile of the net's class)
- the world is cloned once at `Start` (the line taken out of it), not branched at every
  `Move`: nothing in `Move` writes to it;
- the line a placer works on stops where the track width changes (KiCad rewrites the
  whole of it at the first segment's width);
- no pad-to-die length or delay: the model has neither; a time-domain rule of a `.kicad_dru` file
  (`(constraint length (min 149.5ps) ..)`) is not read as the target (the length and skew rules are);
- the status box is a box at the cursor in the canvas, not a `ROUTER_PREVIEW_ITEM` list; the
  tuned path is drawn as the new tracks over the board with the replaced ones hidden;
- a pattern's outline is the baseline offset round (a capsule); KiCad's merged polygon for a pair
  is two of them;
- vertices are micrometres on the board (a meander of 60 segments is within 0.01 mm of the
  length the placer reports; the proof above is at 10 um);
- a pattern is moved, turned and flipped as locked (it and its tracks): KiCad lets them move
  and leaves the baseline behind.

## Stage 9 -- the router's leftovers (P5d, 2026-10-20)

What `docs/parity/GAPS.md` item 7 and `CODE-COMPARE-router.md` D18 listed as missing after Stages 5 to 8, in six steps
(branch `task-router-rest`, one commit each).

1. **Arcs (`ARC_T`).** Decision 3 above is the design. `arcs.rs` is `ArcPts`, the 1 um polygonisation of `SHAPE_ARC::ConvertToPolyline`,
   `ArcHull` (`pns_utils.cpp:68`), `SplitAdjacentArcs` and `CalcArcMid`; `Item::Arc` has the shape, the hull and the collision of `ARC`; the
   node assembles a line through arcs (`AssembleLine`, `followLine` with the arc reversed where the line walks it backwards) and `Node::add_line`
   makes an `ARC` of an arc of the line (`NODE::Add( LINE )`; `findRedundantSegment` / `findRedundantArc` when the line may not be redundant).
   A route starts and ends in the middle of an arc track (`Node::split_arc`; KiCad's `SplitAdjacentArcs` is reached only from Break Track, the
   placer's `SplitAdjacentSegments` returns false for `ARC_T`: **adapted**, as the brief asked). The shove treats an arc as an obstacle
   (`onCollidingArc` is `ShoveObstacleLine` on the line the arc is part of; the arcs of a shoved line it does not reach are kept; for the arcs of
   a line the clearance grows by `ARC_HIGH_DEF`, `pns_shove.cpp:588`); KiCad's main loop looks for `SEGMENT_T` obstacles only, so there
   `onCollidingArc` is never reached and here arcs are searched with the segments. `DM_ARC` (`arc_drag.rs`, `DragKind::Arc`) is
   `EDIT_TOOL::DragArcTrack` (`edit_tool.cpp:1040`) as a drag of the router: the cursor is a point the arc has to go through, the arc stays
   tangent to the two tracks next to it and its ends slide along them (the tangent lines and circles, `tan_tan_pt`, the triangle the cursor is held
   in); KiCad's PNS has no case for `DM_ARC` (`dragMarkObstacles`, `dragWalkaround` and `dragShove` do nothing) and `ROUTER_TOOL::performDragging`
   hands an arc to `DragArcTrack`, which checks nothing against the board -- here the three modes judge the result. `DragArcTrack` refuses an arc of
   179 degrees and more, and so does the dragger. A drag or a shove keeps the arcs it does not cut into (`shove_dragged_line`, `Line::arcs`,
   `DragPreview::arcs`), `Router::build_commit` writes a line as polylines and `(arc ..)` tracks (`line_to_tracks`). The length tuner starts on an
   arc track (`MEANDER_PLACER::Start`: `getSnappedStartPoint`) and goes through the arc (`AddArc` in `doMove`: KiCad's loop steps over the segment
   after an arc, here it does not; `MeanderedLine::AddArc` took `GetP1()` for the arc's start, which is its end). The clean-up of a walked path knows
   the arcs of its line (`Walker::arcs`, `shove_line_to_hull_set`): a fillet's first chord lies on the run before it, so the vertex the run ends and the arc
   starts at is collinear with its neighbours, and a `Simplify` that does not know the arc drops it and the arc with it. The commit of a shove writes the arcs
   the shove left as arc tracks (`Shove::outcome` carries `Line::arcs`). Quirks kept: `onCollidingArc` answers `SH_OK` when it fails to shove. Tests: `tests/arc_tracks.rs` (21: the arc in the world, hull and collision, a route starting and
   ending in one, a shove that keeps and a shove that cuts, a pushed run before a tangent arc, each `DM_ARC` mode, the commit), `tests/arc_boards.rs` (KiCad's own
   `issue12609` and `issue8407`: routes, shoves, segment, corner and arc drags leave the router's world no violation it did not have; short sweep,
   `EDA_SLOW_TESTS=1` for 40 tries a board and mode), units in `arcs.rs`, `arc_drag.rs`, `line.rs`, `node.rs`, `shove.rs`, and the slow kicad-cli proof
   in `crates/cli/src/gate_vs_kicad.rs` (below), `tune_api.rs` (a track that is an arc picked and tuned along).
2. **Locked tracks.** `from_ir` sets `Segment.locked`, `Arc.locked` and `Via.locked` from `DrawingsSection::locked_ids` and from the members of a
   tuning pattern that is not being edited (`PNS_KICAD_IFACE_BASE::syncTrack` / `syncArc` / `syncVia`: `MK_LOCKED`, and `generator && !IsBeingEdited`;
   the length tuner's world has its patterns in edit: `build_node_for_tuning`). A locked item is a solid for the shove
   (`onCollidingSegment` / `onCollidingArc`: `HasLockedSegments` -> `SH_TRY_WALK`; `pushOrShoveVia`), `AssembleLine` does not follow it
   (`aStopAtLockedJoints`, `NextSegment`), the dragger refuses it with "The selected item is locked." (`ROUTER_TOOL::performDragging`) and
   `drag_locked` is Drag Anyway (`DRAGGER::Start` unmarks the item). The halves of a locked track a route cuts stay locked
   (`RouteCommit::lock_tracks` -> `Cmd::CommitRoute::lock_tracks`; `PNS_KICAD_IFACE::AddItem`), and a commit does not leave a redundant copy of a track
   (`findRedundantSegment` / `findRedundantArc` in `NODE::Add( LINE )`). Tests: `tests/locked_tracks.rs` (7: the shove, the dragger's refusal and Drag Anyway,
   the halves a route cuts, the commit), `tune_api.rs` (a pattern's members are locked in a route's world and not in the tuner's) and the browser check (`D` on a locked run asks, Drag Anyway drags it).
3. **A head that ends in a via is shoved together with it** (`LINE_PLACER::rhShoveOnly`: `newHead.AppendVia( makeVia( newHead.CLastPoint() ) )`, then
   `m_shove->AddHeads( newHead )`; `SHOVE::Run` adds the head's via to the node with the head's rank). `LinePlacer::build_head` in Shove mode pushes
   the pending via off the pads under it (`buildInitialLine`: `PushoutForce` with `SOLID_T`), walks the head round the pads, makes the via at its end
   (`makeVia`, spanning the layer the route is on to the one it goes on to: `via_to_layer`, set by `Router::toggle_via`) and runs `Shove::run_heads` with
   the head and its via, as a pair's heads already were: what the via sits on, on every layer it spans, is pushed aside instead of being reported
   as a collision. The other modes place the via on the finished head and test its collision on the whole span. Tests: `tests/head_via.rs` (2), three
   in `line_placer.rs`.
4. **Mouse-trail posture and "Optimize entire track being dragged".** `mouse_trail.rs`: Stage 2's row. The optimizer has KiCad's constraint
   system: `PRESERVE_VERTEX` (a shortcut over the stretch the preserved point is on must pass through it), `KEEP_TOPOLOGY` (no pad of another net
   inside the polygon the shortcut and the stretch enclose, `pointInside2`; `Node::solid_joints_in` is `QueryJoints` for pads) beside `RESTRICT_AREA`.
   `DRAGGER::optimizeAndUpdateDraggedLine` (`Dragger::optimize_and_update_dragged_line`): with `optimize_entire_dragged_track` (default off, as
   KiCad's) the walked or shoved line, and a via's walked fan-out, is run through `MERGE_SEGMENTS | KEEP_TOPOLOGY | RESTRICT_AREA` (and
   `MERGE_COLINEAR` with Smooth dragged segments), in the area the drag changed, the point nearest the cursor preserved. Both rows of Interactive
   Router Settings are live (`auto_posture`, `optimize_entire_dragged_track`). Tests: `tests/mouse_trail_oracle.rs`; units in `mouse_trail.rs`,
   `optimizer.rs`, `dragger.rs`, `line_placer.rs`.
5. **A pair: Route From Other End and a walk that fails.** `ROUTER::ContinueFromEnd` restarts the placer of the same mode, so KiCad does it for
   a pair: `Router::continue_diff_pair_from_end` commits the legs the pair has fixed (one `Cmd::CommitRoute`, nothing if none) and starts a new pair
   at the far end of the P net's connection (`GetNearestRatnestAnchor`: `NearestUnconnectedAnchorPoint` of the P line, the fixed legs counting as
   connected) with the pair's width, gap and via gap, heading for where the old head ended; a far end a pair cannot start on ends the routing with
   KiCad's message after the commit. `tryWalkDp`'s `if( bestScore > 0.0 )` is true all the same (the score starts at 1e14), so when neither
   attempt works `aPair.SetShape( best )` takes the default, empty `DIFF_PAIR` and the function answers true: a pair that cannot be walked has no head
   (Walk around: a fit that places nothing; Shove: a shove with two empty heads, which fails) instead of showing the pair as drawn, flagged.
   `POST /api/route/dp_continue_from_end`; Ctrl+E for a pair in the studio. Tests: `tests/dp_boards.rs` (KiCad's `dp_test.kicad_pcb`: the
   pair continued from the stubs' end with and without a fixed leg), a unit test of the emptied pair (`dp_placer.rs`), an HTTP test (`route_api.rs`: the two commits are
   one undo step) and Ctrl+E in `web/studio/e2e/diff-pairs.check.js`.
6. **The tuner's time domain.** `delay.rs`: `DelayParams` from the net class's tuning profile (`TUNING_PROFILE`: per copper layer
   `delay_as_per_mm`, only with "Enable time-domain tuning"; a via has the override of its layer pair or the profile's delay per millimetre
   over the stackup height between the signal layers), `TuningPath::delay` (`CalculateDelay` of the pad-clipped lines and the vias between
   lines on different layers), `GetTrackLengthForPropagationDelay`. `MeanderSettings` has the delay targets (`DELAY_UNCONSTRAINED`,
   `DEFAULT_DELAY_TOLERANCE` 0.1 ps, `SetTargetLengthDelay`, `SetTargetSkewDelay`); `MeanderPlacer` / `DpMeanderPlacer` turn the delay
   still to add into a length (`calculateTimeDomainTargets`: the signed difference of the target and the current delay over the delay per
   millimetre of the layer) and report the delay (`TuningDelayResult`). Attoseconds inside (`IU_PER_PS` 1e6), picoseconds on the wire and in the
   board file (`is_time_domain`, `target_delay{,_min,_max}` of the `(generated ..)` record). The Tuning Pattern dialog has the Delay radio button
   and the status box reads picoseconds. Not modelled: the pad-to-die delay (the model has none), a time-domain rule of a `.kicad_dru` file as the target
   (the length and skew rules are read). Tests: `delay.rs` (4), `tune_api.rs` (2), the slow kicad-cli test
   `kicad_cli_measures_the_delay_of_a_track_tuned_in_picoseconds_through_its_tuning_profile` (a `length` rule of 149.5 to 150.5 ps flags the
   untuned track and passes the tuned one, to 0.1 ps).

**Proof against kicad-cli** (`crates/cli/src/gate_vs_kicad.rs`, `EDA_SLOW_TESTS=1`):
`routes_across_and_onto_an_arc_track_and_a_dragged_arc_add_no_violation_to_kicad_clis_drc` -- on KiCad's `issue12609` and `issue8407`, a route
across an arc track in Shove mode, a route that ends in the middle of one (splitting it), and an arc dragged add no `clearance`, `shorting_items`,
`tracks_crossing`, `hole_clearance`, `copper_edge_clearance` or `track_width` violation to kicad-cli's report (run with `--all-track-errors`,
or its counts wander with which violation of a track it reports first), and every arc the commit leaves is an `(arc ..)` of the board file (two routes across, two onto and two arcs
dragged on each board; 34 s in a debug build).

**Browser check** (`web/studio/e2e/router-leftovers.check.js` on a scratch board made with `eda board new examples/mcu_board_30plus.yaml`, port 8837): 93 checks in 10
scenarios through `window.__eda` -- the two router-setting rows, the mouse path setting the posture, `D` on an arc, a shoved line keeping its arc, a route ending and starting in the middle of an arc,
a locked track (refused, Drag Anyway, walked round), `V` shoving the track under a head's via, a tuning pattern on an arc track and a target in picoseconds -- and a Ctrl+E scenario in
`diff-pairs.check.js`. It found two bugs the unit tests did not: Drag Anyway had no dialog host on the PCB tab, and the clean-up after a walk dropped the vertex a tangent arc starts at.

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
- ~~`KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` optimizer constraints~~
  **Ported 2026-10-20** (Stage 9 step 4: `optimizer.rs`'s constraints, used by
  "Optimize entire track being dragged").
- ~~Arcs as router items~~ **Ported 2026-10-20** (Stage 9 step 1). Still missing: rounded (arc-filleted)
  corners made by the router itself (`CORNER_MODE`: `SwitchRounding*`,
  `BuildInitialTrace( ROUNDED_45 / ROUNDED_90 )`): the tuner is the only maker of arcs.
- ~~`VIA::PushoutForce`~~ is ported (2026-10-08, `shove::via_pushout_force`); the
  second lead KiCad tries (`m_last_p_end`) is not kept.
- ~~`MOUSE_TRAIL_TRACER`~~ **Ported 2026-10-20** (Stage 9 step 4). Springback
  (an incremental undo stack so backing the cursor up reverts
  shove/walkaround decisions instead of recomputing from scratch) --
  still not implemented; see `line_placer.rs`'s own module doc comment
  (point 1) and `shove.rs`'s Stage 3 doc comment above for why, and
  `docs/parity/GAPS.md` item 7 for what is left in gap #7: springback, `LockJoint`, virtual vias,
  the walkaround cluster timeout, the pad-to-die length and delay, time-domain rules as the tuner's
  target, the rounding of corners, the merged outline of a pair's pattern, a pattern moved, turned
  or flipped, the tuner's world branched at every move, two walk attempts of a pair where KiCad makes
  four, and the tracer's `reduceTail` / `handlePullback` family.
