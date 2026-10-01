import { test } from "node:test";
import assert from "node:assert/strict";
import { trackball } from "./trackball";
import { lengthQuat, multiplyQuat, quatFromAxisAngle, vec3, type Quat } from "./math3d";

const EPS = 1e-6;

function isIdentity(q: Quat, eps = EPS): boolean {
  return Math.abs(q.x) < eps && Math.abs(q.y) < eps && Math.abs(q.z) < eps && Math.abs(Math.abs(q.w) - 1) < eps;
}

test("trackball: identical points -> identity (zero rotation)", () => {
  assert.ok(isIdentity(trackball(0.1, -0.2, 0.1, -0.2)));
});

test("trackball: origin to origin -> identity", () => {
  assert.ok(isIdentity(trackball(0, 0, 0, 0)));
});

test("trackball: a purely horizontal drag (both points at y=0) rotates about a purely-Y axis", () => {
  // source's vcross(p2,p1) with p1=(x1,0,z1), p2=(x2,0,z2) gives
  // (0, z2*x1 - x2*z1, 0) -- the x/z components of the quaternion's axis
  // must vanish for any such drag, regardless of where along x it starts.
  for (const [x1, x2] of [
    [0, 0.3],
    [-0.2, 0.1],
    [0.5, -0.5],
  ]) {
    const q = trackball(x1!, 0, x2!, 0);
    assert.ok(Math.abs(q.x) < EPS, `q.x should be ~0 for horizontal drag, got ${q.x}`);
    assert.ok(Math.abs(q.z) < EPS, `q.z should be ~0 for horizontal drag, got ${q.z}`);
  }
});

test("trackball: a purely vertical drag (both points at x=0) rotates about a purely-X axis", () => {
  // Symmetric case: p1=(0,y1,z1), p2=(0,y2,z2) -> axis=(y2*z1-z2*y1, 0, 0).
  for (const [y1, y2] of [
    [0, 0.3],
    [-0.2, 0.1],
  ]) {
    const q = trackball(0, y1!, 0, y2!);
    assert.ok(Math.abs(q.y) < EPS, `q.y should be ~0 for vertical drag, got ${q.y}`);
    assert.ok(Math.abs(q.z) < EPS, `q.z should be ~0 for vertical drag, got ${q.z}`);
  }
});

test("trackball: dragging back the way you came undoes the forward rotation", () => {
  // trackball(p2,p1)'s axis is -(trackball(p1,p2)'s axis) with the same
  // angle magnitude (vsub/vlength are symmetric in sign only through
  // vcross's operand swap) -- so composing forward then backward must
  // land back on (approximately) identity.
  const cases: Array<[number, number, number, number]> = [
    [0, 0, 0.3, 0.1],
    [-0.4, 0.2, 0.1, -0.3],
    [0.5, 0.5, -0.2, 0.4],
  ];
  for (const [p1x, p1y, p2x, p2y] of cases) {
    const forward = trackball(p1x, p1y, p2x, p2y);
    const backward = trackball(p2x, p2y, p1x, p1y);
    const roundTrip = multiplyQuat(forward, backward);
    assert.ok(isIdentity(roundTrip, 1e-5), `round trip not identity: ${JSON.stringify(roundTrip)}`);
  }
});

test("trackball: rotation angle grows monotonically with drag distance from the center", () => {
  const angleOf = (q: Quat) => 2 * Math.acos(Math.min(1, Math.abs(q.w)));
  const a = angleOf(trackball(0, 0, 0.05, 0));
  const b = angleOf(trackball(0, 0, 0.2, 0));
  const c = angleOf(trackball(0, 0, 0.5, 0));
  assert.ok(a < b, `${a} < ${b}`);
  assert.ok(b < c, `${b} < ${c}`);
});

test("trackball: hand-computed golden case (small rightward drag from dead center)", () => {
  // p1=(0,0): inside the sphere (d=0), z1 = sqrt(0.8^2) = 0.8.
  // p2=(0.1,0): inside the sphere (d=0.1 < 0.8/sqrt(2)=0.5657),
  //   z2 = sqrt(0.64 - 0.01) = sqrt(0.63) = 0.7937253933.
  // axis = p2 x p1 = (0*0.8 - 0.7937253933*0, 0.7937253933*0 - 0.1*0.8, 0.1*0 - 0*0)
  //      = (0, -0.08, 0) -> normalized (0,-1,0).
  // d = p1-p2 = (-0.1, 0, 0.8-0.7937253933) = (-0.1, 0, 0.0062746067)
  // |d| = sqrt(0.01 + 0.0000393596) = 0.1001965...
  // t = |d| / 1.6 = 0.06262283...
  // phi = 2*asin(t) = 0.12532681... rad
  const q = trackball(0, 0, 0.1, 0);
  const expected = quatFromAxisAngle(vec3(0, -1, 0), 0.1253268);
  assert.ok(Math.abs(q.x - expected.x) < 1e-4, `x: ${q.x} vs ${expected.x}`);
  assert.ok(Math.abs(q.y - expected.y) < 1e-4, `y: ${q.y} vs ${expected.y}`);
  assert.ok(Math.abs(q.z - expected.z) < 1e-4, `z: ${q.z} vs ${expected.z}`);
  assert.ok(Math.abs(q.w - expected.w) < 1e-4, `w: ${q.w} vs ${expected.w}`);
});

test("trackball: output is always a unit quaternion", () => {
  const cases: Array<[number, number, number, number]> = [
    [0, 0, 1, 1],
    [-1, -1, 1, 1],
    [0.9, -0.9, -0.9, 0.9],
    [2, 2, -2, -2], // well outside the sphere on both ends (hyperbolic sheet branch)
  ];
  for (const [p1x, p1y, p2x, p2y] of cases) {
    const q = trackball(p1x, p1y, p2x, p2y);
    assert.ok(Math.abs(lengthQuat(q) - 1) < 1e-6, `|q|=${lengthQuat(q)} for (${p1x},${p1y})->(${p2x},${p2y})`);
  }
});
