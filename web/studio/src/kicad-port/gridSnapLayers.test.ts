import { test } from "node:test";
import assert from "node:assert/strict";
import { collectAnchors, DEFAULT_MAGNETIC_SETTINGS, matchesActiveLayer, type AnchorSourceBoard } from "./gridSnap";

// common.Control.magneticSnapToggle / MAGNETIC_SETTINGS::allLayers
// (pcb_grid_helper.cpp: keep an item only if allLayers || its layer set
// intersects the active layer).
const layered: AnchorSourceBoard = {
  parts: [
    { ref: "T", placed: true, at: [0, 0], side: "top", pads: [{ x: 1, y: 1 }, { x: 2, y: 2, th: true }] },
    { ref: "B", placed: true, at: [10, 10], side: "bottom", pads: [{ x: 11, y: 11 }] },
  ],
  routing: {
    tracks: [
      { id: "tf", layer: "F.Cu", pts: [[0, 0], [5, 0]] },
      { id: "tb", layer: "B.Cu", pts: [[0, 9], [5, 9]] },
    ],
    vias: [{ id: "v", x: 7, y: 7 }],
  },
};

test("collectAnchors: allLayers=false keeps only active-layer items (SMD pads by face, THT pads and vias on every copper layer)", () => {
  const f = collectAnchors(layered, DEFAULT_MAGNETIC_SETTINGS, undefined, { allLayers: false, activeLayer: "F.Cu" });
  const owners = new Set(f.map((a) => a.ownerId));
  assert.equal(owners.has("T"), true);
  assert.equal(owners.has("tf"), true);
  assert.equal(owners.has("v"), true);
  assert.equal(owners.has("B"), false);
  assert.equal(owners.has("tb"), false);
  const b = collectAnchors(layered, DEFAULT_MAGNETIC_SETTINGS, undefined, { allLayers: false, activeLayer: "B.Cu" });
  // top part's SMD pad + origin dropped, its THT pad stays (spans all copper)
  assert.deepEqual(b.filter((a) => a.ownerId === "T").map((a) => a.kind), ["pad"]);
  assert.equal(b.some((a) => a.ownerId === "B"), true);
});

test("collectAnchors: allLayers=true or no filter keeps everything; non-copper active layer drops vias/THT pads", () => {
  const all = collectAnchors(layered, DEFAULT_MAGNETIC_SETTINGS);
  assert.equal(collectAnchors(layered, DEFAULT_MAGNETIC_SETTINGS, undefined, { allLayers: true, activeLayer: "F.Cu" }).length, all.length);
  const silk = collectAnchors(layered, DEFAULT_MAGNETIC_SETTINGS, undefined, { allLayers: false, activeLayer: "F.SilkS" });
  assert.equal(silk.some((a) => a.kind === "via"), false);
});

test("matchesActiveLayer: no active layer means nothing to intersect with, so nothing is filtered", () => {
  assert.equal(matchesActiveLayer({ allLayers: false, activeLayer: null }, "B.Cu"), true);
});
