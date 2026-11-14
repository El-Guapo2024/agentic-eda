import { test } from "node:test";
import assert from "node:assert/strict";
import { placeholderOf, wantsPlaceholder } from "./placeholder3d";

test("UI-LEFT-3 X3-2: get3dModelsFromFootprint's placeholder: none without the setting, one for a footprint without models or with one not in, none with an extruded body", () => {
  assert.equal(wantsPlaceholder([], false, false), false);
  assert.equal(wantsPlaceholder([], true, false), true);
  assert.equal(wantsPlaceholder([], true, true), false, "Suppress placeholder if a valid extruded body was built");
  assert.equal(wantsPlaceholder([{ show: true, name: "a.wrl", state: "ready" }], true, false), false);
  assert.equal(wantsPlaceholder([{ show: true, name: "a.wrl", state: "ready" }, { show: true, name: "b.wrl", state: "missing" }], true, false), true, "beside the models that are in");
  assert.equal(wantsPlaceholder([{ show: false, name: "b.wrl", state: "missing" }], true, false), false, "a hidden model is skipped");
  assert.equal(wantsPlaceholder([{ show: true, name: "b.wrl", state: "missing" }], false, false), false);
});

test("UI-LEFT-3 X3-2: CalcPlaceholderLocalBox and renderPlaceholderForFootprint's size: the pads' box in the footprint's frame, 90%, half the narrower side tall", () => {
  // two 1 x 1.2 mm pads 3 mm apart along the footprint's x, the footprint turned 90 degrees at (10, 20) mm
  const pad = (x: number, y: number) => ({ num: "1", net: null, x, y, w: 1_200_000, h: 1_000_000, round: false, th: false, size: [1_000_000, 1_200_000] as [number, number] });
  const part = { at: [10_000_000, 20_000_000] as [number, number], rot: 90, side: "top" as const, pads: [pad(10_000_000, 18_500_000), pad(10_000_000, 21_500_000)] };
  const ph = placeholderOf(part)!;
  assert.deepEqual(ph.centerNm.map(Math.round), [10_000_000, 20_000_000]);
  assert.deepEqual(ph.sizeMm.map((v) => Math.round(v * 1000) / 1000), [3.6, 1.08, 0.54]);
  assert.equal(ph.bottom, false);
  // no pads: the body box
  const bodyOnly = placeholderOf({ at: [0, 0], rot: 0, side: "bottom", pads: [], body: [-1_000_000, -500_000, 1_000_000, 500_000] })!;
  assert.deepEqual(bodyOnly.sizeMm.map((v) => Math.round(v * 1000) / 1000), [1.8, 0.9, 0.45]);
  assert.equal(bodyOnly.bottom, true);
  assert.equal(placeholderOf({ at: [0, 0], rot: 0, side: "top", pads: [] }), null);
});
