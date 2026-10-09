import { test } from "node:test";
import assert from "node:assert/strict";
import { PcbGridHelper, SNAP_HYSTERESIS_PX, SNAP_RANGE_PX, kiRound, type GridEnv, type Visibility } from "./pcbGridHelper";
import { DEFAULT_PCB_MAGNETIC, type PcbMagnetic } from "./snapAnchors";
import { buildScene, type SceneBoard } from "./snapScene";
import { PT, type Pt } from "./snapGeom";
import type { GridCategory } from "./gridOverrides";

// View: 0.1 px per um, so 25 px is 250 um; a 1 mm grid, so the snap range is 250 um (the visible grid is larger) and the hysteresis of 5 px is 50 um:
// an anchor is taken within 200 um (snapIn) and kept up to 300 um (snapOut).
const SNAP_RANGE = 250;
const SNAP_IN = 200;
const SNAP_OUT = 300;

const env = (over: Partial<GridEnv> = {}): GridEnv => ({ scale: 0.1, gridUm: 1000, visibleGridUm: 1000, origin: [0, 0], gridSizeOf: () => 1000, ...over });
const visible: Visibility = { layerVisible: () => true, highContrastLayers: null };
const always: PcbMagnetic = { ...DEFAULT_PCB_MAGNETIC, pads: "always", tracks: "always", graphics: true };

const pad = (num: string, x: number, y: number, w = 600, h = 600, round = false, th = false) => ({ num, net: null, x, y, w, h, round, th });
const part = (ref: string, x: number, y: number, pads: ReturnType<typeof pad>[] = []) => ({ ref, placed: true, at: [x, y] as [number, number], side: "top" as const, pads, courtyard: [x - 1500, y - 1500, x + 1500, y + 1500] as [number, number, number, number] });
const via = (id: string, x: number, y: number, d = 600) => ({ id, net: "N", x, y, d, drill: 300, from: "F.Cu", to: "B.Cu" });
const shapeSeg = (id: string, start: Pt, end: Pt, layer = "F.SilkS") => ({ id, kind: "segment" as const, layer, stroke_width: 150, filled: false, start: [start[0], start[1]] as [number, number], end: [end[0], end[1]] as [number, number] });
const drawings = (shapes: ReturnType<typeof shapeSeg>[]) => ({ shapes, texts: [], dimensions: [] });

function make(board: Partial<SceneBoard>, magnetic: PcbMagnetic = DEFAULT_PCB_MAGNETIC, e: GridEnv = env()) {
  const scene = buildScene({ layers: ["F.Cu", "B.Cu"], parts: [], ...board });
  const h = new PcbGridHelper(e, scene, magnetic, visible);
  let now = 0;
  h.setClock(() => now);
  return { h, advance: (ms: number) => (now += ms) };
}

const ACTIVE = new Set(["F.SilkS"]);
/** The layers of a copper tool: a pad, via or footprint is on one of them. */
const CU = new Set(["F.Cu"]);
const near = (got: Pt, want: Pt, tol = 1e-6) => assert.ok(Math.abs(got[0] - want[0]) <= tol && Math.abs(got[1] - want[1]) <= tol, `expected (${want}) got (${got})`);

test("constants: the tuning range is 25 px and the hysteresis 5 px", () => {
  assert.equal(SNAP_RANGE_PX, 25);
  assert.equal(SNAP_HYSTERESIS_PX, 5);
  const { h } = make({});
  assert.equal(h.snapRange(), SNAP_RANGE);
});

test("KiROUND rounds half away from zero", () => {
  assert.equal(kiRound(2.5), 3);
  assert.equal(kiRound(-2.5), -3);
  assert.equal(kiRound(-2.4), -2);
});

test("the snap range never exceeds the visible grid while the grid is on, and does when it is off", () => {
  const { h } = make({}, DEFAULT_PCB_MAGNETIC, env({ visibleGridUm: 100 }));
  assert.equal(h.snapRange(), 100);
  h.setUseGrid(false);
  assert.equal(h.snapRange(), SNAP_RANGE);
});

test("with nothing near, the cursor goes to the nearest grid point; Ctrl (no grid) leaves it where it is", () => {
  const { h } = make({});
  near(h.bestSnapAnchor([10620, 10030], ACTIVE), [11000, 10000]);
  near(h.bestSnapAnchor([-620, -1530], ACTIVE), [-1000, -2000], 1e-9);
  h.setUseGrid(false);
  near(h.bestSnapAnchor([10620, 10030], ACTIVE), [10620, 10030]);
});

test("the grid is anchored at the grid origin", () => {
  const { h } = make({}, DEFAULT_PCB_MAGNETIC, env({ origin: [250, 250] }));
  near(h.bestSnapAnchor([10620, 10030], ACTIVE), [10250, 10250]);
});

test("a pad's centre is an anchor when pads are magnetic always, and the snap marker says it is a centre", () => {
  const { h } = make({ parts: [part("U1", 10000, 10000, [pad("1", 10600, 10000)])] }, always);
  near(h.bestSnapAnchor([10620, 10030], CU, "current"), [10600, 10000]);
  const overlay = h.overlay();
  assert.ok(overlay.snapPoint);
  near(overlay.snapPoint!.pos, [10600, 10000]);
  assert.equal(overlay.snapPoint!.types & PT.CENTER, PT.CENTER);
  assert.equal(h.getSnapped()?.type, "pad");
});

test("by default (pads 'in track tool') a pad is not an anchor of the drawing tools", () => {
  const { h } = make({ parts: [part("U1", 10000, 10000, [pad("1", 10600, 10000)])] });
  near(h.bestSnapAnchor([10620, 10030], CU), [11000, 10000]);
  assert.equal(h.overlay().snapPoint, null);
});

test("a pad's outline gives corners and side middles too", () => {
  const { h } = make({ parts: [part("U1", 10000, 10000, [pad("1", 10600, 10000, 600, 400)])] }, always);
  // Top-right corner of the 600 x 400 pad is (10900, 10200); the cursor is 100 um from it, 400 from the centre.
  const p = h.bestSnapAnchor([10990, 10260], CU);
  near(p, [10900, 10200]);
  assert.equal(h.overlay().snapPoint!.types & PT.CORNER, PT.CORNER);
  // The middle of the right side.
  h.fullReset();
  near(h.bestSnapAnchor([10990, 9940], CU), [10900, 10000]);
  assert.equal(h.overlay().snapPoint!.types & PT.MID, PT.MID);
});

test("a round pad gives its four quadrants", () => {
  const { h } = make({ parts: [part("U1", 10000, 10000, [pad("1", 10600, 10000, 600, 600, true)])] }, always);
  near(h.bestSnapAnchor([10590, 10330], CU), [10600, 10300]);
  assert.equal(h.overlay().snapPoint!.types & PT.QUADRANT, PT.QUADRANT);
});

test("the footprint's own position is an anchor, and the centre of its box too when that is further than a grid away", () => {
  const board = { parts: [{ ...part("U1", 10130, 10000), courtyard: [9000, 9000, 12000, 11000] as [number, number, number, number] }] };
  const { h } = make(board);
  near(h.bestSnapAnchor([10300, 10030], CU), [10130, 10000]);
  h.fullReset();
  // The box centre is (10500, 10000): 370 from the position -- not a grid away (1000), so it is not an anchor.
  near(h.bestSnapAnchor([10520, 10030], CU), [11000, 10000]);
  const wide = make({ parts: [{ ...part("U1", 10130, 10000), courtyard: [8000, 9000, 14000, 11000] as [number, number, number, number] }] });
  near(wide.h.bestSnapAnchor([11080, 10030], CU), [11000, 10000], 1e-6); // centre (11000, 10000), 870 from the position < grid: still not an anchor, the grid point is the same here
  const farther = make({ parts: [{ ...part("U1", 10130, 10000), courtyard: [8000, 9000, 16000, 11000] as [number, number, number, number] }] });
  near(farther.h.bestSnapAnchor([12020, 10030], CU), [12000, 10000]); // centre (12000, 10000): 1870 > 1000, an anchor (and on the grid, so the point is the same)
});

test("Shift (no anchor snapping) leaves only the grid", () => {
  const { h } = make({ parts: [part("U1", 10000, 10000, [pad("1", 10600, 10000)])] }, always);
  h.setSnap(false);
  near(h.bestSnapAnchor([10620, 10030], CU), [11000, 10000]);
});

test("hysteresis: an anchor is taken within snapIn, kept up to snapOut, and then given up", () => {
  const board = { routing: { tracks: [], zones: [], vias: [via("v1", 10130, 10000)] } };
  assert.ok(SNAP_IN < SNAP_RANGE && SNAP_RANGE < SNAP_OUT);
  // A fresh helper does not take an anchor 250 um away (outside snapIn)...
  const fresh = make(board, always);
  near(fresh.h.bestSnapAnchor([10380, 10000], CU), [10000, 10000]);
  // ...but one that is on the anchor keeps it as the cursor moves out to 250. (Snap lines are off here: with them on, sliding straight away from an anchor follows the
  // horizontal or vertical line through it instead -- see the construction lines test.)
  const { h } = make(board, always);
  h.setSnapLine(false);
  near(h.bestSnapAnchor([10300, 10000], CU), [10130, 10000]);
  near(h.bestSnapAnchor([10380, 10000], CU), [10130, 10000]);
  near(h.bestSnapAnchor([10425, 10000], CU), [10130, 10000], 1e-6); // 295: still inside snapOut
  // ...and gives it up beyond snapOut, after which it needs snapIn again to take it back.
  near(h.bestSnapAnchor([10440, 10000], CU), [10000, 10000]);
  assert.equal(h.getSnapped(), null);
  near(h.bestSnapAnchor([10380, 10000], CU), [10000, 10000]);
});

test("an anchor on another layer is not snapped to unless all layers are magnetic", () => {
  const shapes = [shapeSeg("s1", [2000, 2000], [8000, 2000], "B.SilkS")];
  const { h } = make({ drawings: drawings(shapes) }, always);
  near(h.bestSnapAnchor([2040, 2030], ACTIVE), [2000, 2000]);
  assert.equal(h.overlay().snapPoint, null, "not B.SilkS: no snap, only the grid (which is here the same point)");
  const onB = make({ drawings: drawings([shapeSeg("s1", [2030, 2210], [8030, 2210], "B.SilkS")]) }, always);
  near(onB.h.bestSnapAnchor([2060, 2230], ACTIVE), [2000, 2000]);
  const all = make({ drawings: drawings([shapeSeg("s1", [2030, 2210], [8030, 2210], "B.SilkS")]) }, { ...always, allLayers: true });
  near(all.h.bestSnapAnchor([2060, 2230], ACTIVE), [2030, 2210]);
  const sel = make({ drawings: drawings([shapeSeg("s1", [2030, 2210], [8030, 2210], "B.SilkS")]) }, always);
  near(sel.h.bestSnapAnchor([2060, 2230], new Set(["B.SilkS"])), [2030, 2210]);
  near(make({ drawings: drawings([shapeSeg("s1", [2030, 2210], [8030, 2210], "B.SilkS")]) }, always).h.bestSnapAnchor([2060, 2230], "all"), [2030, 2210]);
});

test("a segment's ends and middle are anchors, with their point types", () => {
  const { h } = make({ drawings: drawings([shapeSeg("s1", [2030, 2210], [8030, 2210])]) }, always);
  near(h.bestSnapAnchor([2060, 2230], ACTIVE), [2030, 2210]);
  assert.equal(h.overlay().snapPoint!.types & PT.END, PT.END);
  near(h.bestSnapAnchor([5060, 2230], ACTIVE), [5030, 2210]);
  assert.equal(h.overlay().snapPoint!.types & PT.MID, PT.MID);
});

test("items being moved are skipped (by their own id or their owner's)", () => {
  const board = { routing: { tracks: [], zones: [], vias: [via("v1", 10130, 10000)] } };
  const { h } = make(board, always);
  near(h.bestSnapAnchor([10300, 10000], CU, "current", new Set(["v1"])), [10000, 10000]);
  const fp = make({ parts: [part("U1", 10130, 10000)] });
  near(fp.h.bestSnapAnchor([10300, 10030], CU, "current", new Set(["U1"])), [10000, 10000]);
});

test("each category of item has its own grid when overrides give it one", () => {
  const sizes: Record<GridCategory, number> = { current: 1000, connectable: 250, wires: 500, vias: 100, text: 200, graphics: 50 };
  const { h } = make({}, DEFAULT_PCB_MAGNETIC, env({ gridSizeOf: (c) => sizes[c] }));
  near(h.bestSnapAnchor([3260, 740], ACTIVE, "current"), [3000, 1000]);
  near(h.bestSnapAnchor([3260, 740], ACTIVE, "wires"), [3500, 500]);
  near(h.bestSnapAnchor([3260, 740], ACTIVE, "graphics"), [3250, 750]);
  near(h.bestSnapAnchor([3260, 740], ACTIVE, "connectable"), [3250, 750]);
  near(h.bestSnapAnchor([3260, 790], ACTIVE, "text"), [3200, 800]);
});

test("construction lines: the snap line through the last anchor pulls the cursor onto it, on the grid along the line", () => {
  const { h } = make({ routing: { tracks: [], zones: [], vias: [via("v1", 10130, 10000)] } }, always);
  near(h.bestSnapAnchor([10300, 10000], CU), [10130, 10000]);
  // 120 um off the horizontal through the via, far along it: snaps onto the line, at the grid point along it.
  const p = h.bestSnapAnchor([13050, 10120], CU);
  near(p, [13000, 10000]);
  const o = h.overlay();
  assert.ok(o.snapLine, "the snap line is drawn from its origin to where the cursor snapped");
  near(o.snapLine!.a, [10130, 10000]);
  near(o.snapLine!.b, [13000, 10000]);
  assert.equal(o.guides.length, 2, "one guide per direction: horizontal and vertical");
  assert.equal(o.guides.filter((g) => g.active).length, 1);
  assert.equal(o.snapPoint, null, "a snap onto a line is not an anchor: no marker");
  // 700 um off it: plain grid.
  near(h.bestSnapAnchor([13060, 10700], CU), [13000, 11000]);
  assert.equal(h.overlay().snapLine, null);
});

test("the vertical snap line works the same way", () => {
  const { h } = make({ routing: { tracks: [], zones: [], vias: [via("v1", 10130, 10000)] } }, always);
  near(h.bestSnapAnchor([10300, 10000], CU), [10130, 10000]);
  near(h.bestSnapAnchor([10200, 14040], CU), [10130, 14000]);
});

test("the active direction reaches half as far again, and Shift (no snapping) disables the snap line too", () => {
  const { h } = make({ routing: { tracks: [], zones: [], vias: [via("v1", 10130, 10000)] } }, always);
  near(h.bestSnapAnchor([10300, 10000], CU), [10130, 10000]);
  near(h.bestSnapAnchor([13050, 10120], CU), [13000, 10000]);
  // 300 um off is outside the range (250) of an inactive direction, but inside 1.5 x of the active one -- and 300 is within the escape range too.
  near(h.bestSnapAnchor([13050, 10300], CU), [13000, 10000]);
  h.setSnap(false);
  near(h.bestSnapAnchor([13050, 10120], CU), [13000, 10000], 1e-6);
});

test("a segment under the cursor with the grid off: the nearest point on it ('point on element')", () => {
  const { h } = make({ drawings: drawings([shapeSeg("s1", [0, 0], [10000, 0])]) });
  h.setUseGrid(false);
  near(h.bestSnapAnchor([4000, 60], ACTIVE), [4000, 0]);
  const o = h.overlay();
  assert.equal(o.snapPoint!.types, PT.ON_ELEMENT);
  // Out of range: left where it is.
  near(h.bestSnapAnchor([4000, 400], ACTIVE), [4000, 400]);
});

test("intersections need both items to have been dwelt on; then the crossing is an anchor", () => {
  const shapes = [shapeSeg("h", [0, 200], [10000, 200]), shapeSeg("v", [5300, -4000], [5300, 4000])];
  const { h } = make({ drawings: drawings(shapes) });
  // On the horizontal only: it becomes involved, the crossing still needs the other.
  near(h.bestSnapAnchor([5320, 210], ACTIVE), [5000, 0]);
  near(h.bestSnapAnchor([5320, 210], ACTIVE), [5000, 0]);
  // On the vertical only (away from the horizontal): now both are involved.
  near(h.bestSnapAnchor([5300, 2000], ACTIVE), [5000, 2000]);
  const p = h.bestSnapAnchor([5320, 210], ACTIVE);
  near(p, [5300, 200]);
  assert.equal(h.overlay().snapPoint!.types & PT.INTERSECTION, PT.INTERSECTION);
});

test("the extension of a segment is construction geometry too: its ray meets another line beyond its end", () => {
  // A horizontal segment 0..4000 at y = 200 and a vertical one at x = 6500; the extension of the first crosses the second at (6500, 200).
  const shapes = [shapeSeg("h", [0, 200], [4000, 200]), shapeSeg("v", [6500, -4000], [6500, 4000])];
  const { h } = make({ drawings: drawings(shapes) });
  near(h.bestSnapAnchor([2000, 210], ACTIVE), [2000, 0]); // dwell on h
  near(h.bestSnapAnchor([6500, 3000], ACTIVE), [7000, 3000]); // dwell on v
  near(h.bestSnapAnchor([6520, 210], ACTIVE), [6500, 200]);
  assert.equal(h.overlay().snapPoint!.types & PT.INTERSECTION, PT.INTERSECTION);
  assert.ok(h.overlay().construction.length > 0, "the rays are on show");
});

test("a persistent construction segment makes its ends reference points: the snap line starts there without snapping to it", () => {
  const { h } = make({ drawings: drawings([shapeSeg("s", [2030, 2210], [8030, 5210])]) });
  h.addConstructionItems([h.scene.items[0]!], false, true);
  // Near the start: nothing is snapped to (it is a reference point) but the grid point, and the snap line has its origin there.
  near(h.bestSnapAnchor([2060, 2230], ACTIVE), [2000, 2000]);
  near(h.bestSnapAnchor([6050, 2300], ACTIVE), [6000, 2210]);
  assert.ok(h.overlay().snapLine);
});

test("AlignToSegment: the point of the segment on the grid's line through the cursor, else an end", () => {
  const { h } = make({});
  near(h.alignToSegment([4300, 40], [0, 0], [10000, 0]), [4000, 0]);
  near(h.alignToSegment([9990, 200], [0, 0], [10000, 0]), [10000, 0]);
  h.setSnap(false);
  near(h.alignToSegment([4300, 40], [0, 0], [10000, 0]), [4000, 0]);
});

test("BestDragOrigin picks the nearest origin or corner of the items being picked up", () => {
  const board = { parts: [part("U1", 10130, 10000, [pad("1", 10600, 10000), pad("2", 11400, 10000)])] };
  const { h } = make(board, always);
  const fp = h.scene.items.filter((i) => i.type === "footprint");
  // The mouse is on pad 1: the pad's centre is the drag origin (a pad under the mouse is a candidate), not the footprint's position.
  near(h.bestDragOrigin([10620, 10040], fp), [10600, 10000]);
  // Away from the pads, the footprint's position.
  near(h.bestDragOrigin([10300, 9000], fp), [10130, 10000]);
});

test("a new scene makes the helper forget what it snapped to", () => {
  const board = { routing: { tracks: [], zones: [], vias: [via("v1", 10130, 10000)] } };
  const { h } = make(board, always);
  near(h.bestSnapAnchor([10300, 10000], CU), [10130, 10000]);
  assert.ok(h.getSnapped());
  h.setScene(buildScene({ parts: [] }));
  assert.equal(h.getSnapped(), null);
  assert.equal(h.overlay().snapLine, null);
});
