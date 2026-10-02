# Interactive router (`crates/pns`) vs. KiCad `pcbnew/router` -- code-level comparison

Scope: `crates/pns/src/*.rs` (hull, line_walk, shove, optimizer, node, walkaround, line_placer, dragger, diff_pair, meander, router, direction45, item, joint, index, line, settings, from_ir) against `/home/user/kicad-src/pcbnew/router/*.cpp|h` and the `libs/kimath` geometry they call (KiCad master @8303b2ad). Read-only analysis: no code in the repo was changed; the only file added is this one.

Method. For the five files that claim to be faithful ports (hull.rs, line_walk.rs, shove.rs, optimizer.rs, node.rs epsilon) every KiCad function was opened and read next to ours, branch by branch. For the remaining files the KiCad side was read at the level needed to classify each function (flows and constants, not every debug line). Where reading was not enough, small differential probes were built **outside the repo** (a scratch cargo crate in the session scratchpad that depends on `eda-pns` by path; KiCad algorithms re-typed from the C++ and compared against our public functions on random inputs). Probe results are quoted with their sample sizes; they are evidence for the specific input class tried, not proofs. The repo working tree was not touched (`git status` shows only other agents' pre-existing changes plus this file).

Status key (as requested): **FAITHFUL** same algorithm and same branch structure (nm->um and language idioms are fine). **ADAPTED** deliberately different; the note says what differs, whether our model justifies it, and whether behaviour changes. **DIVERGENT** claims to port but behaves differently in some branch (a bug or silent simplification). **MISSING** not ported.

Units. KiCad is nm, we are whole um (1 KiCad literal 1000 = 1 um). Literals that were converted: `c_ENDPOINT_ON_HULL_THRESHOLD` 1000 -> 1 (shove.rs:99), `cHullFailureExpansionFactor` 1000 -> 1 (shove.rs:101). Literals that were copied **without** conversion and therefore mean 1000x more in our units are the tolerance constants in the SHAPE_LINE_CHAIN/SEG helpers (`Contains` sq-dist <= 3, `Collinear` |det| <= 1, `PointOnEdge` accuracy+1, `Find( p, 1 )`, `Simplify2` line-distance <= 1). For rounding noise that is partly unavoidable (our intersection points are rounded to 1 um, not 1 nm) but it is applied inconsistently, which is the root of divergence D3 below.

## Summary

Status counts of the rows in the summary table (section "Function table"; a row = one KiCad function/behaviour; the counts are generated from the table):

| Status | Rows |
|---|---|
| FAITHFUL | 31 |
| ADAPTED | 55 |
| DIVERGENT | 33 |
| MISSING | 26 |
| **Total** | **145** |

By section:

| Section | FAITHFUL | ADAPTED | DIVERGENT | MISSING |
|---|---|---|---|---|
| A. Hulls | 5 | 6 | 5 | 1 |
| B. `LINE::Walkaround` and the geometry it stands on | 6 | 0 | 8 | 0 |
| C. Shove | 0 | 6 | 5 | 8 |
| D. Optimizer | 8 | 4 | 4 | 2 |
| E. Node, items, joints, index | 0 | 14 | 2 | 2 |
| F. Walkaround | 0 | 3 | 0 | 2 |
| G. Line placer | 0 | 6 | 3 | 1 |
| H. Dragger | 0 | 3 | 2 | 2 |
| I. Diff pair | 2 | 3 | 0 | 3 |
| J. Meander / length tuning | 0 | 1 | 1 | 1 |
| K. Router session | 0 | 5 | 1 | 1 |
| L. `DIRECTION_45` | 3 | 0 | 1 | 1 |
| M. Constants and tolerances | 7 | 4 | 1 | 2 |

Headline findings (details, repros and fixes in "Divergences to fix"):

1. `Router::build_commit` emits a bogus 1 um via at every intermediate click (D1) -- executed repro, any multi-click route.
2. `Node::assemble_line` never terminates on a closed same-net track loop (D2) -- executed repro (hangs, unbounded memory); reachable from route finish, shove and drag.
3. `Seg::intersect` truncates where KiCad rounds, and `line_walk::split` accepts only sq-dist <= 1 where KiCad accepts <= 3: a hull crossing is silently dropped and `LINE::Walkaround` returns a path **through** the obstacle (D3) -- 3801 of 40000 random chords; 0 after fixing the rounding.
4. `convex_hull_octagon` (hull.rs:360) moves the 45-degree diagonals by the distance to the *finite diagonal segment* instead of KiCad's `LineDistance` to the vertices, so hulls of rotated/trapezoid pads intrude into the clearance zone (D4) -- 88% of random rotated pads, up to 154 um; a 1.0x0.6 mm pad rotated 60 deg is hugged at 236 um instead of 300.
5. Shove mode aborts the whole shove as soon as a pad is hit (`shove.rs:284`) and never does KiCad's solids-only walk pre-pass (D5); via push over/under-shoots (D6); drag in the default (Walkaround) mode never resolves anything (D7).

## Function table

Columns: KiCad function | KiCad file:line | ours file:line | status | note.

### A. Hulls (`hull.rs`; KiCad `pns_utils.cpp`, `pns_via.cpp`, `pns_solid.cpp`, `pns_line.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| OctagonalHull | pns_utils.cpp:37 | hull.rs:246 `octagonal_hull` | FAITHFUL | Same 4-or-8 vertex order and the `aChamfer != 0` guards. The extra `make_clockwise` calls are no-ops (the raw order already has positive shoelace sum). |
| IsSegment45Degree | pns_utils.cpp:154 | hull.rs:267 `is_45` | DIVERGENT | KiCad is tolerant (`\|dx\|<=1`, `\|dy\|<=1`, `\|\|dx\|-\|dy\|\|<=1`); ours is exact (`dx==0 \|\| dy==0 \|\| \|dx\|==\|dy\|`). A (5,1) stub is "almost horizontal" in KiCad (hull built along x, `cl++`) but a diagonal 5x5 kink in ours. Only reachable for segments shorter than `clearance/10`. See D13. |
| SegmentHull | pns_utils.cpp:178-283 | hull.rs:272 | DIVERGENT | Vertex formulas, order and the clockwise `Side` fix-up are identical (lines 269-283 vs 311-335). Three differences, all in the "kink" handling: (a) `len` is truncated (`sqrt() as Um`, hull.rs:276) but KiCad's `SEG::Length()` is `EuclideanNorm()` = `KiROUND(hypot)` (vector2d.h:283-299); (b) `d`, `dr`, `xr2` are computed from the *already bumped* `cl` (hull.rs:301) while KiCad computes them before the `cl++`/`cl+=2` (pns_utils.cpp:184-188); (c) the 45-degree test above. Probe: 200000 random segments (half with |dx|,|dy| <= 30 um, i.e. in the kink range for clearances >= ~100 um): 10074 differ (8919 with both the truncated and the rounded length <= kink -- sources (b) and (c); 1155 where truncated and rounded length straddle the kink threshold -- source (a)); for clearly non-kink lengths the output is bit-identical. Observable only for tiny stubs (e.g. the 9 um stub that a walkaround produced in the router probe). |
| ArcHull | pns_utils.cpp:68 | -- | MISSING | No arcs in the IR (pns/PARITY.md decision 3). Justified; no behaviour change for this data model. |
| MoveDiagonal | pns_utils.cpp:286 | hull.rs:379-384 (closure `mv`, `seg_dist`) | DIVERGENT | KiCad: `SHAPE_LINE_CHAIN::NearestPoint( SEG, dist )` (shape_line_chain.cpp:2463) = min over **vertices** of `SEG::LineDistance` (distance to the infinite line, seg.cpp:746). Ours: min over polygon **edges** of the distance to the *finite* diagonal segment, truncated. The finite segment is only `bh*sqrt(2)` long, so for a vertex whose foot point falls outside it ours returns a larger distance, `moveBy` becomes too large, and the diagonal ends up closer than `cl` to the pad. See D4. |
| ConvexHull (SHAPE_SIMPLE) | pns_utils.cpp:297-351 | hull.rs:360 `convex_hull_octagon` | DIVERGENT | Box inflation, the four box lines, corner points, vertex order of the octagon (left/bottom-left, bottom/bottom-left, ...) all match; the diagonals do not (previous row). Probe: 20000 random rotated rectangular pads, KiCad-reference hull is never closer than `cl` to the pad (0 cases); ours is closer than `cl-1` in 17689 cases, worst shortfall 154 um. 45-degree-aligned and axis-aligned pads are unaffected (gap exact). |
| `SEG::IntersectLines` as used by ConvexHull | seg.cpp:312 (aLines=true) | hull.rs:346 `intersect_lines` | ADAPTED | float formula + `KiROUND` instead of KiCad's `rescale`; <= 1 um difference, only matters if a diagonal and a box line meet at a non-integer point. |
| BuildHullForPrimitiveShape | pns_utils.cpp:475 | hull.rs:401 `primitive_hull` | FAITHFUL | `cl = clearance + (walk+1)/2` (hull.rs:402) matches; RECT -> octagon with chamfer 0, CIRCLE chamfer `2(1-1/sqrt2)(r+cl)` truncated to int (same as KiCad's implicit double->int), SEGMENT -> SegmentHull, SIMPLE -> ConvexHull. Arc branch MISSING (no arcs). |
| Pad -> SOLID shape mapping (what BuildHullForPrimitiveShape is fed) | pns_kicad_iface.cpp:1558-1577, pad.cpp:1143 | item.rs:158, hull.rs:407-419 | ADAPTED | KiCad keeps a pad as a single primitive only when the effective shape has exactly one indexable sub-shape (rect, circle, oval); a rounded-rect pad (rect + 4 segments) becomes `SHAPE_SIMPLE( polygon )` and is hulled with ConvexHull. Ours keeps `Shape::RoundRect` and unions the 5 primitive hulls (`union_outline`). Probe (4 pad sizes, 16-segment arcs): identical polygon area, 12 vs 8 vertices (collinear extras). No geometric change; the vertex count affects walkaround path vertex counts only. The same mapping decides smart-pad breakouts (see D14). |
| SOLID::Hull (compound union branch) | pns_solid.cpp:39-64 | hull.rs:426 `union_outline` | FAITHFUL | `AddOutline` each, `Simplify()`, `Outline(0)` via `eda_shape_poly_set`; first-polygon-only like KiCad. Result is made clockwise (KiCad's Clipper output is already positive-area). Vertex start index may differ. |
| SOLID::Hull (single shape, per-layer arg) | pns_solid.cpp:39 | item.rs:175 | ADAPTED | One shape for all layers (no padstacks). Correct for this IR. |
| VIA::Hull | pns_via.cpp:235-249 | hull.rs:339 `via_hull`, item.rs:174 | FAITHFUL | `cl = clearance + walk/2` (not +1, ours matches), chamfer `(2*cl+width)*(1-1/sqrt2)` truncated. The `m_hole && !IsFlashedOnLayer` branch (hull of the hole only) is MISSING; no unflashed layers in the IR (ADAPTED, justified). |
| SEGMENT::Hull | pns_line.cpp:616 | item.rs:173 | ADAPTED | Calls `SegmentHull` but with `clearance + HULL_ROUNDING_GUARD` (next row). |
| `HULL_ROUNDING_GUARD` (ours only) | -- | hull.rs:208, item.rs:170, shove.rs:234 | ADAPTED | +1 um clearance on every router hull because vertices are rounded to whole um. Observable: a shoved track sits at y = +/-401 where KiCad would give +/-400 (probe b2). Justified by the rounding, but note shove.rs:234 stacks it on top of the per-attempt 1 um expansion, so attempt n is `clearance + n + 1`. |
| `VECTOR2I::Resize` | vector2d.h:385 | hull.rs:215 `resize` | FAITHFUL | Same 45-degree shortcut, same sign handling; float instead of int64 `rescale` (<= 1 unit). |
| `VECTOR2I::EuclideanNorm` / `SEG::Length` | vector2d.h:283 | hull.rs:276, item.rs:85, line.rs:66 | DIVERGENT | Truncation instead of KiROUND in `segment_hull` (see SegmentHull). Elsewhere (`Segment::length`, `Line::length`) f64 lengths are used for costs only; harmless. |
| (pre-port) `hull_of`, `circle_pts`, `convex_hull` | -- | hull.rs:51-150 | ADAPTED (dead code) | The module doc at hull.rs:1-31 still describes this older "circumscribed octagon + convex hull" construction as *the* hull and claims KiCad's chamfer is less safe. `Item::hull` no longer calls it (only tests and one walkaround test do). Misleading doc; delete or mark as legacy. |

### B. `LINE::Walkaround` and the geometry it stands on (`line_walk.rs`; KiCad `pns_line.cpp`, `pns_utils.cpp`, `libs/kimath`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| LINE::Walkaround | pns_line.cpp:250-613 | line_walk.rs:205-381 | FAITHFUL | Compared block by block: `inFirst` early-out, `HullIntersection`, self-intersection split, split of path and hull at every ip, second pass splitting `PointOnEdge` path points into the hull, `!aCw` -> `Reverse`, vertex typing (`inside && !onEdge -> INSIDE`, `onEdge -> ON_EDGE`), graph construction incl. hull-vertex overlap and forward-only hull neighbour links, the three ON_EDGE neighbour scans (outside / `!isHull && areNeighbours && indexh+1` / `indexh+1` with `visited=false` reset and the `inLast` projection exit), OUTSIDE fallback to a visited neighbour, `iterLimit` 1000, loop-break, `appendV`. `out.dedup()` replaces KiCad's `Append` duplicate suppression (equivalent). Extra guard `obstacle.len() < 3 -> None` (KiCad would just see `PointInside` false). The wrong *results* quoted in D3 come from the helpers below, not from this control flow. |
| HullIntersection | pns_utils.cpp:392-473 | line_walk.rs:89 | FAITHFUL | Same corner/neighbour logic, same `Side > 0` test. |
| areNeighbours | pns_line.cpp:235 | line_walk.rs:183 | FAITHFUL | Including the `indexp = -1` quirk (`x < max-1 && x+1 == y` is true for a hull-only vertex when `y == 0`). |
| SHAPE_LINE_CHAIN::Intersect( chain ) | shape_line_chain.cpp:1806 | line_walk.rs:42 | DIVERGENT | Same collinear / corner flagging structure. Differences: `Collinear` is `\|det\| <= 1` in KiCad (seg.h:286) but exact `cross == 0` here; `Contains` is `SquaredDistance <= 3` (seg.cpp:627) but `<= 1` here (line_walk.rs:23); result order follows KiCad's x-sorted scan, ours follows segment index (harmless: `Split` is order independent). |
| SEG::Intersect / intersects | seg.cpp:312-440 | crates/drc/src/kimath.rs:133-179 | DIVERGENT | KiCad: point = `aSeg.A + rescale( param1, dir2, det )` with round-half-away integer rescale (util.cpp:79-120), anchored on the *other* segment; ours: `a + (dir1*t)/det` with Rust integer division = **truncation toward zero**, anchored on `self`. Error up to 1 um per axis instead of <= 0.5. No collinear-overlap branch (KiCad `checkCollinearOverlap`); callers gate that separately so it only matters for `sq_distance_to_seg`'s early-out. This is the root cause of D3. |
| SHAPE_LINE_CHAIN::Split | shape_line_chain.cpp:1185 | line_walk.rs:133 | DIVERGENT | KiCad inserts into the segment with `Distance < 2` where `Distance = isqrt(SquaredDistance)` floor, i.e. squared distance <= 3; ours requires squared distance <= 1 (line_walk.rs:147). Together with the truncating intersect the exit crossing of a chord is frequently **not inserted** (D3). Also KiCad can insert a duplicate of an existing point into a different touching segment; ours returns early when the point exists. |
| SHAPE_LINE_CHAIN::Find( p, thr ) | shape_line_chain.cpp:1241 | line_walk.rs:128 | DIVERGENT | KiCad: `EuclideanNorm() <= thr` (rounded norm: a (1,1) offset has norm 1); ours: squared distance <= thr^2 (a (1,1) offset has 2 > 1). A crossing diagonal-adjacent to an existing vertex is split in ours, snapped in KiCad. Low impact. |
| SHAPE_LINE_CHAIN::PointOnEdge / EdgeContainingPoint | shape_line_chain.cpp:2078, :2084 | line_walk.rs:154 | FAITHFUL | `a==p \|\| b==p \|\| SquaredDistance <= (acc+1)^2`. (In um this is 1000x coarser than in nm; see Units.) |
| SHAPE_LINE_CHAIN::PointInside | shape_line_chain.cpp:1990 | line_walk.rs:163 | FAITHFUL | Same half-open y rule; float `round` equals the int128 rescale (ties away from zero). |
| SHAPE_LINE_CHAIN::SelfIntersecting | shape_line_chain.cpp:2139 | line_walk.rs:219-228; shove.rs:119 | DIVERGENT | KiCad also tests **adjacent** segments (a path that doubles back on itself: `Contains(b2)` of the next segment) and uses `Intersect( ..., ignoreEndpoints=true )` plus the `Contains` tolerance; ours only checks `j >= i+2` with endpoint-inclusive exact intersection. A reversal like (0,0),(10,0),(5,0),(5,5) is split in KiCad and untouched here; shove.rs:119 would not flag it either. Rare. |
| SEG::NearestPoint | seg.cpp:633 | drc kimath.rs:91 | DIVERGENT (minor) | `(t*d)/l_sq` truncates; KiCad rescales with rounding. Used for the `inLast` projection endpoint (line_walk.rs:361) and `hull_nearest`; up to 1 um off the hull edge. |
| SEG::SquaredDistance( pt ) | seg.cpp:714 | drc kimath.rs:70 | FAITHFUL | Including the `g` float formula and rounding. |
| SHAPE_LINE_CHAIN::Simplify2 | shape_line_chain.cpp:2910 | line.rs:96 `Line::simplify` | DIVERGENT (minor) | KiCad: dedup, then drops any point whose line-distance to the chord is <= 1 (or `Collinear`), 3-point chains only dedup'd; ours drops only exactly collinear points that lie *between* their neighbours, also on 3-point chains. Effect: ours keeps near-collinear points KiCad would drop and drops a collinear middle of a 3-point chain KiCad keeps. |
| POINT_INSIDE_TRACKER | shape_line_chain.cpp:3135-3233 | walkaround.rs:33 `point_in_polygon` (used by shove.rs:105,165) | DIVERGENT (minor) | Both give the same answer off the boundary. On the boundary KiCad's tracker sets `m_state = -1` (never inside); ours is arbitrary (strict `>` crossing, no on-edge test) so `checkShoveDirection` can reject a shove whose pusher start lies exactly on the swept polygon. |

### C. Shove (`shove.rs`; KiCad `pns_shove.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| SHOVE::checkShoveDirection | pns_shove.cpp:243 | shove.rs:113 | DIVERGENT | Same tracker construction (`obstacle + reversed(shoved)`). Missing: `SHP_REVERSED` (use the *last* point as the pusher when a head is dragged by its first vertex, set by DRAGGER for corner index 0, pns_dragger.cpp:626; consumed at pns_shove.cpp:253) and the lone-via case. Ours always uses `pusher.first()`, which for `grabbed_index == 0` is the cursor-side tip -> the direction test is inverted for that drag (D7). Also `shoveLineToHullSet` passes the *adjusted* `obs` here (shove.rs:203) where KiCad passes the original `aObstacleLine`. |
| SHOVE::shoveLineToHullSet | pns_shove.cpp:328-519 | shove.rs:152-215 | DIVERGENT (minor) | Structure matches: 4 attempts, `invertTraversal = attempt>=2`, `clockwise = attempt%2`, endpoint snapping (`minDistP`) on the `permitAdjusting*` attempts, per-hull `Walkaround`, `Simplify2`, endpoint equality, direction check, self-intersection, `Collide` with the pusher. Differences: (a) the "vFirst/vLast + CompareGeometry" gate (pns_shove.cpp:437-466) is absent (MISSING sub-branch; redundant with the endpoint check in practice); (b) `l.Line().Append( p1 )` suppresses a duplicate end point but `Insert( 0, p0 )` does not (KiCad); ours pushes/inserts unconditionally, so an endpoint already on the hull yields a zero-length segment fed to Walkaround; (c) KiCad stores the failed direction-check shape into `aResultLine` as a side effect (:491), ours has no such state; (d) `Simplify2` -> `Line::simplify` (see B); (e) `c_ENDPOINT_ON_HULL_THRESHOLD`: `dist < 1000 nm` becomes `d < 1.0` um, which on an integer grid means "exactly on the hull" (ADAPTED, fine). |
| SHOVE::ShoveObstacleLine | pns_shove.cpp:521-637 | shove.rs:226 `push_line` | DIVERGENT | Per-segment hulls at `clearance + extraHullExpansion` with `obstacleLine.Width()` and 3 attempts growing by 1 um: matches. `permitMovingStart/End = attempt>=2 && !voe*` matches. Missing: the pusher's **end via** hull (`viaOnEnd`, :584-603) -- ours builds the pusher from bare points; the shoved line is therefore not pushed off the head's via hull (the via is only a collision item later); the `shoveLineFromLoneVia` branch (next row); arc extra clearance (no arcs); `getClearance` is `Node::clearance` (no hole clearance); the hull clearance carries the extra +1 guard. |
| SHOVE::shoveLineFromLoneVia | pns_shove.cpp:278-326 | -- | MISSING | Pusher on another layer / without segments but with a via. Needed once a layer-switch head pushes tracks on the destination layer. |
| SHOVE::getClearance | pns_shove.cpp:169 | shove.rs:227 | ADAPTED | `GetClearance( a, b, false )` (no epsilon) is reproduced by `Node::clearance`; hole clearances and `m_forceClearance` have no analogue (no holes in the IR items). |
| SHOVE::onCollidingSegment | pns_shove.cpp:639-687 | shove.rs:335-352 | ADAPTED | `HasLockedSegments -> SH_TRY_WALK` (KiCad then walks the *current* line) becomes `return None` (the whole shove fails, the placer falls back to a full walkaround). No `Simplify2` of the shoved line, no rank bookkeeping, no `unwindLineStack`. |
| SHOVE::onCollidingArc | pns_shove.cpp:689 | -- | MISSING | No arcs. |
| SHOVE::onCollidingLine (reverse collision) | pns_shove.cpp:738 | -- | MISSING | Ours has no rank system, so a shoved line pushed back into an already-shoved one is simply re-pushed until the 250-iteration cap (shove.rs:271), where KiCad displaces the *other* line (`onCollidingLine( revLine, currentLine )`, shoveIteration :1700-1724). |
| SHOVE::onCollidingSolid | pns_shove.cpp:776-899 | shove.rs:284 | MISSING | KiCad walks the colliding *current line* around the cluster of the solid (WALKAROUND with `WP_SHORTEST`) and continues shoving; ours returns `None` for any pad hit. See D5 for the observable consequence. |
| SHOVE::onCollidingVia | pns_shove.cpp:1175-1265 | shove.rs:286-334 | DIVERGENT | KiCad moves the via by the **minimum translation vector** of `SHAPE::Collide( via, line, clearance + w/2, &mtv )`, with epsilon-free clearance. Ours moves `max(clearance_needed - actual, 1) + via.diameter/2` along (via - nearest pusher point), where `clearance_needed` already has the 1 um epsilon subtracted (node.rs:373) and `actual` is clamped at 0 once overlapping. Probe: via 100 um off the centreline -> final centre-to-centreline 599 (edge gap 199 < 200); via exactly on the centreline -> 899 (KiCad: 600). D6. |
| SHOVE::pushOrShoveVia | pns_shove.cpp:1041-1173 | shove.rs:304-333 | DIVERGENT | Missing: `ShoveVias()` setting -> `SH_TRY_WALK` (`settings.shove_vias` is never read, settings.rs:54); the "pushed via must not land on an existing joint" loop (`p0_pushed += force.Resize(2)`); attached tracks are re-shaped with `LINE::DragCorner( p0_pushed )` (45-degree re-solve, adds an elbow) -- ours replaces the end vertex (shove.rs:319-326) so the fan-out segments become **non-45-degree**; stitching-via bookkeeping. |
| SHOVE::onReverseCollidingVia | pns_shove.cpp:1267 | -- | MISSING | Part of the rank system. |
| SHOVE::fixupViaCollisions / patchTadpoleVia | pns_shove.cpp:1517, :1599 | -- | MISSING | Via-fanout special cases. |
| SHOVE::shoveIteration | pns_shove.cpp:1633-1878 | shove.rs:269-355 | ADAPTED | KiCad queries `NearestObstacle` per kind in the order SOLID, VIA, SEGMENT, HOLE (nearest *along the line* by hull-intersection path length) and branches on rank. Ours takes the single smallest edge gap over all kinds (`min_by actual`, shove.rs:279): different obstacle order, and no solid-first rule, so a track can be shoved before a pad that makes the whole attempt fail anyway. |
| SHOVE::shoveMainLoop | pns_shove.cpp:1880 | shove.rs:269-273 | ADAPTED | Iteration limit 250 is the same constant (`m_iter >= iterLimit` vs ours `> limit`, off by one); the 1000 ms `ShoveTimeLimit` is MISSING (no time budget per request; the iteration cap is the only bound). |
| SHOVE::Run | pns_shove.cpp:2388-2606 | shove.rs:253-363 | ADAPTED | Heads are locked at their endpoints (`LockJoint`), head via cloned, springback pushed; ours branches a full copy per call, no locks, no springback (justified by the stateless HTTP model, PARITY.md decision 1). |
| SHOVE::runOptimizer | pns_shove.cpp:2013-2121 | -- | MISSING | KiCad optimizes every shoved line after the main loop (OE_LOW: MERGE_OBTUSE x1; MEDIUM/FULL: MERGE_SEGMENTS x2, LIMIT_CORNER_COUNT, SMART_PADS). Ours optimizes only the head (line_placer.rs:196); displaced tracks keep the raw hull-walk geometry (extra vertices, wrap-around shapes like probe b2's 8-point detour). |
| springback (`reduceSpringback`, `pushSpringback`, node stack) | pns_shove.cpp:924, :983 | -- | MISSING | Justified: no tick stream between requests. Observable only as "un-shove" smoothness while dragging. |
| SHOVE::assembleLine / preShoveCleanup | pns_shove.cpp:218, :2359 | node.rs:214 | ADAPTED | `AssembleLine( seg, idx, aStopAtLockedJoints=true )`: stops at locked joints and at locked segments (`NextSegment` ignores them unless `aFollowLockedSegments`), skips segments of a different width. Ours does none of the three: see D10. |

### D. Optimizer (`optimizer.rs`; KiCad `pns_optimizer.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| COST_ESTIMATOR::CornerCost | pns_optimizer.cpp:44-69 | optimizer.rs:49-66 | FAITHFUL | 10 / 5 / 50 / 30 / 60 / 100 table and the segment-pair sum are identical. |
| OPTIMIZER::mergeFull | pns_optimizer.cpp:544-582 | optimizer.rs:198-215 | DIVERGENT | Step schedule (`SegmentCount()-1`, clamp to `n_segs-2`, shrink on failure, restart on success) is the same, but KiCad calls `line.Simplify2()` first (:552) and ours does not. Probe: 20000 random 45-degree staircases (3-7 legs, with collinear runs and back-tracks): ours differs from the KiCad-faithful re-implementation on 6279 (31%); with only the *pre*-simplify added to the KiCad model 4185 (21%) differ from ours, with only the *per-candidate* simplify 3370 (17%); with neither, 81 (0.4%, residual of my re-implementation). Cause: corner costs are compared on un-simplified chains, so collinear points count as 5-cost corners. |
| OPTIMIZER::mergeStep | pns_optimizer.cpp:683-751 | optimizer.rs:82-109 | DIVERGENT | Same candidate construction (bypass from `s1.A` to `s2.B` with start-diagonal false/true). Differences: (a) candidates are not `Simplify2`'d before costing (:735) -- see mergeFull; (b) KiCad collision-checks only the bypass (`checkColliding( aLine, bypass )`), ours checks the whole candidate (stricter: a pre-existing violation elsewhere on the line blocks every merge); (c) tie-break: KiCad prefers `path[1]` (diagonal-first) when `cost[0] == cost[1] < cost_orig`, ours keeps the first (straight-first); no ties occurred in the 20000-sample probe (the flag changed 0 results), so the impact is theoretical; (d) corner mode is hard-coded `Mitered45` (settings has no corner mode), (e) no `checkConstraints`. |
| OPTIMIZER::mergeObtuse | pns_optimizer.cpp:467-542 | optimizer.rs:151-195 | FAITHFUL | Same `step = PointCount-3`, clamp, `step < 2 -> done`, obtuse test, `IntersectLines`, second obtuse test, replace `n+1..n+step` by `ip`. Collision test is on the whole candidate (stricter, as above) and `ip` is truncated rather than rounded (exact for 45-degree families). |
| OPTIMIZER::mergeColinear | pns_optimizer.cpp:584-607 | optimizer.rs:262-274 | DIVERGENT (minor) | KiCad advances `segIdx` after a removal, so a run of 4 collinear points leaves the second one; ours re-examines the same index and collapses the run. KiCad's `Collinear` has a +/-1 tolerance, ours exact. |
| OPTIMIZER::Optimize( LINE*, LINE*, LINE* ) | pns_optimizer.cpp:609-681 | optimizer.rs:238 `optimize_with` | ADAPTED | Pass order (merge segments, obtuse, colinear, smart pads, fanout) is the same. `LIMIT_CORNER_COUNT` is a no-op in KiCad (always returns true, :287-315) so omitting it is faithful; `KEEP_TOPOLOGY`, `PRESERVE_VERTEX`, `RESTRICT_AREA` MISSING. Ours always ends with an unconditional `out.simplify()` (:256). `optimize()` (the default entry, :222) runs MERGE_SEGMENTS **and** MERGE_OBTUSE; KiCad never combines them (OE_MEDIUM/FULL = SEGMENTS only, OE_LOW = OBTUSE only, pns_shove.cpp:2024-2051, pns_line_placer.cpp:758-767, :970-980). |
| OPTIMIZER::Optimize( LINE*, effort, NODE*, v ) | pns_optimizer.cpp:1092 | line_placer.rs:49 `head_effort` | ADAPTED | `OptimizerEffort` is never read (settings.rs:34-40 exists, no consumer): always `MERGE_SEGMENTS (+SMART_PADS)`, i.e. OE_MEDIUM. `SMART_PADS` is also applied after the user flipped posture; KiCad suppresses it when `m_mouseTrailTracer.IsManuallyForced()` (pns_line_placer.cpp:775, :989). |
| OPTIMIZER::smartPadsSingle | pns_optimizer.cpp:944-1063 | optimizer.rs:377-441 | FAITHFUL | Checked: via/offset-pad early outs, `p_end = min(aEndVertex, min(3, n-1))`, contained-in-pad skip, breakouts x `diag` (true then false), forbidden-angle test of the breakout/connect joint, `breakout.Length() > line.Length()`, `CountCorners( forbidden ) == 0`, variant list, final tie rule (`cost < min` or (`cost == min` and `len > max_len`); `max_length` updated only when `cost <= min_cost`). Offset pads do not exist in the IR (ADAPTED). |
| OPTIMIZER::runSmartPads | pns_optimizer.cpp:1065-1090 | optimizer.rs:444-460 | FAITHFUL | Start then end, `vtx < 0 ? n-1 : n-1-vtx`, trailing `Simplify2` (our `simplify`). |
| rectBreakouts | pns_optimizer.cpp:822-876 | optimizer.rs:308-333 | FAITHFUL | 4 orthogonal + 4 diagonal rays, same `d_offset`, same `l`, same ordering for `s.x >= s.y` and the `else` case. |
| circleBreakouts | pns_optimizer.cpp:753-774 | optimizer.rs:341-347 | FAITHFUL | 8 rays of length `r*sqrt2`, same angular order (KiCad `RotatePoint( v0, -angle )` in y-down = our +angle); rounding order differs by <= 1 um. |
| customBreakouts | pns_optimizer.cpp:776-820 | optimizer.rs:348-361 | ADAPTED | 8 rays from the pad position to the first polygon edge hit; ray length `max(w,h)/2 + 5` in KiCad vs `+ 1` here (um, still reaches the polygon). |
| computeBreakouts | pns_optimizer.cpp:878-925 | optimizer.rs:305-365 | DIVERGENT | Segment pads -> `ApproximateSegmentAsRect` ok; but an axis-aligned rounded-rect pad is `SHAPE_SIMPLE` in KiCad (see section A) and gets `customBreakouts`, while ours returns `Vec::new()` for `Shape::RoundRect` (`_ => Vec::new()`, comment at :304 asserts the opposite). Smart pads therefore never reshape the exit from rounded-rect pads (the default footprint pad style since KiCad 7). D14. |
| fanoutCleanup | pns_optimizer.cpp:1107-1155 | optimizer.rs:465-485 | FAITHFUL | `i` as start-diagonal (false, true), `len < 10*width`, `endMatch` rules. Not called by the placer (KiCad calls it from `optimizeTailHeadTransition`, which we do not have). |
| findPadOrVia | pns_optimizer.cpp:927 | optimizer.rs:368 | FAITHFUL | Joint lookup by (pos, net); ours additionally filters by layer overlap. |
| checkColliding / m_collisionKindMask | pns_optimizer.cpp:431-465 | optimizer.rs:68 | ADAPTED | `Node::first_colliding` with the 1 um epsilon (see E). |
| n_passes (2 for MEDIUM/FULL), `m_optimizerQueue`, area restriction | pns_shove.cpp:2013-2121 | -- | MISSING | Part of runOptimizer. |
| DP optimizer (`mergeDpStep`, `mergeDpSegments`, `coupledBypass`, ...), `Tighten` | pns_optimizer.cpp:1157-1539 | -- | MISSING | No coupled-pair or meander tighten support (the diff-pair placer is a different design, section I). |

### E. Node, items, joints, index (`node.rs`, `item.rs`, `joint.rs`, `index.rs`, `from_ir.rs`; KiCad `pns_node.cpp`, `pns_item.cpp`, `pns_joint.h`, `pns_index.cpp`, `pns_kicad_iface.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| clearance epsilon | pns_kicad_iface.cpp:824-825, board_design_settings.cpp:1833, advanced_config.cpp:245 (0.0005 mm) | node.rs:35 `CLEARANCE_EPSILON = 1`, :373 | ADAPTED | KiCad subtracts 0.5 um (and only when `useClearanceEpsilon`, `rv > 0`); ours subtracts 1 um (the nearest whole um). Applied in the same places (`all_colliding`) and not applied to hull clearances/`getClearance` (also same as KiCad). Consequence: this project's DRC runs with epsilon **0** (drc/kimath.rs:20-23), so a router result accepted at a 199 um edge gap is a DRC violation in this project but would not be in KiCad (whose DRC uses the same 0.5 um). Observable in probe c (via at 199). KiCad also takes 1 nm off the shape test (`clearance + wH + wI - 1`, pns_item.cpp:241,272) -- ours uses a strict `<` instead. |
| NODE::GetClearance | pns_node.cpp:143 | node.rs:341 | ADAPTED | Net-class lookup through `eda_drc::constraints::clearance`; no custom-rule resolver, keepouts, net ties, castellation, free pads, flashed-layer checks (pns_item.cpp:166-214). |
| NODE::Branch / Commit | pns_node.cpp:157, :1622 | node.rs:85 | ADAPTED | Full clone instead of copy-on-write overlay (documented, PARITY.md decision 1); semantics preserved. |
| NODE::addSegment/addVia/addSolid + touchJoint | pns_node.cpp:601-760, :1393 | node.rs:123-153 | ADAPTED | One joint per `(pos, net)` instead of one per non-overlapping layer range; only wrong for blind/buried vias, which the IR cannot express. |
| NODE::Add( LINE ) / findRedundantSegment | pns_node.cpp:683, :1707 | node.rs:317 `add_line` | ADAPTED | KiCad links to an existing identical segment instead of adding a duplicate (`aAllowRedundant=false`); ours always adds. Duplicate segments can appear when a shoved/optimized line retraces an existing one. |
| NODE::Remove / doRemove / rebuildJoint | pns_node.cpp:809-1072 | node.rs:162 | ADAPTED | Empty joints are pruned (KiCad leaves them). Harmless. |
| NODE::followLine | pns_node.cpp:1074-1127 | node.rs:225-255 | DIVERGENT | KiCad stops when it returns to its start anchor (`count && guard == p`, :1100-1107) and caps at `MaxVerts = 16384` (:1131); ours has neither: **infinite loop on a closed same-net loop** (D2). Also stops at locked joints/segments (not modelled). |
| NODE::AssembleLine | pns_node.cpp:1129-1214 | node.rs:214-270 | DIVERGENT | Segments whose width differs from the seed segment are dropped from the assembled line (`!aAllowSegmentSizeMismatch`, :1176); ours includes them and later re-emits the whole chain at the seed's width (D10). Duplicate removal ok. |
| JOINT::NextSegment | pns_joint.h (NextSegment) | node.rs:186 `next_segment` | ADAPTED | Same "any solid/via at the joint is a hard stop" and "exactly one candidate" rules; KiCad additionally refuses locked candidates when not `aAllowLockedSegs` and ignores virtual vias. |
| JOINT::IsLineCorner / IsNonFanoutVia / IsStitchingVia / IsTrivialEndpoint / IsTraceWidthChange / Lock | pns_joint.h | joint.rs | MISSING | `Joint` is data only (links + layers); `LockJoint` (pns_node.cpp:1386) used by SHOVE::Run to pin head endpoints is not ported. |
| NODE::NearestObstacle | pns_node.cpp:298-476 | walkaround.rs:168 `first_obstacle`, shove.rs:279 | ADAPTED | KiCad: obstacles from `QueryColliding` per segment (+ via), hull per obstacle, nearest by `PathLength` of the first valid `HullIntersection`. Ours: first colliding leg, nearest by the collision point's distance from that leg's start (walkaround), or smallest edge gap (shove). Documented as "narrowed"; same answer for isolated obstacles, different order for dense ones. |
| NODE::QueryColliding / CheckColliding | pns_node.cpp:267, :478-570 | node.rs:352 `all_colliding`, :387 | ADAPTED | Same broad phase (index inflated by worst-case clearance) + exact shape test; KiCad's filter/kind-mask options are reduced to net, layer and an exclude list. |
| ITEM::collideSimple | pns_item.cpp:104-292 | node.rs:352-383 | ADAPTED | Same-net rule matches (`Net == Net && Net != 0`); hole-to-copper, keepout, net-tie, free-pad, unflashed-layer branches absent (IR has none of these). |
| NODE::FindLinesBetweenJoints | pns_node.cpp:1223 | node.rs:282 `find_lines_between_joints` | ADAPTED | Scans every segment of the net and assembles it (O(n) per call) instead of walking from joint `aA`; combined with D2 this is how a finishing route can hang. KiCad clips the line to the vertex range; ours requires exact endpoints. |
| NODE::SplitAdjacentSegments (LINE_PLACER) | pns_line_placer.cpp:1292 | -- | MISSING | A route cannot start or end on the middle of an existing same-net segment (ours snaps to anchors only, `nearest_anchor`, node.rs:415). |
| INDEX (per-layer sub-indices, `m_netMap`) | pns_index.cpp:28-120 | index.rs | ADAPTED | One uniform grid, layer filtering after the query; `GetItemsForNet` has no counterpart. Same semantics (bbox overlap, caller inflates). Cell = 5000 um. |
| ITEM kinds `ARC_T`, `HOLE_T`, `DIFF_PAIR_T`, `LINE_T` as item | pns_item.h | item.rs:106 | ADAPTED | Closed enum {Solid, Segment, Via}; `LINE` exists as `line.rs::Line` (an assembled polyline, not an item). Justified by the IR. |
| PNS_KICAD_IFACE::syncPad / syncTrack / syncVia | pns_kicad_iface.cpp:1474-1700 | from_ir.rs:31 | ADAPTED | Reuses `eda_drc::board::build`; `Segment.locked` is always `false` (from_ir.rs:54: no lock flag is read), so the "locked track" paths in shove/remove_loops can never trigger from real boards. Pad shape rules: see section A. |

### F. Walkaround (`walkaround.rs`; KiCad `pns_walkaround.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| WALKAROUND::Route / singleStep | pns_walkaround.cpp:303-394, :94-301 | walkaround.rs:180-227 | ADAPTED | KiCad hugs a whole `AssembleCluster` per iteration with `HullCache( item, GetClearance( .., false ), width )`, `Simplify2` before each walk, per-policy status, length-expansion cut-off (`m_lengthLimitOn`), `ST_ALMOST_DONE` when the end point moved. Ours: one obstacle per iteration, both windings as plain candidates, no length cut-off, a path ending short of the target (cursor inside a hull) is returned as success. **The 40-iteration budget (`WalkaroundIterationLimit`, same constant) is therefore spent per item, not per cluster**: a row of >40 overlapping pad hulls that KiCad crosses in a handful of iterations exhausts it here. |
| WALKAROUND::nearestObstacle | pns_walkaround.cpp:50 | walkaround.rs:168 | ADAPTED | `m_useClearanceEpsilon = true` is reproduced by `all_colliding`. |
| WP_SHORTEST arbitration (CW/CCW both processed, check-back against processed items) | pns_walkaround.cpp:220-290 | walkaround.rs:204 `best()` | ADAPTED | Pick the shorter of two independently computed results; no check-back. |
| `RestrictToCluster`, solids-only mask, `SetItemMask` | pns_walkaround.cpp:71-92 | -- | MISSING | Needed by SHOVE (`onCollidingSolid`) and by `rhShoveOnly`'s solids-only pre-pass, which is why D5 exists. |
| `m_PNSProcessClusterTimeout` | pns_walkaround.cpp:161 | -- | MISSING | No wall-clock bound. |

### G. Line placer (`line_placer.rs`; KiCad `pns_line_placer.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| LINE_PLACER::rhWalkBase / rhWalkOnly | pns_line_placer.cpp:560-746, :748-817 | line_placer.rs:209-218 | ADAPTED | KiCad walks `tail+head`, optimizes both windings, falls back to the *hug-the-cursor* heuristic (`cursorDistMinimum`, `WalkaroundHugLengthThreshold` 1.5) when the result is longer than 2x1.5x the direct path, splits head/tail. Ours takes the shorter winding unconditionally; `walkaround_hug_length_threshold` is never read. |
| LINE_PLACER::rhShoveOnly | pns_line_placer.cpp:932-1023 | line_placer.rs:181-199 | DIVERGENT | KiCad first runs `rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove )` (walk the head around **pads only**), then shoves that line. Ours skips the pre-pass and feeds the raw 45-degree head to `shove_line`, which gives up on the first pad (D5). |
| LINE_PLACER::rhMarkObstacles | pns_line_placer.cpp:819-869 | line_placer.rs:202-208 | DIVERGENT | KiCad snaps the head to the nearest obstacle hull when the cursor is within `width/2` of it ("route as tightly as possible"); ours returns the raw trace flagged colliding. |
| LINE_PLACER::routeHead / routeStep / route | pns_line_placer.cpp:1025-1236 | line_placer.rs:173 `build_head`, :255 `preview` | ADAPTED | `reduceTail`, `handlePullback`, `handleSelfIntersections`, `mergeHead`, `optimizeTailHeadTransition`, `MOUSE_TRAIL_TRACER` are not ported (stateless request model, documented at line_placer.rs:1-45). Observable: a head that loops back across an earlier fixed run keeps the loop; auto-posture never flips. |
| LINE_PLACER::buildInitialLine | pns_line_placer.cpp:2031-2120 | line_placer.rs:178 | ADAPTED | `direction.build_initial_trace( start, p, false, Mitered45 )` = KiCad's `m_direction.BuildInitialTrace` for the non-empty-tail case; the first leg uses `guessedDir` (auto posture) in KiCad. Via placement uses `VIA::PushoutForce` (iterative, `ViaForcePropIterationLimit` 40 -- the setting is unused); ours: `place_via` ring search (line_placer.rs:227). |
| LINE_PLACER::Start | pns_line_placer.cpp:1385-1445 | line_placer.rs:143 | ADAPTED | Ours seeds the posture from the clicked segment's direction; in KiCad `lastSegDir`/`initialDir` are computed but only used in a debug message (the tracer is seeded with `m_initial_direction`), so KiCad does not continue the segment's direction. Minor posture difference on the first leg. |
| LINE_PLACER::FixRoute | pns_line_placer.cpp:1548-1750 | line_placer.rs:303 `fix`, :390 `finish` | DIVERGENT | The collision guard is `!AllowDRCViolations()` where `AllowDRCViolations() = (mode == MarkObstacles && can_violate_drc)` (pns_routing_settings.h:117) and `can_violate_drc` defaults to **false**; ours commits a colliding head whenever `mode == MarkObstacles` (line_placer.rs:280-282) (D8). `simplifyNewLine` (merge collinear) is `head.simplify()`; `m_fixedTail`/springback not needed; `SplitAdjacentSegments` MISSING. |
| LINE_PLACER::removeLoops | pns_line_placer.cpp:1821-1885 | line_placer.rs:435 | ADAPTED | Runs once at `finish` and only for the two outer endpoints (documented); KiCad runs it on every `Move` over each internal joint. |
| LINE_PLACER::UnfixRoute / FlipPosture / ToggleVia / SetLayer | pns_line_placer.cpp:1752, :1267, :95, :1352 | line_placer.rs:355, :371, :376, :326 | ADAPTED | `switch_layer` fixes the head and starts a new run (see D1 for the commit side). `SMART_PADS` ignores `manually_forced` (D17). |
| `FixAllSegments`, `FollowMouse`, `AutoPosture`, `JumpOverObstacles` | pns_routing_settings.cpp:41-56 | settings.rs:49-77 | MISSING | `fix_all_segments`, `jump_over_obstacles`, `shove_vias`, `optimizer_effort`, `walkaround_hug_length_threshold`, `via_force_prop_iteration_limit` exist as fields but are never read by any module. |

### H. Dragger (`dragger.rs`; KiCad `pns_dragger.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| DRAGGER::Start / startDragSegment | pns_dragger.cpp:205, :112 | dragger.rs:108 | DIVERGENT | KiCad: grab within `width/2` of an end -> `DM_CORNER`, anywhere else on the segment -> **`DM_SEGMENT`** (slide the whole segment, adjacent legs re-solved at 45). Ours always drags the nearer end vertex (module doc says so). The default gesture "grab the middle of a track" does something else (D7). |
| LINE::DragCorner / dragCorner45 / DragSegment | pns_line.cpp:771-1000 | dragger.rs:132-138 | DIVERGENT | KiCad re-solves the adjacent legs to keep 45-degree geometry (and snaps within `width/4..w/2`); ours moves the vertex to the cursor (free angle), producing arbitrary-angle legs. |
| DRAGGER::dragMarkObstacles | pns_dragger.cpp:282 | dragger.rs:175-194 | ADAPTED | Collision only reported. Commit gate: see D8. |
| DRAGGER::dragWalkaround / tryWalkaround | pns_dragger.cpp:527, :503 | -- | MISSING | In ours `Mode::Walkaround` (the **default** mode) is treated as MarkObstacles, and `finish` returns `None` when colliding in any mode but MarkObstacles (dragger.rs:242), so a default-mode drag onto any obstacle is simply refused (D7). |
| DRAGGER::dragShove | pns_dragger.cpp:591 | dragger.rs:196-235 | ADAPTED | Shove of the dragged line against a read-only node, legs processed independently; `SHP_DONT_LOCK_ENDPOINTS` (no locks here anyway), `SHP_REVERSED` for corner index 0 MISSING (checkShoveDirection row); `DisablePostShoveOptimizations`, post-shove `optimizeAndUpdateDraggedLine` MISSING. |
| DRAGGER::dragViaMarkObstacles / dragViaWalkaround / propagateViaForces | pns_dragger.cpp:334, :374, :56 | dragger.rs:139-157 | MISSING | A dragged via moves exactly to the cursor with its fan-out end points; no via force propagation, no walkaround retry after a failed shove. |
| DRAGGER::FixRoute / Drag (restore last valid point) | pns_dragger.cpp:703, :744 | dragger.rs:240 | ADAPTED | KiCad re-runs the drag at `m_lastValidPoint` on release over a collision and commits that; ours refuses. Needs client-side last-valid-point tracking to match. |

### I. Diff pair (`diff_pair.rs`; KiCad `pns_diff_pair_placer.cpp`, `pns_diff_pair.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| BOARD::MatchDpSuffix | pcbnew/board.cpp:2625 | diff_pair.rs:66 `dp_coupled_net_name` | FAITHFUL | Same scan (digits and `_` skipped), same `count` bookkeeping (`Left( len-count ) + comp + Right( count-1 )`), same '+', '-', 'N', 'P' complement table. |
| SIZES_SETTINGS defaults (125 um width, 180 um gap) | pns_sizes_settings.h:51-52 | model/src/lib.rs:650-657 | FAITHFUL | |
| DIFF_PAIR_PLACER::Start / FindDpPrimitivePair | pns_diff_pair_placer.cpp:623, :515 | diff_pair.rs:221 | ADAPTED | Pair found by net-name suffix and nearest anchor within 20 mm instead of `FindDpPrimitivePair` (DP_PRIMITIVE_PAIR from the start item). |
| DIFF_PAIR::BuildInitial / CoupledSegmentPairs / DP_GATEWAY | pns_diff_pair.cpp:208, :834 | diff_pair.rs:105 `offset_polyline` | ADAPTED | Spine + perpendicular offset instead of building a real DIFF_PAIR from gateways; no `DIFF_PAIR_T` item, no coupled hulls. Gap/width are honored; the pair is not corner-coupled the way KiCad's `DIFF_PAIR::Hull`-based walk keeps it. |
| DIFF_PAIR_PLACER::rhWalkOnly / rhShoveOnly / attemptWalk / tryWalkDp / propagateDpHeadForces / rhMarkObstacles | pns_diff_pair_placer.cpp:104-405 | diff_pair.rs:271-316 | MISSING | Collision is only reported (documented). In every mode the pair is refused when either line collides (diff_pair.rs:326). |
| DIFF_PAIR_PLACER via / layer switch | pns_diff_pair_placer.cpp:74, :93, :437 | -- | MISSING | Single layer only. |
| DIFF_PAIR_PLACER::FixRoute / UnfixRoute / FlipPosture | pns_diff_pair_placer.cpp:809, :419 | diff_pair.rs:324, :342, :358 | ADAPTED | Same session shape as the single-track placer. |
| DIFF_PAIR::Skew / CoupledLength / TotalLength (tuning) | pns_diff_pair.cpp:828-940 | -- | MISSING | No skew/length readout or DP tuning. |

### J. Meander / length tuning (`meander.rs`; KiCad `pns_meander*.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| MEANDER_SHAPE::genMeanderShape / uShape / miter / makeMiterShape | pns_meander.cpp:524, :514, :496, :409 | meander.rs:104 `generate_meander` | DIVERGENT | KiCad builds U-shaped meanders (90-degree legs, rounded or chamfered corners, `MEANDER_SETTINGS`: min amplitude 0.2 mm, max 1.0 mm, spacing 0.6 mm, corner radius 80%, pns_meander.cpp:40-60). Ours builds a 45-degree accordion: each period steps diagonally out by `amplitude`, runs `spacing`, steps back. The extra-length arithmetic checks out (`2*amp*(sqrt2-1)` per period, verified by reading, and measured by the module's own test) but the shape is not KiCad's. |
| MEANDER_SHAPE::Fit / Resize / Recalculate | pns_meander.cpp:662, :775, :762 | meander.rs:138-170 | ADAPTED | Last period shrinks its amplitude to hit the target; KiCad resizes within `[min, max]` amplitude and checks `CheckFit` against the node (ours leaves collision checking to the caller). |
| MEANDER_PLACER::Start / Move / doMove / tuneLineLength | pns_meander_placer.cpp:66-300, pns_meander_placer_base.cpp:173 | -- | MISSING | Interactive session, multi-corner baselines, `1/2/3/4` step keys, tolerance, pad-to-die, time-domain targets; ours is one-shot over an axis-aligned two-point baseline. Documented. |

### K. Router session (`router.rs`; KiCad `pns_router.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| ROUTER::StartRouting / isStartingPointRoutable | pns_router.cpp:433, :221 | router.rs:119 | ADAPTED | Must start on an existing anchor with a net (KiCad also allows free start). `isStartingPointRoutable` has no analogue. |
| ROUTER::Move / movePlacing / moveDragging | pns_router.cpp:493, :777, :655 | router.rs:140, :298 | ADAPTED | Pure preview from the committed node; `markViolations`/`updateView` replaced by a `colliding` flag. |
| ROUTER::FixRoute / ToggleViaPlacement / SwitchLayer | pns_router.cpp:903, :1007, :998 | router.rs:165-186 | ADAPTED | Layer switch folded into the next fix via `pending_via_layer`. |
| ROUTER::CommitRouting (node diff -> board items) | pns_router.cpp:850 | router.rs:216 `build_commit` | DIVERGENT | Reconstructs vias from run boundaries instead of recording them; emits a 1 um via at every same-layer junction (D1). Displaced tracks/vias handling is fine. |
| ROUTER::UndoLastSegment / StopRouting / FlipPosture | pns_router.cpp:934, :955, :989 | router.rs:189, :194, :149 | ADAPTED | |
| ROUTER::Finish / ContinueFromEnd / GetNearestRatnestAnchor / BreakSegmentOrArc | pns_router.cpp:568, :616, :517, :1094 | -- | MISSING | No suggest-finish, no continue-from-end, no ratsnest-anchor lookup, no break-track command. |
| ROUTER::StartDragging | pns_router.cpp:159-219 | router.rs:287 | ADAPTED | Hit test by shape (`Node::item_at`); see section H for what is then dragged. |

### L. `DIRECTION_45` (`direction45.rs`; KiCad `libs/kimath/include/geometry/direction45.h`, `src/geometry/direction_45.cpp`)

| KiCad function | KiCad file:line | Ours | Status | Note |
|---|---|---|---|---|
| DIRECTION_45 ctor / construct_ | direction45.h:76-90, :230 | direction45.rs:63-90 | FAITHFUL | Octant quantization (+/-22.5 deg), zero vector -> UNDEFINED, y flip handled (labels are cosmetic, relations are identical). |
| Angle / IsObtuse / Right / ToVector | direction45.h:123-200 | direction45.rs:100-156 | FAITHFUL | Table 1,7 obtuse / 2,6 right / 3,5 acute / 4 half-full / 0 straight; same results as the mod-8 distance. |
| BuildInitialTrace (MITERED_45) | direction_45.cpp:24-70 (w/h shortcut, `mp0`/`mp1`, `startDiagonal ? mp1 : mp0`) | direction45.rs:132-148 | FAITHFUL | Elbow positions identical (checked algebraically for `w>h` and `h>=w`). A zero-length request returns two identical points where KiCad's `Append` suppresses the duplicate. |
| BuildInitialTrace (MITERED_90) | direction_45.cpp (is90mode branch) | direction45.rs:149-157 | DIVERGENT (unreachable) | KiCad picks horizontal-first when `startDiagonal == (h >= w)` and does not shortcut `w == h`; ours uses `start_diagonal` / `w >= h` directly. No call site passes `Mitered90` (grep), so latent only. |
| BuildInitialTrace (ROUNDED_45 / ROUNDED_90), `CORNER_MODE` setting | direction_45.cpp | -- | MISSING | No arcs (documented). The router has no corner-mode setting; optimizer and placer hard-code MITERED_45. |

### M. Constants and tolerances

| Constant | KiCad | Ours | Status | Note |
|---|---|---|---|---|
| `c_ENDPOINT_ON_HULL_THRESHOLD` | 1000 nm (`dist < 1000`, pns_shove.cpp:330) | 1.0 um, strict `<` (shove.rs:99,166) | ADAPTED | On an integer grid only distance 0 qualifies. Equivalent to "exactly on the hull". |
| `cHullFailureExpansionFactor` | 1000 nm per attempt (pns_shove.cpp:526) | 1 um (shove.rs:101,241) | FAITHFUL | Plus the global +1 guard (hull.rs:208). |
| kink threshold | `aClearance / 10` (pns_utils.cpp:181) | `clearance / 10` (hull.rs:273) | FAITHFUL | Scale-free in the ratio; the integer division now acts on um (rounds 1000x more coarsely: clearance 205 -> 20 um vs 20.5 -> 20 in nm-scaled terms). |
| shove iteration limit | 250, `m_iter >= limit` (pns_routing_settings.cpp:46) | 250, `iterations > limit` (settings.rs:88, shove.rs:271) | FAITHFUL | Off by one. |
| shove time limit | 1000 ms | -- | MISSING | |
| walkaround iteration limit | 40 (`SetIterationLimit( Settings().WalkaroundIterationLimit() )`) | 40 | FAITHFUL | But counted per item, not per cluster (section F). |
| `Walkaround` loop guard | `iterLimit = 1000` (pns_line.cpp:451) | 1000 (line_walk.rs:296) | FAITHFUL | |
| clearance epsilon | 500 nm | 1000 nm | ADAPTED | See section E. |
| DRC-style tolerances (`Contains` 3, `Collinear` 1, `PointOnEdge` +1, `Find` 1, `Simplify2` 1) | nm | copied as um | DIVERGENT where inconsistent | See B and D3. |
| smart-pad vertex window | 3 (pns_optimizer.cpp:1081) | 3 (optimizer.rs:453) | FAITHFUL | |
| `customBreakouts` ray length pad | +5 nm | +1 um | ADAPTED | |
| SIZES_SETTINGS diff-pair width/gap | 125000 / 180000 nm | 125 / 180 um | FAITHFUL | |
| meander defaults (min amp / max amp / spacing) | 0.2 / 1.0 / 0.6 mm | dialog-supplied | ADAPTED | |
| `MaxVerts` in AssembleLine | 16384 | none | MISSING | D2. |

## Divergences to fix

Ordered by impact. "Repro (run)" means the input was executed against the current code in the scratch probe crate.

### D1. Bogus 1 um vias at every intermediate click (router.rs:230-243) -- HIGH, data corruption

`Router::build_commit` reconstructs vias from run boundaries: for every window of two consecutive runs with `w[0].last() == w[1].first()` it pushes a `Via` with `diameter: via_diameter.max(1)`, `drill: via_drill.max(1)`. But `LinePlacer::fix` (line_placer.rs:303-318) starts every next head at the previous run's last point on the **same layer**, so every plain intermediate click satisfies the condition.

Repro (run): two-pad board from `router.rs` tests; `start(-825,0)`, `fix((2000,0))`, `fix((2000,1000))`, `finish((5175,0))`. Result: 3 tracks (correct) **and** `vias = [Via{ at:(2000,0), drill:1, diameter:1, F.Cu->F.Cu }, Via{ at:(2000,1000), drill:1, diameter:1, F.Cu->F.Cu }]`. The only existing tests use a single run, which is why this was missed.

Fix: record vias where they are created (`LinePlacer::switch_layer` knows position, diameter, drill and both layers) and emit exactly those; at minimum require `w[0].layer != w[1].layer`.

### D2. `Node::assemble_line` does not terminate on a closed same-net track loop (node.rs:225-255) -- HIGH, hang + unbounded memory

KiCad's `followLine` has `guard` (stop when the walk returns to the start anchor, pns_node.cpp:1100-1107) and `MaxVerts = 16384` (:1131). Ours loops `while let Some(joint) ... next_segment` with neither: in a ring of same-net segments every joint has exactly one other candidate, so `fwd_pts` grows forever.

Repro (run): four same-net segments (0,0)-(1000,0)-(1000,1000)-(0,1000)-(0,0) on one layer, `node.assemble_line( first )` -> still running after 10 s (killed by `timeout`). Reachable from `LinePlacer::finish` (`remove_loops` -> `find_lines_between_joints` assembles *every* segment of the net, node.rs:285-293), `shove_line`, `Dragger::start`, so one GND ring anywhere on the net hangs a route finish on the HTTP worker.

Fix: port the guard (return to start anchor -> stop and set `guard_hit`, skip the backward walk) and a vertex cap; also track visited segment ids.

### D3. Dropped hull crossing: `Seg::intersect` truncates + `split` tolerance too tight (drc/kimath.rs:176-178, line_walk.rs:147) -- HIGH, wrong geometry

KiCad rounds the crossing point (`rescale`, round-half-away, anchored on the other segment, seg.cpp:416-423) and `SHAPE_LINE_CHAIN::Split` accepts a point up to squared distance 3 from a segment (`Distance < 2`, shape_line_chain.cpp:1203-1210). Ours truncates toward zero (up to ~1 um per axis, anchored on `self`) and accepts only squared distance <= 1. After the entry crossing is inserted the path's sub-segment is already ~0.5 um off the true line, so the exit crossing, off by another ~0.9 um, fails `split` and is **never inserted into `pnew`**. `LINE::Walkaround` then sees `start (OUTSIDE) -> entry (ON_EDGE) -> end (OUTSIDE)` and jumps from the entry straight to the end, through the hull.

Repro (run): `line = [(403,5139), (9992,5639)]`, `hull = [(5157,5508),(4849,5386),(4717,5082),(4839,4774),(6502,4054),(6810,4176),(6942,4480),(6820,4788)]` (a SegmentHull): `Seg::intersect` finds both crossings, (4843,5371) and (5407,5399), but `walkaround(.., cw)` returns `[(403,5139), (4843,5371), (9992,5639)]` for both windings (the exit crossing is the one lost). Statistics: 20000 random chords x 2 windings (hulls = via octagons and segment hulls, 2- and 3-point lines): the returned path enters the hull interior in 3801/40000 (9.5%) with the code as ported; **0/40000 with only the rounding intersect substituted**; 5/40000 with only the `Split` tolerance changed to squared <= 3; 0 with both. `None` was never returned, i.e. the failure is silent. Downstream the bad path is re-detected as colliding by `first_obstacle`, so walkaround spends its 40 iterations and reports "stuck", and `shove_line_to_hull_set` rejects the attempt in `lines_collide` -- so the symptom is a spurious "no route / shove failed" on roughly one crossing in ten.

Fix: in `eda_drc::kimath::Seg::intersect` compute `aSeg.A + rescale( param1, dir2, det )` with KiCad's rounding rescale (and add the collinear-overlap branch); set the `split` acceptance to squared distance <= 3 (and, if any caller needs it, port `Find`'s rounded-norm test and `Contains` <= 3, `Collinear` |det| <= 1). Keep the probe's chord test as a regression test.

### D4. `convex_hull_octagon` diagonals are too close to the pad (hull.rs:379-393) -- HIGH for rotated/trapezoid pads

KiCad's `MoveDiagonal` gets `dist` from `NearestPoint( SEG )` = min over polygon vertices of `SEG::LineDistance` (infinite line). Ours takes the minimum **segment-to-segment** distance between the finite diagonal (length `bh*sqrt2`) and the polygon edges, truncated. When the nearest vertex projects beyond the finite diagonal, ours reports a larger distance, moves the diagonal inward by too much and the hull ends up inside the required clearance.

Repro (run): 1.0 x 0.6 mm rectangle pad rotated by 60 degrees (polygon, as `rotated_pad_shape` produces for non-90-degree footprints), `cl = 300`: our hull is **236.2 um** from the pad (shortfall 63.8 um). By angle (shortfall in um): 10 deg 5.8, 20 deg 17.9, 30 deg 32.7, 50 deg 36.2, 55 deg 58.2, 60 deg 63.8, 70 deg 41.2, 85 deg 3.7; 0 and 45 deg exact. 20000 random rotated rectangles, `cl` 150-450: KiCad-reference hull never closer than `cl-1` (0 cases), ours closer in 17689 (88%), worst shortfall 153.7 um. A path that hugs this hull violates clearance by up to that amount; `all_colliding` then flags it and walkaround/shove cannot converge near such pads. Trapezoid and chamfered pads take the same path.

Fix: replace `seg_dist` by `vertices.iter().map( |v| line_distance( diag, v ) ).min()` with KiCad's `LineDistance` (floor `isqrt` of the rounded squared distance, seg.cpp:746-763). The probe contains a reference implementation.

### D5. Shove mode gives up at the first pad and has no solids-only pre-pass (shove.rs:284, line_placer.rs:181-199) -- HIGH, shove mode degenerates to walkaround

KiCad's `rhShoveOnly` walks the head around solids only (`rhWalkBase( .., ITEM::SOLID_T, RM_Shove )`, pns_line_placer.cpp:937), then shoves tracks/vias; inside the shove, a colliding *current* line is re-walked around the solid (`onCollidingSolid`, pns_shove.cpp:776). Ours: `Some(Item::Solid(_)) => return None` (shove.rs:284) and no pre-pass, so any pad within reach of the head (or of a shoved line) aborts the whole shove and `build_head` falls back to a full walkaround that no longer shoves anything.

Repro (run): SIG head (0,0)-(5000,0); GND pad circle r=500 at (1500,0) and a GND vertical track at x=3500 (width 200, clearance 200). `shove_line` returns `None` (KiCad: head walks around the pad, the track is shoved). With only the track, shove works (returns the 8-point wrap-around track of probe b2).

Fix: implement the pre-pass (`walkaround::route` restricted to `Item::Solid`) before `shove_line`, then `onCollidingSolid` (re-walk the colliding line around the solid cluster) inside the loop; also use KiCad's obstacle order (solid, via, segment) in `shoveIteration` instead of smallest gap.

### D6. Via push distance (shove.rs:288-302) -- MEDIUM

KiCad pushes by the MTV (`clearance + w/2 + r - dist`), epsilon-free. Ours: `max(needed - actual, 1) + diameter/2` with `needed` already reduced by the 1 um epsilon and `actual` clamped at 0.

Repro (run): via (600 dia) at (2500, 100) vs track (0,0)-(5000,0) width 200 clearance 200: result (2500, 599): centre-to-centreline 599, edge gap 199 < 200 (violation of 1 um under this project's DRC, epsilon 0). Via exactly on the centreline (2500, 0): first iteration pushes 499, the loop pushes again, final (2500, 899) where KiCad ends at 600 (299 um of needless displacement of a neighbour's via).

Fix: compute the MTV from the shapes with the un-reduced clearance (`Node::clearance`) and `+1` um margin, and add the "do not land on an existing joint" loop. Re-shape the attached fan-out with a 45-degree `DragCorner` instead of replacing the end vertex (shove.rs:319-326), otherwise the fan-out tracks leave at arbitrary angles.

### D7. Dragger default behaviour (dragger.rs:108-147, 175-194, 242) -- MEDIUM

1. Default mode is Walkaround; drag treats it as MarkObstacles and `finish` refuses when colliding, so dragging onto anything is blocked where KiCad walks the dragged line around it (`dragWalkaround`).
2. Grabbing the middle of a track is `DM_SEGMENT` in KiCad (slide the segment, 45-degree elbows); ours moves the nearer end vertex.
3. `DragCorner` is free-angle here; KiCad's `dragCorner45` keeps 45-degree geometry.
4. For `grabbed_index == 0` KiCad sets `SHP_REVERSED` so `checkShoveDirection` tests the **far** end; ours tests `pusher.first()`, which is the cursor-side tip in that case (direction test inverted).
5. No last-valid-point restore on release.

Fix: port `DM_SEGMENT` + `dragCorner45`, `dragWalkaround`, `SHP_REVERSED` (a `reversed` flag on the pusher consumed by `check_shove_direction`).

### D8. Committing DRC violations in MarkObstacles mode (line_placer.rs:280-282, dragger.rs:242) -- MEDIUM

`AllowDRCViolations()` is `mode == RM_MarkObstacles && can_violate_drc` and `can_violate_drc` defaults to false (pns_routing_settings.h:117-119, .cpp:48). FixRoute therefore refuses a colliding head in every mode by default. Ours treats MarkObstacles as "allowed". Add an `allow_drc_violations` setting (default false) and gate on `mode == MarkObstacles && allow_drc_violations`.

### D9. Clearance epsilon 1 um vs KiCad 0.5 um vs project DRC 0 (node.rs:35,373) -- MEDIUM

See table row (section E). Either pick epsilon 0 for the router (the project's DRC has none, so a router-accepted route can never be a DRC violation) or give the DRC the same epsilon; today the router can leave 1 um gaps the DRC reports (D6 shows one). The `HULL_ROUNDING_GUARD` already compensates for hull-hugging paths, so epsilon 0 plus the guard is consistent.

### D10. `assemble_line` normalizes widths silently (node.rs:214-270, shove.rs:336-346) -- MEDIUM, data-altering

KiCad drops segments of a different width from the assembled line (`AssembleLine`, pns_node.cpp:1176). Ours assembles through them with the seed's width and, after a shove, removes and re-adds every segment with that width.

Repro (run): GND track (3500,-2000)-(3500,-1000) width 200 + (3500,-1000)-(3500,2000) width 500, same `source_track`; shove by a SIG head (0,0)-(5000,0): the displaced line comes back as one 8-point line with `width = 500` -- the 200-wide portion is widened to 500 in the commit.

Fix: stop (or split) at width changes as `AssembleLine` does (also honour locked joints/segments).

### D11. Optimizer: missing `Simplify2` in mergeFull/mergeStep (optimizer.rs:82-109, 198-215) -- MEDIUM

31% of random 45-degree staircase paths optimize differently from the KiCad-faithful procedure (details in the D-table row), because corner costs are compared on chains that still contain collinear points. Apply `Simplify2` (dedup + collinear removal, 3-point rule) to the input at the start of `merge_full` and to each candidate before costing; use KiCad's tie rule (`cost[0] < cost_orig && cost[0] < cost[1]`, else `cost[1] < cost_orig`); collision-check only the bypass; drop MERGE_OBTUSE from the default `optimize()` (KiCad never runs it with MERGE_SEGMENTS); read `optimizer_effort`; and run the optimizer over shoved lines (`runOptimizer`, 2 passes).

### D12. Obstacle selection order in shove (shove.rs:279) -- MEDIUM

`min_by( actual )` is not `NearestObstacle` (nearest along the line by hull-intersection path length, searched per kind in the order solid, via, segment, hole). Multi-obstacle cases are resolved in a different order; with D5 the difference decides success vs failure.

### D13. `SegmentHull` kink handling (hull.rs:267-336) -- LOW

(a) `len` truncated vs `KiROUND`, (b) `d` computed after `cl++`, (c) exact vs +/-1 `IsSegment45Degree`. Probe: 10074/200000 differ (half of the sample had |dx|,|dy| <= 30 um); identical for clearly non-kink lengths. Fix: use `norm()` rounding for `len`, compute `d`/`dr`/`xr2` before the kink branch, port the tolerant 45-degree test.

### D14. Smart pads skip rounded-rect pads (optimizer.rs:334-363) -- LOW/MEDIUM

KiCad turns a rounded-rect pad into a `SHAPE_SIMPLE` polygon (pns_kicad_iface.cpp:1558-1577), so `customBreakouts` applies and the exit is reshaped. Ours has no breakouts for `Shape::RoundRect` (and a comment claiming KiCad has none). Fix: return `customBreakouts`-style rays against the pad outline for `RoundRect` (polygonise like `eda_drc::board::rotated_pad_shape`).

### D15. Walkaround iteration budget per item (walkaround.rs:180-194) -- LOW/MEDIUM

KiCad spends one iteration per obstacle *cluster*; ours one per item, with the same limit 40, and no length cut-off. Dense rows of pads/vias fail earlier here. Fix: assemble the cluster (items whose hulls touch) per iteration, or scale the budget by item count.

### D16. Dead settings and posture/smart-pad gating (settings.rs, line_placer.rs:49) -- LOW

`shove_vias`, `optimizer_effort`, `jump_over_obstacles`, `walkaround_hug_length_threshold`, `via_force_prop_iteration_limit`, `fix_all_segments` are never read; `SMART_PADS` ignores `manually_forced` although KiCad disables it after a posture flip. `RoutingSettings::smart_pads` doc (settings.rs:58-62) says "not yet implemented"; it is.

### D17. Smaller helper differences (line_walk.rs) -- LOW

`Find` rounded-norm test (diagonal-adjacent snap), `Contains` <= 3 and `Collinear` +/-1 in `intersect`, `SelfIntersecting` adjacent/doubling-back case and ignore-endpoints rule, boundary behaviour of `checkShoveDirection`'s tracker, `NearestPoint` truncation, `Simplify2` semantics, duplicate end point on endpoint snapping (shove.rs:174-179). Each is unlikely alone but they interact with D3.

### D18. Other gaps worth tracking

Mark-obstacles hull snap (`rhMarkObstacles`), `SplitAdjacentSegments` (cannot end mid-segment), shove `viaOnEnd` hull and `shoveLineFromLoneVia`, `ShoveTimeLimit`, `LockJoint`/locked joints, `findRedundantSegment` (duplicate segments), `from_ir` never sets `Segment.locked`, diff-pair shove/walkaround/via, meander shape (45-degree accordion vs U-shape) and interactivity.

## Stale or misleading documentation found

- hull.rs:1-31 describes the pre-port circumscribed-octagon hull and claims KiCad's chamfer is less safe; `Item::hull` uses the KiCad-style functions lower in the same file. `hull_of`/`circle_pts`/`convex_hull` are only used by tests.
- shove.rs:39-40 ("the hull walk itself is still our simplified `walk_around_hull`, not a full `LINE::Walkaround` port") -- `walk_around_hull` now delegates to `line_walk::walkaround` (walkaround.rs:91-95).
- walkaround.rs:56 refers to `hull::hull_of` ordering; `walk_around_hull_simple` (:100) is dead code.
- settings.rs:58-62 says smart pads are not implemented.
- optimizer.rs:304 comment ("a rounded rectangle reaches KiCad's router as a compound shape, which has no breakouts") is wrong for pads (pns_kicad_iface.cpp:1558-1577 converts them to a polygon).
- crates/pns/PARITY.md Stage 1/3: still says the hulls are "circumscribing-octagon samples" and that shove uses a "single hull-expansion attempt per obstacle"; both were superseded by the ports in hull.rs/shove.rs.

## Probe notes (how the numbers were produced)

Scratch crate outside the repo (`.../scratchpad/probe`, path dependencies on `eda-pns`, `eda-drc`, `eda-model`, `cargo --offline`). All random inputs use a fixed LCG seed.

- D1: `Router` from the `router.rs` test fixture; calls listed above.
- D2: `Node::add` x4 + `assemble_line`, run under `timeout 10` (exit 124).
- D3: copy of `line_walk.rs` with `SPLIT_TOL` and a KiCad-style rounding `seg_isect` switchable at runtime; 20000 hulls (via octagons / segment hulls) x random 2-3 point chords x both windings; "enters hull" = a sampled point (20 per segment) strictly inside the polygon and more than 1.6 um from every edge.
- D4: KiCad `MoveDiagonal`/`ConvexHull` re-typed (min `LineDistance` over vertices) and compared with `convex_hull_octagon`; gap = minimum segment-to-segment distance between hull edges and pad edges.
- D11: KiCad `mergeFull`/`mergeStep`/`Simplify2` re-typed; compared with `optimizer::optimize_with( MERGE_SEGMENTS )` after both outputs go through `Line::simplify`; 8 on/off combinations of {pre-simplify, candidate-simplify, KiCad tie rule}.
- D13: KiCad `SegmentHull` re-typed line by line (including `EuclideanNorm`, `IsSegment45Degree`, `Resize`) and compared vertex-for-vertex with `hull::segment_hull`.
- D5/D6/D10: `shove_line` with `Node` fixtures as described.
- Section A RoundRect: `primitive_hull( RoundRect )` vs `convex_hull_octagon` of a 16-segments-per-corner polygon: equal areas for 4 pad sizes.
