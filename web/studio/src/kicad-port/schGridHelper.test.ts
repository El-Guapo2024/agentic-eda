import { test } from "node:test";
import assert from "node:assert/strict";
import { SCH_SNAP_RANGE_MIL, SchGridHelper, NM_PER_MIL, type SchSnapItem } from "./schGridHelper";
import type { GridEnv } from "./gridHelperBase";
import { defaultGridOverrides, gridSizeFor, type GridOverrides } from "./gridOverrides";
import { EESCHEMA_GRIDS_NM } from "./gridSettings";
import { box, type Pt } from "./snapGeom";

const GRID = 1_270_000; // 50 mil, the current grid
const env = (overrides: GridOverrides | null = defaultGridOverrides("schematic"), current = GRID): GridEnv => ({
  scale: 0.00005,
  gridNm: current,
  visibleGridNm: current,
  origin: [0, 0],
  gridSizeOf: (c) => gridSizeFor(c, current, EESCHEMA_GRIDS_NM, overrides),
});

const item = (over: Partial<SchSnapItem> & Pick<SchSnapItem, "id" | "kind" | "position">): SchSnapItem => ({
  owner: over.id,
  connectable: true,
  connectionPoints: [],
  bbox: box(over.position[0] - 100_000, over.position[1] - 100_000, over.position[0] + 100_000, over.position[1] + 100_000),
  ...over,
});
const symbol = (id: string, at: Pt, pins: Pt[], size = 3_000_000): SchSnapItem => item({ id, kind: "symbol", position: at, connectionPoints: pins, bbox: box(at[0] - size, at[1] - size, at[0] + size, at[1] + size) });
const wire = (id: string, a: Pt, b: Pt): SchSnapItem => item({ id, kind: "wire", position: a, connectionPoints: [a, b], bbox: box(Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])), line: { a, b, graphic: false } });
const near = (got: Pt, want: Pt, tol = 1e-6) => assert.ok(Math.abs(got[0] - want[0]) <= tol && Math.abs(got[1] - want[1]) <= tol, `expected (${want}) got (${got})`);

test("the snap range is 55 mil", () => {
  assert.equal(SCH_SNAP_RANGE_MIL, 55);
  assert.equal(new SchGridHelper(env(), []).snapRange(), 55 * NM_PER_MIL);
});

test("a click goes to the grid of the category: connected items on 50 mil, text on 10 mil, the current grid for the rest", () => {
  const h = new SchGridHelper(env(defaultGridOverrides("schematic"), 635_000), []);
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "connectable"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "wires"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "text"), [1_270_000, 1_524_000]);
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "graphics"), [1_270_000, 1_270_000], 1e-6); // graphics are not overridden by default: the current 25 mil grid
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "current"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([1_000_000, 1_000_000], "graphics"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([700_000, 700_000], "graphics"), [635_000, 635_000]);
});

test("the grid overrides can be switched off: every category is the current grid", () => {
  const h = new SchGridHelper(env({ ...defaultGridOverrides("schematic"), enabled: false }, 635_000), []);
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "connectable"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([1_300_000, 1_400_000], "text"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([1_000_000, 1_000_000], "text"), [1_270_000, 1_270_000]);
  near(h.bestSnapAnchor([700_000, 700_000], "wires"), [635_000, 635_000]);
});

test("with the grid on the grid wins over an anchor; with the grid off (Ctrl) the anchor is taken", () => {
  const pin: Pt = [1_000_000, 1_000_000];
  const items = [symbol("U1", [3_000_000, 3_000_000], [pin])];
  const h = new SchGridHelper(env(), items);
  near(h.bestSnapAnchor([1_100_000, 1_050_000], "connectable"), [1_270_000, 1_270_000]);
  assert.equal(h.overlay().snapPoint, null);
  h.setUseGrid(false);
  near(h.bestSnapAnchor([1_100_000, 1_050_000], "connectable"), pin);
  assert.ok(h.overlay().snapPoint, "the marker shows where it snapped");
  assert.equal(h.getSnapped()?.id, "U1");
});

test("Shift (no anchor snapping) leaves the grid, or the cursor with the grid off", () => {
  const items = [symbol("U1", [3_000_000, 3_000_000], [[1_000_000, 1_000_000]])];
  const h = new SchGridHelper(env(), items);
  h.setUseGrid(false);
  h.setSnap(false);
  near(h.bestSnapAnchor([1_100_000, 1_050_000], "connectable"), [1_100_000, 1_050_000]);
});

test("an anchor is taken within the diagonal of the range (55 mil x sqrt 2 = 1976 um, 1 976 000 nm), not beyond it", () => {
  const items = [symbol("U1", [10_000_000, 10_000_000], [[1_000_000, 1_000_000]], 12_000_000)];
  const within = new SchGridHelper(env(), items);
  within.setUseGrid(false);
  near(within.bestSnapAnchor([1_000_000 + 1_300_000, 1_000_000 + 1_300_000], "connectable"), [1_000_000, 1_000_000]); // 1838 um away
  const beyond = new SchGridHelper(env(), items);
  beyond.setUseGrid(false);
  near(beyond.bestSnapAnchor([1_000_000 + 1_500_000, 1_000_000 + 1_500_000], "connectable"), [2_500_000, 2_500_000]); // 2121 um away: left where it is
});

test("the connectable grid takes connectable items' anchors only, the graphics grid the others'", () => {
  const sym = symbol("U1", [5_000_000, 5_000_000], [[1_000_000, 1_000_000]], 5_000_000);
  const gfx = item({ id: "L1", kind: "graphic_line", connectable: false, position: [1010, 1010], connectionPoints: [[1010, 1010]], line: { a: [1010, 1010], b: [4000, 1010], graphic: true }, bbox: box(1_010_000, 1_010_000, 4_000_000, 1_010_000) });
  const connectable = new SchGridHelper(env(), [sym, gfx]);
  connectable.setUseGrid(false);
  near(connectable.bestSnapAnchor([1_005_000, 1_008_000], "connectable"), [1_000_000, 1_000_000]);
  // On the graphics grid the symbol's pin is not a candidate, and a graphic line gives no anchors in a plain snap: the cursor stays.
  const graphics = new SchGridHelper(env(), [sym, gfx]);
  graphics.setUseGrid(false);
  near(graphics.bestSnapAnchor([1_005_000, 1_008_000], "graphics"), [1_005_000, 1_008_000]);
});

test("a wire is snapped to along its length at the cursor's other coordinate", () => {
  const wires = [wire("W1", [0, 1_270_000], [5_080_000, 1_270_000]), wire("W2", [2_540_000, 3_810_000], [2_540_000, 8_890_000])];
  const fresh = () => {
    const h = new SchGridHelper(env(), wires);
    h.setUseGrid(false);
    return h;
  };
  near(fresh().bestSnapAnchor([2_640_000, 1_300_000], "wires"), [2_640_000, 1_270_000]);
  near(fresh().bestSnapAnchor([2_500_000, 6_000_000], "wires"), [2_540_000, 6_000_000]);
  // Away from either wire: the cursor, as the grid is off.
  near(fresh().bestSnapAnchor([12_000_000, 12_000_000], "wires"), [12_000_000, 12_000_000]);
});

test("items being moved are skipped, by their id or their owner's", () => {
  const items = [symbol("U1", [3_000_000, 3_000_000], [[1_000_000, 1_000_000]])];
  const h = new SchGridHelper(env(), items);
  h.setUseGrid(false);
  near(h.bestSnapAnchor([1_100_000, 1_050_000], "connectable", new Set(["U1"])), [1_100_000, 1_050_000]);
  const field = item({ id: "U1:ref", owner: "U1", kind: "field", connectable: false, position: [1100, 1050] });
  const h2 = new SchGridHelper(env(), [field]);
  h2.setUseGrid(false);
  near(h2.bestSnapAnchor([1_100_000, 1_050_000], "text", new Set(["U1"])), [1_100_000, 1_050_000]);
});

test("the snap line through the last anchor pulls the cursor along it, onto the grid along the line", () => {
  const pin: Pt = [1_000_000, 1_000_000];
  const h = new SchGridHelper(env(), [symbol("U1", [10_000_000, 10_000_000], [pin], 12_000_000)]);
  h.setUseGrid(false);
  near(h.bestSnapAnchor([1_100_000, 1_050_000], "connectable"), pin);
  h.setUseGrid(true);
  // 100 um off the horizontal through the pin, far to its right: the grid point along the line at the pin's height.
  const p = h.bestSnapAnchor([4_000_000, 1_100_000], "connectable");
  near(p, [3_810_000, 1_000_000]);
  assert.ok(h.overlay().snapLine);
  // 2 mm off it: plain grid.
  near(h.bestSnapAnchor([4_000_000, 3_100_000], "connectable"), [3_810_000, 2_540_000]);
});

test("BestDragOrigin: the pin nearest the mouse, or the symbol's origin when the mouse is nearer that", () => {
  const sym = symbol("U1", [5_000_000, 5_000_000], [[3_810_000, 5_080_000], [6_350_000, 5_080_000]], 4_000_000);
  const h = new SchGridHelper(env(), [sym]);
  near(h.bestDragOrigin([3_900_000, 5_100_000], "connectable", [sym]), [3_810_000, 5_080_000]);
  near(h.bestDragOrigin([5_200_000, 5_600_000], "connectable", [sym]), [5_000_000, 5_000_000]);
});

test("BestDragOrigin of text alone uses the text's position", () => {
  const text = item({ id: "T1", kind: "text", connectable: false, position: [700_000, 800_000] });
  const h = new SchGridHelper(env(), [text]);
  near(h.bestDragOrigin([750_000, 760_000], "text", [text]), [700_000, 800_000]);
  // With a connectable item in the selection the text is left out of the anchors.
  const sym = symbol("U1", [5_000_000, 5_000_000], [[3_810_000, 5_080_000]], 4_000_000);
  near(h.bestDragOrigin([750_000, 760_000], "connectable", [text, sym]), [3_810_000, 5_080_000]); // not (700, 800): a text anchor is left out beside a connectable item
});

test("the grid of a selection is the coarsest of its items", () => {
  const h = new SchGridHelper(env(), []);
  assert.equal(h.getSelectionGrid([{ kind: "symbol" }, { kind: "field" }]), "connectable");
  assert.equal(h.getSelectionGrid([{ kind: "field" }, { kind: "text" }]), "text");
  assert.equal(h.getSelectionGrid([{ kind: "text" }, { kind: "wire" }]), "wires");
  assert.equal(h.getSelectionGrid([]), "current");
  assert.equal(h.getItemGrid({ kind: "junction" }), "wires");
  assert.equal(h.getItemGrid(null), "current");
});

test("a new sheet makes the helper forget what it snapped to", () => {
  const h = new SchGridHelper(env(), [symbol("U1", [3_000_000, 3_000_000], [[1_000_000, 1_000_000]])]);
  h.setUseGrid(false);
  h.bestSnapAnchor([1_100_000, 1_050_000], "connectable");
  assert.ok(h.getSnapped());
  h.setItems([]);
  assert.equal(h.getSnapped(), null);
  assert.equal(h.overlay().snapLine, null);
});
