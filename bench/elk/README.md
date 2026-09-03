# eda-layout vs elkjs differential bench

Compares our Rust port of the ELK "layered" (Sugiyama) algorithm
(`crates/layout`) against the original `elkjs` implementation on a shared
corpus of test graphs, to catch cases where the port produces
significantly worse layouts than the algorithm it was ported from.

## Pipeline

```
bash bench/elk/run.sh
```

1. `cargo run -p eda-layout --example dump_cases -- bench/elk/cases.json`
   generates the corpus (invariants-test-style random graphs plus a handful
   of shape-specific cases), runs `eda_layout::layout` on every case, and
   writes `cases.json` with each case's nodes/edges plus our result
   (`case.port`: positions, polylines, metrics, wall time).
2. `node run.mjs` reads `cases.json`, builds the equivalent `elkjs` graph
   for each case, runs `elk.layered` with matching options, computes the
   same metrics on elkjs's output (`case.elk`), rewrites `cases.json` with
   both sides filled in, and writes `report.md`.

## elkjs options used

- `elk.algorithm = layered`, `elk.direction = RIGHT` — matches our
  pipeline's internal layer-as-row order rotated 90 degrees so layers read
  as columns left-to-right (see the transpose in `crates/layout/src/lib.rs`).
- `elk.edgeRouting = ORTHOGONAL` — matches our Manhattan/orthogonal router.
- `elk.layered.spacing.nodeNodeBetweenLayers` = `LayoutOptions::layer_spacing`
  (12.7mm = 12700um by default).
- `elk.spacing.nodeNode` = `LayoutOptions::node_spacing` (3.81mm = 3810um).
- `elk.portConstraints = FIXED_POS` per node, with each port placed at the
  exact `(x, y)` our `Node::port_point` formula gives (offset from
  top-left along the given side) and `elk.port.side` set from our
  `Side` (`Top`->`NORTH`, `Bottom`->`SOUTH`, `Left`->`WEST`,
  `Right`->`EAST`) — this is the fairest way to compare crossing
  minimization and coordinate assignment without also comparing two
  different port-placement heuristics.

### Options we could not map faithfully

- **Grid snapping**: our port snaps every coordinate to a 1.27mm grid
  (`LayoutOptions::grid`); elkjs has no equivalent "snap all coordinates to
  a grid" option, so `case.elk` coordinates are elkjs's native (unsnapped)
  output. This mostly affects bbox-area/wire-length by a small constant
  factor, not crossings.
- **Cycle-breaking heuristic**: elkjs's `layered` algorithm does its own
  internal cycle breaking (greedy, not configurable to match our specific
  `cycle::break_cycles` heuristic exactly), so on the `with_cycles_*` case
  the two implementations may pick a different feedback edge set. This is
  an inherent difference between "two independent algorithms implementing
  the same spec," not a bug in either.
- **Wall-clock time** is not a faithful apples-to-apples comparison: `elk`
  runs across a JS<->Java(GWT)-transpiled worker with startup/dispatch
  overhead per call, while `port` runs natively in-process. Take the
  `time_ns` column as a rough sanity check only, not a performance claim.

## Metrics

Computed identically (same semantics) in
`crates/layout/examples/dump_cases.rs::compute_metrics` (Rust) and
`bench/elk/run.mjs::computeMetrics` (JS). All units are micrometers (or
none, for counts). Definitions:

- **crossings**: number of pairs of orthogonal polyline segments, from two
  *different* edges, that cross properly — i.e. one is a horizontal
  segment, the other vertical, and they intersect at a point strictly
  interior to both segments (touching at a shared endpoint, or two
  parallel/collinear segments, does not count).
- **wire_length**: sum, over every polyline of every edge, of the
  Manhattan length of each consecutive-point segment (`|dx| + |dy|`).
- **bbox_area**: `(max_x - min_x) * (max_y - min_y)` of the union of all
  node bounding boxes (node positions + width/height only; wire routing is
  not included).
- **node_overlaps**: number of pairs of node boxes whose interiors overlap
  (strict inequality on all four sides — boxes that only touch at an edge
  don't count).
- **non_orthogonal_segments**: number of polyline segments where neither
  `dx == 0` nor `dy == 0` (should be 0 for both implementations, since both
  claim orthogonal routing; a nonzero count here is a routing bug).
- **max_bend_count**: the maximum, over all edges, of `points_in_polyline - 2`
  (0 for a straight two-point wire, 1 for one bend, etc).
- **time_ns**: wall-clock nanoseconds for the single layout call on that
  case (see caveat above for elkjs).

## Reading report.md

One row per case with `port/elk` values and a `port_value / elk_value`
ratio for the three metrics with a natural "bigger is worse" reading
(crossings, wire_length, bbox_area) — a ratio above 1 means the port did
worse on that metric for that case. The summary flags any case where the
port is worse than elkjs by more than 25% on crossings or wire length.
