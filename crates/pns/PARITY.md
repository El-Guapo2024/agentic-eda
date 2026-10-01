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
   Reusing `eda_drc`'s geometry/constraint-resolution means this router
   and the DRC engine agree on clearance by construction.
3. **No `ARC_T`.** This project's IR represents every track as a
   straight-segment polyline (`eda_model::ir::Track::pts`); nothing ever
   constructs an arc item. Every KiCad pass that branches on "does this
   line contain an arc" takes the non-arc path unconditionally here.
4. **No diff pairs, no meandering/length-tuning, no component dragger,
   no multi-selection drag.** Single track route and single-item
   (segment/via) drag only, matching the task's explicit scope.
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

## Stage 4 -- API + frontend

`pns_router.{h,cpp}` -> `src/router.rs`; `router_tool.cpp`'s interaction
state machine -> `web/studio/src/kicad-port/routeTool.ts` +
`Canvas.tsx`/`routing.ts` wiring; HTTP endpoints in `crates/cli/src/
studio.rs`. Commit granularity is **per finished line**, not KiCad's
per-segment `AddItem`/`RemoveItem`/`UpdateItem`: this project's own
`Track` IR already holds a whole polyline, so a finished route becomes one
`Track` (or two, split at a placed via, plus the `Via`) instead of one
`Cmd` per segment -- a deliberate adaptation to this project's own data
model (see the task's "our JSON is the only source of truth" rule), not a
fidelity gap. A newly added `Cmd::CommitRoute` (`crates/ops/src/lib.rs`)
removes whatever existing tracks/vias a shove displaced and adds the
session's final geometry in one undo step, generalizing the existing
`PasteItems` (pure-add) the same way KiCad's own `NODE::Commit` generalizes
a pure add into "remove the overridden set, add the branch's own items."

## Stage 5 -- DRAGGER

`pns_dragger.{h,cpp}` -> `src/dragger.rs`. Segment and via drag only (no
component/multi-item drag, matching decision #4). Reuses `shove`/
`walkaround` exactly as KiCad's own `DRAGGER` does, rather than
reimplementing displacement.

## Known gaps vs. upstream (won't-fix for this task, tracked for later)

- `MERGE_OBTUSE`, `SMART_PADS`, `FANOUT_CLEANUP` optimizer passes (`src/
  optimizer.rs` only ports `MERGE_SEGMENTS`/`MERGE_COLINEAR`, the two every
  plain interactive route and post-shove cleanup actually uses by
  default).
- `KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` optimizer constraints
  (every candidate is still collision-checked, which is the one
  constraint that must never be skipped; the others are refinements).
- Rounded (arc-filleted) corners -- no arc geometry in this port's `Item`
  at all (decision #3).
- `VIA::PushoutForce`'s iterative lead-direction search for via lead-in
  while routing (`buildInitialLine`'s via-placement path uses a simpler
  direct placement -- see `line_placer.rs`).
- Loop removal (`RemoveLoops`/`FindLinesBetweenJoints`) -- not implemented;
  a route that reconnects to an existing same-net path leaves both in
  place rather than deleting the redundant one.
