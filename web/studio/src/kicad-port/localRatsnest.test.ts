import { test } from "node:test";
import assert from "node:assert/strict";
import { offsetRatsnestForPreview, type RatsnestBoardLike, type RatsnestEdgeLike } from "./localRatsnest";

const board: RatsnestBoardLike = {
  parts: [
    { ref: "U1", at: [1000, 1000], pads: [{ x: 1100, y: 1000 }, { x: 900, y: 1000 }] },
    { ref: "R1", at: [5000, 5000], pads: [{ x: 5100, y: 5000 }] },
  ],
};

test("offsetRatsnestForPreview: null preview returns the edges unchanged", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, null);
  assert.deepEqual(result, edges);
});

test("offsetRatsnestForPreview: a moving part's pad endpoint shifts by the preview delta, the stationary end does not", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, { refs: ["U1"], kind: "part", dxUm: 50, dyUm: -20 });
  assert.deepEqual(result[0]!.from, [1150, 980]);
  assert.deepEqual(result[0]!.to, [5100, 5000]);
});

test("offsetRatsnestForPreview: a non-part move (via/shape/text) never touches the ratsnest", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, { refs: ["U1"], kind: "via", dxUm: 50, dyUm: -20 });
  assert.deepEqual(result, edges);
});

test("offsetRatsnestForPreview: zero delta is a no-op even for a part move", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, { refs: ["U1"], kind: "part", dxUm: 0, dyUm: 0 });
  assert.deepEqual(result, edges);
});

test("offsetRatsnestForPreview: both endpoints of an edge move when both their parts are in the preview", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, { refs: ["U1", "R1"], kind: "part", dxUm: 10, dyUm: 10 });
  assert.deepEqual(result[0]!.from, [1110, 1010]);
  assert.deepEqual(result[0]!.to, [5110, 5010]);
});
