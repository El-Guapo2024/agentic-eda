import { test } from "node:test";
import assert from "node:assert/strict";
import { compareByRef, footprintBBox, optimalCountPerLine, packFootprints, packRects, planPack, refDesPrefix, trailingInt, type Box, type PackItem, type PackPart } from "./packFootprints";

test("refDesPrefix (UTIL::GetRefDesPrefix): up to the last non-digit, non-? character", () => {
  assert.equal(refDesPrefix("R12"), "R");
  assert.equal(refDesPrefix("U?"), "U");
  assert.equal(refDesPrefix("U1A"), "U1A");
  assert.equal(refDesPrefix("123"), "");
  assert.equal(refDesPrefix("TP"), "TP");
});

test("trailingInt (GetTrailingInt)", () => {
  assert.equal(trailingInt("R12"), 12);
  assert.equal(trailingInt("R"), 0);
  assert.equal(trailingInt("U1A"), 0);
  assert.equal(trailingInt("C007"), 7);
});

test("compareByRef (compareFootprintsbyRef): prefix first, then the number, not the text", () => {
  assert.ok(compareByRef("C2", "R1") < 0);
  assert.ok(compareByRef("R2", "R10") < 0, "2 < 10 numerically");
  assert.ok(compareByRef("R10", "R2") > 0);
  assert.equal(compareByRef("R5", "R5"), 0);
  assert.ok(compareByRef("R1", "RN1") < 0, "prefix 'R' < prefix 'RN'");
});

test("optimalCountPerLine: a short run stays one line; a long one wraps near a square and takes SpreadFootprints' remainder rule", () => {
  // 4 wide parts: 1000*4/2000 = 2 <= 5 -> no wrapping, remainder 0
  assert.equal(optimalCountPerLine(4, [2000, 1000]), 4);
  // 30 wide parts: ratio 15 > 5 -> initial = trunc(sqrt(2000*1000*30)/1000) = 7, remainder 2;
  // the search over 5..9 ends on 8 (the C++ keeps any `r == 0 || r >= optimalRemainder`)
  assert.equal(optimalCountPerLine(30, [2000, 1000]), 8);
  // one part
  assert.equal(optimalCountPerLine(1, [2000, 1000]), 1);
  // tall parts (not "vertical"): 20 of 1000 x 2000 -> ratio trunc(1000*20/2000) = 10 > 5, initial = trunc(6324/1000) = 6,
  // remainder 2; i = 4..8 gives r = 0, 0, 2, 6, 4 and the `r == 0 || r >= remainder` rule keeps 7 (r = 6)
  assert.equal(optimalCountPerLine(20, [1000, 2000]), 7);
});

function box(x: number, y: number, w: number, h: number): Box {
  return [x, y, x + w, y + h];
}

/** The footprint boxes after applying `offsets`. */
function moved(items: PackItem[], offsets: Record<string, [number, number]>): { ref: string; b: [number, number, number, number] }[] {
  return items.map((i) => {
    const [dx, dy] = offsets[i.ref]!;
    return { ref: i.ref, b: [i.bbox[0] + dx, i.bbox[1] + dy, i.bbox[2] + dx, i.bbox[3] + dy] as [number, number, number, number] };
  });
}

function overlap(a: [number, number, number, number], b: [number, number, number, number]): boolean {
  return a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
}

test("packFootprints: identical parts become one ordered column/row starting at the selection's top-left", () => {
  // four 2 x 1 mm parts scattered, given out of order
  const items: PackItem[] = [
    { ref: "R3", bbox: box(10000, 8000, 2000, 1000) },
    { ref: "R1", bbox: box(2000, 3000, 2000, 1000) },
    { ref: "R4", bbox: box(6000, 12000, 2000, 1000) },
    { ref: "R2", bbox: box(9000, 2000, 2000, 1000) },
  ];
  const { offsets, origin } = packFootprints(items);
  assert.deepEqual(origin, [2000, 2000], "the union box's top-left");
  const m = moved(items, offsets);
  const byRef = Object.fromEntries(m.map((x) => [x.ref, x.b]));
  // wide parts (x >= y) are stacked in a column ("vertical"), ordered R1..R4, one cell (size + 1 mm gap) apart
  for (const [i, ref] of ["R1", "R2", "R3", "R4"].entries()) {
    assert.deepEqual(byRef[ref]!.slice(0, 2), [2000, 2000 + 2000 * i], ref);
    assert.equal(byRef[ref]![2] - byRef[ref]![0], 2000, "a footprint keeps its size");
  }
});

test("packFootprints: a single footprint stays put (its own box is the target box)", () => {
  const { offsets } = packFootprints([{ ref: "U1", bbox: box(5000, 6000, 4000, 3000) }]);
  assert.deepEqual(offsets["U1"], [0, 0]);
});

test("packFootprints: mixed sizes never overlap, keep the selection's top-left, and each keep their size", () => {
  const sizes: [number, number][] = [
    [2000, 1000],
    [2000, 1000],
    [3000, 3000],
    [1000, 500],
    [1000, 500],
    [1000, 500],
    [8000, 5000],
    [2000, 1000],
  ];
  const items: PackItem[] = sizes.map(([w, h], i) => ({ ref: `X${i + 1}`, bbox: box(1000 * i, 700 * (i % 3), w, h) }));
  const { offsets, origin } = packFootprints(items);
  const m = moved(items, offsets);
  const minX = Math.min(...m.map((x) => x.b[0]));
  const minY = Math.min(...m.map((x) => x.b[1]));
  assert.deepEqual([minX, minY], origin, "the packed arrangement's top-left is the original selection's top-left");
  for (let i = 0; i < m.length; i++) {
    for (let j = i + 1; j < m.length; j++) assert.ok(!overlap(m[i]!.b, m[j]!.b), `${m[i]!.ref} overlaps ${m[j]!.ref}`);
  }
  for (const f of m) {
    const src = items.find((i) => i.ref === f.ref)!;
    assert.equal(f.b[2] - f.b[0], src.bbox[2] - src.bbox[0]);
    assert.equal(f.b[3] - f.b[1], src.bbox[3] - src.bbox[1]);
  }
});

test("packFootprints: a part and its 90-degree twin share one group (VECTOR2 orders by squared length), first-seen size wins", () => {
  // 1.6 x 0.8 and 0.8 x 1.6 -> cells (2600,1800) and (1800,2600): equal length, one std::map key
  const items: PackItem[] = [
    { ref: "R1", bbox: box(0, 0, 1600, 800) },
    { ref: "R2", bbox: box(5000, 5000, 800, 1600) },
  ];
  const { offsets } = packFootprints(items);
  const m = moved(items, offsets);
  const r1 = m[0]!.b;
  const r2 = m[1]!.b;
  // R1's size (2600 >= 1800) is the group key -> a column ("vertical"), ordered R1 then R2, one 1800 um cell apart
  assert.deepEqual([r1[0], r1[1]], [0, 0]);
  assert.deepEqual([r2[0], r2[1]], [0, 1800]);
  assert.ok(!overlap(r1, r2));
});

test("packFootprints: a 1 mm gap separates neighbours of different groups too", () => {
  const items: PackItem[] = [
    { ref: "A1", bbox: box(0, 0, 3000, 3000) },
    { ref: "B1", bbox: box(100, 100, 1000, 1000) },
    { ref: "B2", bbox: box(200, 200, 1000, 1000) },
  ];
  const m = moved(items, packFootprints(items).offsets);
  for (let i = 0; i < m.length; i++) {
    for (let j = i + 1; j < m.length; j++) {
      const a = m[i]!.b;
      const b = m[j]!.b;
      const gapX = Math.max(b[0] - a[2], a[0] - b[2]);
      const gapY = Math.max(b[1] - a[3], a[1] - b[3]);
      assert.ok(Math.max(gapX, gapY) >= 0, `${m[i]!.ref}/${m[j]!.ref} overlap`);
    }
  }
});

test("packRects: the smallest square bin the shelf packer can use, never overlapping", () => {
  const rects = [
    { w: 100, h: 50 },
    { w: 100, h: 50 },
    { w: 50, h: 50 },
    { w: 50, h: 50 },
  ];
  const pos = packRects(rects, 10);
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      const a = [pos[i]!.x, pos[i]!.y, pos[i]!.x + rects[i]!.w, pos[i]!.y + rects[i]!.h] as const;
      const b = [pos[j]!.x, pos[j]!.y, pos[j]!.x + rects[j]!.w, pos[j]!.y + rects[j]!.h] as const;
      assert.ok(!(a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]), `${i} overlaps ${j}`);
    }
  }
  const side = Math.max(...rects.map((r, i) => pos[i]!.x + r.w), ...rects.map((r, i) => pos[i]!.y + r.h));
  assert.ok(side <= 150, `bin side ${side} should be tight for 4 rects of total area 15000 (<= 150)`);
});

test("packRects: nothing to pack", () => {
  assert.deepEqual(packRects([], 5), []);
});

test("footprintBBox: anchor +- 0.25 mm merged with the courtyard and the pads; null when unplaced", () => {
  assert.equal(footprintBBox({ ref: "U1", placed: false }), null);
  // no courtyard, no pads: the minimum box around the anchor
  assert.deepEqual(footprintBBox({ ref: "TP1", placed: true, at: [1000, 2000] }), [750, 1750, 1250, 2250]);
  // a courtyard bigger than the anchor box wins
  assert.deepEqual(footprintBBox({ ref: "R1", placed: true, at: [1000, 2000], courtyard: [200, 1500, 1800, 2500] }), [200, 1500, 1800, 2500]);
  // a pad poking out of the courtyard grows the box
  assert.deepEqual(
    footprintBBox({ ref: "J1", placed: true, at: [1000, 2000], courtyard: [500, 1500, 1500, 2500], pads: [{ x: 1700, y: 2000, w: 800, h: 400 }] }),
    [500, 1500, 2100, 2500],
  );
});

test("planPack: keeps footprints only (unknown refs, unplaced parts and locked parts drop out)", () => {
  const parts: PackPart[] = [
    { ref: "R1", placed: true, at: [1000, 1000], courtyard: [0, 500, 2000, 1500] },
    { ref: "R2", placed: true, at: [9000, 1000], courtyard: [8000, 500, 10000, 1500] },
    { ref: "R3", placed: true, at: [5000, 5000], courtyard: [4000, 4500, 6000, 5500] },
    { ref: "R4", placed: false },
  ];
  const plan = planPack(parts, ["R2", "R1", "R3", "R4", "T7", "R2"], new Set(["R3"]));
  assert.ok(plan);
  assert.deepEqual(plan.refs, ["R2", "R1"], "order kept, dupes/unknown/unplaced/locked gone");
  assert.deepEqual(Object.keys(plan.offsets).sort(), ["R1", "R2"]);
  assert.equal(planPack(parts, ["R4", "T7"], new Set()), null);
  assert.equal(planPack(parts, ["R3"], new Set(["R3"])), null);
});
