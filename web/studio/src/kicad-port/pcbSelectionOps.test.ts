import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { DEFAULT_FILTER_OPTIONS, filterSelection, netItems, netsOfItems, selectAllConnectedTracks, selectConnections, unrouteSelected } from "./pcbSelectionOps";

function part(ref: string, x: number, y: number, pads: { num: string; x: number; y: number; net: string | null; th?: boolean }[], side = "top") {
  return { ref, placed: true, at: [x, y], side, pads: pads.map((p) => ({ num: p.num, net: p.net, x: p.x, y: p.y, w: 500, h: 500, round: false, th: p.th ?? false })) };
}

/** R1.1 -- track a -- via -- track b -- R2.1 on net N, plus an unrelated track on net M. */
function board(): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [part("R1", 0, 0, [{ num: "1", x: 0, y: 0, net: "N" }]), part("R2", 10000, 0, [{ num: "1", x: 10000, y: 0, net: "N" }]), part("R3", 0, 5000, [{ num: "1", x: 0, y: 5000, net: "M" }])] as never,
    rules: [],
    routing: {
      tracks: [
        { id: "a", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [5000, 0]] },
        { id: "b", net: "N", layer: "B.Cu", width: 200, pts: [[5000, 0], [10000, 0]] },
        { id: "m", net: "M", layer: "F.Cu", width: 200, pts: [[0, 5000], [4000, 5000]] },
      ],
      vias: [{ id: "v", net: "N", x: 5000, y: 0, d: 600, drill: 300, from: "F.Cu", to: "B.Cu" }],
      zones: [{ id: "z", net: "N", layer: "F.Cu", outline: [[0, 0], [1, 0], [0, 1]] } as never],
      track_width_presets: [],
      via_presets: [],
      teardrop_settings: {} as never,
    },
    drawings: {
      shapes: [
        { kind: "segment", id: "s1", layer: "Edge.Cuts", stroke_width: 100, filled: false, start: [0, 0], end: [1, 1] },
        { kind: "segment", id: "s2", layer: "F.SilkS", stroke_width: 100, filled: false, start: [0, 0], end: [1, 1] },
      ],
      texts: [{ id: "x", content: "A", x: 0, y: 0, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false }],
      groups: [{ id: "g", name: "", member_ids: [] }],
      dimensions: [],
      dimension_settings: {} as never,
    },
    checks: [],
    activity: [],
    job: "idle",
  } as BoardState;
}

test("netsOfItems takes the nets of tracks, vias and zones; netItems lists a net's tracks and vias", () => {
  const b = board();
  assert.deepEqual([...netsOfItems(b, ["a", "z", "R1"])], ["N"]);
  assert.deepEqual(netItems(b, new Set(["N"])).sort(), ["a", "b", "v"]);
  assert.deepEqual(netItems(b, new Set(["M"])), ["m"]);
});

test("filterSelection keeps only what the options include (itemIsIncludedByFilter)", () => {
  const b = board();
  const all = ["R1", "a", "v", "z", "s1", "s2", "x", "g"];
  assert.deepEqual(filterSelection(b, all, DEFAULT_FILTER_OPTIONS), ["R1", "a", "v", "z", "s1", "s2", "x"]); // a group is not on the list
  assert.deepEqual(filterSelection(b, all, { ...DEFAULT_FILTER_OPTIONS, includeTracks: false, includeVias: false, includeZones: false }), ["R1", "s1", "s2", "x"]);
  assert.deepEqual(filterSelection(b, all, { ...DEFAULT_FILTER_OPTIONS, includeBoardOutlineLayer: false }), ["R1", "a", "v", "z", "s2", "x"]);
  assert.deepEqual(filterSelection(b, all, { ...DEFAULT_FILTER_OPTIONS, includePcbTexts: false, includeItemsOnTechLayers: false }), ["R1", "a", "v", "z", "s1"]);
  b.locked = ["R1"];
  assert.deepEqual(filterSelection(b, ["R1"], { ...DEFAULT_FILTER_OPTIONS, includeLockedFootprints: false }), []);
  assert.deepEqual(filterSelection(b, ["R1"], DEFAULT_FILTER_OPTIONS), ["R1"]);
});

test("selectAllConnectedTracks from a pad follows the track, the via and the track on the other layer, and stops at the far pad", () => {
  const b = board();
  const { trackIds, viaIds } = selectAllConnectedTracks(
    {
      tracks: b.routing!.tracks,
      vias: b.routing!.vias,
      pads: [
        { key: "R1.1", x: 0, y: 0, layers: ["F.Cu"] },
        { key: "R2.1", x: 10000, y: 0, layers: ["B.Cu"] },
        { key: "R3.1", x: 0, y: 5000, layers: ["F.Cu"] },
      ],
      layers: b.layers,
    },
    [{ kind: "pad", id: "R1.1" }],
    "pad"
  );
  assert.deepEqual([...trackIds].sort(), ["a", "b"]);
  assert.deepEqual([...viaIds], ["v"]);
});

test("a pad that is not a start pad stops the flood (STOP_AT_PAD), and a start track is always taken", () => {
  const flood = {
    tracks: [
      { id: "a", layer: "F.Cu", pts: [[0, 0], [5000, 0]] as [number, number][] },
      { id: "b", layer: "F.Cu", pts: [[5000, 0], [9000, 0]] as [number, number][] },
    ],
    vias: [],
    pads: [{ key: "P.1", x: 5000, y: 0, layers: ["F.Cu"] }],
    layers: ["F.Cu", "B.Cu"],
  };
  const stopped = selectAllConnectedTracks(flood, [{ kind: "track", id: "a" }], "pad");
  assert.deepEqual([...stopped.trackIds], ["a"]); // b is beyond the pad
  const through = selectAllConnectedTracks(flood, [{ kind: "track", id: "a" }], "never");
  assert.deepEqual([...through.trackIds].sort(), ["a", "b"]);
});

test("unrouteSelected on a footprint removes the connection up to the next pad and keeps the footprint selected", () => {
  const r = unrouteSelected(board(), ["R1"]);
  assert.deepEqual(r.trackIds.sort(), ["a", "b"]);
  assert.deepEqual(r.viaIds, ["v"]);
  assert.deepEqual(r.reselect, ["R1"]);
  // the unrelated net is untouched
  assert.ok(!r.trackIds.includes("m"));
});

test("unrouteSelected on a track takes its whole run", () => {
  const r = unrouteSelected(board(), ["a"]);
  assert.deepEqual(r.trackIds.sort(), ["a", "b"]);
  assert.deepEqual(r.viaIds, ["v"]);
  assert.deepEqual(r.reselect, []);
});

test("selectConnections: the routed copper of the footprints' pads, and every track of a net the footprints fully own", () => {
  const b = board();
  // R1 and R2 own both pads of net N: its whole copper comes along
  assert.deepEqual(selectConnections(b, ["R1", "R2"]).sort(), ["a", "b", "v"]);
  // R1 alone: net N has another pad (R2.1), so only what is connected to R1's pad
  assert.deepEqual(selectConnections(b, ["R1"]).sort(), ["a", "b", "v"]);
  // R3 owns net M entirely
  assert.deepEqual(selectConnections(b, ["R3"]), ["m"]);
});
