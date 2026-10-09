import { test } from "node:test";
import assert from "node:assert/strict";
import { carryRatsnest, offsetRatsnestForPreview, type RatsnestBoardLike, type RatsnestEdgeLike } from "./localRatsnest";

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

test("offsetRatsnestForPreview: Pack and Move -- each part's endpoint takes its own shift on top of the shared one, even at zero shared delta", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, { refs: ["U1", "R1"], kind: "part", dxUm: 0, dyUm: 0, perRefOffsetUm: { U1: [200, 0], R1: [-300, 50] } });
  assert.deepEqual(result[0]!.from, [1300, 1000]);
  assert.deepEqual(result[0]!.to, [4800, 5050]);
  const dragged = offsetRatsnestForPreview(edges, board, { refs: ["U1", "R1"], kind: "part", dxUm: 10, dyUm: 10, perRefOffsetUm: { U1: [200, 0] } });
  assert.deepEqual(dragged[0]!.from, [1310, 1010]);
  assert.deepEqual(dragged[0]!.to, [5110, 5010], "a part with no own shift just follows the cursor");
});

test("offsetRatsnestForPreview: both endpoints of an edge move when both their parts are in the preview", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  const result = offsetRatsnestForPreview(edges, board, { refs: ["U1", "R1"], kind: "part", dxUm: 10, dyUm: 10 });
  assert.deepEqual(result[0]!.from, [1110, 1010]);
  assert.deepEqual(result[0]!.to, [5110, 5010]);
});

test("carryRatsnest: an endpoint on a carried pad, via or track end goes where the carry takes it, the other end stays", () => {
  const edges: RatsnestEdgeLike[] = [
    { net: "N1", from: [1100, 1000], to: [5100, 5000] },
    { net: "N2", from: [7000, 7000], to: [8000, 8000] },
    { net: "N3", from: [9000, 9000], to: [9500, 9000] },
  ];
  const carried = { moving: { parts: [board.parts[0]!], routing: { vias: [{ x: 7000, y: 7000 }], tracks: [{ pts: [[9000, 9000], [9200, 9000]] as [number, number][] }] } } };
  // a turn of 180 degrees about the origin, then nothing else
  const result = carryRatsnest(edges, board, carried, (pt) => [-pt[0], -pt[1]]);
  assert.deepEqual(result[0]!.from, [-1100, -1000]);
  assert.deepEqual(result[0]!.to, [5100, 5000]);
  assert.deepEqual(result[1]!.from, [-7000, -7000]);
  assert.deepEqual(result[1]!.to, [8000, 8000]);
  assert.deepEqual(result[2]!.from, [-9000, -9000]);
  assert.deepEqual(result[2]!.to, [9500, 9000]);
});

test("carryRatsnest: nothing carried leaves the edges alone", () => {
  const edges: RatsnestEdgeLike[] = [{ net: "N1", from: [1100, 1000], to: [5100, 5000] }];
  assert.deepEqual(carryRatsnest(edges, board, { moving: { parts: [], routing: null } }, (pt) => [pt[0] + 1, pt[1]]), edges);
});
