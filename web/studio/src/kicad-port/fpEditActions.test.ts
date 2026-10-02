import { test } from "node:test";
import assert from "node:assert/strict";
import { duplicatePads, padCanHaveNumber, uniqueFootprintName } from "./fpEditActions";
import type { LibraryPad } from "../api/types";

function pad(number: string, x = 0, y = 0, kind: LibraryPad["kind"] = "smd"): LibraryPad {
  return {
    id: `pad_${number}`,
    number,
    at: { x, y },
    offset: { x: 0, y: 0 },
    size: [1000, 1000],
    shape: "rect",
    kind,
    drill: kind === "smd" ? null : 800,
    drill_slot: null,
    rot: 0,
    roundrect_ratio: null,
    trapezoid_delta: null,
    chamfer_ratio: null,
    chamfer_corners: { top_left: false, top_right: false, bottom_left: false, bottom_right: false },
    layers: [],
    clearance_override: null,
    thermal_gap_override: null,
    thermal_spoke_width_override: null,
  };
}

// ---------------------------------------------------- CreateNewFootprint

test("uniqueFootprintName: Untitled, then Untitled_1, Untitled_2 (CreateNewFootprint)", () => {
  assert.equal(uniqueFootprintName([]), "Untitled");
  assert.equal(uniqueFootprintName(["R_0402"]), "Untitled");
  assert.equal(uniqueFootprintName(["Untitled"]), "Untitled_1");
  assert.equal(uniqueFootprintName(["Untitled", "Untitled_1"]), "Untitled_2");
  assert.equal(uniqueFootprintName(["Untitled", "Untitled_2"]), "Untitled_1", "the first free suffix, counted from 1");
});

test("uniqueFootprintName: an explicit base is suffixed the same way; an empty one is Untitled", () => {
  assert.equal(uniqueFootprintName(["Foo"], "Foo"), "Foo_1");
  assert.equal(uniqueFootprintName([], ""), "Untitled");
});

// --------------------------------------------------- Duplicate / Increment

test("padCanHaveNumber: an NPTH hole carries no number", () => {
  assert.equal(padCanHaveNumber(pad("1", 0, 0, "smd")), true);
  assert.equal(padCanHaveNumber(pad("1", 0, 0, "through_hole")), true);
  assert.equal(padCanHaveNumber(pad("", 0, 0, "non_plated_hole")), false);
});

test("duplicatePads without increment: exact copies at the same place, ids dropped, numbers kept", () => {
  const pads = [pad("1", 0, 0), pad("2", 2000, 0)];
  const { pads: dup, lastPadNumber } = duplicatePads(pads, [pads[1]!], false, "2");
  assert.equal(dup.length, 1);
  assert.equal(dup[0]!.number, "2");
  assert.deepEqual(dup[0]!.at, { x: 2000, y: 0 });
  assert.equal(dup[0]!.id, undefined, "the backend assigns the new id");
  assert.equal(lastPadNumber, "2", "plain Duplicate leaves the pad tool's last number alone");
});

test("duplicatePads with increment: the copy takes GetNextPadNumber( last ) and updates the last number", () => {
  const pads = [pad("1"), pad("2"), pad("3")];
  const { pads: dup, lastPadNumber } = duplicatePads(pads, [pads[0]!], true, "3");
  assert.equal(dup[0]!.number, "4");
  assert.equal(lastPadNumber, "4");
});

test("duplicatePads with increment: several selected pads are numbered consecutively", () => {
  const pads = [pad("1"), pad("2")];
  const { pads: dup, lastPadNumber } = duplicatePads(pads, [pads[0]!, pads[1]!], true, "2");
  assert.deepEqual(
    dup.map((p) => p.number),
    ["3", "4"]
  );
  assert.equal(lastPadNumber, "4");
});

test("duplicatePads with increment skips numbers already used (GetNextPadNumber's collision loop)", () => {
  const pads = [pad("1"), pad("2"), pad("4")];
  const { pads: dup } = duplicatePads(pads, [pads[0]!], true, "2");
  assert.equal(dup[0]!.number, "3");
  const { pads: dup2 } = duplicatePads([...pads, pad("3")], [pads[0]!], true, "2");
  assert.equal(dup2[0]!.number, "5", "3 and 4 are taken");
});

test("duplicatePads with increment keeps a prefix (A1 -> A2)", () => {
  const pads = [pad("A1"), pad("A2")];
  const { pads: dup } = duplicatePads(pads, [pads[0]!], true, "A2");
  assert.equal(dup[0]!.number, "A3");
});

test("duplicatePads with increment leaves an NPTH hole's number alone (CanHaveNumber)", () => {
  const hole = pad("", 0, 0, "non_plated_hole");
  const pads = [pad("1"), hole];
  const { pads: dup, lastPadNumber } = duplicatePads(pads, [hole], true, "1");
  assert.equal(dup[0]!.number, "");
  assert.equal(lastPadNumber, "1");
});
