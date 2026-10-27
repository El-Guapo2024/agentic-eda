import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { trackViaCmds, trackViaForm, viaLayerDepth } from "./trackViaDialog";

const board = {
  parts: [],
  locked: ["T2"],
  routing: {
    tracks: [
      { id: "T1", net: "A", layer: "F.Cu", width: 200_000, pts: [[0, 0], [1_000_000, 0]] },
      { id: "T2", net: "A", layer: "F.Cu", width: 250_000, pts: [[0, 1_000_000], [1_000_000, 1_000_000]] },
    ],
    vias: [{ id: "V1", net: "A", x: 5, y: 6, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" }],
    zones: [],
  },
} as unknown as BoardState;

test("Track & Via Properties over several items: shared values shown, differing ones indeterminate, one batch for what changed (r2-pcb-edit A5)", () => {
  const before = trackViaForm(board, ["T1", "T2", "V1"]);
  assert.deepEqual([before.net, before.width, before.layer, before.locked, before.startX, before.startY], ["A", null, "F.Cu", null, 0, null]);
  const out = trackViaCmds(board, before, { ...before, net: "B", width: 300_000, locked: false }, (n) => `${n / 1e6} mm`);
  assert.ok("cmds" in out);
  assert.deepEqual(out.cmds, [
    { op: "set_locked", ids: ["T1", "T2", "V1"], locked: false },
    { op: "set_item_net", ids: ["T1", "T2", "V1"], net: "B" },
    { op: "edit_tracks_and_vias", ids: ["T1", "T2"], track_width: { kind: "value", nm: 300_000 }, via_size: null, layer: null },
  ]);
  const bad = trackViaCmds(board, before, { ...before, diameter: 300_000 }, (n) => `${n / 1e6} mm`);
  assert.deepEqual(bad, { error: "Via hole size must be smaller than via diameter" });
  const one = trackViaForm(board, ["T1"]);
  const moved = trackViaCmds(board, one, { ...one, endX: 2_000_000 }, String);
  assert.ok("cmds" in moved && moved.cmds.length === 1 && JSON.stringify(moved.cmds[0]) === JSON.stringify({ op: "edit_track", id: "T1", end: { x: 2_000_000, y: 0 } }));
});

test("the via rows: type, layer pair and annular rings as one set_via_layers over the dialog's vias, a through via's pair F.Cu..B.Cu (r2-pcb-edit A5)", () => {
  const two = {
    ...board,
    routing: {
      ...board.routing!,
      vias: [
        { id: "V1", net: "A", x: 5, y: 6, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" },
        { id: "V2", net: "A", x: 9, y: 6, d: 600_000, drill: 300_000, from: "F.Cu", to: "In1.Cu", type: "blind", unconnected: "remove_all" },
      ],
    },
  } as unknown as BoardState;
  const before = trackViaForm(two, ["V1", "V2"]);
  assert.deepEqual([before.viaType, before.viaFrom, before.viaTo, before.rings], [null, "F.Cu", null, null], "what differs is indeterminate");
  const out = trackViaCmds(two, before, { ...before, viaType: "buried", viaFrom: "In1.Cu", viaTo: "In2.Cu" }, String);
  assert.ok("cmds" in out);
  assert.deepEqual(out.cmds, [{ op: "set_via_layers", ids: ["V1", "V2"], kind: "buried", from_layer: "In1.Cu", to_layer: "In2.Cu" }]);
  const rings = trackViaCmds(two, before, { ...before, rings: "keep_all" }, String);
  assert.ok("cmds" in rings && JSON.stringify(rings.cmds) === JSON.stringify([{ op: "set_via_layers", ids: ["V1", "V2"], unconnected: "keep_all" }]));
  // a through via sends no pair: the backend's `SanitizeLayers` puts it on F.Cu..B.Cu
  const through = trackViaCmds(two, before, { ...before, viaType: "through", viaFrom: "F.Cu", viaTo: "B.Cu" }, String);
  assert.ok("cmds" in through && JSON.stringify(through.cmds) === JSON.stringify([{ op: "set_via_layers", ids: ["V1", "V2"], kind: "through" }]));
  assert.deepEqual(trackViaCmds(two, before, { ...before, viaType: "blind", viaFrom: "In1.Cu", viaTo: "In1.Cu" }, String), { error: "Via start layer and end layer cannot be the same" });
  assert.equal(viaLayerDepth("through", "F.Cu", "B.Cu", ["F.Cu", "B.Cu"]), 1, "two layers: no annular rings row");
  assert.equal(viaLayerDepth("blind", "F.Cu", "In1.Cu", ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), 1);
  assert.equal(viaLayerDepth("through", null, null, ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), 3);
});
