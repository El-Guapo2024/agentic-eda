import { test } from "node:test";
import assert from "node:assert/strict";
import { drillColumns, withDrill } from "./padTable";
import type { LibraryPad } from "../api/types";

const pad = (over: Partial<LibraryPad> = {}): LibraryPad => ({
  id: "p",
  number: "1",
  at: { x: 0, y: 0 },
  offset: { x: 0, y: 0 },
  size: [1600, 1600],
  shape: "circle",
  kind: "through_hole",
  drill: 800,
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
  ...over,
});

test("drillColumns: (d, d) for a round hole, (w, h) for a slot, (0, 0) for an SMD pad", () => {
  assert.deepEqual(drillColumns(pad()), [800, 800]);
  assert.deepEqual(drillColumns(pad({ drill: null, drill_slot: [600, 1700] })), [600, 1700]);
  assert.deepEqual(drillColumns(pad({ kind: "smd", drill: null })), [0, 0]);
  assert.deepEqual(drillColumns(pad({ kind: "non_plated_hole", drill: 1200 })), [1200, 1200]);
});

test("withDrill: a missing side takes the other; equal sides are a round hole, unequal ones a slot", () => {
  assert.deepEqual(withDrill(pad(), 900, 0).drill, 900);
  assert.equal(withDrill(pad(), 900, 0).drill_slot, null);
  assert.deepEqual(withDrill(pad(), 0, 700).drill, 700);
  const slot = withDrill(pad(), 600, 1700);
  assert.equal(slot.drill, null);
  assert.deepEqual(slot.drill_slot, [600, 1700]);
  assert.equal(withDrill(slot, 800, 800).drill, 800, "back to round once the sides agree");
  assert.equal(withDrill(slot, 800, 800).drill_slot, null);
});

test("withDrill: nothing changes unless a side is positive", () => {
  const p = pad();
  assert.equal(withDrill(p, 0, 0), p);
  assert.equal(withDrill(p, -5, 0), p);
});
