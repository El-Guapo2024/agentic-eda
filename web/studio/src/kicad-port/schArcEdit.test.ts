import { test } from "node:test";
import assert from "node:assert/strict";
import { arcEditPoints, findArcPoint, moveArcPoint, schArcEditMode, setSchArcEditMode } from "./schArcEdit";

// a quarter of a 10.16 mm circle about (0, 0): from (10.16, 0) through 45 degrees to (0, 10.16) (y down)
const R = 10_160_000;
const M = Math.round(R / Math.SQRT2);
const arc = { type: "arc" as const, start: { x: R, y: 0 }, mid: { x: M, y: M }, end: { x: 0, y: R } };

test("MakePoints: start, middle, end and centre, and the lines from the centre to the ends", () => {
  const e = arcEditPoints(arc);
  assert.deepEqual(e.points.map((p) => p.role), ["start", "mid", "end", "center"]);
  const c = e.points[3]!.pos;
  assert.ok(Math.abs(c[0]) <= 1 && Math.abs(c[1]) <= 1, `centre ${c}`);
  assert.deepEqual(e.lines.map(([, b]) => b), [[R, 0], [0, R]]);
  assert.equal(findArcPoint(e, [R + 100, 0], 200), "start");
  assert.equal(findArcPoint(e, [R / 2, R / 2], 200), null);
});

test("UpdateItem: the centre moves the arc in the keep-centre modes; an end moves round the centre; the middle sets the radius", () => {
  const moved = moveArcPoint(arc, "center", [2_540_000, 0], "keep_center_adjust_angle_radius")!;
  assert.deepEqual([moved.start, moved.end], [{ x: R + 2_540_000, y: 0 }, { x: 2_540_000, y: R }]);
  // KEEP_CENTER_ADJUST_ANGLE_RADIUS: the end goes to the cursor, the start follows to the new radius
  const end = moveArcPoint(arc, "end", [-R * 2, 0], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(end.end, { x: -R * 2, y: 0 });
  assert.equal(Math.round(Math.hypot(end.start.x, end.start.y)), R * 2);
  // KEEP_CENTER_ENDS_ADJUST_ANGLE: the radius is kept
  const kept = moveArcPoint(arc, "end", [-R * 2, 0], "keep_center_ends_adjust_angle")!;
  assert.deepEqual(kept.end, { x: -R, y: 0 });
  assert.deepEqual(kept.start, arc.start);
  const mid = moveArcPoint(arc, "mid", [R, R], "keep_center_adjust_angle_radius")!;
  assert.equal(Math.round(Math.hypot(mid.start.x, mid.start.y) / 1000), Math.round((R * Math.SQRT2) / 1000), "the radius is the cursor's distance");
  // KEEP_ENDPOINTS_OR_START_DIRECTION: the ends stay where they are
  const bulge = moveArcPoint(arc, "mid", [R, R], "keep_endpoints_or_start_direction")!;
  assert.deepEqual([bulge.start, bulge.end], [arc.start, arc.end]);
});

test("the mode starts at KiCad's default and Ctrl+Space cycles it", () => {
  assert.equal(schArcEditMode(), "keep_center_adjust_angle_radius");
  assert.equal(setSchArcEditMode("cycle"), "keep_center_ends_adjust_angle");
  assert.equal(setSchArcEditMode("keep_endpoints_or_start_direction"), "keep_endpoints_or_start_direction");
  assert.equal(setSchArcEditMode("cycle"), "keep_center_adjust_angle_radius");
});
