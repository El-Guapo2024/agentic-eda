import { test } from "node:test";
import assert from "node:assert/strict";
import type { Part } from "../api/types";
import { DEFAULT_STACK } from "./model3d";
import { padCopperSides, pickFootprintByPads } from "./padPick3d";

const pad = (x: number, y: number, extra: Record<string, unknown> = {}) => ({ num: "1", net: null, x, y, w: 1_000_000, h: 1_000_000, round: false, th: false, size: [1_000_000, 1_000_000] as [number, number], shape: "rect" as const, ...extra });
const down = (xMm: number, yMm: number) => ({ origin: [xMm, 50, yMm] as const, dir: [0, -1, 0] as const });
const up = (xMm: number, yMm: number) => ({ origin: [xMm, -50, yMm] as const, dir: [0, 1, 0] as const });

test("gate v19: a pad's outer copper sides -- an SMD pad on its footprint's side, a through-hole pad on both, a non-plated hole on none", () => {
  assert.deepEqual(padCopperSides({ th: false }, false), { top: true, bottom: false });
  assert.deepEqual(padCopperSides({ th: false }, true), { top: false, bottom: true });
  assert.deepEqual(padCopperSides({ th: true }, false), { top: true, bottom: true });
  assert.deepEqual(padCopperSides({ th: false, layers: ["F.Cu", "F.Mask"] }, true), { top: false, bottom: true });
  assert.deepEqual(padCopperSides({ th: true, layers: ["*.Cu", "*.Mask"] }, false), { top: true, bottom: true });
  assert.deepEqual(padCopperSides({ th: true, kind: "non_plated_hole", layers: ["*.Cu"] }, false), { top: false, bottom: false });
});

test("gate v19: EDA_3D_CANVAS::OnLeftUp's PCB_PAD_T -- a ray on a pad's copper picks its footprint, off the pads nothing, the nearer face first", () => {
  const parts = [
    { ref: "U1", at: [10_000_000, 10_000_000], side: "top", pads: [pad(10_000_000, 10_000_000)] },
    { ref: "U2", at: [20_000_000, 10_000_000], side: "bottom", pads: [pad(20_000_000, 10_000_000)] },
    { ref: "U3", pads: [pad(30_000_000, 10_000_000)] },
  ] as unknown as Part[];
  const hit = pickFootprintByPads(parts, down(10.2, 9.8), DEFAULT_STACK);
  assert.equal(hit?.ref, "U1");
  assert.ok(Math.abs(hit!.distance - (50 - 0.8 - 0.035)) < 1e-9, `${hit!.distance}`);
  assert.equal(pickFootprintByPads(parts, down(10.6, 10), DEFAULT_STACK), null, "beside the pad");
  assert.equal(pickFootprintByPads(parts, down(20, 10), DEFAULT_STACK)?.ref, "U2", "the back copper, the board body not in the ray tracer's copper-only scene");
  // the nearer face first: from below, the back copper; a front SMD pad has none there, so the ray goes on to the front copper
  assert.ok(Math.abs(pickFootprintByPads(parts, up(10, 10), DEFAULT_STACK)!.distance - (50 + 0.8 + 0.035)) < 1e-9);
  assert.ok(Math.abs(pickFootprintByPads(parts, up(20, 10), DEFAULT_STACK)!.distance - (50 - 0.8 - 0.035)) < 1e-9);
  assert.equal(pickFootprintByPads(parts, down(30, 10), DEFAULT_STACK), null, "an unplaced part");
});
