// Port of 3d-viewer/3d_rendering/trackball.cpp -- the classic SGI "virtual
// trackball" (Gavin Bell, Silicon Graphics, 1988), used byte-for-byte by
// KiCad's 3D viewer camera (track_ball.cpp's TRACK_BALL::Drag, see
// camera3d.ts). Dragging the left mouse button over the 3D view behaves
// like spinning a ball the size of the viewport: points under the cursor
// are projected onto a virtual sphere (or a hyperbolic sheet once the
// cursor strays outside it, so a drag far from the center still produces
// a smooth, bounded rotation instead of an undefined one), and the
// rotation that would carry the first projected point to the second is
// returned as a quaternion.
//
// Uses this directory's own dependency-free math3d.ts (see that file's
// header comment for why kicad-port/ never imports three.js, even though
// the app depends on it) rather than THREE.Vector3/Quaternion.
// math3d.ts's quatFromAxisAngle is defined exactly as source's
// axis_to_quat (vnormal(a); q.xyz = a*sin(phi/2); q.w = cos(phi/2)), so
// only the deformed-sphere projection and the cross-product axis/angle
// derivation below are KiCad/SGI-specific and actually ported by hand.
import { crossV, lengthSqV, lengthV, normalizeV, quatFromAxisAngle, subV, vec3, type Quat } from "./math3d";

/**
 * trackball.cpp: `#define TRACKBALLSIZE (0.8f)` -- radius of the virtual
 * sphere, in the same normalized [-1,1] screen-space `p1x/p1y/p2x/p2y`
 * below are given in. A real KiCad constant, not a tunable.
 */
const TRACKBALLSIZE = 0.8;

const SQRT1_2 = Math.SQRT1_2; // 1/sqrt(2)
const SQRT2 = Math.SQRT2;

/**
 * trackball.cpp:tb_project_to_sphere, unchanged. Projects a 2D point onto
 * the virtual sphere of radius `r` if it falls inside the sphere's
 * silhouette (`d < r/sqrt(2)`, the point on the sphere directly above
 * (x,y)), or onto a hyperbolic sheet outside it (so the projected depth
 * decreases smoothly instead of the sphere equation going imaginary past
 * the silhouette).
 */
function projectToSphere(r: number, x: number, y: number): number {
  const d = Math.sqrt(x * x + y * y);

  if (d < r * SQRT1_2) {
    // Inside the sphere: on the sphere itself, z = sqrt(r^2 - d^2).
    return Math.sqrt(r * r - d * d);
  }

  // On the hyperbolic sheet: t = r/sqrt(2); z = t*t/d.
  const t = r / SQRT2;
  return (t * t) / d;
}

const IDENTITY: Quat = { x: 0, y: 0, z: 0, w: 1 };

/**
 * trackball.cpp:trackball, ported condition-for-condition. `p1x,p1y` is
 * the drag's last position and `p2x,p2y` its current one, both already
 * scaled by the caller to the sphere's own [-1,1] normalized space (see
 * camera3d.ts's TrackballCamera3D.drag, which reproduces
 * TRACK_BALL::Drag's exact `(2*px - W)/W, (H - 2*py)/H` pixel-to-NDC
 * conversion). Returns the incremental rotation as a unit quaternion;
 * identity when the two points coincide ("zero rotation", source's own
 * early-out).
 *
 * Source clamps `t = |p1-p2| / (2*TRACKBALLSIZE)` to [-1,1] "to avoid
 * problems with out-of-control values" before `phi = 2*asin(t)` --
 * reproduced exactly, including that this is the ONLY numeric guard
 * source has; a degenerate zero-length cross product (axis) past that
 * point is not something source's own real mouse-move events can
 * trigger, but this port adds one more defensive fallback to identity
 * rather than ever handing quatFromAxisAngle a zero-length axis.
 */
export function trackball(p1x: number, p1y: number, p2x: number, p2y: number): Quat {
  if (p1x === p2x && p1y === p2y) {
    return IDENTITY;
  }

  const p1 = vec3(p1x, p1y, projectToSphere(TRACKBALLSIZE, p1x, p1y));
  const p2 = vec3(p2x, p2y, projectToSphere(TRACKBALLSIZE, p2x, p2y));

  // source: `vcross(p2, p1, a)` -- axis = p2 x p1. This operand order
  // (not p1 x p2) is load-bearing: swapping it reverses every drag.
  const axis = crossV(p2, p1);

  // source: `vsub(p1, p2, d); t = vlength(d) / (2*TRACKBALLSIZE)`.
  const d = subV(p1, p2);
  let t = lengthV(d) / (2 * TRACKBALLSIZE);
  t = Math.min(1, Math.max(-1, t));
  const phi = 2 * Math.asin(t);

  if (lengthSqV(axis) < 1e-12) {
    // See header comment: not an observed source case, just a defensive
    // fallback so a degenerate drag can never produce a NaN axis.
    return IDENTITY;
  }

  // source: axis_to_quat(a, phi, q).
  return quatFromAxisAngle(normalizeV(axis), phi);
}
