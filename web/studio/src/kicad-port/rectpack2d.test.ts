import { test } from "node:test";
import assert from "node:assert/strict";
import { EmptySpaces, findBestPacking, insertAndSplit, spreadRectangles, type RectXYWHF } from "./rectpack2d";
import { packFootprints, type PackItem } from "./packFootprints";

const r = (w: number, h: number): RectXYWHF => ({ x: 0, y: 0, w, h, flipped: false });

const overlaps = (a: RectXYWHF, b: RectXYWHF) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

test("insert_and_split: too big fails, an exact fit leaves no space, one equal side one split, else a huge and a tiny split along the axis with more left", () => {
  const sp = { x: 0, y: 0, w: 30, h: 40 };
  assert.equal(insertAndSplit({ w: 31, h: 1 }, sp).count, -1);
  assert.deepEqual(insertAndSplit({ w: 30, h: 40 }, sp), { count: 0, spaces: [] });
  assert.deepEqual(insertAndSplit({ w: 20, h: 40 }, sp), { count: 1, spaces: [{ x: 20, y: 0, w: 10, h: 40 }] });
  assert.deepEqual(insertAndSplit({ w: 30, h: 10 }, sp), { count: 1, spaces: [{ x: 0, y: 10, w: 30, h: 30 }] });
  // free_w 20 > free_h 10: the bigger split to the right, full height; the lesser under the image, its width
  assert.deepEqual(insertAndSplit({ w: 10, h: 30 }, sp), { count: 2, spaces: [{ x: 10, y: 0, w: 20, h: 40 }, { x: 0, y: 30, w: 10, h: 10 }] });
  // free_w 10 <= free_h 30: the bigger split below, full width; the lesser right of the image, its height
  assert.deepEqual(insertAndSplit({ w: 20, h: 10 }, sp), { count: 2, spaces: [{ x: 0, y: 10, w: 30, h: 30 }, { x: 20, y: 0, w: 10, h: 10 }] });
});

test("empty_spaces: the spaces are tried from the last; the one taken gives its slot to the last and its splits go at the end", () => {
  const root = new EmptySpaces({ w: 30, h: 40 });
  assert.deepEqual(root.insert({ w: 10, h: 30 }), { x: 0, y: 0, w: 10, h: 30, flipped: false });
  // spaces now [(10,0,20,40), (0,30,10,10)]: a 10x10 takes the last, (0,30)
  assert.deepEqual(root.insert({ w: 10, h: 10 }), { x: 0, y: 30, w: 10, h: 10, flipped: false });
  // then a 20x40 fills what is left exactly
  assert.deepEqual(root.insert({ w: 20, h: 40 }), { x: 10, y: 0, w: 20, h: 40, flipped: false });
  assert.equal(root.insert({ w: 1, h: 1 }), null);
  assert.deepEqual(root.rectsAabb(), { w: 30, h: 40 });
});

test("find_best_packing: one rectangle's bin is the rectangle (the both-sides search, then the width, then the height)", () => {
  const rects = [r(10, 5)];
  assert.deepEqual(findBestPacking(rects, { maxBinSide: 20, discardStep: 1, onSuccess: () => true, onFailure: () => true }), { w: 10, h: 5 });
  assert.deepEqual([rects[0]!.x, rects[0]!.y], [0, 0]);
  // a bin too small for any order: nothing fits, the order that fitted most is used and the failure reported
  let failed = 0;
  const big = [r(30, 30)];
  const out = findBestPacking(big, { maxBinSide: 20, discardStep: 1, onSuccess: () => true, onFailure: () => (failed++, false) });
  assert.ok(out !== null);
  assert.equal(failed, 1);
});

test("find_best_packing: a rectangle of no area takes no part and keeps its place", () => {
  const rects = [r(0, 7), r(4, 4)];
  rects[0]!.x = 3;
  findBestPacking(rects, { maxBinSide: 16, discardStep: 1, onSuccess: () => true, onFailure: () => true });
  assert.deepEqual([rects[0]!.x, rects[0]!.y], [3, 0]);
});

test("spreadRectangles: every block packed, none overlapping, inside the box it answers; the bin grows by a fifth until they fit", () => {
  const sizes: [number, number][] = [[120, 40], [40, 40], [40, 40], [80, 60], [30, 200], [55, 25], [90, 90], [10, 10]];
  const rects = sizes.map(([w, h]) => r(w, h));
  // a start far too small (10 x 10 units of 0.01 mm): `max_side * 1.2` until every block fits
  const aabb = spreadRectangles(rects, 100_000, 100_000, 10_000);
  assert.ok(aabb);
  for (let i = 0; i < rects.length; i++) {
    const a = rects[i]!;
    assert.ok(a.x >= 0 && a.y >= 0 && a.x + a.w <= aabb!.w && a.y + a.h <= aabb!.h, `rect ${i} inside the box`);
    for (let j = i + 1; j < rects.length; j++) assert.ok(!overlaps(a, rects[j]!), `${i} and ${j} apart`);
  }
  const total = sizes.reduce((s, [w, h]) => s + w * h, 0);
  assert.ok(aabb!.w * aabb!.h >= total);
  // the same blocks pack the same way every time
  const again = sizes.map(([w, h]) => r(w, h));
  spreadRectangles(again, 100_000, 100_000, 10_000);
  assert.deepEqual(again.map((a) => [a.x, a.y]), rects.map((a) => [a.x, a.y]));
});

test("packFootprints: blocks of different sizes are packed by rectpack2D: apart, a 1 mm gap kept, the arrangement's top-left on the selection's", () => {
  const mm = (v: number) => v * 1_000_000;
  const box = (x: number, y: number, w: number, h: number) => [mm(x), mm(y), mm(x + w), mm(y + h)] as const;
  const items: PackItem[] = [
    { ref: "U1", bbox: box(50, 50, 10, 10) },
    { ref: "J1", bbox: box(0, 30, 4, 20) },
    { ref: "C1", bbox: box(10, 10, 2, 1) },
    { ref: "C2", bbox: box(80, 10, 2, 1) },
    { ref: "R1", bbox: box(20, 70, 3, 1.5) },
    { ref: "R2", bbox: box(25, 70, 3, 1.5) },
  ];
  const { offsets, origin } = packFootprints(items);
  const moved = items.map((it) => {
    const [dx, dy] = offsets[it.ref]!;
    return [it.bbox[0] + dx, it.bbox[1] + dy, it.bbox[2] + dx, it.bbox[3] + dy] as const;
  });
  assert.deepEqual(origin, [0, mm(10)]);
  assert.equal(Math.min(...moved.map((b) => b[0])), origin[0]);
  assert.equal(Math.min(...moved.map((b) => b[1])), origin[1]);
  for (let i = 0; i < moved.length; i++)
    for (let j = i + 1; j < moved.length; j++) {
      const [a, b] = [moved[i]!, moved[j]!];
      const gapX = Math.max(b[0] - a[2], a[0] - b[2]);
      const gapY = Math.max(b[1] - a[3], a[1] - b[3]);
      assert.ok(Math.max(gapX, gapY) >= mm(1) - 10_000, `${items[i]!.ref} and ${items[j]!.ref} keep the gap`);
    }
});
