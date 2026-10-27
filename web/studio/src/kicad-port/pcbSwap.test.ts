import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { planSwap } from "./pcbSwap";

const part = (ref: string, x: number, y: number) => ({ ref, placed: true, at: [x, y], rot: 0, side: "top", pads: [{ num: "1", net: "N", x, y, w: 500, h: 500, round: false, th: false }] });

function board(locked: string[] = []): BoardState {
  return {
    name: "t",
    layers: ["F.Cu", "B.Cu"],
    parts: [part("U1", 0, 0), part("U2", 10_000, 0)],
    locked,
    routing: {
      tracks: [{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [5000, 0]] }],
      vias: [{ id: "v1", net: "N", x: 1000, y: 2000, d: 600, drill: 300 }, { id: "v2", net: "N", x: 7000, y: 9000, d: 600, drill: 300 }],
      zones: [],
    },
    drawings: {
      shapes: [],
      texts: [
        { id: "a", content: "A", x: 0, y: 5000, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false },
        { id: "b", content: "B", x: 3000, y: 6000, angle: 0, layer: "B.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false },
      ],
      groups: [],
      dimensions: [],
    },
  } as unknown as BoardState;
}

test("Swap takes vias, texts and other items, not footprints only (round 6 E6-2)", () => {
  assert.deepEqual(planSwap(board(), ["v1", "v2"]), [
    { op: "move_items", ids: ["v1"], dx: 6000, dy: 7000 },
    { op: "move_items", ids: ["v2"], dx: -6000, dy: -7000 },
  ]);
  // Two texts: positions and, as items of one type, layers.
  const cmds = planSwap(board(), ["a", "b"])!;
  assert.deepEqual(cmds.filter((c) => c.op === "move_items"), [
    { op: "move_items", ids: ["a"], dx: 3000, dy: 1000 },
    { op: "move_items", ids: ["b"], dx: -3000, dy: -1000 },
  ]);
  assert.deepEqual(cmds.filter((c) => c.op === "edit_text").map((c) => [(c as { id: string }).id, (c as { layer: string }).layer]), [["a", "B.SilkS"], ["b", "F.SilkS"]]);
  // Footprints only: swap_chain (orientation and side too); a pad stands for its footprint; a straight track is left out.
  assert.deepEqual(planSwap(board(), ["U1.1", "U2", "t1"]), [{ op: "swap_chain", parts: ["U1", "U2"] }]);
});

test("Swap leaves locked items out, and does nothing with fewer than two left (round 6 E6-3)", () => {
  assert.equal(planSwap(board(["U1"]), ["U1", "U2"]), null);
  assert.equal(planSwap(board(), ["v1"]), null);
});
