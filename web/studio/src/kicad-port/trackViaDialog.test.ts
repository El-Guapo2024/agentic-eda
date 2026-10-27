import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { trackViaCmds, trackViaForm, withNetAnswers } from "./trackViaDialog";

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
    { op: "edit_tracks_and_vias", ids: ["T1", "T2"], track_width: { kind: "value", nm: 300_000 }, via_size: null, layer: null },
    // the net last, through the questions KiCad asks (round 3 C-2)
    { op: "track_via_nets", ids: ["T1", "T2", "V1"], net: "B" },
  ]);
  const bad = trackViaCmds(board, before, { ...before, diameter: 300_000 }, (n) => `${n / 1e6} mm`);
  assert.deepEqual(bad, { error: "Via hole size must be smaller than via diameter" });
  const one = trackViaForm(board, ["T1"]);
  const moved = trackViaCmds(board, one, { ...one, endX: 2_000_000 }, String);
  assert.ok("cmds" in moved && moved.cmds.length === 2 && JSON.stringify(moved.cmds[0]) === JSON.stringify({ op: "edit_track", id: "T1", end: { x: 2_000_000, y: 0 } }));
  assert.deepEqual("cmds" in moved && moved.cmds[1], { op: "track_via_nets", ids: ["T1"], net: "A" }, "a short the edit makes is asked about whatever changed");
});

test("Track & Via Properties' width error is UNIT_BINDER::Validate's (round 3 C-3); the answers go on the net command", () => {
  const one = trackViaForm(board, ["T1"]);
  assert.deepEqual(trackViaCmds(board, one, { ...one, width: 500 }, () => "0.001 mm"), { error: "Track width must be at least 0.001 mm." });
  const out = trackViaCmds(board, one, { ...one, net: "B" }, String);
  assert.ok("cmds" in out);
  assert.deepEqual(withNetAnswers(out.cmds, { change_pads: true }), [{ op: "track_via_nets", ids: ["T1"], net: "B", change_pads: true }]);
});
