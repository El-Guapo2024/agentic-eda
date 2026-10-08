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
| `pns_routing_settings.h`, `pns_sizes_settings.h` | `src/settings.rs` | Done. Defaults cross-checked against `pns_routing_settings.cpp`'s real constructor (including the non-obvious default `RM_Walkaround`, not `RM_Shove`). `SizesSettings::for_net` pulls from this project's own `BoardRules` net-class resolution instead of a live KiCad dialog. `free_angle_mode` is permanently off (45-degree-only router, see decision #4-adjacent scope note in the task). |
| `geometry/direction45.h` | `src/direction45.rs` | `CORNER_MODE` narrowed to `Mitered45`/`Mitered90` (no arcs to fillet a rounded corner with -- see decision #3). `BuildInitialTrace`'s "only honor `start_diagonal` when direction is `Undefined`" subtlety preserved. |

## Stage 2 -- LINE_PLACER + WALKAROUND

| KiCad file | Port | Status |
|---|---|---|
| `pns_walkaround.{h,cpp}` | `src/walkaround.rs` | Core `Route()` loop (hug nearest obstacle, re-detect, chain) ported. Simplified: hugs one obstacle item per iteration rather than a whole `TOPOLOGY::AssembleCluster` blob (still converges to the same place over a few more iterations); no `RestrictToCluster` scoping; CW/CCW reported as two plain candidates (`WalkResult::best()` picks the shorter), no live-cursor-proximity fallback (no continuous mouse-tick stream to fall back from in this architecture) or length-expansion telemetry. |
| `pns_line_placer.{h,cpp}`, `pns_mouse_trail_tracer.{h,cpp}` | `src/line_placer.rs` | See that file's own doc comment for the detailed list; headline simplification is posture: this port keeps the explicit direction state and the `/` toggle (`Direction45::right()`) and continues from the last fixed segment's direction, but does not implement `MOUSE_TRAIL_TRACER`'s continuous mouse-trail-area heuristic (automatic posture guessing from how the cursor swept toward the target) -- not meaningful without a continuous mouse-move stream to measure. |

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

`pns_shove.{h,cpp}` -> `src/shove.rs`. See that file's doc comment. Headline
simplifications: no springback stack (always branch fresh -- a batch/
per-request architecture doesn't need cross-call incremental reuse); single
hull-expansion attempt per obstacle, not KiCad's 3-retry-with-growing-
clearance x 4-winding-order search; via push is the direct one-shot
MTV-displacement KiCad itself uses (not `VIA::PushoutForce`'s iterative
search, which KiCad reserves for lead-in/drag, not SHOVE's own via push);
no forward/reverse rank bookkeeping (KiCad's anti-ping-pong mechanism for
"don't shove something back into what already shoved you this run") --
instead the propagation stack has a hard iteration cap and simply fails
(falls back to walkaround) rather than looping, which is the same outcome
KiCad's own iteration-limit path produces.

`shove.rs` operates on its own scratch branch internally (it must -- shoving
genuinely mutates the board) and returns only the diff (`ShoveOutcome`'s
`displaced_lines`/`displaced_vias`); `LinePlacer` stays stateless for
`preview()` the same way it already was for walkaround (every call re-runs
shove fresh from the real world), and only `fix()`/`finish()` absorb an
accepted call's displacement into the session's running
`displaced_tracks()`/`displaced_vias()` for the eventual commit. One real
bug surfaced here too: pushing a via whose centre sits exactly on the
pusher's own centreline (common -- a straight route running directly
through a stitching via) produced a zero-length, direction-less push
vector that never actually moved it; fixed by falling back to a direction
perpendicular to the pusher's own heading in that degenerate case.

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
- `Mode::Walkaround` while dragging behaves like `Mode::MarkObstacles`
  (reports collisions, doesn't resolve them) -- only `Mode::Shove` keeps a
  drag obstacle-free.

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
`pcbnew.InteractiveRouter.SettingsDialog`)**, backing `state.routerSettings
= { mode, removeLoops }` -- see `web/studio/PARITY-pcb.md` for the full
field-by-field scope (deliberately narrower than upstream's own dialog:
only `mode` and `removeLoops` have any real effect in this crate, so only
those two are exposed as working controls; Free Angle Mode is shown
disabled with its reason rather than omitted, since the task named it
explicitly). `POST /api/route/start` gained `remove_loops`;
`POST /api/route/drag_start` gained `mode` (previously silently always
`Mode::Walkaround` regardless of what a route session was using -- a
small, real inconsistency this closes in passing). A setting change takes
effect on the *next* `X`/`D` session, not a live one already in progress
-- this app starts a brand new `Router`/session per route or drag (no
persistent one to push a live update into the way upstream's own dialog
does), a deliberate, documented adaptation rather than a gap.

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
  `RoutingSettings::smart_pads` exists as a field but, like
  `shove_vias`/`jump_over_obstacles`/`optimizer_effort`/
  `fix_all_segments`/`walkaround_hug_length_threshold`, is never read by
  any routing code in this crate -- struct-shape parity only, not
  implemented behavior (confirmed by grepping for each field's use
  outside `settings.rs` itself: none).
- `KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` optimizer constraints
  (every candidate is still collision-checked, which is the one
  constraint that must never be skipped; the others are refinements).
- Rounded (arc-filleted) corners -- no arc geometry in this port's `Item`
  at all (decision #3).
- `VIA::PushoutForce`'s iterative lead-direction search for via lead-in
  while routing (`buildInitialLine`'s via-placement path uses a simpler
  direct placement -- see `line_placer.rs`).
- `MOUSE_TRAIL_TRACER` (posture guessed from the swept mouse trail) and
  springback (an incremental undo stack so backing the cursor up reverts
  shove/walkaround decisions instead of recomputing from scratch) --
  still not implemented; see `line_placer.rs`'s own module doc comment
  (point 1) and `shove.rs`'s Stage 3 doc comment above for why, and
  `docs/parity/GAPS.md` for this task's own priority ranking of what's
  left in gap #7.
