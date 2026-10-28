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
   line, `Simplify` keeps an arc's points) the same branch is taken. The router makes
   arcs of its own since Stage 10 step 2: the rounded corners of `CORNER_MODE`.
4. **No component dragger, no multi-selection drag.** Single-item
   (segment/via) drag only. Diff pairs (Stage 7) and length tuning
   (Stage 8: single track, pair and skew, KiCad's placers and tool) are
   ported; KiCad's dragger has no pair mode, so there is nothing to port for a
   pair drag.
   **Status (2026-10-11):** the diff pair is KiCad's `DIFF_PAIR_PLACER` now
   (Stage 7): coupled in all three modes, walked and shoved as one unit, with
   vias.
5. **Units**: KiCad's own, integer nanometres, typed as `eda_model::units::Nm` (the IR moved to nanometres on 2026-10-23, the length
   type came on 2026-10-24; Stage 13 below). The router's geometry *is* the IR's: a point is `eda_model::ir::Point` (two `Nm`), a vector
   (`diff_pair::Vec2`, a force, an MTV) is `(Nm, Nm)`, and every width, clearance, distance and length is `Nm`. Some module docs older
   than the nanometre move still speak of micrometres; the numbers are nanometres throughout.

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
| `geometry/direction45.h`, `direction_45.cpp` | `src/direction45.rs` | `CORNER_MODE`'s four modes and `BuildInitialTrace` in each (Stage 10 step 2: on the nanometre grid, checked against KiCad's own compiled `direction_45.cpp`, 100 000 random cases). `BuildInitialTrace`'s "only honor `start_diagonal` when direction is `Undefined`" subtlety preserved. |

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

- **The springback stack is the placer's, not the shove's.** KiCad's `SHOVE` owns `m_nodeStack`; here it is
  [`crate::springback`], kept by the placers that shove (the line placer, the dragger, the pair placer), and a shove is one `Run` from the
  node it is given: `shove_line_after` / `shove_heads_after` start from a reduced frame's world with its area (`totalAffectedArea`), the
  plain entry points from the committed world (decision 1, and the HTTP-per-sample driver). The shove main loop is bounded
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
| W / Shift+W (width) | `pcbnew.EditorControl.trackWidthInc`/`trackWidthDec` -> the board's width list by index (`routing.track_width_presets`, entry 0 the net class's), or the pair list while a pair is routed (Stage 18, r5 R5-5/R5-6) |

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
"free-angle" is superseded; what stayed out then and is ported now (P5d, Stage 9, then Stage 10's virtual vias and Stage 14's
springback): `OptimizeEntireDraggedTrack`, arcs (`DM_ARC`), locked tracks, virtual vias, springback, which is now a `DragShove` per
gesture (`dragShove` / `dragViaShove`'s one `SHOVE`, the via's place in each frame for `m_draggedVias`).

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
- **Springback** (Stage 14): in Shove mode a `Move` starts its shove from the newest frame the new heads still press on
  (`reduceSpringback` with both lines' heads at once), routes, walks and shoves in that frame (`m_currentNode`), and pushes the world the
  shove leaves; a fix makes a fresh `SHOVE` over the committed world (`FixRoute`), so the stack restarts empty and no frame is locked, and
  Backspace clears it.
- **Differences from KiCad, on purpose** (the module header of `dp_placer.rs`
  has them, with the reasoning): two walk attempts where KiCad makes
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
   the shove left as arc tracks (`Shove::outcome` carries `Line::arcs`). A quirk not kept: KiCad's `onCollidingArc` answers `SH_OK` when it fails to shove (the main loop then runs into its iteration limit, which fails the shove all the same); here it answers `SH_INCOMPLETE` at once. Tests: `tests/arc_tracks.rs` (21: the arc in the world, hull and collision, a route starting and
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

**Browser check** (`web/studio/e2e/router-leftovers.check.js` on a scratch board made with `eda board new examples/mcu_board_30plus.yaml`, port 8837): 95 checks in 10
scenarios through `window.__eda` -- the two router-setting rows, the mouse path setting the posture, `D` on an arc, a shoved line keeping its arc, a route ending and starting in the middle of an arc,
a locked track (refused, Drag Anyway, walked round), `V` shoving the track under a head's via, a tuning pattern on an arc track and a target in picoseconds -- and a Ctrl+E scenario in
`diff-pairs.check.js`. It found two bugs the unit tests did not: Drag Anyway had no dialog host on the PCB tab, and the clean-up after a walk dropped the vertex a tangent arc starts at.

## Stage 10 -- the router's details (springback, rounded corners, virtual vias; 2026-10-23)

`docs/parity/GAPS.md` item 7's three leftovers of the interactive router, in this order (branch `task-router-details`, one commit each).

1. **Springback** (`src/springback.rs`; `pns_shove.cpp`: `SPRINGBACK_TAG`, `reduceSpringback`, `pushSpringback`, `AddLockedSpringbackNode`,
   `RewindSpringbackTo`, `RewindToLastLockedNode`; `pns_line_placer.cpp`: `rhShoveOnly`, `FixRoute`, `UnfixRoute`). In Shove mode every `Move`
   pops the frames, newest first, whose world the head (as the pad walk left it, with its via) no longer collides with -- never a locked one, never
   the last one -- shoves from the newest frame left (`m_currentNode = parent->Branch()`), and pushes the world the shove left with its area merged
   into the frame below's (the optimizer's restriction area is that merged area and the shove's own: `totalAffectedArea`, `shove_line_after` /
   `shove_heads_after`). A shove that fails is walked in the parent frame's world (`rhWalkOnly` in `m_currentNode`). A click pushes a locked frame
   with the fixed run (and a placed via) in it; Backspace erases the stack from the frame the undone run was fixed in (`RewindSpringbackTo(
   st.commit )`). The other modes resolve the head in the world the last click left. The commit replaces what the newest frame has moved against
   the session's world: every shove of the session that is still in it, the earlier legs' included (`LinePlacer::displaced_tracks`), and the
   preview reports the same (`Preview::displaced_lines` is the frame's, not only this move's). **Differences:** a frame is a diff against the
   session's world (the IR tracks and vias it moved, by id, and the fixed runs), not a `NODE` -- a `Node` is a full clone here (decision 1) and a
   route pushes a frame per mouse move; the frame's world is made from it when a shove starts there, and `reduceSpringback`'s `CheckColliding`
   asks the session's world without what the frame moved plus a small node of what it moved to. The fixed runs were **locked** in the frame (until the review below: now they are tracks a later shove may push, as KiCad's). When the stage's `commit` node is not a frame (no shove had succeeded before the click),
   KiCad's `RewindSpringbackTo` finds nothing and keeps the undone run's locked frame; here it is erased. `SetSpringbackDoNotTouchNode` has no
   counterpart (the end item is picked from the session's world, never a frame). Not ported here: the dragger's and the pair placer's use of the
   stack (`DRAGGER::dragShove`, `DIFF_PAIR_PLACER::tryShove`), which Stage 14 ports (the dragger one `SHOVE` per gesture; the pair a frame per
   move of the leg, a fresh `SHOVE` at each fix and no locked frames).
   **Before:** every move shoved from the session's world as it was when the route started, and the fixed legs' shoves were merged by IR id
   afterwards, so a second leg could push a track back over the first. **Proof:** KiCad's recorded sessions match better --
   `simple-shove-1` reproduces 24 of the 28 segments KiCad committed (12 before), `issue22749-shove-weird-drag-track-end` all 11 (4 before;
   `tests/qa_regressions.rs`); units in `springback.rs` (4) and `line_placer.rs` (a pushed track pushed on and sprung back; the next leg shoving
   from the click's world, a clean commit, Backspace taking the push back); kicad-cli
   (`gate_vs_kicad::tests::springback_sessions_add_no_violation_to_kicad_clis_drc`, `EDA_SLOW_TESTS=1`): the `issue22749` session replayed and
   routes on `pic_programmer` that push, draw back, push again, click, Backspace and go on add no copper violation to kicad-cli's report.
   The browser check (`router-leftovers.check.js`, the springback scenario) found a bug of the route API: a cursor that is not a whole
   micrometre (the studio sends `32000.000000000004` after Backspace, and on `/`) was read as missing, and the head went to 0 -- `route_api::num`
   rounds it now.

2. **Rounded corners** (`CORNER_MODE`; `libs/kimath/src/geometry/direction_45.cpp`: `BuildInitialTrace`; `pns_routing_settings.cpp`: `m_cornerMode`,
   `corner_mode`; `pns_router.cpp`: `ToggleCornerMode`; `router_tool.cpp`: `ACT_SwitchCornerMode*`, `handlePnSCornerModeChange`, the Track Corner Mode
   submenu; `pns_line_placer.cpp`: `buildInitialLine`, the smart-pads condition of `rhWalkOnly` / `rhShoveOnly`, `FixRoute`'s "act as though we are in
   fix-all mode" with arcs; `pns_optimizer.cpp`: `mergeStep`'s bypass in the corner mode, the arc segments it skips; `pns_shove.cpp`: `runOptimizer`'s
   smart-pads condition). `CornerMode` has KiCad's four modes; `Direction45::build_initial_line` is `BuildInitialTrace` in each, computed on the
   nanometre grid (`ConstructFromStartEndAngle`, `ConstructFromStartEndCenter`, the centre-and-angle constructor, the `MIN_PRECISION_IU` endpoint fix,
   `Simplify`) and rounded to micrometres where it becomes points; an arc whose polyline is one segment is its chord, as
   `SHAPE_LINE_CHAIN::Append( SHAPE_ARC )` makes it. The mitered 90 mode also had a bug, fixed: the leg it started with was chosen by `w >= h`, where
   KiCad's is `startDiagonal == ( h >= w )`. `RoutingSettings::corner_mode` is read by the line placer (the head is built in it; a rounded corner is an
   arc of the head, `Line::arcs`, through the pad walk, the shove, the walk and the optimizer, and of the fixed run, which the commit writes as an
   `(arc ..)` track), by the optimizer (`Constraints::corner_mode`: `mergeStep`'s bypasses, which in a rounded mode are arcs; KiCad's optimizer reads
   the router's settings itself) through the placer, the walk (`walkaround::walk_base_line`), the shove and the dragger, and for smart pads (45-degree
   modes only). The studio: the five actions (`pcbnew.InteractiveRouter.SwitchRoundingToNext`, `SwitchRounding45`, `SwitchRoundingArc45`,
   `SwitchRounding90`, `SwitchRoundingArc90`) in `pcbEditSweep.ts`, the submenu's check items from the setting, the hotkeys (Ctrl+/, Ctrl+W, Ctrl+Shift+W,
   Ctrl+Alt+W, Alt+W) in the router tool (they are static `TOOL_ACTION`s of `router_tool.cpp` that the action catalog does not list; a browser keeps
   Ctrl+W), and `corner_mode` on the wire (`routerSettings.ts`, `route_api::settings_over`); a running route takes a new mode at once and redraws the
   head. **Differences:** `m_orthoMode` is not ported (nothing sets it); the arcs of a rounded corner are held on the micrometre grid, so an arc of a few
   micrometres that the grid cannot curve is its chord; the end of a fix in the middle of an arc is not possible (KiCad's `FixRoute` turns fix-all on
   when the head has arcs, and so does `fix`). **Proof:** `tests/direction45_oracle.rs` against KiCad's `direction_45.cpp` compiled over the real
   kimath (`oracle/build_direction.sh`, g++ on Linux): 100 000 random cases of all four modes and both postures agree (the points exactly, an arc's ends
   exactly and its middle to 1 um), 160 baked in; units in `line_placer.rs` (every mode in each routing mode, the fixed run keeps its arc, a rounded head
   shoves what its arc meets and commits clean), `optimizer.rs` (an arc bypass), `router.rs` (the corner committed as an arc track), `route_api.rs`;
   kicad-cli (`gate_vs_kicad::tests::routes_in_every_corner_mode_add_no_violation_to_kicad_clis_drc`, `EDA_SLOW_TESTS=1`): 16 two-leg routes on
   `pic_programmer`, four modes by Shove and Walk around, add no copper violation and their arcs are `(arc ..)` of the file -- but for violations with
   a board text on copper, which this router does not see (`syncTextItem` is not ported: four of them, two in a mitered mode; a gap of its own); the browser check's Track Corner Mode
   scenario.

3. **Virtual vias** (`pns_node.cpp`: `NODE::FixupVirtualVias`, run by `ROUTER::SyncWorld`; `pns_via.h`: `VVIA`; `pns_item.h`: `m_isVirtual`;
   `pns_node.cpp:148, :273`: virtual items never collide; `pns_joint.h`: `NextSegment` with `aAllowLockedSegs`; `pns_dragger.cpp`: `checkVirtualVia`,
   `DRAGGER::Start`; `router_preview_item.cpp` and `pns_router.cpp::CommitRouting`: a virtual item is neither drawn nor committed). **What KiCad means
   by it:** a via the router itself puts in its world at every joint on one layer with no pad and no via where the track width changes or
   where a locked segment ends (not where three segments meet: the review below) (and at that segment's other end), so that the point is a joint with a via in it. It has no hole,
   collides with nothing, is not drawn and not committed. **What the person sees:** `D` on a segment within half its width of such an end drags that
   point as a via (`DM_VIA`): every track meeting there follows, in each mode, and no via appears or is committed -- before, the drag took the corner of
   one line and tore it off the others. And a line ends at such a joint (`NextSegment` stops at a via, unless locked segments are followed). Ported:
   `Via::is_virtual`, `Node::fixup_virtual_vias` (called by `from_ir::build_node`, as `SyncWorld` does), a virtual item in the joints but not in the
   spatial index nor in `Node::iter` / the anchor and unconnected-item searches (nothing finds it but its joint), never an obstacle (`via_vs_line`,
   `items_collide`, `via_colliding_world`, `nearest_obstacle`'s via, `VIA::PushoutForce`, the dragger's own via check), `check_virtual_via` in
   `Dragger::start_checked`, `Dragger::via_virtual` (no via in the commit), `virtual_via` on the drag reply and no via ghost in the studio's drag
   preview. **Differences:** `PNS_HULL_MARGIN` (10 nm) is below the micrometre grid, so the virtual via is as wide as the widest track; the solder mask
   KiCad compares besides the width is not in the model. **Proof:** units in `dragger.rs` (a T dragged as its virtual via in all three modes, three tracks
   follow, no via committed; the middle of a segment still slides; where the width changes and the ends of a locked segment get one, a real via's joint
   none), the joint counts in `router.rs`; kicad-cli (`gate_vs_kicad::tests::dragging_a_junction_as_a_virtual_via_adds_no_violation_to_kicad_clis_drc`,
   `EDA_SLOW_TESTS=1`): six of `pic_programmer`'s 27 junctions dragged 0.3 mm in Shove, Walk around and Highlight collisions add no copper violation
   and no dangling track; the browser check's virtual via scenario. All of `tests/qa_regressions.rs` replays agree as before with the virtual vias in the
   world.

## Stage 10 review -- the three ports held to KiCad's code (branch `task-router-review`, 2026-10-23)

An adversarial review, function by function against KiCad 8303b2ad (not against the doc comments above), of springback, the four corner
modes and virtual vias. Each divergence found has a failing test that the fix makes pass.

**Fixed.**
1. *The route's own tail could not be pushed* (decision 2 of Stage 10). `FixRoute` adds the fixed segments to `m_lastNode` as ordinary
   tracks, so in KiCad a later leg's head that pushes another net's track into the route's tail pushes the tail on. The port locked the
   runs: in `a_later_leg_pushes_the_routes_own_tail_as_kicads_shove_does` (three legs, then a fourth pushing GND's track into the first leg)
   the head stopped 150 um short of the cursor. Now a run is a track of the frame's world with a tag of its own (`springback::run_tag`); what a
   shove made of it is in the frame's `World::tracks` under the tag and the preview and the commit draw the runs from there
   (`World::placed_runs`, `LinePlacer::placed_runs`). Its ends are locked joints: `NODE::LockJoint` / `JOINT::m_locked` and `AssembleLine`'s
   `aStopAtLockedJoints` are ported (`Joint::locked`, `Node::lock_joint`, `AssembleOpts::stop_at_locked_joints`, set by the shove's own
   `assemble`); `SHOVE::Run` locks each head's start and end, and the frames inherit the locks, so every leg's start stays one. Left: the
   vias the clicks place stay locked (KiCad's are ordinary vias, a shove could push one not at the head's start; the commit here puts a via
   where two runs on different layers meet).
2. *Backspace after a leg none of whose shoves succeeded.* That stage's `commit` is the locked frame of the click before (`Run` fell back to
   its parent), and `RewindSpringbackTo` erases it too: the earlier leg leaves the world while it is still fixed, and `CommitPlacement`
   (`RewindToLastLockedNode`) then commits a frame without it. A bug of KiCad's; here the locked frame stays (`Springback::rewind_to`,
   `backspace_after_a_leg_whose_shoves_all_failed_keeps_the_leg_before`).
3. *The 90-degree modes walked round octagons.* `WALKAROUND::processCluster` and `NODE::NearestObstacle` read the corner mode themselves and,
   in `MITERED_90` / `ROUNDED_90`, use the bounding box of every hull (`cachedHull.BBox()`, `simplifyHull`). Ported (`Walker::corner_mode`,
   `QueryOpts::box_hull`, `hull::bbox_hull`), set by the head's walks, the shove (nearest obstacle and walk), the dragger's walk and the
   pair's walk: a walk in a 90-degree mode goes round a pad in axis legs (`a_walk_in_a_90_degree_mode_goes_round_the_box_of_the_hull`).
4. *A virtual via at every T.* `FixupVirtualVias` tests `n_seg >= 3`, but its loop never counts a segment into `n_seg`: KiCad's world has a
   virtual via only where the width changes and at the ends of a locked segment. The port counted them, so `D` at a T of one width took hold
   of a via KiCad does not have (KiCad drags the corner of the one line). Fixed; the units, the kicad-cli proof (now 18 width-change
   junctions on `pic_programmer`, six dragged) and the browser check use width changes. `checkVirtualVia` compares the distance as a double.
5. *Copper board text was no obstacle* (`syncTextItem`, the leftover the corner-mode proof found): ported in `from_ir::add_copper_texts` --
   a solid of no net on the text's layer, not routable (`Solid::routable`, `ITEM::IsRoutable`), the text's bounding hull
   (`TransformShapeToPolygon`: `buildBoundingHull` of the glyphs with the pen and `GetMaxError()`, 5 um), for the imported copper texts and a
   plain `Text` on copper. The kicad-cli proofs no longer leave the "PCB text" violations out (the corner-mode one had four; none now). Not
   here: a footprint's copper texts, tables, a knockout's margin.
6. *The Track Corner Mode actions were not in the action catalog*: the extractor read `TOOL_ACTION X::y( .. )` only; router_tool.cpp's
   `static const TOOL_ACTION ACT_..` (13 of them) are read now. With their hotkeys in the catalog the global listener would have fired them
   anywhere, so the five are registered only while the router tool is active (`AS_CONTEXT`).

**Checked, no divergence.** `reduceSpringback` (pop while unlocked and not colliding, never the last; the head set is the head before the
shove with its via), `pushSpringback`'s area merge, a failed shove leaving `m_currentNode = parent` with the stack already reduced, the
stage's `commit` (the newest frame when the click came) and `RewindSpringbackTo`'s erase from it, `CommitPlacement`; `direction45.rs` (the
oracle of 100 000 cases); `mergeStep`'s bypass in the corner mode (`orig_start` / `orig_end` are computed but unused in KiCad); the smart-pads
conditions; `ToggleCornerMode` / `handlePnSCornerModeChange`'s order and its refresh `Move`; `VVIA`'s single layer and `aDiameter / 2` drill;
virtual items neither drawn nor committed; `JOINT::NextSegment`'s skip of a virtual via when locked segments are followed.

**Decisions of Stage 10, judged.** (1) *Frames as diffs*: equivalent for the line placer -- everything a shove can change there is a track
with an IR id, a via with one, or (now) a run with its tag, and the collision question `reduceSpringback` asks is the same world's; what is
not carried is KiCad's per-item state that outlives a frame (ranks are cleared every `Run`; the root-line history only steers which line is
the root of a shove). (2) *Locked runs*: wrong, fixed (1 above). (3) *Virtual vias in every world*: right -- `ROUTER::SyncWorld` (tool reset,
undo/redo, model change, before a footprint drag; the tuner's proxy too) is the only way KiCad builds its world, and it calls
`FixupVirtualVias`; KiCad does not recompute them after a commit inside one tool session (the port rebuilds its world from the IR, so a
junction made by the last commit has its virtual via at once: a staleness of KiCad's not reproduced). (4) *`SwitchRounding*` and the
catalog*: fixed (6 above).

**Left, with sizes.** ~~Springback in the dragger and in the pair placer~~ **Ported 2026-10-25 (Stage 14).** The mark-obstacles hull
snap (`rhMarkObstacles`, with its 90-degree box) as before. And a pre-existing difference met on
the way: when the head cannot be resolved KiCad keeps the previous head (`routeStep`: `m_head = prevHead`) and `FixRoute` in Shove mode is
refused only by a solid, where the port shows the straight line flagged colliding and refuses the click.

## Stage 11 -- the router tool's via, posture and size actions (branch `task-router-actions`, 2026-10-23)

The seven `static const TOOL_ACTION`s of router_tool.cpp that the action catalog listed as "referenced" are registered as the router tool's
actions (`AS_CONTEXT`: only while the router tool is active, as Stage 10 review's corner modes): `PlaceBlindVia` (Alt+Shift+V), `PlaceMicroVia`
(Ctrl+V), `SelLayerAndPlaceVia` (<), `SelLayerAndPlaceBlindVia` (Alt+<), `SelLayerAndPlaceMicroVia`, `SwitchPosture` (/) and
`CustomTrackViaSize` (Q) -- `web/studio/src/actions/routerViaActions.ts`, their keys in `Canvas.tsx`'s router block (Ctrl+V is Paste's
elsewhere).

- **The via commands** (`ROUTER_TOOL::onViaCommand` -> `handleLayerSwitch( aEvent, true )`, ported in `kicad-port/routerVia.ts`
  `planViaCommand`): a via already armed is taken back (`ToggleViaPlacement`); Select Layer needs a route in progress and opens
  `PCB_ONE_LAYER_SELECTOR` (`SelectOneLayer`: copper layers only, less the current one for a via that is not a through via, PgUp / PgDn
  shown for F.Cu / B.Cu); a picked blind or micro via from F.Cu to B.Cu is a through via; without a picked layer a blind / micro via spans
  the layer pair when the route is on it and goes from the current layer to the pair's top otherwise, and a pair of one layer is refused with
  "Via needs 2 different layers."; the sizes take `AddLayerPair( currentLayer, targetLayer )` and the route goes on on `PairedLayer`, else
  the pair's top (`switchLayerOnViaPlacement`). The via's size is the net class's (`VIA_DIAMETER_CONSTRAINT` / `HOLE_SIZE_CONSTRAINT` of a
  dummy via: a microvia's is the class's `uvia_dia` / `uvia_drill`, 0.3 / 0.1 mm by default) unless a via size of the list or a custom one is
  chosen (`UseNetClassVia()`), which a microvia ignores.
- **The router** (`Router::toggle_via_typed`, `LinePlacer::via_span`): the armed via spans its own layers (`LINE_PLACER::makeVia`: F_Cu ..
  B_Cu for a through via, the pair for any other), which the head's collision check, the via's pushout and the shove see; each via a fix
  places is kept with its layers, size and type (`Session::placed_vias`), so a route with vias of two kinds commits each with its own (before,
  every via of a route took the last armed size). `RouteCommit::via_kinds` and `Cmd::CommitRoute { via_kinds }` (serde default) give the
  board via its type (`PNS_KICAD_IFACE::AddItem`: `SetViaType( via->ViaType() )`) in `DrawingsSection::via_extras`, in the same step (one
  undo); a via a shove or a drag moves keeps its type (`syncVia` reads it), and a removed via's extras go with it. `POST /api/route/via`
  takes `via_type` and `span`.
- **Switch Track Posture** is `performRouting`'s `FlipPosture()` and refreshing Move, the single track's and the pair's (it replaces the raw
  `/` handler of `Canvas.tsx`).
- **Custom Track/Via Size** is `DIALOG_TRACK_VIA_SIZE` ("Track and Via Dimensions", `PCB_VIA::ValidateViaParameters`'s messages word for
  word); OK sets `UseCustomTrackViaSize( true )` and `m_TempOverrideTrackWidth`, and the route in progress takes the width at once
  (`onTrackViaSizeChanged`). The width menu's "Use Custom Values..." is checked while it is in use; a width or via size of the list turns it
  off (`SetTrackWidthIndex` / `SetViaSizeIndex`).

**Not as the brief expected.** KiCad 8303b2ad has no `BDS::m_BlindBuriedViaAllowed` / `m_MicroViasAllowed` (they were removed from KiCad
before this commit) and no "microvias only from an outer layer to its neighbour" rule in `handleLayerSwitch`; nothing refuses those here
either. The box's kicad-cli nightly, newer than 8303b2ad, has a `microvia_crosses_core` check: a microvia through a core dielectric (KiCad's
default stackup has a core under each outer layer) is a DRC violation there. The kicad-cli proof uses a build-up stackup (prepreg outside).

**Differences.** The implicit blind via on the F.Cu / B.Cu pair stays a blind via spanning the board, as in KiCad (the through conversion is
tested before the implicit target is chosen). In the pair tool the via commands do nothing: the pair placer places through vias only (KiCad's
`DIFF_PAIR_PLACER::makeVia` takes the sizes' type and pair too). Place Through Via (V) is still the layer pair's other layer
(`layerToggle`), not KiCad's nearest-ratsnest-anchor choice. The width menu's "Use Custom Values..." opens the dialog without first turning
custom sizes on (KiCad turns them on before the dialog, and leaves them on if it is cancelled). The dialog refuses a track width that is not
a positive length (KiCad has no check).

**Found on the way.** After a click placed the armed via the studio still thought one was armed (`fixInteractiveRoute` kept `placingVia`), so
the next V or via command took back a via that was no longer there instead of arming one; KiCad's `IsPlacingVia()` is false after `FixRoute`,
and so is the studio's now (`components/canvas/routing.ts`). `e2e/run-check.mjs` prints the failed checks' notes (it printed only the
scenario names).

Proof: `routerVia.test.ts` (12: the flags, the pair, the type, the refusals, the picker's layers, the sizes, the dialog's messages);
`route_api` `a_blind_via_and_a_microvia_are_committed_with_their_type_layers_and_size_as_one_undo_step` and the request words; `router`
`a_shoved_blind_via_keeps_its_type`; ops `commit_route_gives_its_vias_their_type_and_drops_the_extras_of_removed_vias`; the slow
`gate_vs_kicad::a_route_with_a_blind_via_and_one_with_a_microvia_are_clean_for_kicad_clis_drc_on_four_layers` (`(via blind ..)` F.Cu ..
In1.Cu and `(via micro ..)` In2.Cu .. B.Cu in the file, kicad-cli finds nothing but the stubs' dangling ends); and the "via actions" scenario
of `web/studio/e2e/router-leftovers.check.js`.

## Stage 12 -- parity batch E: the router findings fixed (branch `task-fix-e-router`, 2026-10-24)

The fixes of `docs/parity/findings/p5-router.md` (status per finding there). Each was re-read in KiCad 8303b2ad, a failing test
written, then fixed.

- **KiCad's recorded sessions replay to the nanometre.** `SHAPE_LINE_CHAIN::Simplify2` is ported (`Line::simplify2`: `LineDistance <= 1`
  to the infinite line, the 3-point rule) and called where KiCad calls it (`processCluster`, the shove's hull walk, the shoved line,
  `onCollidingSolid`, `pushOrShoveVia`, `preShoveCleanup`, `mergeFull`, `mergeStep`, smart pads, the pair bypasses); the micrometre-era
  `HULL_ROUNDING_GUARD` is gone and `ShoveObstacleLine`'s arc padding piles up; the shove's line and via collision tests use
  `collideSimple`'s `clearance - 1` ("the hulls are built to exactly the clearance distance"). `tests/qa_regressions.rs` asserts KiCad's
  exact compare (`PNS_LOG_FILE::COMMIT_STATE::Compare`): backspace1 2/2, simple-shove-1 28/28, issue22749 11/11,
  walk_drag_seg_against_board_edge 3/3, simple-drag-shove-singlelayer 134/134, walk-with-teardrops 19/19 (new), and the via-drag crash
  session clean. The corpus is `$KICAD_QA_DATA` or `$KICAD_SRC_DIR/qa/data`; without it `EDA_SLOW_TESTS=1` fails.
- **The shove:** `ShoveObstacleLine`'s `LayersOverlap` test (a head's via shoves an other-layer track round the via alone), lines on
  different layers never collide, `pushOrShoveVia` stops at a locked joint, `VIA::PushoutForce` moves the whole MTV (KiCad's "limit to
  25%" is a discarded `Resize`), `SelfIntersecting` sees a fold-back.
- **The line placer:** a via that ends a route is committed (`FixRoute` adds it to `m_lastNode`; `LinePlacer::fixed_vias`, Backspace takes
  it with its run and restores the run's layer); Walk around pushes the via before walking the head to it (`rhWalkBase`'s two rounds);
  a failed `routeHead` keeps the last good head (`routeStep`); Shove's failed pad walk is a failure (`rhShoveOnly`); the posture starts at
  `InitialDirection()`; one `/` keeps Smart Pads off for its leg only; `removeLoops` takes the line the route is part of.
- **The router:** Esc and a refused finish commit what was fixed (`CommitRouting()` after the loop; `Router::commit_fixed`); the start
  check (`isStartingPointRoutable`: "The routing start point violates DRC.") and a start in empty space on the orphaned net;
  AttemptFinish routes to the nearest unconnected anchor (`ROUTER::Finish`, `Router::attempt_finish`); `simplifyNewLine` on the commit
  (`Router::simplify_new_lines`); a drag keeps the mode it started in.
- **The dragger:** a via dragged in Highlight collisions keeps its tracks at 45 degrees (`dragViaMarkObstacles`' `DragCorner`); a refused
  click leaves the drag going; a free-angle grab next to an arc takes the segment's start.
- **Smart Pads** break out of a rounded-rectangle pad (`customBreakouts` of its outline); `mergeObtuse` checks only its bypass.
- **Pairs:** `ImportSizes`' gap and via gap (the pair gap, at least the board minimum clearance; not the P-N clearance, not the class via
  gap), KiCad's Default class sizes as the fallback (0.2 / 0.25 / 0.25 mm), "Cannot start a differential pair in the middle of nowhere.".
- **The tuner:** the skew box's range is the skew's; a track of another pattern does not start one.

After the review of 2026-10-24: no net is one net (`same_net`: `None` and `""` are KiCad's `OrphanedItem()`), a route starts on a no-net
item and not on a non-plated hole; Backspace takes a leg that ended in a via back with the via armed again (`UnfixRoute`'s
`m_placingVia`); Cancel Current Item refreshes the board after its commit. **Differs:** the outline, copper text and NPTH pads carry
`not_a_net()`, so a no-net route keeps its clearance from them. Lows the review found, left:

- `Router::commit_fixed` cuts the track the route ended in the middle of (`session_splits( true )`) also in Shove mode, where the
  commit is the newest locked frame's (`RewindToLastLockedNode`), which may not have reached that end.
- `F` (AttemptFinish) with a via armed fixes the head with its via and goes on on the other layer instead of finishing on the anchor.
- `F` in a diff pair still finishes at the cursor (no `ROUTER::Finish` for the pair placer).
- A via placed with no run before it (V at the start pad) continues the route where KiCad's `FixRoute` ends it (`m_idle = true`), and
  Backspace cannot take it back by itself.
- `simplifyNewLine`'s `MERGE_COLINEAR` and its first stage (removing a dangling collinear segment the new one subsumes,
  `processJoint`) are not ported; `Simplify()` does the merge.
- `Node::colliding_with` (the node's queries) lacks `collideSimple`'s `clearance - 1`; only the shove's line and via tests have it.

Left, with the reason, in the findings file: F2-11 (KiCad's grab radius is the grid size, which the router is not told), F4-2 (arcs in
the pair router and tuners), and the lows F1-7, F1-8, F1-9, F1-10 (in part), F2-15 (the `line_walk.rs` tolerances, FANOUT_CLEANUP, the
mark-obstacles hull snap, the cluster timeout), F3-10 (by decision), F3-13, F4-9.

## Stage 13 -- the length type (branch `task-nm2-pns`, 2026-10-25)

Wave 2 of the length type (`docs/NANOMETRES.md` "The length type"): the ~3500 wave-1 shims of this crate are gone; lengths are `Nm` in
fields, locals, parameters and returns.

**The decision: the router's geometry carries `Nm`, with no conversion where the IR enters (`from_ir`) or leaves (the commits).** It
was already the IR's `Point` (wave 1 left the router computing on IR points through shims), and KiCad's router computes in the same
units it stores (`VECTOR2I` of `int` nanometres), so a second, raw `i64` geometry would only move the conversions inside the crate --
to every call into `eda_drc::kimath` (the router's collision shapes) and every point that goes back to the IR. What keeps KiCad's
integer arithmetic exact is that nothing about the bits changes: `Nm` is `repr(transparent)` over `i64`, every operator is the `i64`
operator, and the places where KiCad leaves `int` for something wider or for floating point say so in the code:

- products of coordinates (`VECTOR2I::Cross` / `Dot`, `SEG::SquaredDistance`, `ecoord`) are `i128`: `a.area(b)`, or `v.get() as i128`
  inside a kernel that works on the wide integers (`rescale`, `SEG::LineProject`, `mutualDistanceSquared`), and come back as a length
  with `Nm::new` of the kernel's result;
- floating point (`VECTOR2D`, `EuclideanNorm`, `RotatePoint` of doubles, the hulls' trigonometry) goes through `as_f64()` and comes
  back at KiCad's rounding: `kigeom::ki_round_nm` (that module's own `KiROUND`, `(v +- 0.5)` truncated, which is not `f64::round`
  just below a half), `Nm::kiround` (`f64::round`, where the code had it), `Nm::new( f as i64 )` where the C++ truncates;
- `MINOPTMAX` (`meander::MinOptMax`) stays `i64`: it holds a length or a delay (attoseconds) by the tuning's domain, as the model's
  `TargetRange` does; a length goes in and out of it with `get()` / `Nm::new()`. Delays, areas, counts and angles are not `Nm`.

The studio (`crates/cli`) read some of the crate's public items as counts in wave 2; wave 3 typed them with their callers: one typed
`ShapeLineChain::length` / `point_on_edge`, `ShapeArc::new`, `TuningPath::length` and `tuning_path::stackup_distance` (the `_nm` twins are
gone), and `MeanderSettings::{min_amplitude, max_amplitude, spacing}`, `PairAssembly::gap` (`-1 nm`, KiCad's `-1`: not parallel enough),
`SkewSetup::{own_length, coupled_length}`, `MeanderPlacer::{start_skew's coupled_length, tuning_length_result, target}` and
`DpMeanderPlacer::{tuning_length_result, net_lengths}` are `Nm`. `MinOptMax` and `meander::LENGTH_UNCONSTRAINED` stay `i64` (above);
`target()` reads the `MINOPTMAX` at its edge.

No behaviour change: every test passes with its values unchanged, KiCad's QA replays (`tests/qa_regressions.rs`) stay exact, and
`tools/same_design.sh` writes the same files as main. One micrometre leftover in a test harness: `tests/mouse_trail_oracle.rs` divided
KiCad's nanometre cases by 1000 before giving them to the port (and multiplied the answers back); it now runs them at KiCad's scale, and
the baked answers of `pns_mouse_trail_tracer.cpp` all agree.

## Stage 14 -- springback in the dragger and the pair placer (branch `ds/handoff-1025`, 2026-10-25)

Stage 10 review's "Left" list, first item: the two placers that kept no shove branch between moves, in the sizes that list gave them --
the dragger's one `SHOVE` per gesture (KiCad makes it in `Start`, for `RM_Shove` and not free angle) and the pair's frame per move of the
leg, with a fresh `SHOVE` at every fix (`DIFF_PAIR_PLACER::FixRoute`).

**The dragger** (`src/dragger.rs`). The stack is a `DragShove` beside the dragger: a [`Springback`] and, per frame, the place a via drag left
its via in (`SPRINGBACK_TAG::m_draggedVias`; a [`crate::springback::World`] is a diff and does not carry it, and only the dragger has vias
of its own to carry). A line drag pops the frames the candidate line no longer collides with (`reduceSpringback` with the line as its head),
shoves the stretched line from the reduced frame's world with that frame's area (`totalAffectedArea`, now a parameter of
`shove_dragged_line`), and pushes the world the shove left. A position the shove cannot resolve keeps the pops -- the next position starts
from the frame left on top, as in KiCad -- and the dragger falls back to marking obstacles; a via the shove cannot move goes to
`dragViaWalkaround`, as `dragViaShove` does. A via drag's head is the via at the top frame's recorded place, so a frame popped puts the via
back to the frame below's place before the shove starts, and the frame it pushes records where the shove left it. The dragged via may itself
be virtual or unnamed, which a `World` cannot carry: the frame node is made first and the via put back at the frame's place in it. The commit
(`commit_of`) is built from the preview at the last valid point, whose `displaced_lines` / `displaced_vias` are the newest frame's against
the session's world, so everything the gesture pushed is committed, not only what the last position did. `preview()` reduces a copy of the
stack: `m_nodeStack` lives as long as the dragger, and a preview does not move the gesture on.

**The pair placer** (`src/dp_placer.rs`). `rhShoveOnly` starts with `m_currentNode = m_shove->CurrentNode()` -- `frame_node()`, the newest
frame's world against the session's with the fixed legs in it -- and fits, walks (`tryWalkDp`) and shoves in it. The reduce takes both lines'
heads at once (`Springback::reduce_all`): `CheckColliding( aHeadSet )` asks about the one head set, so a frame one line still presses on
stays for the other too. The shove starts from the reduced frame's world (`shove_heads_after`, the frame's area in `totalAffectedArea`) and
its world is pushed as the newest frame; a failed shove leaves the reduced frame as `m_currentNode` and the stack as the reduce left it.
`FixRoute` commits that world with the pair in it, and makes a fresh `SHOVE` over the committed world (`m_shove =
std::make_unique<SHOVE>( m_world, .. )` before `CommitPlacement`): the stack restarts empty at every fix, so no frame is ever locked, and
`undo_last_segment` (the port's Backspace; KiCad's pair placer has no `UnfixRoute`) clears it with the leg. `Move`'s
`m_lastNode = m_currentNode->Branch()` is the port's `last_node`: the frame the move started from, then the world the shove left, or the
reduced frame when it failed, so a fix under `AllowDRCViolations` commits the pushed world as KiCad's does.

**Proof.** Failing first, then passing: the dragger's `a_pushed_via_stays_pushed_while_the_drag_still_presses_on_the_frame` (a segment
dragged over a GND via, the cursor taken past a pad and drawn back: the via stays where the first frame pushed it, in the preview and in the
commit, and the oldest frame is never popped) and the pair's `a_via_the_pair_pushed_stays_pushed_until_the_leg_is_fixed` (the pair pushes
a GND via off the N line, the cursor drawn back to where neither line presses on it: the via stays pushed and the fix commits it there);
`springback.rs`'s `a_frame_stays_while_any_of_the_heads_presses_on_it` for the two-head reduce. All of `cargo test -p eda-pns` passes
(301 unit tests and every integration suite), the QA replays of `tests/qa_regressions.rs` exact, `tests/dp_boards.rs` on KiCad's pair board,
and the slow tier (`EDA_SLOW_TESTS=1`) green.

**Review fixes (branch `task-ds-springback-fix`, 2026-10-26).**
- *The walkaround fallback of a via drag keeps the frame.* `dragShove` (`DM_VIA`) calls `dragViaWalkaround( m_draggedVia, m_lastNode, aP )`
  when the shove fails, and edits that branch of the reduced frame (`m_lastNode->Remove( origLine ); m_lastNode->Add( draggedLine )`), so
  the commit keeps what the earlier positions pushed beside the walked fan-out. The port's fallback preview had dropped the frame: no
  `displaced_lines` / `displaced_vias` (the pushes vanished from the preview and the commit), and the fan-out legs, assembled from the
  frame's node, carried item ids the session's world does not have, so `commit_of` could not find their IR tracks and committed the walked
  legs beside the original ones. Now `drag_via_walkaround` takes the frame's [`crate::springback::World`]: the preview carries its
  displacement without the via's own legs (the fan-out replaces them) and without the dragged via, and `DragPreview::fanout_tracks` names
  each leg's IR tracks as the frame's items have them, which `commit_of` uses. Failing first, then passing:
  `a_via_shove_that_fails_walks_around_in_the_frame_and_keeps_its_pushes` (the via pushes a GND track aside, then is dragged onto a locked
  GND track: the shove fails, the via is pushed off and its track walked; the preview and the commit keep the push, the original leg is
  removed and only the walked one added).
- *A failed position shows the reduced frame.* `DRAGGER::Drag`'s "Restore last solution" is `NODE* parent = m_lastNode->GetParent()->Branch();
  .. m_lastNode->Add( m_lastDragSolution )`: in Shove mode the parent is the frame this failed position reduced the stack to, so a frame it
  popped has sprung back in what is shown. The port showed `last_solution` whole, the frame from before this move's pops. Now
  (`restore_onto_frame`) a line drag shows the last good line with the reduced frame's displacement; a via drag (`dragShove` sets
  `m_lastDragSolution` only for lines) shows the via and its legs where that frame has them. ~~Walk around mode is unchanged: its parent is the
  session's world.~~ **Corrected (round-2 R-7):** in Walk around mode the parent is the session's world and `m_lastDragSolution` is still the line
  as `startDragSegment` assembled it (`dragWalkaround` never sets it), so the track is shown where it started, not at the last good position (see
  the round-2 section). Failing first, then passing: `a_failed_via_position_shows_the_frame_it_reduced_the_stack_to` (two frames, then a position
  neither the shove nor the pushout can place: the newest frame is popped and the preview shows the via, its leg and the push of the frame
  below). The line-drag half has no test of its own: a line head that pops a frame touches nothing of it, so its shove does not fail.

## Stage 15 -- round-2 router findings (branch `task-r2-router`, 2026-10-27)

The findings of `findings/round2/r2-route-rules.md` (R-1, R-2, R-3, R-6, R-7, R-8; R-4 and R-5 are Board Setup's, another lane's). Each was
re-read against KiCad 8303b2ad, and each fix has a test that failed first.

- **R-1, the router's clearance is the DRC engine's.** `PNS_PCBNEW_RULE_RESOLVER::Clearance` asks `QueryConstraint` -> `DRC_ENGINE::EvalRules`
  for every layer two items share and takes the largest of `CT_HOLE_TO_HOLE`, `CT_HOLE_CLEARANCE`, `CT_CLEARANCE`, `CT_EDGE_CLEARANCE` and
  `CT_PHYSICAL_CLEARANCE`. `src/rule_resolver.rs` ports it over `eda_drc::constraints::RuleEngine::eval_rules` and the board items of
  `eda_drc::rule_board::RuleBoard` (the same engine the Clearance Resolution dialog and the via stitcher use), so a pad's or footprint's local
  clearance, the net-tie rule, the custom rules of the `.kicad_dru` and the board minimum count. `from_ir::build_node` gives the world its
  resolver (`NODE::SetRuleResolver`); every clearance the router asks -- collisions, hulls, the walkaround, the shove's hulls and MTVs, the pair
  placer, the meander spacing (`MEANDER_PLACER_BASE::Clearance`) -- goes through `Node::clearance_between` and is cached per pair of items
  (`m_clearanceCache`). A world built item by item (`Node::new`, the unit tests) has no resolver and keeps the net-class answer. The search margin
  is the resolver's worst rule or local clearance (`SetMaxClearance`).
  - *Holes.* A pad's drilled hole and a via's are tested as KiCad's `HOLE_T` items are (`collideSimple`): the item's hole against the probe's
    copper of another net, the probe via's hole (`QueryOpts::head_hole`, `Node::via_colliding`) against copper of another net and against holes
    of any net (hole to hole), a geometrically identical via of the same net excepted. **Differs:** a hole is part of its pad or via, not an
    item of its own, so an item is one obstacle and the hull it is walked round is the larger of its copper's and its hole's
    (`Node::obstacle_hull`); a shoved via's MTV is still measured from its copper.
  - *Net ties.* `collideSimple`'s `checkNetTie`: a collision with a pad of a net-tie footprint is allowed where it is on a pad of that
    footprint's groups of the probe's own net (`IsNetTieExclusion`, at the position on the pad, as `shapeH->Collide( shapeI, .., &pos )` gives it).
    The finding's "clearance 0" holds only there: the router keeps the clearance from the other pad elsewhere, as KiCad does.
  - *Differs:* an existing track or via is asked about as `getBoardItem`'s dummy of its net and layer (KiCad asks the `PCB_TRACK`, with its own
    width); `evaluateLineSegments` is not ported (only geometry-dependent rules would see it).
  - *Cost.* The answers are cached per pair as KiCad's `m_clearanceCache` is, a net known by its handle (`from_ir` gives all the items of a net
    one `Rc`, as a board has one `NETINFO_ITEM`) and a solid by the number of its board item (`Solid::rule`). On a board whose clearance rules
    are its board setup's and its net classes' plain ones (`RuleResolver::is_plain`: no `.kicad_dru` rule of a clearance type, no "(diff pair)"
    clearance), a pair of items with no rules of their own (tracks, vias, their holes, pads with no local clearance, the edge) is answered without
    the engine, from the same numbers `EvalRules` takes there; `the_plain_answer_is_the_engines_on_kicads_boards` holds this to the engine on
    about 100 000 pairs of ten QA boards. A hole whose copper reaches further than the worst hole clearance can is not tested
    (`RuleResolver::hole_reach`). The search margin counts the barcodes' implicit 1 mm physical clearance once the world has a barcode
    (Stage 18; KiCad's `QueryWorstConstraint` counts it always). Measured on `simple-shove-1` replayed in Shove mode (debug build, CPU time, the best of
    nine): 0.65 s with the rules, 0.63 s with the net classes alone.
  - Tests: `tests/rules_kicad.rs` -- a pad's local clearance, a footprint's, a custom rule, a net tie, a mounting hole's hole clearance, hole to
    hole between two vias of one net. Each routes in Walk around mode through `Router`, checks the copper, and under `EDA_SLOW_TESTS=1` has
    kicad-cli's DRC judge the board with the route on it (no violation), and the route the net class alone gives (a violation: kicad-cli sees
    the rule). All eight failed with the resolver taken out. A ninth, `the_plain_answer_is_the_engines_on_kicads_boards`, holds the resolver's
    shortcut to the engine (below).
- **R-2, keepout rule areas are obstacles.** `syncZone`: each triangle of a keepout area's outline is a `SOLID` of no net, not routable
  (`from_ir::add_keepouts`, `Solid::KEEPOUT_SOURCE`); `IsKeepout`'s `checkKeepout` keeps out tracks or vias by the area's flags (a pad has no
  board item to the router, so never), as an exact boundary (clearance 0); its hull is the class clearance (`GetClearance` of the zone item); in
  `AssembleCluster` it touches only what it keeps out. The triangles are `POLYGON_TRIANGULATION::TesselatePolygon`'s ear cutting
  (`src/triangulation.rs`). **Differs:** `subdividePolygon` / `splitPolygon` are not ported (an outline the ear cutting cannot finish is
  left out with KiCad's warning since Stage 16, R3-9; the holes are cut out since R3-5). Tests: `a_keepout_rule_area_blocks_tracks` (the route goes round; kicad-cli: no
  `items_not_allowed`, and the straight route is one), `a_vias_only_keepout_lets_tracks_through_but_not_vias`.
- **R-3, a pair's legs are simplified.** `DIFF_PAIR_PLACER::FixRoute`'s `topo.SimplifyLine( &lineP ); topo.SimplifyLine( &lineN )`:
  `Router::simplify_pair_legs` runs the single-track `simplifyNewLine` merge (findings p5 F2-6) on the pair's legs at the commit (finish, Esc,
  Route From Other End), not on the lines its shove moved. A leg that goes straight on from the leg before it, or from the track it starts on,
  is one segment with it; a track it continues is taken in (`a_pair_that_goes_straight_on_across_a_fix_is_one_segment_per_line`, and the QA
  board `a_pair_continues_from_a_pair_of_tracks_..`, which now takes in the J1-side tracks and the stubs it meets). KiCad simplifies at each fix
  with the legs before it in the node; once over all the legs gives the same segments.
- **R-6, the pair's Shove move that cannot be routed.** `rhShoveOnly` sets `m_currentNode = m_shove->CurrentNode()` first and `Move` branches
  `m_lastNode` from it whatever `route()` returned: the preview shows the newest frame's pushes. KiCad's `tryWalkDp` always returns true (as here),
  so the early return is `routeHead`'s (`propagateDpHeadForces` out of steps). Test: `a_via_the_pair_pushed_stays_pushed_until_the_leg_is_fixed`.
- **R-7, the dragger's restore.** `Drag`'s restore adds `m_lastDragSolution` to a branch of the last node's parent. `dragWalkaround` never sets
  it (it is the line `startDragSegment` assembled), so in Walk around mode a position that cannot be walked shows the track where it started;
  `dragShove` sets it to `draggedPostShove`, split at the anchor but not optimized (`optimizeAndUpdateDraggedLine` optimizes a copy). The
  Stage 14 sentence "Walk around mode is unchanged" was wrong and is corrected there. Tests: `walkaround_restore_shows_the_original_line`,
  `shove_restore_shows_the_line_before_the_optimizer`.
- **R-8** is the connectivity crate's (`crates/connectivity/src/dangling.rs`: the integer accuracy, the integer `getMinDist`, true arcs).

## Stage 16 -- round-3 router findings (branch `task-r3-router`, 2026-10-27)

The findings of `findings/round3/r3-route-rules.md` (R3-1 .. R3-11), each re-read against `pns_kicad_iface.cpp` (`SyncWorld`, `syncZone`,
`syncGraphicalItem`, `syncDimension`, `syncBarcode`, `syncTextItem`, `ImportSizes`, `IsFlashedOnLayer`, `getBoardItem`), `pns_router.cpp`
(`isStartingPointRoutable`), `pns_item.cpp` (`collideSimple`), `router_tool.cpp` (`handleLayerSwitch`) and `footprint.cpp`
(`MapPadNumbersToNetTieGroups`) at 8303b2ad. Each fix has a test that failed without it; the highs and R3-4 have kicad-cli proofs under
`EDA_SLOW_TESTS=1` in `tests/rules_kicad.rs`.

- **R3-1, a keepout on several copper layers.** `syncZone` adds the triangles on each copper layer of `GetLayerSet()`; `add_keepouts` now does
  (`Zone::copper_layers`), so an area on F.Cu and B.Cu or on `*.Cu` keeps every one of them. Tests: `a_keepout_is_an_obstacle_on_every_copper_layer_of_its_set`,
  `a_keepout_on_both_copper_layers_blocks_a_route_on_the_back` (two through-hole pads, the route on B.Cu goes round; kicad-cli: no `items_not_allowed`,
  and the straight B.Cu route is one).
- **R3-5, a keepout's holes.** `CacheTriangulation` triangulates the outline with its holes; `keepout_triangles` fractures the outline and its holes
  (`ShapePolySet::fracture`) and cuts the one chain into ears. **Differs:** KiCad first tries `TesselatePolygon( const POLYGON& )`'s own
  `eliminateHoles`; the triangles differ, the area they cover does not. Tests: `a_keepouts_hole_is_left_open` (84 of 100 mm2 covered),
  `a_route_may_cross_the_hole_of_a_keepout` (straight across; kicad-cli: nothing, and a track across the area's body is flagged).
- **R3-9, a malformed keepout.** An outline the ear cutting cannot finish is left out of the world, as `!IsTriangulationUpToDate()` does, with
  KiCad's words ("Rule area 'ANT' on F.Cu and B.Cu is malformed." and the dialog's detail, `RuleResolver::warnings`), which `/api/route/start`
  returns and the studio shows as a toast (KiCad: a `KIDIALOG`). Test: `a_malformed_keepout_is_left_out_with_a_warning`.
- **R3-2, copper graphics.** `SyncWorld` syncs `m_board->Drawings()` and each footprint's `GraphicalItems()`; `from_ir::add_copper_graphics` now
  adds, on their copper layer: a board shape (`syncGraphicalItem`: one solid per effective shape, of the shape's net, routable, anchored at
  `GetConnectionPoints()` -- `Solid::anchor_points`, `Solid::GRAPHIC_SOURCE`), a footprint's copper shape (of its footprint, so a net tie's bridge is
  in the tie: `IsInNetTie`), a text box's border (`PCB_TEXTBOX` as a `PCB_SHAPE`), a dimension's lines and text (`syncDimension`), a barcode's turned
  box (`syncBarcode`), a table's box grown by half its widest rule (`syncTextItem` of a `PCB_TABLE`), and a footprint's copper texts (`syncTextItem`,
  GAPS 7's old "Left"). The last four are `Solid::OBSTACLE_SOURCE` / texts: not routable, of no net. **Differs:** a dimension's and a barcode's text
  is the box of its glyphs; a barcode's printed text is left out; a rounded rectangle on copper is its square corners. Tests:
  `copper_graphics_are_solids_on_their_layer`, `a_footprints_copper_shape_is_of_its_footprint`, `a_copper_polygon_is_an_obstacle` (the route keeps
  the clearance), `a_copper_shape_with_a_net_is_an_obstacle_to_other_nets_and_an_end_for_its_own` (a GND route ends on the GND polygon's centre;
  kicad-cli: nothing), `kicads_boards_with_copper_graphics_have_them_in_the_router_world` (the 20 boards of `qa/data/pcbnew` with copper graphics).
  *kicad-cli 10.99 reports no track against a copper graphic on boards this small -- not the straight track through the polygon, nor two crossing
  tracks of two nets -- while it does on KiCad's `simple.kicad_pcb`; so for the no-net polygon the router-side gap is the proof, not kicad-cli.*
- **R3-6, Margin.** `syncGraphicalItem`'s `Edge_Cuts || Margin` and `isEdge`'s `IsOnLayer( Margin )`: a board graphic on Margin is a board edge on
  every copper layer (`from_ir::add_margin`, `margin:<n>`), kept at the copper-to-edge clearance from its stroke's edge (it keeps its width;
  `SetWidth( 0 )` is Edge.Cuts'). **Differs:** a footprint's Margin graphics are not read. Tests: `a_margin_line_is_an_edge`,
  `a_margin_line_holds_a_route_off_as_an_edge` (kicad-cli: no `copper_edge_clearance`; the straight route is one).
- **R3-3, the start width.** `ImportSizes` with `UseNetClassTrack()`: `settings::import_sizes` asks `CT_WIDTH` of a dummy track of the start item's
  net on the start layer (`RuleResolver::query_constraint`, `EvalRules`), the larger of it and the board's minimum width, else the current (Default)
  width; `Router::import_sizes` picks the start item as `start` does. `/api/route/start` without a width starts there and replies with `width`; the
  studio sends no width while the width list is on its first entry, the net class's (`chosenTrackWidth`, `UseNetClassTrack()`: Canvas.tsx's
  route tool, Route Selected / Autoroute). Change Track Width and Convert to Tracks keep `GetCurrentTrackWidth()`, as KiCad's do (not
  `ImportSizes`). Tests: `a_route_takes_the_start_nets_class_width` (a Power class of 0.5 mm; a `.kicad_dru` `track_width (opt 0.6mm)` wins;
  kicad-cli under a 0.5 mm minimum: nothing for the route, `track_width` for the Default width), `route_api`'s
  `a_start_without_a_width_takes_the_nets_class_width`, `routerVia.test.ts`.
- **R3-4, the pair's and the via's sizes.** `import_sizes` also asks `CT_WIDTH` of the track and its coupled one, `CT_DIFF_PAIR_GAP` (gap and via
  gap), `CT_VIA_DIAMETER` / `CT_VIA_HOLE` of a dummy via (over the board's minimums) and `CT_HOLE_TO_HOLE` of two dummy vias on no layer and of the
  via and its coupled one; `DpSizes::import` takes the pair's from it, and `Router::start_diff_pair` checks the gap against the board's minimum
  clearance first (`isStartingPointRoutable`). A via armed without a size (`/api/route/via`) is `handleLayerSwitch`'s: `VIA_DIAMETER_CONSTRAINT` /
  `HOLE_SIZE_CONSTRAINT` of a dummy via of the route's net, type and span (`RuleResolver::via_size`), the studio sending none on the net class's
  entry or for a microvia (`chosenViaSize`). Tests: `a_custom_rule_sets_the_pairs_gap_and_width_and_the_via`,
  `a_pair_routed_at_the_rules_sizes_passes_kicad_clis_checks` (0.3 mm at 0.35 mm from a rule; kicad-cli: no `track_width`, `diff_pair_gap` or
  `clearance`, and the class's 0.2 mm pair is a `track_width`; kicad-cli 10.99 reported no `diff_pair_gap` on that board even for a pair under
  the rule's minimum, so the gap is held by the test, not by it).
- **R3-7, unflashed layers.** `collideSimple`'s `!IsFlashedOnLayer( this, aHead->Layers() )` -> `clearance = -1`: a board pad or via that removes
  its unconnected layers (`eda_connectivity::flash::flashed_layers`, the layers Remove Unused Pads keeps) has no copper on the others to the router
  (`RuleResolver::is_flashed_on`), its hole still collides at `max( HOLE_CLEARANCE, CLEARANCE )`, and the hull it is walked round there is its
  hole's (`VIA::Hull`'s `HullForHole`; a pad's too, since its copper is no obstacle there). Test: `an_unflashed_via_is_its_hole_on_that_layer`.
- **R3-8, the start's refusals.** `isStartingPointRoutable` walks `QueryHoverItems( aWhere )`: `Router::start_refusal` takes the items under the
  start in the world's order, passes over the board edge, stops at a routable one, and otherwise names the last: "Cannot start routing from a
  non-plated hole.", "Rule area '%s' disallows tracks." / "Rule area disallows tracks." (any keepout, one of vias only too), "Cannot start
  routing from a text item.". It runs in every mode before the dot of track. Test: `a_start_inside_a_rule_area_or_on_a_text_says_so`.
- **R3-10, the dummy via's layer.** `getBoardItem`'s `m_dummyVias[aIdx].SetLayer( aBoardLayer )`: the dummy via's `A.Layer` is the layer asked
  about. Test: `a_rule_on_a_vias_layer_reads_the_layer_asked_about`.
- **R3-11, net-tie groups.** `MapPadNumbersToNetTieGroups`' backslash escape (`net_tie_group_pads`), and `IsNetTieExclusion`'s net codes: a
  head of no net matches a pad of no net. Tests: `net_tie_groups_read_an_escaped_comma`, `a_no_net_head_is_excused_at_a_no_net_pad_of_a_net_tie`.

## Stage 17 -- round-4 router findings (branch `task-r4-router`, 2026-10-28)

The findings of `findings/round4/r4-route-rules.md` (R4-1, R4-2, R4-4 .. R4-7; R4-3 is the Board Setup lane's, R4-8 is the studio's width
list), each re-read against `pns_kicad_iface.cpp` (`ImportSizes`, `inheritTrackWidth`, `IsFlashedOnLayer`, `syncGraphicalItem`), `pns_item.cpp`
(`collideSimple`), `pns_router.cpp` (`isStartingPointRoutable`), `router_tool.cpp` (`prepareInteractive`, `TRACK_WIDTH_MENU`), `pns_tool_base.cpp`
(`pickSingleItem`), `project_local_settings.cpp` and `shape_poly_set.cpp` (`inflate2`, `inflateLine2`) at 8303b2ad. Each fix has a test that
failed without it; R4-1 and R4-2 have kicad-cli proofs under `EDA_SLOW_TESTS=1` in `tests/rules_kicad.rs`.

- **R4-1, the joined track's width.** `ImportSizes` asks `inheritTrackWidth` first when `m_UseConnectedTrackWidth && !m_TempOverrideTrackWidth`
  (the project's `board.auto_track_width`, on by default; `prepareInteractive` clears the override before each start, so a chosen width gives way
  too). `settings::inherit_track_width` is its port (a track's own width; for a pad or via, of the tracks joined to it on the start layer the one
  whose far end is nearest the cursor, else the narrowest on the layer, else of all), `settings::import_sizes( .., use_connected_track_width )`
  takes it for the route ("existing track") and the pair. `Router::use_connected_track_width` (default on), `Router::connected_track_width`;
  `/api/route/start` and `/api/route/dp_start` take `auto_track_width` (default on). The studio's "Use Starting Track Width" (`bcx.autoTrackWidth`)
  is on by default and is sent with every start; its own track-end-only `connectedTrackWidth` / `trackUnder` are gone. Test:
  `a_route_started_on_a_pad_takes_the_width_of_its_stub` (a pad's 0.15 mm stub in a 0.25 mm class, a via's stub, two stubs by the cursor; the
  route at 0.15 mm goes straight through a 0.6 mm channel between two GND pads; kicad-cli: no `clearance`, `shorting_items` or `tracks_crossing`,
  and the straight track at the class's 0.25 mm is a `clearance`).
- **R4-2, the head's side of `IsFlashedOnLayer`.** `collideSimple` also asks `IsFlashedOnLayer( aHead, Layers() )`: a board via dragged or shoved
  (`QueryOpts::head_via`, its `source_via`), or a world item as the head (the shove's `items_collide`), that removes its unconnected layers has no
  copper on the world item's layers there -- against the item's copper and against its hole (`holeI->collideSimple( aHead )` asks the same) --
  while its own hole still collides (`RuleResolver::is_via_flashed_on`). Two things the fix needs, also KiCad's: a via the router moves comes
  back under its own id, so `Cmd::CommitRoute` keeps its extras (`remove_unused_layers`; KiCad modifies the `PCB_VIA`), and `Router::drag_start`
  takes hold of a via under the cursor before a track joined to it (`pickSingleItem`'s `prioritized[0]`). Test:
  `an_unflashed_head_via_collides_by_its_hole_only` (a four-layer board, an In1.Cu GND track 0.45 mm from the via's path: the hole's 0.4 mm is
  kept, a router-made via's ring is not; the drag gets there, keeps `v1`; kicad-cli: no `clearance` / `hole_clearance`, and the same via flashed is
  a `clearance`).
- **R4-4, a pair started on arcs.** `isStartingPointRoutable`'s start-gap check is for a start item of `SEGMENT_T | ARC_T`; `DiffPairPlacer::start`
  checks it so, with the width the start item inherits (`inheritTrackWidth( aStartItem, .. )`, N's when the pair starts on N), now taken inside
  `DiffPairPlacer::start` (`DpSizes::import_from`). Test: `a_pair_started_on_arcs_checks_the_gap` (concentric arcs at 0.5 mm: refused with
  KiCad's words; at 0.35 mm: starts at the arcs' 0.2 mm, not the class's 0.25 mm).
- **R4-5, `ImportSizes` details.** `CT_HOLE_TO_HOLE` is asked with no start item too (a no-net via); the pair's dummy tracks are at
  `aStartItem->Anchor( 0 )`; the dummy via is default-built, at (0, 0). Test: `hole_to_hole_is_asked_with_no_start_item_too`.
- **R4-6, copper graphics.** A text box with its border off keeps its stroke's width (`GetEffectiveWidth()`; `m_borderEnabled` is only the text
  box's own drawing); an arc or a circle's ring on copper is its chords with `ARC_HIGH_DEF` added to the stroke, so the band holds KiCad's
  `SHAPE_ARC`. **Not router bugs:** a hidden board copper text and a hidden dimension text are not in the model (the importer leaves them out,
  so the derived board kicad-cli checks has none either; the reader's loss is noted in GAPS 7), and a footprint's hidden text is a hidden field,
  which `syncTextItem` drops as well. Test: `from_ir::tests::a_borderless_text_box_keeps_its_stroke_and_an_arc_its_flattening_error`.
- **R4-7, `inflate2` / `inflateLine2` with `aSimplify`.** KiCad's `Clipper2Lib::SimplifyPaths( paths, .. );` drops its result, so the union is
  of the offset as it is; ours now does the same (latent: no caller passes `simplify`). Test (`eda-shape-poly-set`):
  `inflate_with_simplify_keeps_the_offsets_vertices`.
- **Left:** R4-8 (a pre-defined width equal to the Default class's taken for "use net class": the studio keeps the width, not KiCad's index,
  `routerVia.ts::chosenTrackWidth`, `useActionRunner.ts`'s width list; no server half needed) and R4-3 (Board Setup's Differential Pairs list,
  the via rows' checks), other lanes'. A pair's custom dimensions are still applied after `start_diff_pair`'s start-gap check, where KiCad's
  `ImportSizes` has them before it (`UseCustomDiffPairDimensions`).

## Stage 18 -- round-5 router findings (branch `task-r5-router`, 2026-10-28)

The router and studio findings of `findings/round5/r5-route-rules.md` (R5-1 .. R5-11; R5-D* and R5-Z* are another lane's), each re-read against
`pns_tool_base.cpp` (`pickSingleItem`, `updateStartItem`), `router_tool.cpp` (`getStartLayer`, `prepareInteractive`, `finishInteractive`,
`TRACK_WIDTH_MENU`, `DIFF_PAIR_MENU`, `UpdateMessagePanel`), `pns_kicad_iface.cpp` (`ImportSizes`, `createBoardItem`, `RemoveItem`, `Commit`),
`board_editor_control.cpp` (`TrackWidthInc`/`Dec`, `AutoTrackWidth`), `board_design_settings.cpp` (the three lists' `PARAM_LAMBDA`s,
`GetCurrentDiffPair*`, `SetDiffPairIndex`) and `panel_setup_tracks_and_vias.cpp` at 8303b2ad.

- **R5-1, the start layer.** `Router::pick_start` ranks as `pickSingleItem( pos, nullptr, -1 )`: at slop 0 a pad/via/track end on the active
  layer, a track there, then the same on any copper layer (`prioritized[2]`, `[3]`); then within the snap, the active layer's and any layer's.
  `start_layer_of` is `getStartLayer` (the item's first layer when it is not on the active one); `start`, `import_sizes`, `connected_track_width`
  (`SetStartLayerFromPCBNew`) and `start_diff_pair` use it; `Router::start_layer`. The studio's `pickRouteItem` takes the top layer (an end only its
  layer's, `aLayer >= 0`); a start the server put on another layer switches the active layer (`SetActiveLayer( pcbLayer )`) and the route's end gives
  it back (`finishInteractive`'s `m_originalActiveLayer`, the store's `SET_DRAW_STATE`). Tests: `tests/start_layer.rs`, `routePick.test.ts`; kicad-cli:
  `a_route_started_on_a_bottom_pad_with_the_top_layer_active_connects_on_the_bottom` (no unconnected item, clearance, short or crossing; the board
  without the route has the two pads unconnected). **Differs:** visibility and high contrast are the studio's; on another layer the first track by
  layer order wins, not the nearest end.
- **R5-2, the commit's sources.** `RouteCommit::track_sources` (index, board track) is `GetSourceItem()`: a shoved line's tracks (the line placer's
  and the pair placer's `displaced_tracks`), a dragged line's (`DragCommit::sources`: the grabbed segment's track, an uncovered run's, a fan-out
  leg's, a shoved line's), a split track's halves, kept through `drop_redundant` and `simplify_new_lines` (a merged line takes its seed's, as
  `AssembleLine` does). `Cmd::CommitRoute { track_sources, group }`: each sourced track takes its source's `track_masks` entry and joins its group
  (`createBoardItem`, `m_replacementMap[src]`); a track with no source and a new via join `group`, the entered group the studio sends with each
  start (`group`, `ENTERED_GROUP_MAGIC_NUMBER`); a replacement of a track in no group stays in none; removed tracks' mask entries go. Tests:
  `commit_route_carries_a_replaced_tracks_mask_and_group` (ops), `head_via.rs` (shove and drag sources); kicad-cli (`gate_vs_kicad`): a dragged
  masked, grouped track and a route made with a group entered, through the route API, resaved by `kicad-cli pcb upgrade --force`: the segment is
  `(layers "F.Cu" "F.Mask")` with its margin, both groups list the new items. **Not done:** Cleanup's and Break Track's replacements carry no source.
- **R5-3 / R5-11, the project's lists.** The writer puts `track_widths` and `via_dimensions` in the project with the net class's dummy first, as
  `diff_pair_dimensions`; `merge_project_predefined_sizes` reads all three, rows filtered as KiCad's loaders filter them and then the first dropped
  (p22 F5). Test: `the_project_carries_the_predefined_track_widths_and_via_sizes` (eda-kicad).
- **R5-4, a pair's user dimensions at the start.** `Router::user_diff_pair` (`UserDpSizes`) is `ImportSizes`' "user choice" handed to the router
  before `StartRouting` (`DiffPairPlacer::start_with`): the min-clearance and start-gap checks judge them, over the inherited width too. `dp_start`
  sets it from `width` / `gap` / `via_gap`; the pair's Route From Other End from the sizes it ran at. Test:
  `a_pair_with_custom_dimensions_is_checked_with_them_at_the_start`.
- **R5-5 .. R5-7, R5-9, the size choices (studio, `kicad-port/routerSizes.ts`).** The pair menu lists Pre-defined Sizes' pairs with
  `DIFF_PAIR_MENU`'s labels (`studio.Router.setDiffPair`, `pcbx.diffPairIndex`, Use Net Class Values sets index 0); W / Shift+W step the pair list
  while the pair tool is active and re-size a running pair (`dp_dims`; entry 0 the rule's sizes from the `dp_start` reply). Outside a pair, W steps
  `[class, ...track_width_presets]` by index, wrapping; the first press of a route at its inherited width only sets `widthOverride`
  (`m_TempOverrideTrackWidth`) and goes to the current entry; entry 0 while routing is the net's rule width (`rule_width` of the start reply,
  `Router::rule_sizes`); `UseCustomTrackViaSize( false )`. The width and via menu entries and Custom Track/Via Size no longer turn "Use Starting
  Track Width" off; Auto Track Width turns custom sizes off first (`AutoTrackWidth`). The chosen width and via are read from the board's list
  at route time (`chosenTrackWidthOf`; the via action's list entry). Tests: `routerSizes.test.ts`; browser: router-leftovers' r5 scenario,
  diff-pairs' pair-sizes scenario.
- **R5-8, the message panel.** `/api/route/start` and `dp_start` reply `sizes`: the width's source ("existing track", "user choice", the rule's),
  `clearance` and its source (`ImportedSizes::clearance_source`), the pair's width and gap sources, the resolved class
  (`GetHumanReadableName` of `effective_class`), and the rule's sizes. `MessagePanel` shows `routerMessagePanel` while routing: "Routing Track: %s"
  / "Resolved Netclass: %s" (or "Routing Diff Pair"), "Corner Style", "Mode", "Track Width: %s" / "(from %s)", "Min Clearance", "Diff Pair Gap".
  **Differs:** "DP Max Uncoupled-length" is not shown; after a via the clearance is the start's (KiCad re-imports on a layer change).
- **R5-10 (part).** Apply sorts the three lists (`TransferDataFromWindow`). **Left:** Add rows pre-filled with the Default class's values, and the
  ops' own "every preset width must be positive" refusal (KiCad checks no width).

## Stage 19 -- round-5 rule findings (branch `task-r5-rules-zones`, 2026-10-28)

The router side of `work/findings/round5/r5-route-rules.md` section D (R5-D1, R5-D2; the rule engine's side is `web/studio/PARITY-pcb.md` section 59).

- **R5-D1.** The rule engine's footprints carry their sheet, fields and component class (`eda_drc::rule_board`), so a `.kicad_dru` rule on
  `memberOfSheet`, `hasComponentClass` or (for a footprint) `getField` holds the route off as kicad-cli's DRC holds it.
  Test: `a_rule_on_the_sheet_or_a_component_class_holds_a_route_off` (kicad-cli under `EDA_SLOW_TESTS=1`; it failed before).
- **R5-D2.** `syncBarcode`, `syncGraphicalItem` of a `PCB_TEXTBOX` and `syncTextItem` of a `PCB_TABLE` give the solid its board item
  (`solid->SetParent( aItem )`), so `A.Type` is `ENUM_MAP<KICAD_T>`'s "Barcode", "Text Box" or "Table" (`RuleResolver::add_graphic_as`, `RuleItem::type_name`);
  a leader dimension is "Leader" (`Class::Leader`). The implicit "barcode visual separation default" (1 mm `physical_clearance`, `A.Type == 'Barcode'`) now holds:
  a barcode's solid is a special side, which `engine_clearance` answers with `CT_PHYSICAL_CLEARANCE`, and the search margin takes the 1 mm once a barcode is
  in the world. Tests: `a_copper_barcode_keeps_its_1mm_physical_clearance`, `a_rule_on_a_text_box_holds_a_route_off` (both kicad-cli-proved; both failed before).
- `EDA_SLOW_TESTS=1 cargo test -p eda-pns --test qa_regressions`: 13 passed; `--test rules_kicad`: 28 passed.

## Stage 20 -- round-6 router findings (branch `task-r6-router`, 2026-10-28)

The router side of `work/findings/round6/r6-route-rules.md` section R (and R6-D1 on the router's rules), each re-read against `router_tool.cpp`
(`handleLayerSwitch`, `onLayerCommand`, `onViaCommand`, `MainLoop`'s `m_startWithVia`, `performRouting`'s `syncRouterAndFrameLayer`,
`finishInteractive`, `RouteSelected`), `pns_line_placer.cpp` (`SetLayer`, `FixRoute`, `UnfixRoute`), `pns_router.cpp` (`SwitchLayer`, `Finish`) and
`pns_kicad_iface.cpp::ImportSizes` at 8303b2ad.

- **R6-R1, V's via size.** V while routing (`ACT_PlaceThroughVia` -> `onViaCommand` -> `handleLayerSwitch( aEvent, true )`) sizes its via as every via
  command: no size while the net class's are in use (the server's `net_class_via_size`: `VIA_DIAMETER_CONSTRAINT` / `HOLE_SIZE_CONSTRAINT` of a dummy
  via of the route's net), else the chosen Pre-defined Sizes entry or the custom size (`kicad-port/routerVia.ts` `chosenViaOf`, shared with the blind,
  micro and Select Layer commands). Tests: `routerVia.test.ts`; kicad-cli: `a_via_armed_while_routing_has_the_nets_class_size` (a Power class's 0.8 /
  0.4 mm via passes a `via_diameter` / `hole_size` rule, the board's 0.6 / 0.3 mm default is flagged).
- **R6-R2, the board's minimum width.** `settings::chosen_track_width` is `ImportSizes`' `!found` branch: `std::max( bds.m_TrackMinWidth,
  GetCurrentTrackWidth() )`, "user choice" when the chosen width is routed, else "board minimum track width". `route_api::start` takes it for a chosen
  width; `Router::set_width` (W, the width menu: `onTrackViaSizeChanged` imports the sizes again) raises the width too and the move reply says
  `width_source`, which the studio's `drawStateFromPreview` takes when the width was raised. Tests: `a_chosen_width_below_the_board_minimum_routes_at_the_minimum`
  (route_api and, kicad-cli-proved, `rules_kicad.rs`: no `track_width` in the route, the 0.1 mm one flagged).
- **R6-R3, the layer keys while routing.** `LinePlacer::set_layer` is `LINE_PLACER::SetLayer`: idle, any layer; refused once a fix chained the route to
  a segment (`m_chainedPlacement = !pl.EndsWithVia()`) or when it started on a track (`m_startItem` not a via or a `SOLID` on the layer); otherwise the
  head goes to the layer and starts again from the leg's start in the first direction. `Router::switch_layer` (`ROUTER::SwitchLayer`), `POST
  /api/route/layer`. The studio registers layerNext / layerPrev / layerTop / layerInner1..30 / layerBottom while a route is drawn
  (`actions/routerViaActions.ts`; `routerLayerTarget` is `handleLayerSwitch`'s target: next / prev in UI order past hidden layers, a layer the board
  has); refused, a through via to the layer is armed, sized as V's. Tests: `a_layer_key_moves_the_head_to_the_layer_until_a_fix_chains_it`
  (route_api), `routerVia.test.ts`, router-leftovers' r6 scenario. **Differs:** `updateSizesAfterRouterEvent` (the clearance re-imported on the new
  layer) is not run, as after a via (Stage 18).
- **R6-R4, Autoroute.** Each connection starts at its anchor and goes through `ROUTER::Finish` (`attempt_finish`, as the F key), and `RouteSelected`'s
  undo groups are kept: `append_undo` on every connection of a run but the first of a group, so the routes `Finish` makes are one undo step, and a
  connection it cannot make stays live with the head where it got, and its commit, finished by hand, is an undo step of its own (`performRouting`
  clears the flag when `Finish` fails: `m_iface->SetCommitFlags( 0 )`, "the user gets nice checkpoints"; this sentence said the opposite until
  round 7, R7-R4, Stage 21). The studio's own toasts ("Nothing to route ...", "Autorouted %d of %d ...") are gone. Test:
  `autoroute_of_three_free_connections_is_one_undo_step` (route_api).
- **R6-R5, V before a route.** With the route tool armed and no route, V (`ACT_PlaceThroughVia` in `MainLoop`) flips the active layer
  (`PCB_CONTROL::LayerToggle`: the pair's other layer) and sets `pcbx.startWithVia`; the next route starts with a through via armed
  (`performRouting`'s `handleLayerSwitch( ACT_PlaceThroughVia, true )`, `components/canvas/routing.ts`), once. Browser: router-leftovers' r6 scenario.
  **Differs:** the via's implicit target is the pair's other layer, as V's (Stage 11), not the nearest ratsnest anchor's.
- **R6-R6, the active layer.** `syncRouterAndFrameLayer`: while a route or pair runs the active layer is the one being routed, made visible
  (the store's `SET_DRAW_STATE`); `m_originalActiveLayer` is saved at every start and given back at the end (`finishInteractive`). GAPS item 7's R5 line
  said the active layer followed the route; now it does. Browser: router-leftovers' r6 scenario.
- **R6-D1 on the router.** `A.Orientation` of a footprint and a pad is KiCad's (`web/studio/PARITY-pcb.md` section 63). kicad-cli:
  `a_rule_on_the_orientation_reads_kicads_angle` (a pad orientation rule and a Rotation component class hold a route off a footprint KiCad has at
  90, and not one at -90).
- **R6-R8, not fixed.** A shoved line that stood on several board tracks is committed under one of them (`ShoveOutcome`: the first of
  `source_tracks`, which sorts the ids), so its segments share one mask and group, as KiCad's; KiCad's is the seed segment's source
  (`AssembleLine`), ours the track whose id sorts first. Equal when the tracks share a mask and group, the usual case. Fixing it needs the seed carried
  through `Line`.
- Gates: see the HANDOFF row (`qa_regressions` 13/13 exact, `rules_kicad` all passed).

## Stage 21 -- round-7 router findings (branch `task-r7-router`, 2026-10-28)

The router side of `work/findings/round7/r7-route-rules.md` section R, each re-read against `router_tool.cpp` (`prepareInteractive`, `finishInteractive`,
`performRouting`, `performDragging`, `InlineDrag`, `MainLoop`, `onLayerCommand`, `handleLayerSwitch`), `pns_diff_pair_placer.cpp` (`SetLayer`, `FixRoute`,
`makeVia`, `routeHead`), `pns_router.cpp` (`SwitchLayer`) and `undo_redo.cpp` (`RestoreCopyFromUndoList`) at 8303b2ad.

- **R7-R1, the edit keys while routing (high).** `prepareInteractive`, `performDragging` and `InlineDrag` set `UndoRedoBlock( true )`; `performRouting`
  takes Undo and Delete as `routerUndoLastSegment` and rings the bell on the zone-fill actions; `performDragging` treats an undo as an escape. The studio's
  `routerEditKey` (`kicad-port/drawingCancel.ts`): with a route or a pair drawn, Ctrl+Z and Delete are Undo Last Segment, Redo and Fill / Fill All / Unfill /
  Unfill All do nothing; with a drag, Ctrl+Z drops it, Redo, Delete and the fills do nothing. The server refuses `/api/undo` and `/api/redo` of the board
  while a route, pair or drag session is open (`route_api::undo_redo_blocked`, `route_session: true` in the reply), and a studio that draws no route ends
  such a session (one a reload left) and asks again. **Differs:** Delete during a drag is passed on in KiCad (`SetPassEvent`) and would reach the
  selection tool's Remove under the drag; here it does nothing. Tests: `drawingCancel.test.ts`, `an_undo_is_refused_while_a_route_session_is_open`
  (route_api), diff-pairs' r7 scenario.
- **R7-R2, a pair's via size (high).** V while routing a pair sends `chosenViaOf( "through", .. )` as a track's V (`handleLayerSwitch`:
  `GetCurrentViaSize()` unless the net class's vias are in use); `DiffPairPlacer::toggle_via` takes it, and with none goes back to the size the pair
  started with (`rule_via`). **Differs:** with the net class's in use the size is the start layer's rules', not asked again at the layer being routed
  (`net_class_via_size`). Tests: `a_pair_via_takes_the_chosen_via_size` (dp_placer; `dpTool.test.ts`; kicad-cli in `rules_kicad.rs`: a pair via rule of
  0.8 / 0.4 mm passes the chosen vias and flags the class's 0.6 / 0.3 mm).
- **R7-R3, the pair tool's layer keys and V before a pair.** `DiffPairPlacer::set_layer` is `DIFF_PAIR_PLACER::SetLayer`: idle, any layer; refused when
  chained (`m_chainedPlacement`, now kept: a plain fix chains, a fix ending in vias does not) or before the first Move; else when the pair goes on from no
  item or from two vias on the layer, it moves there (`m_start = *m_prevPair; initPlacement()`). `Router::switch_layer` is the placer's of either mode.
  The studio registers the layer actions while a pair is drawn (refused: a pair of through vias to the layer is armed, sized as V), and V with the pair
  tool armed sets `startWithVia`, spent by `startDiffPairRoute`. Tests: `a_layer_key_moves_a_pair_on_vias_to_the_layer` (dp_placer), diff-pairs' r7 scenario.
- **R7-R4, Autoroute's hand-finished connection.** `performRouting`'s `routerAttemptFinish`: when `Finish` fails under the autorouter, `*autoRouted = false;
  m_iface->SetCommitFlags( 0 );` -- `route_api::attempt_finish` clears `append_undo` on a refusal, so the connection the user finishes by hand is an undo
  step of its own. Stage 20's R6-R4 sentence said the opposite and is corrected. Test: `a_hand_finished_autoroute_connection_is_its_own_undo_step`.
- **R7-R5, E while routing.** `evt->IsAction( &PCB_ACTIONS::properties )` runs `ACT_CustomTrackWidth`: E with a route or a pair drawn opens Track and Via
  Dimensions (`pcbnew.InteractiveRouter.CustomTrackViaSize`), not the hovered item's properties. Tests: `drawingCancel.test.ts`, diff-pairs' r7 scenario.
- Left (low), in GAPS item 7: R7-R6 (`startWithVia` outlives the tool), R7-R7 (Autoroute's active layer afterwards), R7-R8 (the mode toasts), R7-R9
  (`COMPONENT_DRAGGER` / `MULTI_DRAGGER`).
- Gates: see the HANDOFF row.

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
- ~~Arcs as router items~~ **Ported 2026-10-20** (Stage 9 step 1). ~~Rounded corners~~ **Ported 2026-10-23**
  (Stage 10 step 2: `CORNER_MODE`, `SwitchRounding*`). Not ported: `m_orthoMode` (`SetOrthoMode`, which
  forces `MITERED_45` in `buildInitialLine`; nothing here sets it), and the 90-degree hull box of
  `rhMarkObstacles` (the mark-obstacles hull snap is not ported at all).
- ~~`VIA::PushoutForce`~~ is ported (2026-10-08, `shove::via_pushout_force`); the
  second lead KiCad tries (`m_last_p_end`) is not kept.
- ~~`MOUSE_TRAIL_TRACER`~~ **Ported 2026-10-20** (Stage 9 step 4). ~~Springback~~
  **Ported 2026-10-23** for the line placer (Stage 10 step 1) and **2026-10-25** for the dragger and the pair placer (Stage 14). See
  `docs/parity/GAPS.md` item 7 for what is left in gap #7: `LockJoint`, ~~virtual vias~~ (Stage 10 step 3),
  the walkaround cluster timeout, the pad-to-die length and delay, time-domain rules as the tuner's
  target, the rounding of corners, the merged outline of a pair's pattern, a pattern moved, turned
  or flipped, the tuner's world branched at every move, two walk attempts of a pair where KiCad makes
  four, and the tracer's `reduceTail` / `handlePullback` family.
