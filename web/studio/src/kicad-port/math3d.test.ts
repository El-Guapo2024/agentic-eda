import { test } from "node:test";
import assert from "node:assert/strict";
import {
  addV,
  conjugateQuat,
  crossV,
  dotV,
  lengthQuat,
  lengthV,
  multiplyQuat,
  normalizeQuat,
  normalizeV,
  QUAT_IDENTITY,
  quatFromAxisAngle,
  rotateVecByQuat,
  scaleV,
  subV,
  vec3,
} from "./math3d";

const EPS = 1e-9;
function closeV(a: { x: number; y: number; z: number }, b: { x: number; y: number; z: number }, eps = EPS) {
  assert.ok(Math.abs(a.x - b.x) < eps && Math.abs(a.y - b.y) < eps && Math.abs(a.z - b.z) < eps, `${JSON.stringify(a)} != ${JSON.stringify(b)}`);
}

test("vector basics", () => {
  closeV(addV(vec3(1, 2, 3), vec3(4, 5, 6)), vec3(5, 7, 9));
  closeV(subV(vec3(5, 7, 9), vec3(1, 2, 3)), vec3(4, 5, 6));
  closeV(scaleV(vec3(1, 2, 3), 2), vec3(2, 4, 6));
  assert.equal(dotV(vec3(1, 0, 0), vec3(0, 1, 0)), 0);
  assert.equal(dotV(vec3(2, 3, 4), vec3(2, 3, 4)), 4 + 9 + 16);
  closeV(crossV(vec3(1, 0, 0), vec3(0, 1, 0)), vec3(0, 0, 1));
  assert.equal(lengthV(vec3(3, 4, 0)), 5);
});

test("normalizeV: zero vector stays zero, non-zero gets unit length", () => {
  closeV(normalizeV(vec3(0, 0, 0)), vec3(0, 0, 0));
  const n = normalizeV(vec3(3, 4, 0));
  assert.ok(Math.abs(lengthV(n) - 1) < EPS);
  closeV(n, vec3(0.6, 0.8, 0));
});

test("quatFromAxisAngle: 90deg about Z rotates +X to +Y", () => {
  const q = quatFromAxisAngle(vec3(0, 0, 1), Math.PI / 2);
  const rotated = rotateVecByQuat(vec3(1, 0, 0), q);
  closeV(rotated, vec3(0, 1, 0), 1e-9);
});

test("quatFromAxisAngle: 180deg about X flips Y and Z", () => {
  const q = quatFromAxisAngle(vec3(1, 0, 0), Math.PI);
  closeV(rotateVecByQuat(vec3(0, 1, 0), q), vec3(0, -1, 0), 1e-9);
  closeV(rotateVecByQuat(vec3(0, 0, 1), q), vec3(0, 0, -1), 1e-9);
});

test("multiplyQuat: applies the right-hand operand's rotation first", () => {
  // Rotate +X by 90 about Z then 90 about Y (applied via a single composed
  // quaternion) should match applying them one at a time in that order.
  const rz = quatFromAxisAngle(vec3(0, 0, 1), Math.PI / 2);
  const ry = quatFromAxisAngle(vec3(0, 1, 0), Math.PI / 2);
  const composed = multiplyQuat(ry, rz); // apply rz first, then ry
  const stepwise = rotateVecByQuat(rotateVecByQuat(vec3(1, 0, 0), rz), ry);
  closeV(rotateVecByQuat(vec3(1, 0, 0), composed), stepwise, 1e-9);
});

test("conjugateQuat is the inverse of a unit quaternion", () => {
  const q = quatFromAxisAngle(normalizeVec(1, 2, 3), 1.234);
  const inv = conjugateQuat(q);
  const roundTrip = multiplyQuat(inv, q);
  assert.ok(Math.abs(roundTrip.x) < 1e-9 && Math.abs(roundTrip.y) < 1e-9 && Math.abs(roundTrip.z) < 1e-9);
  assert.ok(Math.abs(Math.abs(roundTrip.w) - 1) < 1e-9);

  function normalizeVec(x: number, y: number, z: number) {
    const len = Math.sqrt(x * x + y * y + z * z);
    return vec3(x / len, y / len, z / len);
  }
});

test("normalizeQuat: zero quaternion falls back to identity, others become unit length", () => {
  const z = normalizeQuat({ x: 0, y: 0, z: 0, w: 0 });
  assert.deepEqual(z, { ...QUAT_IDENTITY });
  const n = normalizeQuat({ x: 1, y: 1, z: 1, w: 1 });
  assert.ok(Math.abs(lengthQuat(n) - 1) < 1e-9);
});
