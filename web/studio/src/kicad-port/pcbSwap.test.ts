import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { planSwap } from "./pcbSwap";

const part = (ref: string, x: number, y: number) => ({ ref, placed: true, at: [x, y], rot: 0, side: "top", pads: [{ num: "1", net: "N", x, y, w: 500, h: 500, round: false, th: false }] });

function board(locked: string[] = []): BoardState {
  return {
    name: "t",
    layers: ["F.Cu", "B.Cu"],
    parts: [part("U1", 0, 0), part("U2", 10_000, 0)],
    locked,
    routing: {
      tracks: [{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [5000, 0]] }],
      vias: [{ id: "v1", net: "N", x: 1000, y: 2000, d: 600, drill: 300 }, { id: "v2", net: "N", x: 7000, y: 9000, d: 600, drill: 300 }],
      zones: [],
    },
    drawings: {
      shapes: [],
      texts: [
        { id: "a", content: "A", x: 0, y: 5000, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false },
        { id: "b", content: "B", x: 3000, y: 6000, angle: 0, layer: "B.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false },
      ],
      groups: [],
      dimensions: [],
    },
  } as unknown as BoardState;
}

test("Swap takes vias, texts and other items, not footprints only (round 6 E6-2)", () => {
  assert.deepEqual(planSwap(board(), ["v1", "v2"]), [
    { op: "move_items", ids: ["v1"], dx: 6000, dy: 7000 },
    { op: "move_items", ids: ["v2"], dx: -6000, dy: -7000 },
  ]);
  // Two texts: positions and, as items of one type, layers.
  const cmds = planSwap(board(), ["a", "b"])!;
  assert.deepEqual(cmds.filter((c) => c.op === "move_items"), [
    { op: "move_items", ids: ["a"], dx: 3000, dy: 1000 },
    { op: "move_items", ids: ["b"], dx: -3000, dy: -1000 },
  ]);
  assert.deepEqual(cmds.filter((c) => c.op === "edit_text").map((c) => [(c as { id: string }).id, (c as { layer: string }).layer]), [["a", "B.SilkS"], ["b", "F.SilkS"]]);
  // Footprints only: swap_chain (orientation and side too); a pad stands for its footprint; a straight track is left out.
  assert.deepEqual(planSwap(board(), ["U1.1", "U2", "t1"]), [{ op: "swap_chain", parts: ["U1", "U2"] }]);
});

test("Swap leaves locked items out, and does nothing with fewer than two left (round 6 E6-3)", () => {
  assert.equal(planSwap(board(["U1"]), ["U1", "U2"]), null);
  assert.equal(planSwap(board(), ["v1"]), null);
});

/** A zone with KiCad's default settings (what `edit_zone` has to send back as it was). */
const zone = (id: string, net: string, layers: string[], x0: number) => ({
  id,
  net,
  layer: layers[0]!,
  layers,
  teardrop: false,
  outline: [[x0, 0], [x0 + 10_000, 0], [x0 + 10_000, 10_000], [x0, 10_000]],
  clearance: 500,
  min_thickness: 250,
  thermal_gap: 500,
  thermal_spoke_width: 500,
  pad_connection: "Thermal",
  priority: 3,
  island_removal_mode: "Always",
  min_island_area: 0,
  fill_mode: "Polygons",
  hatch_thickness: 1000,
  hatch_gap: 1500,
  hatch_orientation_mdeg: 0,
  hatch_smoothing_level: 0,
  hatch_smoothing_value: 0.1,
  hatch_hole_min_area: 0.3,
  hatch_border_algorithm: 1,
  smoothing: "none",
  corner_radius: 0,
  is_rule_area: false,
  keepout_tracks: false,
  keepout_vias: false,
  keepout_pads: false,
  keepout_copper_pour: false,
  keepout_footprints: false,
});

function zoneBoard(zones: ReturnType<typeof zone>[]): BoardState {
  const b = board();
  return { ...b, routing: { ...b.routing!, zones } } as unknown as BoardState;
}

const layerEdits = (cmds: readonly { op: string }[]) =>
  cmds.filter((c) => c.op === "edit_zone").map((c) => {
    const z = c as unknown as { id: string; layer: string; layers?: string[]; priority: number; clearance: number };
    return [z.id, z.layer, z.layers, z.priority, z.clearance];
  });

test("Swap leaves zones where they are and swaps the layers of two (ZONE::SetPosition is a no-op; round 7 R7-2)", () => {
  // GND on F.Cu at x 0..10, VCC on B.Cu at x 20..30: both outlines stay, GND goes to B.Cu and VCC to F.Cu, every other setting as it was.
  const cmds = planSwap(zoneBoard([zone("z1", "GND", ["F.Cu"], 0), zone("z2", "VCC", ["B.Cu"], 20_000)]), ["z1", "z2"])!;
  assert.equal(cmds.filter((c) => c.op === "move_items").length, 0, "no zone moves");
  assert.deepEqual(layerEdits(cmds), [["z1", "B.Cu", ["B.Cu"], 3, 500], ["z2", "F.Cu", ["F.Cu"], 3, 500]]);
  assert.equal((cmds[0] as { net: string }).net, "GND", "a zone keeps its net");
});

test("Swap moves the item paired with a zone onto the zone's first corner, and leaves the zone (round 7 R7-2)", () => {
  // Via v1 at (1000, 2000), zone z1 with its first corner at (20000, 0): only the via moves; a via and a zone are not of one type, so no layer change.
  assert.deepEqual(planSwap(zoneBoard([zone("z1", "GND", ["F.Cu"], 20_000)]), ["v1", "z1"]), [{ op: "move_items", ids: ["v1"], dx: 19_000, dy: -2000 }]);
  // In a chain the zone keeps its own position for the next pair too: v1 -> z1's corner, then z1 / v2: v2 -> z1's corner, z1 stays.
  assert.deepEqual(planSwap(zoneBoard([zone("z1", "GND", ["F.Cu"], 20_000)]), ["v1", "z1", "v2"]), [
    { op: "move_items", ids: ["v1"], dx: 19_000, dy: -2000 },
    { op: "move_items", ids: ["v2"], dx: 13_000, dy: -9000 },
  ]);
});

test("Swap: a zone on several layers is UNDEFINED_LAYER, which SetLayer ignores (round 7 R7-2)", () => {
  // Multi-layer z1 (F.Cu and B.Cu) and single-layer z2 on B.Cu: z1 becomes a B.Cu zone alone, z2 keeps B.Cu.
  const cmds = planSwap(zoneBoard([zone("z1", "GND", ["F.Cu", "B.Cu"], 0), zone("z2", "VCC", ["B.Cu"], 20_000)]), ["z1", "z2"])!;
  assert.deepEqual(layerEdits(cmds), [["z1", "B.Cu", ["B.Cu"], 3, 500]]);
  // Two zones on several layers: nothing changes.
  assert.equal(planSwap(zoneBoard([zone("z1", "GND", ["F.Cu", "B.Cu"], 0), zone("z2", "VCC", ["F.Cu", "B.Cu"], 20_000)]), ["z1", "z2"]), null);
});
