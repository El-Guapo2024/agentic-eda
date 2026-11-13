import { test } from "node:test";
import assert from "node:assert/strict";
import { FP_ARC_EDIT_MODES, fpAddCorner, fpArcEditMode, fpArcEditModeIndex, fpCanAddCorner, fpCanChamferCorner, fpCanRemoveCorner, fpChamferCorner, fpMovePoint, fpMovePointTitle, fpPointAt, fpPointDrag, fpPointEditTarget, fpRemoveCorner } from "./fpPointEdit";
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

// ---------------------------------------------------------------------------------------------- UI-LEFT-3 F3-2: the corner operations, a locked graphic

const square: CmdShape = { kind: "polygon", id: "g3", layer: "F.SilkS", stroke_width: 120_000, filled: false, pts: [{ x: 0, y: 0 }, { x: 4_000_000, y: 0 }, { x: 4_000_000, y: 4_000_000 }, { x: 0, y: 4_000_000 }] };

test("a locked graphic has no edit points (OnSelectionChange: item->IsLocked())", () => {
  assert.equal(fpPointEditTarget([line], new Set(["g2"]), (id) => id === "g2"), null);
  assert.equal(fpPointEditTarget([line], new Set(["g2"]), () => false)?.kind, "segment");
});

test("Create Corner splits a segment at the point nearest the cursor, and a polygon takes a corner on its nearest side", () => {
  const seg = fpPointEditTarget([line], new Set(["g2"]))!;
  assert.ok(fpCanAddCorner(seg));
  const split = fpAddCorner(seg, [500_000, 300_000]);
  assert.equal(split?.kind, "split");
  if (split?.kind !== "split") return;
  assert.deepEqual([split.first.kind === "segment" && split.first.end, split.second.kind === "segment" && split.second.start], [{ x: 500_000, y: 0 }, { x: 500_000, y: 0 }]);
  assert.equal(split.first.id, "g2", "the graphic is the first half");
  assert.equal(fpAddCorner(seg, [-1_000_000, 0]), null, "at an end: nothing to split");
  const poly = fpPointEditTarget([square], new Set(["g3"]))!;
  const added = fpAddCorner(poly, [2_000_000, -100_000]);
  assert.equal(added?.kind, "set");
  if (added?.kind === "set" && added.shape.kind === "polygon") assert.deepEqual(added.shape.pts[1], { x: 2_000_000, y: 0 });
  // a circle has no corner to add
  assert.equal(fpCanAddCorner({ ...seg, kind: "circle", center: [0, 0], end: [1, 0] } as unknown as Parameters<typeof fpCanAddCorner>[0]), false);
});

test("Remove Corner takes the corner under the pointer of a polygon of more than three; of three the polygon goes; Chamfer breaks the nearest corner", () => {
  const poly = fpPointEditTarget([square], new Set(["g3"]))!;
  const corner = fpPointAt(poly, [4_000_000, 0], 50_000)!;
  assert.equal(corner.kind, "corner");
  assert.ok(fpCanRemoveCorner(poly, corner));
  const removed = fpRemoveCorner(poly, corner);
  assert.ok(removed?.kind === "set" && removed.shape.kind === "polygon" && removed.shape.pts.length === 3);
  const side = fpPointAt(poly, [2_000_000, 0], 50_000)!;
  assert.equal(side.kind, "midpoint");
  assert.equal(fpCanRemoveCorner(poly, side), false, "a side's middle is no corner");
  const tri = fpPointEditTarget([{ ...square, pts: square.kind === "polygon" ? square.pts.slice(0, 3) : [] } as CmdShape], new Set(["g3"]))!;
  const v = fpPointAt(tri, [0, 0], 50_000)!;
  assert.equal(fpCanRemoveCorner(tri, v), false, "CanRemoveCorner: an outline of three keeps its corners");
  assert.deepEqual(fpRemoveCorner(tri, v), { kind: "remove" }, "removeCorner itself removes the polygon");
  assert.ok(fpCanChamferCorner(poly));
  const ch = fpChamferCorner(poly, [3_900_000, 3_900_000]);
  assert.ok(ch?.kind === "set" && ch.shape.kind === "polygon" && ch.shape.pts.length === 5, "the corner is two corners now");
  // 5 mm, but a quarter of a 4 mm side at most: 1 mm back along each side
  if (ch?.kind === "set" && ch.shape.kind === "polygon") assert.deepEqual([ch.shape.pts[2], ch.shape.pts[3]], [{ x: 4_000_000, y: 3_000_000 }, { x: 3_000_000, y: 4_000_000 }]);
});

test("Move Corner To / Move Midpoint To: the title by the point's kind, the point put at the typed place", () => {
  const poly = fpPointEditTarget([square], new Set(["g3"]))!;
  const corner = fpPointAt(poly, [0, 0], 50_000)!;
  assert.equal(fpMovePointTitle(corner), "Move Corner to Location");
  const moved = fpMovePoint(poly, corner, [-1_000_000, -1_000_000], 0);
  assert.ok(moved?.kind === "polygon" && moved.pts[0]!.x === -1_000_000 && moved.id === "g3");
  const side = fpPointAt(poly, [2_000_000, 0], 50_000)!;
  assert.equal(fpMovePointTitle(side), "Move Midpoint to Location");
  const seg = fpPointEditTarget([line], new Set(["g2"]))!;
  const end = fpPointAt(seg, [2_000_000, 0], 50_000)!;
  const m2 = fpMovePoint(seg, end, [3_000_000, 1_000_000], 0);
  assert.ok(m2?.kind === "segment" && m2.end.x === 3_000_000 && m2.end.y === 1_000_000);
});

test("UI-LEFT-3 F3-2: a footprint zone's point editor -- corners and sides of its outline and holes, Create / Remove / Chamfer Corner, a locked zone none", async () => {
  const { fpZoneEditTarget, fpZoneEditPoints, fpZonePointAt, fpZoneDrag, fpZoneWith, fpZoneAddCorner, fpZoneCanRemoveCorner, fpZoneRemoveCorner, fpZoneChamferCorner } = await import("./fpPointEdit");
  const sq = (x0: number, y0: number, s: number) => [{ x: x0, y: y0 }, { x: x0 + s, y: y0 }, { x: x0 + s, y: y0 + s }, { x: x0, y: y0 + s }];
  const zone = { id: "z1", net: "", layer: "F.Cu", outline: sq(0, 0, 10_000_000), holes: [sq(4_000_000, 4_000_000, 2_000_000).slice(0, 3)], priority: 3 } as never;
  assert.equal(fpZoneEditTarget([zone], new Set(["z1"]), () => true), null, "locked: no edit points");
  const z = fpZoneEditTarget([zone], new Set(["z1"]))!;
  assert.equal(z.contours.length, 2);
  assert.equal(fpZoneEditPoints(z).length, 2 * (4 + 3), "a corner and a side per vertex of the outline and the hole");
  // a hole's corner dragged
  const hv = fpZonePointAt(z, [6_000_000, 4_000_000], 50_000)!;
  assert.equal(hv.id, "v5");
  const dragged = fpZoneDrag(z, hv, [7_000_000, 4_000_000])!;
  assert.deepEqual(dragged[1]![1], [7_000_000, 4_000_000]);
  const set = fpZoneWith(z, dragged) as { outline: unknown[]; holes?: { x: number; y: number }[][]; priority: number };
  assert.deepEqual(set.holes?.[0]?.[1], { x: 7_000_000, y: 4_000_000 });
  assert.equal(set.priority, 3, "the rest of the zone as it was");
  // Create Corner on the nearest side, of the hole too
  const added = fpZoneAddCorner(z, [5_000_000, -200_000]);
  assert.ok(added?.kind === "contours" && added.contours[0]!.length === 5);
  // a hole's corner of three can go (the hole goes); the outline of four keeps three
  assert.ok(fpZoneCanRemoveCorner(z, hv));
  const r = fpZoneRemoveCorner(z, hv);
  assert.ok(r?.kind === "contours" && r.contours.length === 1, "the hole of three is removed whole");
  const ov = fpZonePointAt(z, [0, 0], 50_000)!;
  const r2 = fpZoneRemoveCorner(z, ov);
  assert.ok(r2?.kind === "contours" && r2.contours[0]!.length === 3);
  const ch = fpZoneChamferCorner(z, [9_900_000, 9_900_000]);
  assert.ok(ch?.kind === "contours" && ch.contours[0]!.length === 5);
});
