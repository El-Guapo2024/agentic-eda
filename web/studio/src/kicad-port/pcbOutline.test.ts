import { test } from "node:test";
import assert from "node:assert/strict";
import { boardOutlinePolygons, boardOutlineRings, chainClosedRings } from "./pcbOutline";
import type { BoardState } from "../api/types";

type P = [number, number];
const square = (x0: number, y0: number, x1: number, y1: number): P[] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];

function board(over: Partial<BoardState>): Pick<BoardState, "outline" | "outline_polys" | "drawings"> {
  return { outline: null, drawings: null, ...over } as Pick<BoardState, "outline" | "outline_polys" | "drawings">;
}

test("the backend's outlines and their cutouts are the board", () => {
  const polys = boardOutlinePolygons(board({ outline: square(0, 0, 40000, 30000), outline_polys: [{ outer: square(0, 0, 40000, 30000), holes: [square(10000, 10000, 12000, 12000)] }] }));
  assert.equal(polys.length, 1);
  assert.equal(polys[0]!.holes.length, 1);
});

test("without outline_polys the single polygon is the board", () => {
  const polys = boardOutlinePolygons(board({ outline: square(0, 0, 10, 10) }));
  assert.deepEqual(polys, [{ outer: square(0, 0, 10, 10), holes: [] }]);
  assert.deepEqual(boardOutlinePolygons(board({ outline: null })), []);
  assert.deepEqual(boardOutlinePolygons(board({ outline: [[0, 0], [1, 1]] })), [], "two points are not an outline");
});

test("several outlines are all kept", () => {
  const two = boardOutlinePolygons(board({ outline_polys: [{ outer: square(0, 0, 10, 10), holes: [] }, { outer: square(20, 0, 30, 10), holes: [] }] }));
  assert.equal(two.length, 2);
});

test("the rings are the outers and the cutouts of the backend's outline", () => {
  const rings = boardOutlineRings(board({ outline: square(0, 0, 10, 10), outline_polys: [{ outer: square(0, 0, 10, 10), holes: [square(2, 2, 4, 4), square(6, 6, 8, 8)] }] }));
  assert.equal(rings.length, 3);
});

test("without the backend's outline the rings fall back to the drawn Edge.Cuts, then the polygon", () => {
  const b = board({
    outline: square(0, 0, 10, 10),
    drawings: { shapes: [{ id: "a", kind: "circle", layer: "Edge.Cuts", stroke_width: 50, filled: false, center: [5, 5], end: [7, 5] }] } as unknown as BoardState["drawings"],
  });
  const rings = boardOutlineRings(b);
  assert.equal(rings.length, 1, "the circle is a loop by itself");
  assert.deepEqual(boardOutlineRings(board({ outline: square(0, 0, 10, 10) })), [square(0, 0, 10, 10)]);
});

test("open polylines whose ends meet within the epsilon chain into a closed ring", () => {
  const rings = chainClosedRings([[[0, 0], [10, 0]], [[10, 0], [10, 10]], [[10, 10], [0, 10]], [[0, 10], [0, 0]]]);
  assert.equal(rings.length, 1);
  assert.equal(rings[0]!.length, 4);
  assert.equal(chainClosedRings([[[0, 0], [10, 0]], [[10, 0], [10, 10]]]).length, 0, "a chain that never closes is dropped");
});
