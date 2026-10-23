import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { gateSwapChoice, padsOnly, unselectedPadLabel } from "./gateSwap";

function board(parts: Part[]): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts, rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" } as BoardState;
}
const pad = (num: string): Pad => ({ num, net: null, x: 0, y: 0, w: 600, h: 600, round: false, th: false });
/** A quad gate U1 (four units of two pins) and a single-gate R1; U2 is a second quad. */
const part = (ref: string, units?: Part["units"]): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: null, pads: ["1", "2", "3", "4", "5", "6", "7"].map(pad), units }) as Part;
const quad = [
  { name: "A", pins: ["1", "2"] },
  { name: "B", pins: ["3", "4"] },
  { name: "C", pins: ["5", "6"] },
  { name: "D", pins: ["7"] },
];

test("pads of two units of one footprint are the gates to swap with each other, when the units have as many pins", () => {
  const b = board([part("U1", quad), part("R1")]);
  const two = gateSwapChoice(b, ["U1.1", "U1.3"]);
  assert.deepEqual([two.footprint, two.hit, two.multiple, two.targets], ["U1", [0, 1], true, null]);
  // The selection order does not matter to what is hit: the units come in unit order (the verb takes the pads' order).
  assert.deepEqual(gateSwapChoice(b, ["U1.5", "U1.1"]).hit, [0, 2]);
  // Unit D has one pin where A has two: not swappable with it.
  assert.equal(gateSwapChoice(b, ["U1.1", "U1.7"]).multiple, false);
});

test("a pad of one unit offers the other units with as many pins, by name", () => {
  const b = board([part("U1", quad)]);
  const one = gateSwapChoice(b, ["U1.2"]);
  assert.deepEqual([one.footprint, one.hit, one.multiple, one.targets], ["U1", [0], false, ["B", "C"]]);
  assert.deepEqual(gateSwapChoice(b, ["U1.7"]).targets, [], "D has no unit with one pin to go to: the entry list is empty");
  assert.deepEqual(gateSwapChoice(b, ["U1.1", "U1.2"]).targets, ["B", "C"], "two pads of one unit are one unit");
});

test("a footprint with fewer than two units, a pad in no unit, two footprints and anything but pads give no gate swap", () => {
  const b = board([part("U1", quad), part("U2", quad), part("R1"), part("Q1", [{ name: "A", pins: ["1", "2"] }])]);
  assert.equal(gateSwapChoice(b, ["R1.1", "R1.2"]).footprint, null, "no units");
  assert.equal(gateSwapChoice(b, ["Q1.1"]).footprint, null, "one unit is no gate to swap");
  assert.equal(gateSwapChoice(b, ["U1.1", "U2.3"]).footprint, null, "two footprints");
  assert.equal(gateSwapChoice(b, ["U1"]).footprint, null, "a footprint is no pad");
  assert.equal(gateSwapChoice(b, []).footprint, null);
  // A pad that is in no unit is passed over, like the C++ `continue`.
  const odd = board([part("U1", [{ name: "A", pins: ["1"] }, { name: "B", pins: ["2"] }])]);
  assert.deepEqual(gateSwapChoice(odd, ["U1.5", "U1.1"]).hit, [0]);
});

test("a selection of pads only is what Swap Pad Nets takes, in its order", () => {
  const b = board([part("U1", quad), part("R1")]);
  assert.deepEqual(padsOnly(b, ["U1.3", "R1.1", "U1.1"]), ["U1.3", "R1.1", "U1.1"]);
  assert.equal(padsOnly(b, ["U1.3", "R1"]), null, "a footprint among them");
  assert.equal(padsOnly(b, []), null);
  assert.equal(padsOnly(b, ["U9.1"]), null, "a pad the board does not have");
});

test("the unselected pads are listed as KiCad lists them, the reference, a colon and the number", () => {
  assert.equal(unselectedPadLabel("R2.1"), "R2:1");
  assert.equal(unselectedPadLabel("J1.A1#2"), "J1:A1");
  assert.equal(unselectedPadLabel("nothing"), "nothing");
});
