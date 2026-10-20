import { test } from "node:test";
import assert from "node:assert/strict";
import { FOOTPRINT_EDITOR_MAGNETIC, FOOTPRINT_ITEM_ID, footprintScene, padSnapLayers } from "./fpSnapScene";
import { PcbGridHelper, type GridEnv, type Visibility } from "./pcbGridHelper";
import type { Pt } from "./snapGeom";
import type { CmdShape, LibraryFootprint, LibraryPad } from "../api/types";

// View: 0.1 px per um, a 1 mm grid, so the snap range is 250 um: an anchor is taken within 200 um.
const env = (over: Partial<GridEnv> = {}): GridEnv => ({ scale: 0.1, gridUm: 1000, visibleGridUm: 1000, origin: [0, 0], gridSizeOf: () => 1000, ...over });
const visible: Visibility = { layerVisible: () => true, highContrastLayers: null };

const pad = (id: string, number: string, x: number, y: number, over: Partial<LibraryPad> = {}): LibraryPad =>
  ({ id, number, at: { x, y }, size: [900, 600], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", drill: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"], ...over }) as unknown as LibraryPad;
const seg = (id: string, x0: number, y0: number, x1: number, y1: number, layer = "F.SilkS"): CmdShape => ({ kind: "segment", id, layer, stroke_width: 150, filled: false, start: { x: x0, y: y0 }, end: { x: x1, y: y1 } });
const fp = (over: Partial<LibraryFootprint> = {}): LibraryFootprint => ({ name: "T", pads: [], graphics: [], texts: [], fields: [], courtyard: null, anchor: { x: 0, y: 0 }, published: false, ...over }) as unknown as LibraryFootprint;
const near = (got: Pt, want: Pt, tol = 1e-6) => assert.ok(Math.abs(got[0] - want[0]) <= tol && Math.abs(got[1] - want[1]) <= tol, `expected (${want}) got (${got})`);
const helper = (f: LibraryFootprint, e = env()) => {
  const h = new PcbGridHelper(e, footprintScene(f), FOOTPRINT_EDITOR_MAGNETIC, visible);
  let now = 0;
  h.setClock(() => now);
  return h;
};

test("the Footprint Editor's magnetic settings are its own: pads always, tracks never, graphics on", () => {
  assert.deepEqual(FOOTPRINT_EDITOR_MAGNETIC, { pads: "always", tracks: "never", graphics: true, allLayers: false });
});

test("pad layers: a wildcard is both sides, a through-hole pad is on every copper layer", () => {
  assert.deepEqual(padSnapLayers({ layers: ["*.Cu", "*.Mask"], kind: "through_hole" } as unknown as LibraryPad).sort(), ["B.Cu", "B.Mask", "F.Cu", "F.Mask"]);
  assert.deepEqual(padSnapLayers({ layers: ["F.Cu"], kind: "smd" } as unknown as LibraryPad), ["F.Cu"]);
  assert.deepEqual(padSnapLayers({ layers: ["F.Cu"], kind: "through_hole" } as unknown as LibraryPad).sort(), ["B.Cu", "F.Cu"]);
});

test("the scene has each pad, graphic and text under the editor's own ids -- and not the footprint itself", () => {
  const scene = footprintScene(fp({ pads: [pad("p1", "1", 0, 0), pad("p2", "2", 3000, 0)], graphics: [seg("g1", -2000, -2000, 2000, -2000)], texts: [{ id: "t1", content: "REF", at: { x: 0, y: 4000 }, angle: 0, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "center", mirror: false }] }));
  assert.deepEqual(scene.items.map((i) => [i.type, i.id]), [["pad", "p1"], ["pad", "p2"], ["shape", "g1"], ["text", "t1"]]);
  assert.ok(scene.items.every((i) => i.type !== "pad" || i.footprint === FOOTPRINT_ITEM_ID));
  assert.deepEqual(footprintScene(null).items, []);
});

test("a pad's centre is an anchor with the Footprint Editor's defaults: the cursor 150 um from it goes to it on every layer, off the grid", () => {
  const f = fp({ pads: [pad("p1", "1", 5400, 2300)] });
  near(helper(f).bestSnapAnchor([5500, 2400], "all", "current", new Set()), [5400, 2300], 1e-6);
  near(helper(f).bestSnapAnchor([5300, 2400], new Set(["F.SilkS"]), "current", new Set()), [5000, 2000], 1e-6); // the active layer lacks the pad: the grid
});

test("a pad's outline gives corners and side middles, and a shape's end points", () => {
  const h = helper(fp({ pads: [pad("p1", "1", 0, 0, { size: [1000, 600] })], graphics: [seg("g1", 4300, 700, 7300, 700)] }));
  // the corner of the pad at (500, 300), reached from near it with the grid off
  h.setUseGrid(false);
  near(h.bestSnapAnchor([560, 360], "all", "current", new Set()), [500, 300]);
  // the end of the silk line, with the silk layer active
  near(h.bestSnapAnchor([4380, 760], new Set(["F.SilkS"]), "current", new Set()), [4300, 700]);
});

test("the footprint's own origin is no anchor there, and the items being moved are left out", () => {
  const f = fp({ pads: [pad("p1", "1", 5400, 2300)] });
  const h = helper(f);
  h.setUseGrid(false);
  near(h.bestSnapAnchor([90, 70], "all", "current", new Set()), [90, 70]); // the origin is not an anchor
  // the pad being moved is left out: its centre and corners are not there to snap to
  const g = helper(f);
  g.setUseGrid(false);
  near(g.bestSnapAnchor([5440, 2360], "all", "current", new Set(["p1"])), [5440, 2360]);
  near(helper(f).bestSnapAnchor([5440, 2360], "all", "current", new Set()), [5400, 2300]);
});

test("Shift (no anchors) puts the cursor on the grid; Ctrl (no grid) lets it stay", () => {
  const h = helper(fp({ pads: [pad("p1", "1", 5400, 2300)] }));
  h.setSnap(false);
  near(h.bestSnapAnchor([5300, 2400], "all", "current", new Set()), [5000, 2000]);
  const g = helper(fp({ pads: [pad("p1", "1", 5400, 2300)] }));
  g.setUseGrid(false);
  near(g.bestSnapAnchor([5500, 2400], "all", "current", new Set()), [5400, 2300]);
});
