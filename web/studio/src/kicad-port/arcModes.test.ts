import { test } from "node:test";
import assert from "node:assert/strict";
import { arcFromStartEndCenterDrag, arcFromStartEndMidDrag, arcFromStartTangentEnd, arcThroughPoints, projectToChordBisector } from "./arcConstruction";
import { arcClick, arcHasTangentSeed, arcMotion, arcRemoveLastPoint, arcSetMode, arcSetTangentSeed, arcToggleClockwise, incrementArcDrawMode, newArcGeomIn, newArcGeom } from "./arcGeom";
import { arcTangentSeedAt, arcTangentSeedNear } from "./arcTangentSeed";

const MM = 1_000_000;
const near = (a: readonly number[], b: readonly number[], tol = 2) => a.every((v, i) => Math.abs(v - b[i]!) <= tol);

test("the solvers keep the clicked ends and give the midpoint the sweep runs through", () => {
  // a half circle of radius 5 from (0,0) to (10,0) through (5,-5)
  const s = arcThroughPoints([0, 0], [5 * MM, -5 * MM], [10 * MM, 0]);
  assert.ok(s.valid);
  assert.deepEqual([s.start, s.end], [[0, 0], [10 * MM, 0]]);
  assert.ok(near(s.center, [5 * MM, 0]) && Math.abs(s.radius - 5 * MM) < 2, `${s.center} ${s.radius}`);
  assert.ok(near(s.mid, [5 * MM, -5 * MM]));
  assert.equal(arcThroughPoints([0, 0], [5, 0], [10, 0]).valid, false, "collinear points make no arc");
  // the mid drag: the cursor goes onto the chord's bisector
  assert.deepEqual(projectToChordBisector([0, 0], [10 * MM, 0], [3 * MM, -2 * MM]), [5 * MM, -2 * MM]);
  const m = arcFromStartEndMidDrag([0, 0], [10 * MM, 0], [3 * MM, -2 * MM]);
  assert.deepEqual(m.mid, [5 * MM, -2 * MM]);
  assert.equal(arcFromStartEndMidDrag([0, 0], [10 * MM, 0], [3 * MM, 0]).valid, false, "on the chord: no arc");
});

test("start-end-centre: the minor arc bulges away from the centre, the major one toward it", () => {
  const minor = arcFromStartEndCenterDrag([0, 0], [10 * MM, 0], [5 * MM, 5 * MM], false);
  const major = arcFromStartEndCenterDrag([0, 0], [10 * MM, 0], [5 * MM, 5 * MM], true);
  assert.ok(minor.valid && major.valid);
  assert.ok(near(minor.center, [5 * MM, 5 * MM]) && near(major.center, [5 * MM, 5 * MM]));
  assert.ok(minor.mid[1] < 0, `the minor arc's middle is above the chord (${minor.mid})`);
  assert.ok(major.mid[1] > 5 * MM, `the major arc's middle is past the centre (${major.mid})`);
});

test("start-tangent-end: the arc leaves the start along the tangent", () => {
  const t = arcFromStartTangentEnd([0, 0], [1, 0], [10 * MM, 10 * MM]);
  assert.ok(t.valid);
  // the centre is on the normal to the tangent at the start, as far from both ends: (0, 10)
  assert.ok(near(t.center, [0, 10 * MM]) && Math.abs(t.radius - 10 * MM) < 2, `${t.center} ${t.radius}`);
  assert.equal(arcFromStartTangentEnd([0, 0], [1, 0], [10 * MM, 0]).valid, false, "an end on the tangent line makes no arc");
});

test("the exact modes: three clicks, a refused click is ignored (no step back), the ends are the clicks", () => {
  let g = newArcGeomIn("start_end_mid");
  let r = arcClick(g, [0, 0], false);
  g = r.geom!;
  r = arcClick(g, [0, 0], false);
  assert.ok(r.geom && r.geom.step === 1, "an end on the start is refused and the step stays");
  g = arcClick(r.geom!, [10 * MM, 0], false).geom!;
  assert.equal(g.step, 2);
  g = arcMotion(g, [4 * MM, -3 * MM], false);
  r = arcClick(g, [4 * MM, -3 * MM], false);
  assert.ok(r.arc, "the third click ends the arc");
  assert.deepEqual([r.arc!.start, r.arc!.end, r.arc!.mid], [[0, 0], [10 * MM, 0], [5 * MM, -3 * MM]]);
  // Backspace steps back a click
  const back = arcRemoveLastPoint(g);
  assert.equal(back.step, 1);
});

test("start-direction-end: the second click gives the direction, the third the end", () => {
  let g = arcClick(newArcGeomIn("start_dir_end"), [0, 0], false).geom!;
  g = arcClick(g, [5 * MM, 0], false).geom!;
  assert.deepEqual(g.dir, [5 * MM, 0]);
  const r = arcClick(g, [10 * MM, 10 * MM], false);
  assert.ok(r.arc && near(r.arc.center, [0, 10 * MM]));
});

test("tangent: a seed makes one click enough; an axis seed takes the nearer way and `/` flips it", () => {
  let g = arcSetTangentSeed(newArcGeomIn("tangent"), [0, 0], [1, 0]);
  assert.ok(arcHasTangentSeed(g));
  const r = arcClick(g, [10 * MM, 10 * MM], false);
  assert.ok(r.arc && near(r.arc.start, [0, 0]) && near(r.arc.center, [0, 10 * MM]));
  g = arcSetTangentSeed(newArcGeomIn("tangent"), [0, 0], [1, 0], true);
  g = arcMotion(g, [-10 * MM, 10 * MM], false);
  assert.ok(near(g.dir!, [-1, 0], 0), `the cursor behind the start takes the axis the other way (${g.dir})`);
  g = arcToggleClockwise(g);
  assert.ok(near(g.dir!, [1, 0], 0));
  // without a seed the tangent mode takes its start and direction from clicks, as start-direction-end
  let h = arcClick(newArcGeomIn("tangent"), [0, 0], false).geom!;
  h = arcClick(h, [0, 5 * MM], false).geom!;
  assert.deepEqual(h.dir, [0, 5 * MM]);
});

test("the mode changes mid-draw keep the start between modes that start at it; the centre mode restarts; the cycle order", () => {
  let g = arcClick(newArcGeomIn("start_end_mid"), [MM, MM], false).geom!;
  g = arcSetMode(g, "start_dir_end");
  assert.equal(g.mode, "start_dir_end");
  assert.deepEqual(g.pts![0], [MM, MM]);
  assert.equal(g.step, 1);
  const c = arcSetMode(g, "center_start_end");
  assert.equal(c.step, 0);
  assert.equal(incrementArcDrawMode("center_start_end"), "start_end_mid");
  assert.equal(incrementArcDrawMode("start_dir_end"), "center_start_end");
  // the centre mode (8303b2ad's manager, the board's) is as it was
  assert.equal(newArcGeomIn("center_start_end").mode, undefined);
  assert.deepEqual(newArcGeom().mode, undefined);
});

test("tangent seeds: an end of an arc gives its outward direction, a line's middle its axis, a polyline's free end the last side's direction", () => {
  // a quarter arc from (10,0) to (0,10) about the origin (y down): leaving its end (0,10) it heads to -x
  const R = 10 * MM;
  const M = Math.round(R / Math.SQRT2);
  const arc = { kind: "arc" as const, start: [R, 0] as [number, number], mid: [M, M] as [number, number], end: [0, R] as [number, number] };
  const seed = arcTangentSeedNear(arc, [0, R - 5])!;
  assert.deepEqual(seed.start, [0, R]);
  assert.ok(near(seed.direction, [-1, 0], 1e-6), `${seed.direction}`);
  const atStart = arcTangentSeedAt(arc, [R + 3, 2], 100)!;
  assert.deepEqual(atStart.start, [R, 0]);
  assert.ok(near(atStart.direction, [0, -1], 1e-6), `at the start it heads against the sweep (${atStart.direction})`);
  const seg = arcTangentSeedAt({ kind: "segment", start: [0, 0], end: [10 * MM, 0] }, [5 * MM, 50], 100)!;
  assert.deepEqual(seg, { start: [5 * MM, 0], direction: [1, 0], directionIsAxis: true });
  const end = arcTangentSeedAt({ kind: "polygon", pts: [[0, 0], [MM, 0], [MM, MM]], closed: false }, [MM, MM + 20], 100)!;
  assert.deepEqual(end, { start: [MM, MM], direction: [0, 1], directionIsAxis: false });
  assert.equal(arcTangentSeedAt({ kind: "segment", start: [0, 0], end: [MM, 0] }, [MM / 2, 500], 100), null);
});
