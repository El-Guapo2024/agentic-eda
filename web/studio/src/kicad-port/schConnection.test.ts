import { test } from "node:test";
import assert from "node:assert/strict";
import { selectConnection, selectNodeAt, type ExpandInput } from "./schConnection";

// w1 (0,0)-(100,0); w2 (100,0)-(200,0); w3 (200,0)-(300,0) with a pin at (200,0).
const input: ExpandInput = {
  wires: [
    { id: "w1", pts: [[0, 0], [100, 0]] },
    { id: "w2", pts: [[100, 0], [200, 0]] },
    { id: "w3", pts: [[200, 0], [300, 0]] },
    { id: "b1", pts: [[100, 0], [100, 50]], bus: true },
  ],
  labels: [{ id: "l1", at: [50, 0] }],
  pinPoints: [[200, 0]],
};

test("stage 1 (stop at junction/pin) stops at the pin; repeat press widens past it", () => {
  const first = selectConnection(input, ["w1"]);
  assert.deepEqual(first.sort(), ["l1", "w1", "w2"]);
  const second = selectConnection(input, first);
  assert.deepEqual(second.sort(), ["l1", "w1", "w2", "w3"]);
});

test("a bus never joins a wire", () => {
  assert.ok(!selectConnection(input, ["w1"]).includes("b1"));
});

test("a 3-way junction stops stage 1", () => {
  const j: ExpandInput = {
    wires: [
      { id: "a", pts: [[0, 0], [100, 0]] },
      { id: "b", pts: [[100, 0], [200, 0]] },
      { id: "c", pts: [[100, 0], [100, 100]] },
    ],
    labels: [],
    pinPoints: [],
  };
  // stage 1 adds nothing (junction) so it advances to stage 3 and takes everything
  assert.deepEqual(selectConnection(j, ["a"]).sort(), ["a", "b", "c"]);
});

test("selectNodeAt picks wires and labels at the point", () => {
  assert.deepEqual(selectNodeAt(input, [100, 0]).sort(), ["b1", "w1", "w2"]);
  assert.deepEqual(selectNodeAt(input, [50, 0]).sort(), ["l1", "w1"]);
});
