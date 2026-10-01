import { test } from "node:test";
import assert from "node:assert/strict";
import { junctionPoints, type JunctionWire } from "./junctions";

function wire(net: string, pts: [number, number][]): JunctionWire {
  return { net, pts };
}

test("junctionPoints: two wires just touching end-to-end (2 endpoints) is not a junction", () => {
  const wires = [wire("A", [[0, 0], [1000, 0]]), wire("A", [[1000, 0], [2000, 0]])];
  assert.deepEqual(junctionPoints(wires), []);
});

test("junctionPoints: three wire endpoints coincident on the same net is a junction (the original rule)", () => {
  const wires = [
    wire("A", [[0, 0], [1000, 0]]),
    wire("A", [[1000, 0], [2000, 1000]]),
    wire("A", [[1000, 0], [2000, -1000]]),
  ];
  assert.deepEqual(junctionPoints(wires), [[1000, 0]]);
});

test("junctionPoints: three coincident endpoints on *different* nets is not a junction (would be a short, not a legitimate branch)", () => {
  const wires = [
    wire("A", [[0, 0], [1000, 0]]),
    wire("B", [[1000, 0], [2000, 1000]]),
    wire("C", [[1000, 0], [2000, -1000]]),
  ];
  assert.deepEqual(junctionPoints(wires), []);
});

test("junctionPoints: a wire endpoint landing on another wire's interior is a T-junction -- the gap this session fixes", () => {
  const bus = wire("GND", [[0, 0], [10_000, 0]]);
  const branch = wire("GND", [[5_000, 0], [5_000, 5_000]]);
  assert.deepEqual(junctionPoints([bus, branch]), [[5_000, 0]]);
});

test("junctionPoints: a T-junction is net-agnostic (landing on the interior is what merges the nets, so it can't require them to already match)", () => {
  const bus = wire("", [[0, 0], [10_000, 0]]);
  const branch = wire("NET_7", [[5_000, 0], [5_000, 5_000]]);
  assert.deepEqual(junctionPoints([bus, branch]), [[5_000, 0]]);
});

test("junctionPoints: a wire endpoint landing exactly on another wire's own endpoint (not interior) is not a T -- that's a plain 2-wire join, case (a) already decides it", () => {
  const w1 = wire("A", [[0, 0], [5_000, 0]]);
  const w2 = wire("A", [[5_000, 0], [5_000, 5_000]]);
  assert.deepEqual(junctionPoints([w1, w2]), []);
});

test("junctionPoints: a power symbol anchor dropped onto a wire's interior gets a dot", () => {
  const bus = wire("GND", [[0, 0], [10_000, 0]]);
  assert.deepEqual(junctionPoints([bus], [[5_000, 0]]), [[5_000, 0]]);
});

test("junctionPoints: a label anchor dropped onto a wire's interior gets a dot, same as a power symbol", () => {
  const bus = wire("DATA", [[0, 0], [10_000, 0]]);
  assert.deepEqual(junctionPoints([bus], [[2_500, 0]]), [[2_500, 0]]);
});

test("junctionPoints: an extra anchor NOT on any wire's interior contributes nothing", () => {
  const bus = wire("GND", [[0, 0], [10_000, 0]]);
  assert.deepEqual(junctionPoints([bus], [[50_000, 50_000]]), []);
});

test("junctionPoints: a diagonal wire never produces a T (every real segment here is axis-aligned; a diagonal one is defensively just never matched)", () => {
  const diag = wire("A", [[0, 0], [10_000, 10_000]]);
  const branch = wire("A", [[5_000, 5_000], [5_000, 8_000]]);
  assert.deepEqual(junctionPoints([diag, branch]), []);
});

test("junctionPoints: endpoint-coincidence and T-junction results are deduplicated when both would fire at the same point", () => {
  // Three wires end at (5000,0), AND a fourth's endpoint lands on one of
  // their interiors at that same point -- still exactly one dot there.
  const wires = [
    wire("A", [[0, 0], [5_000, 0]]),
    wire("A", [[5_000, 0], [5_000, 5_000]]),
    wire("A", [[5_000, 0], [5_000, -5_000]]),
  ];
  const dots = junctionPoints(wires, [[5_000, 0]]);
  assert.equal(dots.length, 1);
  assert.deepEqual(dots[0], [5_000, 0]);
});
