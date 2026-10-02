import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ARC_COMPLETE,
  ARC_SET_ANGLE,
  ARC_SET_ORIGIN,
  ARC_SET_START,
  angleOfVector,
  arcAddPoint,
  arcEndRadiusEnd,
  arcRemoveLastPoint,
  arcStartRadiusEnd,
  arcSubtended,
  arcToThreePoints,
  arcToggleClockwise,
  newArcGeom,
  type ArcGeom,
} from "./arcGeom";

/** Origin at (0,0), start locked in at `startPt`, step = SET_ANGLE. */
function armed(startPt: [number, number], angleSnap = false): ArcGeom {
  let g = { ...newArcGeom(), angleSnap };
  g = arcAddPoint(g, [0, 0], true);
  assert.equal(g.step, ARC_SET_START);
  g = arcAddPoint(g, startPt, true);
  assert.equal(g.step, ARC_SET_ANGLE);
  return g;
}

/** The integer board point at `a` degrees on the radius-100 circle (the real cursor is integer too, so e.g. 90 degrees is exactly (0, 100), not (6e-15, 100)). */
function at(a: number): [number, number] {
  const rad = (a * Math.PI) / 180;
  return [Math.round(100 * Math.cos(rad)), Math.round(100 * Math.sin(rad))];
}
/** The angle `ARC_GEOM_MANAGER` actually sees for that point (the rounding moves it a hair off `a`). */
function seen(a: number): number {
  const [x, y] = at(a);
  let v = angleOfVector(x, y);
  while (v < 0) v += 360;
  return v;
}

/** Walk the cursor through each angle, as a real mouse move does between clicks. */
function sweep(g: ArcGeom, anglesDeg: number[]): ArcGeom {
  for (const a of anglesDeg) g = arcAddPoint(g, at(a), false);
  return g;
}

test("angleOfVector: EDA_ANGLE( VECTOR2 ) special cases (the negative x axis is -180, not +180)", () => {
  assert.equal(angleOfVector(0, 0), 0);
  assert.equal(angleOfVector(5, 0), 0);
  assert.equal(angleOfVector(-5, 0), -180);
  assert.equal(angleOfVector(0, 5), 90);
  assert.equal(angleOfVector(0, -5), -90);
  assert.equal(angleOfVector(3, 3), 45);
  assert.equal(angleOfVector(-3, -3), -135);
  assert.equal(angleOfVector(3, -3), -45);
  assert.equal(angleOfVector(-3, 3), 135);
  assert.ok(Math.abs(angleOfVector(1, 2) - 63.4349488) < 1e-6);
});

test("three clicks: origin, then start (radius + start angle), then end -> COMPLETE", () => {
  let g = newArcGeom();
  assert.equal(g.step, ARC_SET_ORIGIN);
  assert.equal(g.clockwise, true, "m_clockwise initialises to true");
  g = arcAddPoint(g, [10, 20], true);
  assert.equal(g.step, ARC_SET_START);
  assert.deepEqual(g.origin, [10, 20]);
  g = arcAddPoint(g, [110, 20], true);
  assert.equal(g.step, ARC_SET_ANGLE);
  assert.equal(g.radius, 100);
  assert.equal(g.startAngle, 0);
  g = arcAddPoint(g, [10, 120], true);
  assert.equal(g.step, ARC_COMPLETE);
});

test("a start click on the origin is rejected (zero radius) and steps the manager back (performStep(false))", () => {
  let g = arcAddPoint(newArcGeom(), [5, 5], true);
  g = arcAddPoint(g, [5, 5], true);
  assert.equal(g.step, ARC_SET_ORIGIN);
});

test("an end click exactly on the start radius is rejected and steps back to SET_START", () => {
  const g = arcAddPoint(armed([100, 0]), [250, 0], true);
  assert.equal(g.step, ARC_SET_START);
});

test("motion without lock-in moves the geometry but not the step", () => {
  let g = armed([100, 0]);
  g = arcAddPoint(g, [0, 100], false);
  assert.equal(g.step, ARC_SET_ANGLE);
  assert.equal(g.endAngle, 90);
});

test("direction follows the shorter way round until the cursor is 90 degrees away, then locks", () => {
  // start at angle 0 (+x); creep the cursor round
  let g = sweep(armed([100, 0]), [10, 45, 80]);
  assert.equal(g.directionLocked, false);
  assert.equal(g.clockwise, false, "ccw (80) < cw (280): the short way is the non-clockwise sweep");
  assert.ok(Math.abs(arcSubtended(g) - -seen(80)) < 1e-9);
  g = sweep(g, [100]);
  assert.equal(g.directionLocked, true, "min(ccw, cw) >= 90 locks");
  assert.equal(g.clockwise, false);
  // past 180 degrees the arc keeps going the same way round (locked) instead of flipping to the shorter way
  g = sweep(g, [200, 300]);
  assert.equal(g.clockwise, false);
  assert.ok(Math.abs(arcSubtended(g) - -seen(300)) < 1e-9);
  // coming back under 90 degrees releases the lock ...
  g = sweep(g, [350, 30]);
  assert.equal(g.directionLocked, false, "|subtended| < 90 unlocks");
  // ... and the next motion re-evaluates the shorter way
  g = sweep(g, [20]);
  assert.equal(g.clockwise, false);
  assert.ok(Math.abs(arcSubtended(g) - -seen(20)) < 1e-9);
});

test("approaching from the other side picks the clockwise sweep", () => {
  let g = sweep(armed([100, 0]), [-10, -45, -80]);
  assert.equal(g.clockwise, true);
  assert.equal(g.directionLocked, false);
  // end = 280 (normalised); clockwise: -(280 - 0 - 360) = +80
  assert.ok(Math.abs(arcSubtended(g) - (360 - seen(-80))) < 1e-9, `${arcSubtended(g)}`);
});

test("ToggleClockwise reverses the sweep and locks the direction (`/`)", () => {
  let g = sweep(armed([100, 0]), [20, 45, 60]);
  assert.ok(Math.abs(arcSubtended(g) - -seen(60)) < 1e-9);
  g = arcToggleClockwise(g);
  assert.equal(g.directionLocked, true);
  assert.equal(g.clockwise, true);
  assert.ok(Math.abs(arcSubtended(g) - (360 - seen(60))) < 1e-9, "the long way round");
  // locked: moving the cursor a little does not flip it back
  g = sweep(g, [62]);
  assert.equal(g.clockwise, true);
  // toggling twice returns to the short way
  g = arcToggleClockwise(g);
  assert.equal(g.clockwise, false);
});

test("arcToThreePoints: a quarter arc from +x to +y (short way)", () => {
  // creep up to 80 first (sets clockwise = false), then land exactly on 90
  const g = sweep(armed([100, 0]), [30, 60, 80, 90]);
  assert.equal(g.clockwise, false);
  assert.equal(g.directionLocked, true, "exactly 90: min(90, 270) >= 90");
  const arc = arcToThreePoints(g)!;
  assert.deepEqual(arc.start, [100, 0]);
  assert.deepEqual(arc.end, [0, 100]);
  assert.deepEqual(arc.mid, [71, 71]);
  assert.ok(Math.abs(arc.sweepDeg - 90) < 1e-9);
  assert.equal(arc.radius, 100);
});

test("arcToThreePoints: the posture key draws the long way, start/end swapped as updateArcFromConstructionMgr does", () => {
  let g = sweep(armed([100, 0]), [30, 60, 80, 90]);
  g = arcToggleClockwise(g);
  assert.ok(Math.abs(arcSubtended(g) - 270) < 1e-9);
  const arc = arcToThreePoints(g)!;
  assert.deepEqual(arc.start, [0, 100], "subtended >= 0: start is the END radius end");
  assert.deepEqual(arc.end, [100, 0]);
  assert.deepEqual(arc.mid, [-71, -71], "half way round the 270-degree sweep (angle 90 + 135)");
  assert.ok(Math.abs(arc.sweepDeg - 270) < 1e-9);
  // toggling back restores the short arc
  const back = arcToThreePoints(arcToggleClockwise(g))!;
  assert.deepEqual(back.start, [100, 0]);
  assert.deepEqual(back.mid, [71, 71]);
  assert.deepEqual(back.end, [0, 100]);
});

test("arcToThreePoints needs a radius and a distinct end angle", () => {
  assert.equal(arcToThreePoints(newArcGeom()), null);
  assert.equal(arcToThreePoints(armed([100, 0])), null, "end angle == start angle: subtended 0 (clockwise) -> nothing to draw");
});

test("angle snap rounds the start and end angles to 45 degrees (SetAngleSnap)", () => {
  let g = armed([100, 20], true);
  assert.equal(g.startAngle, 0, "11.3 degrees snaps to 0");
  g = arcAddPoint(g, [60, 80], false);
  assert.equal(g.endAngle, 45, "53 degrees snaps to 45");
});

test("radius-end points sit on the circle (GetStartRadiusEnd / GetEndRadiusEnd)", () => {
  const g = sweep(armed([0, -100]), [-60]);
  assert.deepEqual(arcStartRadiusEnd(g), [0, -100]);
  const e = arcEndRadiusEnd(g);
  assert.ok(Math.abs(Math.hypot(e[0], e[1]) - 100) < 1);
});

test("RemoveLastPoint steps back and reprocesses the last point in the earlier step", () => {
  let g = armed([100, 0]);
  g = arcAddPoint(g, [0, 100], false);
  g = arcRemoveLastPoint(g);
  assert.equal(g.step, ARC_SET_START);
  // the last point (0,100) was reprocessed as a start point: radius 100, start angle 90
  assert.equal(g.startAngle, 90);
  g = arcRemoveLastPoint(arcRemoveLastPoint(g));
  assert.equal(g.step, ARC_SET_ORIGIN);
});
