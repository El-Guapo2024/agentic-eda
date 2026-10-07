import { test } from "node:test";
import assert from "node:assert/strict";
import { collectSheetRefs, crossProbeScale, crossProbeView, sheetOfRef, type SheetView } from "./crossProbe";

const near = (a: number, b: number, eps = 1e-9) => assert.ok(Math.abs(a - b) < eps, `${a} != ${b}`);

test("a small part zooms to show context: the LUT scales the fit ratio up", () => {
  // 2 x 1 mm part on a 1000 x 800 px canvas at 0.1 px/um: ratio 1800/8000 = 0.225, compRatio 1.8 -> bent 4.6 -> 1.035
  near(crossProbeScale(0.1, { minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 1000, 800), 0.1 / (0.225 * 4.6));
});

test("a probe whose zoom would barely change leaves the scale alone", () => {
  // ratio 0.15 * 4.6 = 0.69, inside [0.5, 1.0]
  assert.equal(crossProbeScale(1000 / 15000, { minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 1000, 800), 1000 / 15000);
});

test("a part much wider than tall falls back to the plain fit ratio", () => {
  // 40 x 2 mm on the same canvas: ratio 2.25, plain ratio max(56000/10000, 2.25) = 5.6
  near(crossProbeScale(0.1, { minX: 0, minY: 0, maxX: 40000, maxY: 2000 }, 1000, 800), 0.1 / 5.6);
});

test("a box with no width leaves the zoom and the view alone", () => {
  assert.equal(crossProbeScale(0.1, { minX: 5, minY: 0, maxX: 5, maxY: 100 }, 1000, 800), 0.1);
  const v = { scale: 0.1, x: 7, y: 9 };
  assert.equal(crossProbeView(v, { minX: 5, minY: 0, maxX: 5, maxY: 100 }, 1000, 800), v);
});

test("the view ends up centred on the box", () => {
  const b = { minX: 10000, minY: 4000, maxX: 12000, maxY: 5000 };
  const v = crossProbeView({ scale: 0.1, x: 0, y: 0 }, b, 1000, 800);
  const cx = 11000 * v.scale + v.x;
  const cy = 4500 * v.scale + v.y;
  near(cx, 500, 1e-6);
  near(cy, 400, 1e-6);
});

test("the scale stays inside the studio's zoom limits", () => {
  const s = crossProbeScale(40, { minX: 0, minY: 0, maxX: 10, maxY: 10 }, 1000, 800);
  assert.ok(s <= 50 && s >= 1e-5);
});

const hierarchy: Record<string, SheetView> = {
  "": { symbolIds: ["R1", "U1"], childIds: ["a", "b"] },
  a: { symbolIds: ["C1", "C2"], childIds: ["c"] },
  b: { symbolIds: ["Q1"], childIds: [] },
  "a/c": { symbolIds: ["D1"], childIds: [] },
};

test("collectSheetRefs walks the hierarchy breadth first and keys each sheet by its path", async () => {
  const seen: string[] = [];
  const sheets = await collectSheetRefs(async (path) => {
    seen.push(path.join("/"));
    return hierarchy[path.join("/")]!;
  });
  assert.deepEqual(seen, ["", "a", "b", "a/c"]);
  assert.deepEqual([...sheets.keys()], ["", "a", "b", "a/c"]);
  assert.equal(sheetOfRef(sheets, "C2"), "a");
  assert.equal(sheetOfRef(sheets, "D1"), "a/c");
  assert.equal(sheetOfRef(sheets, "R1"), "");
  assert.equal(sheetOfRef(sheets, "X9"), null);
});

test("a hierarchy that never ends is cut off", async () => {
  const sheets = await collectSheetRefs(async () => ({ symbolIds: [], childIds: ["x"] }), 5);
  assert.equal(sheets.size, 5);
});
