import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { trackViaCmds, trackViaForm, viaLayerDepth, withNetAnswers } from "./trackViaDialog";

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
  assert.deepEqual(out.cmds, [{ op: "set_via_layers", ids: ["V1", "V2"], kind: "buried", from_layer: "In1.Cu", to_layer: "In2.Cu" }, { op: "track_via_nets", ids: ["V1", "V2"], net: "A" }], "the net command comes last, asking about shorts (round 3 C-2)");
  const rings = trackViaCmds(two, before, { ...before, rings: "keep_all" }, String);
  assert.ok("cmds" in rings && JSON.stringify(rings.cmds) === JSON.stringify([{ op: "set_via_layers", ids: ["V1", "V2"], unconnected: "keep_all" }, { op: "track_via_nets", ids: ["V1", "V2"], net: "A" }]));
  // a through via sends no pair: the backend's `SanitizeLayers` puts it on F.Cu..B.Cu
  const through = trackViaCmds(two, before, { ...before, viaType: "through", viaFrom: "F.Cu", viaTo: "B.Cu" }, String);
  assert.ok("cmds" in through && JSON.stringify(through.cmds) === JSON.stringify([{ op: "set_via_layers", ids: ["V1", "V2"], kind: "through" }, { op: "track_via_nets", ids: ["V1", "V2"], net: "A" }]));
  assert.deepEqual(trackViaCmds(two, before, { ...before, viaType: "blind", viaFrom: "In1.Cu", viaTo: "In1.Cu" }, String), { error: "Via start layer and end layer cannot be the same" });
  assert.equal(viaLayerDepth("through", "F.Cu", "B.Cu", ["F.Cu", "B.Cu"]), 1, "two layers: no annular rings row");
  assert.equal(viaLayerDepth("blind", "F.Cu", "In1.Cu", ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), 1);
  assert.equal(viaLayerDepth("through", null, null, ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), 3);
});

test("UI-LEFT P3: the via's protection, backdrill, post-machining and padstack as one set_via_props per via, its teardrops as set_teardrop_params", () => {
  const vb = {
    ...board,
    routing: {
      ...board.routing!,
      teardrop_settings: { enabled: false, target_vias: true, target_pth_pads: true, target_smd_pads: true, best_length_ratio: 0.5, best_width_ratio: 1, max_len_nm: 1_000_000, max_width_nm: 2_000_000, width_to_size_filter_ratio: 0.9 },
      vias: [
        { id: "V1", net: "A", x: 5, y: 6, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu", props: { front_tenting: true, back_tenting: false, front_covering: false, back_covering: false, front_plugging: false, back_plugging: false, filling: false, capping: false } },
        { id: "V2", net: "A", x: 9, y: 6, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu", props: { tertiary_drill: { size: 400_000, start: "B.Cu", end: "In2.Cu" } } },
      ],
    },
  } as unknown as BoardState;
  const before = trackViaForm(vb, ["V1", "V2"]);
  assert.deepEqual([before.protection, before.backdrill, before.backdrillBackLayer, before.topPost, before.padstackMode], [null, null, null, "none", "normal"], "what differs is indeterminate");
  assert.equal(trackViaForm(vb, ["V1"]).protection, "ia");
  assert.equal(trackViaForm(vb, ["V2"]).backdrill, "bottom", "a tertiary drill is the bottom backdrill");
  // Type IV-b, a top backdrill to In1.Cu, a countersink on the front, and teardrops on
  const form = { ...before, protection: "ivb" as const, backdrill: "top" as const, backdrillFrontSize: 450_000, backdrillFrontLayer: "In1.Cu", topPost: "countersink" as const, topPostSize: 900_000, topPostSize2: 90, teardrops: { ...before.teardrops, enabled: true } };
  const out = trackViaCmds(vb, before, form, String);
  assert.ok("cmds" in out);
  const props = out.cmds.filter((c) => c.op === "set_via_props");
  assert.equal(props.length, 2);
  assert.deepEqual(props[0], {
    op: "set_via_props",
    id: "V1",
    props: {
      front_tenting: true,
      back_tenting: true,
      front_covering: false,
      back_covering: false,
      front_plugging: true,
      back_plugging: true,
      filling: false,
      capping: false,
      backdrill: { size: 450_000, start: "F.Cu", end: "In1.Cu" },
      front_post_machining: { mode: "countersink", size: 900_000, angle_ddeg: 900, depth: 0 },
    },
  });
  const v2 = props[1] as unknown as { props: Record<string, unknown> };
  assert.equal(v2.props.tertiary_drill, undefined, "Top makes the tertiary drill go");
  assert.deepEqual(out.cmds.filter((c) => c.op === "set_teardrop_params"), [{ op: "set_teardrop_params", ids: ["V1", "V2"], params: { enabled: true, allow_two_segments: false, prefer_zone_connections: true, best_length_ratio: 0.5, max_length: 1_000_000, best_width_ratio: 1, max_width: 2_000_000, curved_edges: false, filter_ratio: 0.9 } }]);
  // nothing changed sends nothing
  const none = trackViaCmds(vb, before, before, String);
  assert.ok("cmds" in none && none.cmds.length === 0);
  // a front/inner/back padstack: the inner layers' diameter of its own
  const one = trackViaForm(vb, ["V1"]);
  const fib = trackViaCmds(vb, one, { ...one, padstackMode: "front_inner_back", layerSizes: { Inner: 500_000 } }, String);
  assert.ok("cmds" in fib);
  const sp = fib.cmds.find((c) => c.op === "set_via_props") as { props: { padstack?: unknown } };
  assert.deepEqual(sp.props.padstack, { mode: "front_inner_back", sizes: { Inner: 500_000 } });
});
