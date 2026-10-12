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

test("a selected pin starts the walk from the wires on its point, and stays selected; Select Node on a pin end picks the pin", () => {
  const withPins: ExpandInput = { ...input, pins: [{ id: "pin:U1:1", at: [0, 0] }, { id: "pin:U1:2", at: [200, 0] }] };
  assert.deepEqual(selectConnection(withPins, ["pin:U1:1"]).sort(), ["l1", "pin:U1:1", "w1", "w2"]);
  // a pin with no wire on it has nothing to walk
  assert.deepEqual(selectConnection({ ...withPins, pins: [{ id: "pin:U1:3", at: [900, 900] }] }, ["pin:U1:3"]), ["pin:U1:3"]);
  assert.deepEqual(selectNodeAt(withPins, [0, 0]), ["pin:U1:1"], "a pin wins over the wire that ends at it");
  assert.deepEqual(selectNodeAt(withPins, [100, 0]).sort(), ["b1", "w1", "w2"]);
});
