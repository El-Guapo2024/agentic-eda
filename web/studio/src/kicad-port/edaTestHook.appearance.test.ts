import { test } from "node:test";
import assert from "node:assert/strict";
import { appearanceSummary, type AppearanceSource } from "./edaTestHook";
import { asAppearanceOp, APPEARANCE_OP_NAMES, reduceView } from "./appearanceOps";
import { makeCtx, makeSlice } from "./appearanceFixture";

function source(): AppearanceSource {
  const s = makeSlice();
  return { appearance: s.appearance, layerVisible: s.layerVisible, highContrast: s.highContrast, showRatsnest: s.showRatsnest, gridVisible: s.gridVisible, bcx: { ratsnestMode: s.ratsnestMode, hiddenRatsnestNets: s.hiddenNets, boardFlipped: s.boardFlipped } };
}

test("the summary of a new project says only what differs from KiCad's defaults", () => {
  const a = appearanceSummary(source());
  assert.deepEqual(a.hiddenObjects, ["board_outline_area", "drawing_sheet", "drc_exclusions"]);
  assert.deepEqual([a.hiddenLayers, a.opacity, a.contrast, a.netColorMode, a.ratsnest, a.hiddenNets, a.hiddenNetclasses, a.preset, a.flipped], [[], {}, "normal", "ratsnest", "all", [], [], "All Layers", false]);
});

test("the summary follows the panel's edits", () => {
  let s = makeSlice();
  const ctx = makeCtx();
  for (const op of [
    { op: "object", id: "tracks", visible: false },
    { op: "object", id: "grid", visible: false },
    { op: "opacity", key: "zones", value: 0.3 },
    { op: "contrast", mode: "hidden" },
    { op: "net_color", net: "GND", color: "rgb(1, 2, 3)" },
    { op: "net_visible", net: "VCC", visible: false },
    { op: "layer", key: "f_fab", visible: false },
    { op: "ratsnest_display", mode: "none" },
  ] as const) s = reduceView(s, op, ctx);
  const a = appearanceSummary({ appearance: s.appearance, layerVisible: s.layerVisible, highContrast: s.highContrast, showRatsnest: s.showRatsnest, gridVisible: s.gridVisible, bcx: { ratsnestMode: s.ratsnestMode, hiddenRatsnestNets: s.hiddenNets, boardFlipped: s.boardFlipped } });
  assert.deepEqual(a.hiddenObjects, ["board_outline_area", "drawing_sheet", "drc_exclusions", "grid", "ratsnest", "tracks"]);
  assert.deepEqual(a.opacity, { zones: 0.3 });
  assert.equal(a.contrast, "hidden");
  assert.deepEqual(a.netColors, { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(a.hiddenNets, ["VCC"]);
  assert.deepEqual(a.hiddenLayers, ["f_fab"]);
  assert.equal(a.ratsnest, "none");
});

test("a script names an op by its `op` field; anything else is refused", () => {
  assert.deepEqual(asAppearanceOp({ op: "object", id: "tracks", visible: false }), { op: "object", id: "tracks", visible: false });
  for (const bad of [null, undefined, 3, "object", {}, { op: "nope" }, { op: 4 }]) assert.equal(asAppearanceOp(bad), null);
  assert.equal(APPEARANCE_OP_NAMES.length, 24);
  assert.equal(new Set(APPEARANCE_OP_NAMES).size, APPEARANCE_OP_NAMES.length);
});
