import { test } from "node:test";
import assert from "node:assert/strict";
import { FP_ARC_EDIT_MODES, fpArcEditMode, fpArcEditModeIndex, fpPointAt, fpPointDrag, fpPointEditTarget } from "./fpPointEdit";
import type { CmdShape } from "../api/types";

test("the Footprint Editor's arc mode: the page's index through arcEditModeToEnum (0 keep centre, 1 keep endpoints, 2 keep centre and radius), out of range the first", () => {
  assert.deepEqual([0, 1, 2].map(fpArcEditMode), ["keep_center_adjust_angle_radius", "keep_endpoints_or_start_direction", "keep_center_ends_adjust_angle"]);
  assert.equal(fpArcEditMode(7), "keep_center_adjust_angle_radius");
  for (const m of FP_ARC_EDIT_MODES) assert.equal(fpArcEditMode(fpArcEditModeIndex(m)), m);
});

const arc: CmdShape = { kind: "arc", id: "g1", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: 1_000_000, y: 0 }, mid: { x: 0, y: -1_000_000 }, end: { x: -1_000_000, y: 0 } };
const line: CmdShape = { kind: "segment", id: "g2", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: 0, y: 0 }, end: { x: 2_000_000, y: 0 } };

test("the point editor works on the one selected graphic", () => {
  assert.equal(fpPointEditTarget([arc, line], new Set(["g1", "g2"])), null);
  assert.equal(fpPointEditTarget([arc, line], new Set(["pad:1"])), null);
  assert.equal(fpPointEditTarget([arc, line], new Set(["g2"]))?.kind, "segment");
});

test("an arc's end dragged moves it as the Footprint Editor's arc mode says", () => {
  const shape = fpPointEditTarget([arc], new Set(["g1"]))!;
  const start = fpPointAt(shape, [1_000_000, 10_000], 50_000)!;
  assert.ok(start, "the start point is under the pointer");
  const to: [number, number] = [2_000_000, 0];
  const byMode = [0, 1, 2].map((i) => fpPointDrag(shape, start, to, i) as Extract<CmdShape, { kind: "arc" }> | null);
  assert.ok(byMode.every((a) => a && a.id === "g1"), "each mode gives an arc with the shape's id");
  // keep the centre, adjust the radius: the radius follows the dragged end, the centre stays at the origin
  const r0 = Math.hypot(byMode[0]!.start.x, byMode[0]!.start.y);
  assert.ok(Math.abs(r0 - 2_000_000) < 2_000, `radius ${r0}`);
  // keep the end points: the other end stays where it was
  assert.deepEqual(byMode[1]!.end, arc.end);
  // keep the centre and the radius: the start stays on the radius-1 mm circle
  const r2 = Math.hypot(byMode[2]!.start.x, byMode[2]!.start.y);
  assert.ok(Math.abs(r2 - 1_000_000) < 2_000, `radius ${r2}`);
  assert.notDeepEqual(byMode[0], byMode[1]);
});

test("a line's end dragged", () => {
  const shape = fpPointEditTarget([line], new Set(["g2"]))!;
  const end = fpPointAt(shape, [2_000_000, 0], 50_000)!;
  const moved = fpPointDrag(shape, end, [3_000_000, 1_000_000], 0) as Extract<CmdShape, { kind: "segment" }>;
  assert.deepEqual([moved.start, moved.end, moved.id], [{ x: 0, y: 0 }, { x: 3_000_000, y: 1_000_000 }, "g2"]);
});
