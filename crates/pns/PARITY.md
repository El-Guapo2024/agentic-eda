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
   straight-segment polyline (`eda_model::ir::Track::pts`); nothing ever
   constructs an arc item. Every KiCad pass that branches on "does this
   line contain an arc" takes the non-arc path unconditionally here.
4. **No component dragger, no multi-selection drag.** Single-item
   (segment/via) drag only. Diff pairs (Stage 7) and single-track length
   tuning (Stage 8) *are* now ported, but narrower than upstream in each
   case -- no coupled shove/walkaround for a pair, no via/layer-switch
   mid-pair-route, no diff-pair length/skew tuning, length tuning scoped
   to a straight axis-aligned single-segment track and driven by a
   dialog rather than a live mouse session; see those stages' own
   sections for exactly why and what each still is.
   **Status (2026-10-07):** diff-pair length and skew tuning (`8`, `9`) are
   ported since (Stage 8b below).
5. **Units**: integer micrometers throughout (`eda_model::ir::Um`), like
   the rest of this workspace, not KiCad's internal nanometers.

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

- **No springback, no time limit.** Every call branches fresh from the
  committed world (decision 1, and the HTTP-per-sample driver); the 250
  iterations are the only bound.
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
| random routes from random pads (`simple`, `pic_programmer`, `backspace1`, `dp_test`) and random corner drags in Shove mode | -- | 0 new violations: 250 routes per board while writing it, 50 per board in the committed test; Shove never failed where Walkaround succeeded. Dragging a *via* was the exception: the dragged via itself was never shoved against, so a via dropped onto a track left a violation (6 of 200 random via drags on `simple`). **Fixed 2026-10-08 (D7):** `random_via_drags_never_leave_a_new_violation`, 60 drags per board and mode on `simple` and `pic_programmer`, Shove and Walk around, leaves none. |

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

Scoped down further than `shove`/`walkaround` already are (see
`dragger.rs`'s own doc comment for the full reasoning):
- **Corner drag only** -- grabbing a segment always drags its *nearer*
  endpoint (`Node::item_at`'s shape hit-test finds the segment, then the
  closer of its two assembled-line neighbours is what follows the
  cursor). KiCad's segment-sideways-slide (`DM_SEGMENT`, grabbing a
  segment's middle to slide the whole run sideways) isn't implemented.
- **Free-angle corner relocation**, not KiCad's default 45-degree-
  constrained `dragCorner45`.
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
  falls back. The attached tracks are re-solved at 45 degrees (`DragCorner`), where a
  corner drag is still free-angle.

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
tip. `POST /api/route/{start,drag_start,dp_start}` take a `settings` object
(`route_api.rs` `settings_of`: those switches and `shove_iteration_limit`,
`walkaround_iteration_limit`, `via_force_prop_iteration_limit`,
`walkaround_hug_length_threshold`, which KiCad has no row for); `mode` and
`remove_loops` at the top level still work. **OK in the dialog also applies
the settings to the route, drag or diff pair in progress**
(`POST /api/route/settings`), so a change takes effect from the next move as
upstream's does; the session itself is still a fresh backend `Router` per
route or drag.

## Stage 7 -- differential pairs

`pns_diff_pair_placer.{h,cpp}` -> `src/diff_pair.rs`, task item 6 (`6`
key). See that module's own extensive header comment for the full
design; the short version:

- **Pair detection**: `dp_coupled_net_name` is `BOARD::MatchDpSuffix`
  (`pcbnew/board.cpp`) ported verbatim -- walk a net name backward past
  trailing digits/underscores, swap a trailing `+`/`-`/`P`/`N` for its
  complement (`"LVDS_P0"` -> `"LVDS_N0"`).
- **Sizing**: `BoardRules::diff_pair_width_of`/`diff_pair_gap_of`/
  `diff_pair_via_gap_of` (new, `crates/model/src/lib.rs`) resolve the
  net-class fields the task brief points at (`NetClass::diff_pair_*`,
  which existed on the model already -- carried through `.kicad_pro`
  import since an earlier session, per that field's own doc comment --
  but nothing read them until now), falling back to upstream's own
  `SIZES_SETTINGS` hardcoded defaults (125um width, 180um gap) when a
  board has no diff-pair-specific class, which is every example board in
  this workspace today.
- **Routing itself is structurally simpler than upstream, on purpose**:
  upstream routes a genuine new `PNS::ITEM` kind (`DIFF_PAIR_T`, not
  modeled in this port's `Item` enum at all -- decision #3) with its own
  dedicated hull/collision/shove/walkaround machinery, so the pair itself
  is a first-class obstacle-resolution unit. Building that whole second
  collision model was out of proportion to the remaining scope, so this
  port instead builds one direct-45-trace **spine** centerline (no
  walkaround/shove), offsets it perpendicular by `(gap + width) / 2` on
  each side (`offset_polyline`, a standard "parallel curve of a
  polyline" construction reusing `optimizer::intersect_lines` -- widened
  to `pub(crate)` for this -- to miter each new corner), and snaps each
  line's first/last point to its own real pad rather than the
  geometrically offset point (so the pair always actually reaches the
  pads it started from, regardless of their exact spacing relative to
  `gap + width`). Each line is collision-checked independently; *either*
  colliding flags the whole preview, matching `Mode::MarkObstacles`'s
  report-only contract -- **there is no shove or walkaround for a pair in
  this port**, and no via/layer-switch mid-pair-route either (single
  layer only). A real, usable feature for the common case the task asks
  for (lay a clean, gap-matched pair along an open path), just not a
  collision-resolving one yet.
- **Commit**: both lines' finished runs become ordinary `Track` IR
  entries on their own nets in the *same* `RouteCommit`/`Cmd::CommitRoute`
  a single-track finish produces -- needed no new commit shape at all,
  just more tracks in the existing one (`Router::finish_diff_pair`).
- **HTTP**: `POST /api/route/dp_{start,move,fix,undo_segment,finish}`
  (`crates/cli/src/route_api.rs`), sharing `RouteCell`/`Router` with the
  route/drag sessions (mutually exclusive, same as those two already are
  with each other); the existing `/api/route/cancel` already ends a
  diff-pair session too (it just drops the whole cell).
- **Frontend**: `6` (`pcbnew.InteractiveRouter.DiffPair`) arms the tool
  the same toggle-arm way `X` does; `components/canvas/diffPairRouting.ts`
  + `kicad-port/dpTool.ts` mirror `routing.ts`/`routeTool.ts`'s own split
  exactly. `/` flips the spine's posture; Backspace undoes the last leg;
  no via/width hotkeys (not supported, see above). `painter.ts` draws
  both lines, coloring both the violation color if *either* collides
  (the pair reads as one unit to the user even though each line is its
  own collision check).
- **Test coverage note**: `diff_pair.rs`'s own algorithmic core (suffix
  matching, polyline offsetting/mitering, pair-finding, full route/fix/
  finish flows, collision reporting) has 8 direct unit tests; the
  `Router`-level `*_diff_pair` methods are thin, direct delegations
  (compiled and type-checked, same disjoint-field-borrow pattern the
  already-tested `drag_*` methods use) without their own dedicated
  integration test, to keep this task item's scope proportionate to the
  remaining ones -- see the final report.

## Stage 8 -- length tuning (single track)

`pns_meander_placer.cpp`/`pns_meander.cpp` -> `src/meander.rs`, task item
4's first key (`7`, Tune Length of a Single Track). See that module's own
extensive header comment for the full reasoning; short version:

- **One-shot, dialog-driven, not a third interactive session.** Upstream
  drives `MEANDER_PLACER` the same live, mouse-dragged way `LINE_PLACER`/
  `DIFF_PAIR_PLACER` are driven -- a status-bar length readout updates as
  you drag the mouse away from the track. Building a *third* session type
  (after route and drag) with its own mouse-driven preview state was out
  of proportion to the time left after items 1-3, so this port exposes
  the same underlying operation ("lengthen this track to a target length
  with this amplitude/spacing") as a stateless, one-shot computation --
  `crates/cli/src/tune_api.rs`'s `/api/tune_length/{preview,apply}` (no
  `RouteCell`, no session: every call re-reads `design.json` fresh), the
  length readout the task asked for is `LengthTuningDialog.tsx`'s own
  current/achieved-length display rather than a canvas/status-bar one.
- **Shape scope: a straight, axis-aligned (N/E/S/W), single-segment
  track only.** `generate_meander`'s own doc comment explains exactly why
  the construction (`direction + perpendicular` / `direction -
  perpendicular` as the two diagonal legs of each zigzag) only produces a
  provably-correct, uniform shape when `direction` itself is axis-
  aligned -- a diagonal baseline would need a separately-derived formula
  this session didn't have time to work out and verify. A track with any
  corner (more than 2 points) or a diagonal run is refused with a plain
  message, not silently mishandled.
- **Geometry**: a standard alternating accordion (each period steps
  diagonally off the baseline by `amplitude`, runs parallel for
  `spacing`, steps back, alternating sides each period). The extra length
  one period contributes is `2 * amplitude * (sqrt(2) - 1)`, derived from
  first principles (each diagonal leg's own length vs. how far it
  actually advances along the baseline) and cross-checked against a
  *measured* (not just formula-trusted) polyline length in every test.
  The final period shrinks its own amplitude -- and, if the baseline
  itself is what's actually run out of room rather than the requested
  extra length, shrinks further to whatever the remaining baseline
  length can physically fit -- to land as close to the requested target
  as the geometry genuinely allows, rather than always overshooting by a
  whole period or refusing outright one period early. 9 tests.
- **Collision**: report-only, same `Mode::MarkObstacles`-style contract
  every other scoped-down piece of this crate already uses (diff pairs,
  Stage 7) -- no automatic rerouting around something the meander would
  hit, just a `colliding` flag the dialog surfaces before `Apply` is
  even enabled.
- **Commit**: needed no new `Cmd` at all -- the generated replacement
  becomes one `Track` IR entry, removed-and-re-added via the exact same
  `Cmd::CommitRoute` a finished route/drag/diff-pair already uses
  (`tune_api::apply`).

The settings dialog's own amplitude/spacing fields (`1`/`2`/`3`/`4`
upstream) are `LengthTuningDialog.tsx`'s plain number inputs rather than a
separate `Ctrl+L` settings dialog + live keystroke adjustment -- see that
component's own header comment.

### Stage 8b -- differential-pair length (`8`) and skew (`9`) tuning

`pns_dp_meander_placer.cpp` / `pns_meander_skew_placer.cpp` ->
`src/dp_tune.rs` (the pair/skew orchestration) and `src/meander.rs`'s
`generate_dp_meander` (the dual meander). `tune_api.rs`'s
`/api/tune_length/{preview,apply}` take a `mode` (`single` | `diffpair` |
`skew`, default `single`) and `LengthTuningDialog.tsx` is the one dialog
for all three. Same dialog-driven shape as `7` (no live mouse session) and
the same scope: both lines of the pair must be a **single straight
axis-aligned run** (one 2-point track each) side by side on one layer.

- **`8` `DP_MEANDER_PLACER`**: the clicked line's partner is the
  complementary-net track (`dp_coupled_net_name` = `BOARD::MatchDpSuffix`)
  that runs alongside it (nearest, then longest overlap).
  `baselineSegment` -> the midline of the stretch both lines share; one
  trapezoid meander is built along it and each line is that shape shifted
  by its own signed lateral offset (`SetBaselineOffset`, `offset_polyline`
  -- corners mitered), so the lines are parallel at the pair's pitch
  everywhere. Both lines keep their ends exactly where they were (a
  straight lead-in/out keeps the first/last mitered corner inside the
  stretch) and the unshared stubs of a longer line stay. The target is the
  pair's length, `max( P, N )` over the whole nets (`origPathLength`);
  `tuneLineLength`'s even spread of the elongation over the bumps is
  `pick_periods` (fewest bumps at the biggest amplitude that fits, the
  amplitudes of the bumps differing by at most 1 um so the sum is exact);
  the longer line's real length after the miters is measured and the
  elongation corrected (a pass or two). `MEANDER_SHAPE::MinAmplitude`
  (`|offset|`) and the dual `spacing()` (`2 * |offset|`) floors are
  enforced rather than letting a bump fold its inner line over itself;
  `lines_conflict` is the safety net (`Overlap`). Both replaced tracks are
  one `Cmd::CommitRoute` (one undo step).
- **`9` `MEANDER_SKEW_PLACER`** is not a pair operation upstream either:
  the single-line placer lengthening the *selected* line until its length
  is `m_coupledLength + targetSkew` (the partner net's routed length plus
  the requested skew, default 0). Same here (`tune_skew` ->
  `generate_meander`); a line that is already long enough is refused with
  a message (it only lengthens).
- **Differences from upstream**: the meander is this module's 45-degree
  trapezoid wave (not upstream's rectangular/rounded bumps, same as `7`);
  the pair is tuned across the whole shared stretch, not between a click
  and the cursor; lengths are routed copper only (no pad-to-die length, no
  via barrels -- the model has none); a pair that already meanders, bends
  or has vias is refused (`7`'s straight-run scope).
- Tests: 19 in `meander.rs`/`dp_tune.rs` (geometry: ends on their pads,
  pitch preserved to the micrometre, target hit within 3 um, reversed
  point order, partial overlap, vertical pairs, the refusals) and 5 in
  `tune_api.rs` (preview/apply/undo through a real project directory,
  collision flagged for a bump toward the partner).

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
  `copper_edge_clearance`). The IR keeps one closed outline: an inner cutout and a
  footprint's own Edge.Cuts graphic are not obstacles. A walk around an edge in a
  shove may not grow past 2 x `WalkaroundHugLengthThreshold` of the line's own
  length (KiCad has no bound; with no room left the shortest clear walk is the long
  way round the whole outline).
- `KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` optimizer constraints
  (every candidate is still collision-checked, which is the one
  constraint that must never be skipped; the others are refinements).
- Rounded (arc-filleted) corners -- no arc geometry in this port's `Item`
  at all (decision #3).
- ~~`VIA::PushoutForce`~~ is ported (2026-10-08, `shove::via_pushout_force`); the
  second lead KiCad tries (`m_last_p_end`) is not kept.
- `MOUSE_TRAIL_TRACER` (posture guessed from the swept mouse trail) and
  springback (an incremental undo stack so backing the cursor up reverts
  shove/walkaround decisions instead of recomputing from scratch) --
  still not implemented; see `line_placer.rs`'s own module doc comment
  (point 1) and `shove.rs`'s Stage 3 doc comment above for why, and
  `docs/parity/GAPS.md` for this task's own priority ranking of what's
  left in gap #7.
