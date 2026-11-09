import { test } from "node:test";
import assert from "node:assert/strict";
import { zonesToRefill } from "./autoRefill";
import type { BoardState } from "../api/types";

const sq = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
const zone = (id: string, layer: string, outline: [number, number][], more: Record<string, unknown> = {}) => ({ id, net: "GND", layer, outline, priority: 0, ...more });
const board = (tracks: { id: string; layer: string; pts: [number, number][] }[]): BoardState =>
  ({
    layers: ["F.Cu", "B.Cu"],
    parts: [],
    routing: {
      tracks: tracks.map((t) => ({ ...t, net: "SIG", width: 200_000 })),
      vias: [],
      zones: [zone("zf", "F.Cu", sq(0, 0, 10_000_000, 10_000_000)), zone("zb", "B.Cu", sq(0, 0, 10_000_000, 10_000_000)), zone("far", "F.Cu", sq(50_000_000, 0, 60_000_000, 10_000_000)), zone("ra", "F.Cu", sq(0, 0, 10_000_000, 10_000_000), { is_rule_area: true })],
    },
    drawings: { shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {} },
  }) as unknown as BoardState;

test("PP10 auto-refill: a track moved on F.Cu damages the F.Cu pour it crosses (as it was and as it is), not the B.Cu one, the far one or a rule area", () => {
  const before = board([{ id: "t", layer: "F.Cu", pts: [[20_000_000, 5_000_000], [30_000_000, 5_000_000]] }]);
  const after = board([{ id: "t", layer: "F.Cu", pts: [[2_000_000, 5_000_000], [8_000_000, 5_000_000]] }]);
  const filled = new Set(["zf", "zb", "far"]);
  assert.deepEqual(zonesToRefill(before, after, filled), ["zf"]);
  // moved back out: the box it had is damage too (`propagateDamage( boardItemCopy )`)
  assert.deepEqual(zonesToRefill(after, before, filled), ["zf"]);
  // nothing changed: only the zones not filled (`!zone->IsFilled()`)
  assert.deepEqual(zonesToRefill(before, before, filled), []);
  assert.deepEqual(zonesToRefill(before, before, new Set(["zf"])), ["zb", "far"]);
});

test("PP10 auto-refill: an edge of the board damages every layer's pour it meets; a zone edited is refilled itself", () => {
  const before = board([]);
  const after = board([]);
  after.drawings!.shapes = [{ id: "e", kind: "segment", layer: "Edge.Cuts", stroke_width: 100_000, filled: false, start: [5_000_000, 0], end: [5_000_000, 10_000_000] } as never];
  assert.deepEqual(zonesToRefill(before, after, new Set(["zf", "zb", "far"])), ["zf", "zb"]);
  const moved = board([]);
  moved.routing!.zones[2] = { ...moved.routing!.zones[2]!, outline: sq(51_000_000, 0, 60_000_000, 10_000_000) };
  assert.deepEqual(zonesToRefill(before, moved, new Set(["zf", "zb", "far"])), ["far"]);
});
