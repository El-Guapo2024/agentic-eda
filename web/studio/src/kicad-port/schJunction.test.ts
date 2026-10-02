import { test } from "node:test";
import assert from "node:assert/strict";
import { analyzePoint, isExplicitJunctionAllowed, junctionCandidates, type JunctionSchematic } from "./schJunction";

const empty: JunctionSchematic = { wires: [], pinTips: [], labels: [], busEntries: [] };
const wire = (...pts: [number, number][]) => ({ pts });
const bus = (...pts: [number, number][]) => ({ pts, bus: true });

test("a T (one wire ends on another's middle) allows a junction", () => {
  const sch = { ...empty, wires: [wire([0, 0], [20, 0]), wire([10, 0], [10, 10])] };
  assert.equal(isExplicitJunctionAllowed(sch, [10, 0]), true);
});

test("two wires crossing allow a junction; the same two meeting end to end do not", () => {
  const cross = { ...empty, wires: [wire([0, 0], [20, 0]), wire([10, -10], [10, 10])] };
  assert.equal(isExplicitJunctionAllowed(cross, [10, 0]), true, "four directions");
  const chain = { ...empty, wires: [wire([0, 0], [10, 0]), wire([10, 0], [20, 0])] };
  assert.equal(isExplicitJunctionAllowed(chain, [10, 0]), false, "two directions: nothing to join");
});

test("a polyline's own bend is two lines meeting: a third wire there makes three directions", () => {
  const bend = { ...empty, wires: [wire([0, 0], [10, 0], [10, 10])] };
  assert.equal(isExplicitJunctionAllowed(bend, [10, 0]), false);
  const branch = { ...empty, wires: [wire([0, 0], [10, 0], [10, 10]), wire([10, 0], [20, 0])] };
  assert.equal(isExplicitJunctionAllowed(branch, [10, 0]), true);
});

test("a pin adds one direction of its own: pin + wire end is not enough, pin on a through-wire is", () => {
  const pinAndEnd = { ...empty, wires: [wire([0, 0], [10, 0])], pinTips: [[10, 0]] as [number, number][] };
  assert.equal(isExplicitJunctionAllowed(pinAndEnd, [10, 0]), false);
  const pinOnWire = { ...empty, wires: [wire([0, 0], [20, 0])], pinTips: [[10, 0]] as [number, number][] };
  assert.equal(isExplicitJunctionAllowed(pinOnWire, [10, 0]), true);
});

test("nothing, a lone wire or a lone through-wire never qualifies", () => {
  assert.equal(isExplicitJunctionAllowed(empty, [0, 0]), false);
  const lone = { ...empty, wires: [wire([0, 0], [20, 0])] };
  assert.equal(isExplicitJunctionAllowed(lone, [10, 0]), false);
  assert.equal(isExplicitJunctionAllowed(lone, [0, 0]), false);
  assert.equal(isExplicitJunctionAllowed(lone, [50, 50]), false);
});

test("a bus entry at the point: allowed only when it feeds several wires", () => {
  // a vertical bus with an entry whose root is on it
  const entry = { a: [0, 10] as [number, number], b: [2, 8] as [number, number] };
  const base = { ...empty, wires: [bus([0, 0], [0, 20])], busEntries: [entry] };
  // at the root: the bus passes through (2 bus directions) and the entry adds one -- a bus junction, but no wire feeds it
  const root = analyzePoint(base, [0, 10]);
  assert.equal(root.hasBusEntry, true);
  assert.equal(root.isJunction, true);
  assert.equal(root.hasBusEntryToMultipleWires, false);
  assert.equal(isExplicitJunctionAllowed(base, [0, 10]), false);
  // at the far end with a single wire: the entry + the wire are two directions, nothing to join
  const one = { ...base, wires: [...base.wires, wire([2, 8], [10, 8])] };
  assert.equal(analyzePoint(one, [2, 8]).isJunction, false);
  // two wires leave it: three directions on the wire layer and exactly one bus (the entry's own)
  const two = { ...base, wires: [...base.wires, wire([2, 8], [10, 8]), wire([2, 8], [2, 0])] };
  const far = analyzePoint(two, [2, 8]);
  assert.equal(far.hasBusEntryToMultipleWires, true);
  assert.equal(isExplicitJunctionAllowed(two, [2, 8]), true);
});

test("three buses meeting make a bus junction on their own layer", () => {
  const sch = { ...empty, wires: [bus([0, 0], [20, 0]), bus([10, 0], [10, 10])] };
  assert.equal(analyzePoint(sch, [10, 0]).isJunction, true);
  assert.equal(analyzePoint(sch, [10, 0]).hasBusAtPoint, true);
});

test("junctionCandidates: wire vertices, pins and interior crossings -- each once", () => {
  const sch = { ...empty, wires: [wire([0, 10], [20, 10]), wire([10, 0], [10, 20]), wire([20, 10], [20, 30])], pinTips: [[5, 5]] as [number, number][] };
  const got = junctionCandidates(sch).map((p) => `${p[0]},${p[1]}`).sort();
  assert.deepEqual(got, ["0,10", "10,0", "10,10", "10,20", "20,10", "20,30", "5,5"].sort());
});

test("junctionCandidates: parallel and end-touching segments make no crossing", () => {
  const sch = { ...empty, wires: [wire([0, 0], [10, 0]), wire([0, 5], [10, 5]), wire([10, 0], [10, 20])] };
  assert.deepEqual(
    junctionCandidates(sch).map((p) => `${p[0]},${p[1]}`).sort(),
    ["0,0", "0,5", "10,0", "10,20", "10,5"].sort(),
  );
});

test("a diagonal crossing rounds to a whole um", () => {
  const sch = { ...empty, wires: [wire([0, 0], [10, 10]), wire([0, 10], [10, 0])] };
  assert.ok(junctionCandidates(sch).some((p) => p[0] === 5 && p[1] === 5));
});

test("zero-length wires are ignored", () => {
  const sch = { ...empty, wires: [wire([5, 5], [5, 5]), wire([0, 5], [10, 5])] };
  assert.equal(isExplicitJunctionAllowed(sch, [5, 5]), false);
});
