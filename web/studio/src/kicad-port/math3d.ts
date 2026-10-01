// Minimal dependency-free 3D vector/quaternion math shared by trackball.ts
// and camera3d.ts. This file intentionally does NOT import three.js (even
// though the app depends on it): every file under kicad-port/ is compiled
// and run standalone by `npm run test:unit` (tools/run-unit-tests.sh, see
// its own header comment) via `tsc` to a scratch directory outside
// web/studio -- Node's CommonJS `require("three")` resolution walks up
// from *that* file's own directory looking for node_modules, and a
// mktemp'd scratch directory has no such ancestor, so a kicad-port module
// that imports "three" fails at test time with MODULE_NOT_FOUND even
// though it type-checks fine and runs fine inside the real app bundle.
// Every other kicad-port file is already dependency-free for this same
// reason (see e.g. view.ts/viewControls.ts's own plain {x,y} objects);
// this file keeps camera3d.ts/trackball.ts consistent with that rather
// than being the one exception. The viewer3d/ components (real React/DOM
// code, never compiled by the test script) convert between these plain
// types and THREE.Vector3/THREE.Quaternion at the point they hand camera
// state to Three.js -- see scene.ts's toThreeVector3/toThreeQuaternion.
//
// Quaternion convention matches three.js's THREE.Quaternion exactly
// (Hamilton product, `multiplyQuat(a,b)` = "apply b's rotation first,
// then a's", same as THREE's `a.multiply(b)`), so nothing is lost or
// reinterpreted at that conversion boundary.

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface Quat {
  x: number;
  y: number;
  z: number;
  w: number;
}

export function vec3(x = 0, y = 0, z = 0): Vec3 {
  return { x, y, z };
}

export const QUAT_IDENTITY: Readonly<Quat> = Object.freeze({ x: 0, y: 0, z: 0, w: 1 });

export function addV(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z };
}

export function subV(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z };
}

export function scaleV(a: Vec3, s: number): Vec3 {
  return { x: a.x * s, y: a.y * s, z: a.z * s };
}

export function negV(a: Vec3): Vec3 {
  return { x: -a.x, y: -a.y, z: -a.z };
}

export function dotV(a: Vec3, b: Vec3): number {
  return a.x * b.x + a.y * b.y + a.z * b.z;
}

export function crossV(a: Vec3, b: Vec3): Vec3 {
  return {
    x: a.y * b.z - a.z * b.y,
    y: a.z * b.x - a.x * b.z,
    z: a.x * b.y - a.y * b.x,
  };
}

export function lengthSqV(a: Vec3): number {
  return a.x * a.x + a.y * a.y + a.z * a.z;
}

export function lengthV(a: Vec3): number {
  return Math.sqrt(lengthSqV(a));
}

/** Zero vector in, zero vector out (no direction to preserve) -- same convention as KiCad's own VECTOR2D::Resize/trackball.cpp's vnormal, which never guard against this either; callers that can hit it (trackball.ts) check first. */
export function normalizeV(a: Vec3): Vec3 {
  const len = lengthV(a);
  if (len === 0) return vec3();
  return scaleV(a, 1 / len);
}

/** Axis must already be a unit vector -- exactly THREE.Quaternion.setFromAxisAngle's own precondition and formula (q.xyz = axis*sin(angle/2), q.w = cos(angle/2)). */
export function quatFromAxisAngle(axis: Vec3, angleRad: number): Quat {
  const half = angleRad / 2;
  const s = Math.sin(half);
  return { x: axis.x * s, y: axis.y * s, z: axis.z * s, w: Math.cos(half) };
}

/** Hamilton product: `multiplyQuat(a,b)` applies b's rotation first, then a's -- i.e. rotateVecByQuat(v, multiplyQuat(a,b)) === rotateVecByQuat(rotateVecByQuat(v,b), a). Matches THREE.Quaternion's `a.clone().multiply(b)`. */
export function multiplyQuat(a: Quat, b: Quat): Quat {
  return {
    w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
    y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
    z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
  };
}

/** The inverse of a *unit* quaternion is its conjugate -- every quaternion this module produces or consumes is kept normalized, so this is used throughout as a plain, cheap inverse (matches THREE.Quaternion.invert's own fast path). */
export function conjugateQuat(q: Quat): Quat {
  return { x: -q.x, y: -q.y, z: -q.z, w: q.w };
}

export function lengthQuat(q: Quat): number {
  return Math.sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
}

export function normalizeQuat(q: Quat): Quat {
  const len = lengthQuat(q);
  if (len === 0) return { ...QUAT_IDENTITY };
  const inv = 1 / len;
  return { x: q.x * inv, y: q.y * inv, z: q.z * inv, w: q.w * inv };
}

/**
 * Rotates `v` by unit quaternion `q` (v' = q*v*q^-1), using the standard
 * "double cross product" expansion rather than two full quaternion
 * multiplies -- the same optimization THREE.Vector3.applyQuaternion uses,
 * so results match it exactly.
 */
export function rotateVecByQuat(v: Vec3, q: Quat): Vec3 {
  const qv = { x: q.x, y: q.y, z: q.z };
  const t = scaleV(crossV(qv, v), 2);
  const cross2 = crossV(qv, t);
  return { x: v.x + q.w * t.x + cross2.x, y: v.y + q.w * t.y + cross2.y, z: v.z + q.w * t.z + cross2.z };
}
