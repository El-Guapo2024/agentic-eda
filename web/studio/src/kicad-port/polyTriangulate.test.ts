import { test } from "node:test";
import assert from "node:assert/strict";
import { triangulate, type Pt, type Triangle } from "./polyTriangulate";

const ringArea = (r: readonly Pt[]): number => {
  let twice = 0;
  for (let i = 0; i < r.length; i++) {
    const [x0, y0] = r[i]!;
    const [x1, y1] = r[(i + 1) % r.length]!;
    twice += x0 * y1 - x1 * y0;
  }
  return Math.abs(twice) / 2;
};

const trianglesArea = (tris: readonly Triangle[]): number => tris.reduce((sum, t) => sum + ringArea(t), 0);

const square = (x: number, y: number, side: number): Pt[] => [
  [x, y],
  [x + side, y],
  [x + side, y + side],
  [x, y + side],
];

test("triangulate: a triangle is itself, a square is two triangles covering it", () => {
  assert.equal(triangulate({ outline: [[0, 0], [10, 0], [0, 10]], holes: [] }).length, 1);
  const tris = triangulate({ outline: square(0, 0, 10), holes: [] });
  assert.equal(tris.length, 2);
  assert.equal(trianglesArea(tris), 100);
});

test("triangulate: n vertices of a simple polygon give n-2 triangles that cover exactly its area, whichever way it winds", () => {
  const l: Pt[] = [[0, 0], [30, 0], [30, 10], [10, 10], [10, 30], [0, 30]];
  for (const ring of [l, [...l].reverse()]) {
    const tris = triangulate({ outline: ring, holes: [] });
    assert.equal(tris.length, ring.length - 2);
    assert.equal(trianglesArea(tris), ringArea(l), "an L: 30x10 + 10x20");
  }
});

test("triangulate: a concave star's triangles stay inside it (their area is the star's)", () => {
  const star: Pt[] = [];
  for (let k = 0; k < 10; k++) {
    const r = k % 2 === 0 ? 100 : 40;
    const a = (k * Math.PI) / 5;
    star.push([Math.round(r * Math.cos(a)), Math.round(r * Math.sin(a))]);
  }
  const tris = triangulate({ outline: star, holes: [] });
  assert.equal(tris.length, 8);
  assert.ok(Math.abs(trianglesArea(tris) - ringArea(star)) < 1e-6, `${trianglesArea(tris)} vs ${ringArea(star)}`);
});

test("triangulate: a hole is left out -- the triangles cover the outline minus the hole", () => {
  const tris = triangulate({ outline: square(0, 0, 100), holes: [square(40, 40, 20)] });
  // 8 vertices and one hole: vertices + 2 * holes - 2 triangles.
  assert.equal(tris.length, 8);
  assert.equal(trianglesArea(tris), 100 * 100 - 20 * 20);
  // No triangle's centre falls inside the hole.
  for (const [a, b, c] of tris) {
    const cx = (a[0] + b[0] + c[0]) / 3;
    const cy = (a[1] + b[1] + c[1]) / 3;
    assert.ok(!(cx > 40 && cx < 60 && cy > 40 && cy < 60), `a triangle sits in the hole: ${[a, b, c]}`);
  }
});

test("triangulate: several holes, a hole wound the same way as the outline, and a hole near the edge all work", () => {
  const holes = [square(10, 10, 10), square(60, 10, 10), square(10, 60, 10), [...square(60, 60, 10)].reverse()];
  const tris = triangulate({ outline: square(0, 0, 100), holes });
  // 20 vertices and 4 holes would be 26 triangles; collinear vertices a bridge leaves are dropped (`filterPoints`), so up to that many.
  assert.ok(tris.length >= 18 && tris.length <= 26, `${tris.length}`);
  assert.equal(trianglesArea(tris), 100 * 100 - 4 * 100);
  const near = triangulate({ outline: square(0, 0, 100), holes: [square(1, 1, 10)] });
  assert.equal(trianglesArea(near), 100 * 100 - 100);
});

test("triangulate: nothing to triangulate gives no triangles, and a collapsed hole is ignored", () => {
  assert.deepEqual(triangulate({ outline: [], holes: [] }), []);
  assert.deepEqual(triangulate({ outline: [[0, 0], [5, 5]], holes: [] }), []);
  assert.deepEqual(triangulate({ outline: [[0, 0], [10, 0], [20, 0]], holes: [] }), [], "collinear: no area");
  const tris = triangulate({ outline: square(0, 0, 10), holes: [[[5, 5]], [[3, 3], [4, 4]]] });
  assert.equal(trianglesArea(tris), 100, "a one-point and a two-point 'hole' leave the outline whole");
});

test("triangulate: the polygon of a real pour (a rounded plate with thermal-relief slots) covers its area", () => {
  // A plate with 3x3 round-ish holes (octagons), the shape a zone fill has around pads.
  const octagon = (cx: number, cy: number, r: number): Pt[] => Array.from({ length: 8 }, (_, k) => [Math.round(cx + r * Math.cos((k * Math.PI) / 4)), Math.round(cy + r * Math.sin((k * Math.PI) / 4))] as Pt);
  const holes: Pt[][] = [];
  for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) holes.push(octagon(200 + i * 300, 200 + j * 300, 60));
  const tris = triangulate({ outline: square(0, 0, 1000), holes });
  const expected = 1000 * 1000 - holes.reduce((sum, h) => sum + ringArea(h), 0);
  assert.ok(Math.abs(trianglesArea(tris) - expected) < 1, `${trianglesArea(tris)} vs ${expected}`);
});
