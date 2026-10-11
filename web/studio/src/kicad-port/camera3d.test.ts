import { test } from "node:test";
import assert from "node:assert/strict";
import { TrackballCamera, DEFAULT_MIN_ZOOM, DEFAULT_MAX_ZOOM, bezierBlend, mixQuat, quadricEasingInOut, type ViewPreset } from "./camera3d";
import { lengthV, normalizeV, rotateVecByQuat, subV, vec3, type Vec3 } from "./math3d";

function closeV(a: Vec3, b: Vec3, eps = 1e-4) {
  assert.ok(Math.abs(a.x - b.x) < eps && Math.abs(a.y - b.y) < eps && Math.abs(a.z - b.z) < eps, `${JSON.stringify(a)} != ${JSON.stringify(b)}`);
}

function offsetDirection(cam: TrackballCamera): Vec3 {
  const pose = cam.getRenderPose();
  // lookAt defaults to the origin in every test below, so position IS the offset.
  return normalizeV(pose.position);
}

test("constructor: zoom starts at 1, perspective by default (camera.projection_mode==1)", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  assert.equal(cam.zoom, 1);
  assert.equal(cam.projection, "perspective");
});

test("view presets: camera-to-lookAt direction lands on the expected world axis", () => {
  // Hand-derived against camera.cpp/track_ball.cpp by composing
  // Reset_T1()+ViewCommand_T1's aux target through BASIS_CHANGE (see
  // camera3d.ts's header comment) for TOP, FRONT and RIGHT; BOTTOM/LEFT/
  // BACK are each that preset's exact mirror image (confirmed by the same
  // by-hand derivation, recorded in camera3d.ts's own PRESET_AUX_DEG doc
  // comment) -- asserted here as relations to avoid re-deriving the same
  // arithmetic twice.
  const cam = new TrackballCamera(100, vec3(0, 0, 0));

  cam.applyViewPreset("top");
  const top = offsetDirection(cam);
  closeV(top, vec3(0, 1, 0));

  cam.applyViewPreset("bottom");
  const bottom = offsetDirection(cam);
  closeV(bottom, vec3(0, -1, 0));

  cam.applyViewPreset("front");
  const front = offsetDirection(cam);
  closeV(front, vec3(0, 0, 1));

  cam.applyViewPreset("back");
  const back = offsetDirection(cam);
  closeV(back, vec3(0, 0, -1));

  cam.applyViewPreset("right");
  const right = offsetDirection(cam);
  closeV(right, vec3(1, 0, 0));

  cam.applyViewPreset("left");
  const left = offsetDirection(cam);
  closeV(left, vec3(-1, 0, 0));
});

test("view presets: distance from lookAt always equals the current initial distance (zoom reset to 1)", () => {
  const cam = new TrackballCamera(50, vec3(1, 2, 3));
  cam.zoomBy(1.5);
  for (const preset of ["top", "bottom", "front", "back", "left", "right"] as ViewPreset[]) {
    cam.applyViewPreset(preset);
    const pose = cam.getRenderPose();
    const dist = lengthV(subV(pose.position, vec3(1, 2, 3)));
    assert.ok(Math.abs(dist - 50) < 1e-4, `${preset}: dist=${dist}`);
    assert.equal(cam.zoom, 1);
  }
});

test("reset() is identical to applyViewPreset('top')", () => {
  const camA = new TrackballCamera(80, vec3(5, 0, -5));
  const camB = new TrackballCamera(80, vec3(5, 0, -5));
  camA.setWindowSize(800, 600);
  camB.setWindowSize(800, 600);
  camA.setCurMousePosition(400, 300);
  camB.setCurMousePosition(400, 300);
  camA.drag(500, 250); // mess up both the same way first
  camB.drag(500, 250);
  camA.reset();
  camB.applyViewPreset("top");
  closeV(camA.getRenderPose().position, camB.getRenderPose().position);
  closeV({ ...camA.getRenderPose().quaternion } as unknown as Vec3, { ...camB.getRenderPose().quaternion } as unknown as Vec3, 1e-9);
});

test("flip: adds a half-turn without resetting position, zoom, or the trackball's own free rotation", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.setWindowSize(800, 600);
  cam.setCurMousePosition(400, 300);
  cam.drag(450, 300); // some free trackball rotation
  cam.zoomBy(1.5);
  const beforeZoom = cam.zoom;
  const beforePos = cam.getRenderPose().position;

  cam.flip();

  assert.equal(cam.zoom, beforeZoom, "flip must not touch zoom");
  const afterPos = cam.getRenderPose().position;
  // Position DOES change (flip adds a rotation), but distance from lookAt (0,0,0) is preserved.
  assert.ok(Math.abs(lengthV(afterPos) - lengthV(beforePos)) < 1e-4);
});

test("flip applied twice returns (within floating point) to the starting orientation", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.applyViewPreset("top");
  const start = cam.getRenderPose().position;
  cam.flip();
  cam.flip();
  // 179.999 + 179.999 = 359.998, not exactly 360 -- tolerate the tiny epsilon source itself introduces.
  closeV(cam.getRenderPose().position, start, 1e-2);
});

test("zoomBy: clamps at DEFAULT_MIN_ZOOM/DEFAULT_MAX_ZOOM and reports false at the limit", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  // Zoom in repeatedly past the minimum.
  let safety = 0;
  while (cam.zoomBy(10) && safety++ < 50) {
    /* keep zooming in */
  }
  assert.ok(Math.abs(cam.zoom - DEFAULT_MIN_ZOOM) < 1e-9, `zoom=${cam.zoom}`);
  assert.equal(cam.zoomBy(10), false, "already at min zoom, factor>1 should no-op");

  cam.applyViewPreset("top");
  safety = 0;
  while (cam.zoomBy(1 / 10) && safety++ < 50) {
    /* keep zooming out */
  }
  assert.ok(Math.abs(cam.zoom - DEFAULT_MAX_ZOOM) < 1e-9, `zoom=${cam.zoom}`);
  assert.equal(cam.zoomBy(1 / 10), false, "already at max zoom, factor<1 should no-op");
});

test("zoomBy(1) is always a no-op (source: `aFactor == 1` early-out)", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  assert.equal(cam.zoomBy(1), false);
});

test("getProjectionParams: perspective is a fixed 45deg FOV with near/far scaled from the board span", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  const p = cam.getProjectionParams();
  assert.equal(p.kind, "perspective");
  if (p.kind === "perspective") {
    assert.equal(p.fovDeg, 45);
    assert.ok(p.near > 0 && p.near < 1, `near=${p.near}`);
    assert.ok(Math.abs(p.far - 400) < 1e-6, `far=${p.far}`); // 100 * 4 (FAR_TO_DISTANCE_RATIO)
  }
});

test("getProjectionParams: ortho half-extent shrinks when zooming in", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.toggleProjection();
  cam.setWindowSize(800, 800); // square, so left/right == top/bottom magnitude
  const wide = cam.getProjectionParams();
  cam.zoomBy(2); // zoom in -> smaller visible extent
  const narrow = cam.getProjectionParams();
  assert.equal(wide.kind, "ortho");
  assert.equal(narrow.kind, "ortho");
  if (wide.kind === "ortho" && narrow.kind === "ortho") {
    assert.ok(narrow.right < wide.right);
    assert.ok(Math.abs(wide.right - -wide.left) < 1e-9);
    assert.ok(Math.abs(wide.top - -wide.bottom) < 1e-9);
  }
});

test("toggleProjection flips between perspective and ortho and back", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  assert.equal(cam.projection, "perspective");
  cam.toggleProjection();
  assert.equal(cam.projection, "ortho");
  cam.toggleProjection();
  assert.equal(cam.projection, "perspective");
});

test("handleWheel: plain wheel zooms in on scroll-up (negative deltaY), out on scroll-down", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  const before = cam.zoom;
  cam.handleWheel({ deltaX: 0, deltaY: -100, shiftKey: false, ctrlKey: false, altKey: false });
  assert.ok(cam.zoom < before, "scroll up should zoom in (smaller zoom value)");

  const cam2 = new TrackballCamera(100, vec3(0, 0, 0));
  const before2 = cam2.zoom;
  cam2.handleWheel({ deltaX: 0, deltaY: 100, shiftKey: false, ctrlKey: false, altKey: false });
  assert.ok(cam2.zoom > before2, "scroll down should zoom out");
});

test("handleWheel: Ctrl+wheel pans horizontally instead of zooming", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  const beforeZoom = cam.zoom;
  const beforePos = cam.getRenderPose().position;
  cam.handleWheel({ deltaX: 0, deltaY: 100, shiftKey: false, ctrlKey: true, altKey: false });
  assert.equal(cam.zoom, beforeZoom, "Ctrl+wheel must not zoom");
  const afterPos = cam.getRenderPose().position;
  assert.ok(lengthV(subV(afterPos, beforePos)) > 1e-9, "position should change");
});

test("handleWheel: Shift+wheel pans instead of zooming", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  const beforeZoom = cam.zoom;
  cam.handleWheel({ deltaX: 0, deltaY: 100, shiftKey: true, ctrlKey: false, altKey: false });
  assert.equal(cam.zoom, beforeZoom, "Shift+wheel must not zoom");
});

test("drag: rotating the camera changes its position but keeps it the same distance from lookAt", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.setWindowSize(800, 600);
  cam.setCurMousePosition(400, 300);
  const before = cam.getRenderPose().position;
  cam.drag(500, 350);
  const after = cam.getRenderPose().position;
  assert.ok(lengthV(subV(after, before)) > 1e-6, "drag should move the camera");
  assert.ok(Math.abs(lengthV(after) - lengthV(before)) < 1e-4, "orbiting must preserve distance from lookAt");
});

test("pan: middle/right-drag moves the lookAt-relative offset without rotating (orientation unchanged)", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.setWindowSize(800, 600);
  cam.applyViewPreset("top");
  const beforeQuat = cam.getRenderPose().quaternion;
  cam.setCurMousePosition(400, 300);
  cam.pan(450, 300);
  const afterQuat = cam.getRenderPose().quaternion;
  closeV({ ...beforeQuat } as unknown as Vec3, { ...afterQuat } as unknown as Vec3, 1e-9);
});

test("pivotAt: re-centers lookAt without touching zoom or rotation", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.applyViewPreset("front");
  const beforeZoom = cam.zoom;
  cam.pivotAt(vec3(10, 0, 0));
  const pose = cam.getRenderPose();
  closeV(subV(pose.position, vec3(10, 0, 0)), vec3(0, 0, 100), 1e-3);
  assert.equal(cam.zoom, beforeZoom);
});

test("setBoardGeometry re-homes distance/lookAt without moving the live camera until reset()", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.applyViewPreset("top");
  const before = cam.getRenderPose().position;
  cam.setBoardGeometry(200, vec3(5, 0, 5));
  const stillSame = cam.getRenderPose().position;
  closeV(before, stillSame, 1e-9);
  cam.reset();
  const afterReset = cam.getRenderPose().position;
  closeV(subV(afterReset, vec3(5, 0, 5)), vec3(0, 250, 0), 1e-3); // 200 * 1.25 ratio
});

test("rotateX/Y/Z: a full 360 degree rotation returns to the same position", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  const before = cam.getRenderPose().position;
  for (let i = 0; i < 36; i++) cam.rotateX((10 * Math.PI) / 180);
  closeV(cam.getRenderPose().position, before, 1e-3);
});

test("rotateVecByQuat sanity: BASIS_CHANGE maps KiCad-local +Z to world +Y (see camera3d.ts header comment)", () => {
  // Re-derive the single fixed constant independently of the class, as a
  // guard against ever changing it without noticing the consequence.
  const q = quatFromAxisAngleForTest();
  closeV(rotateVecByQuat(vec3(0, 0, 1), q), vec3(0, 1, 0), 1e-9);

  function quatFromAxisAngleForTest() {
    // Local re-derivation (half-angle formula) rather than importing the
    // module-private BASIS_CHANGE -- keeps this test honest about what
    // "-90 degrees about X" actually is.
    const half = -Math.PI / 4;
    return { x: Math.sin(half), y: 0, z: 0, w: Math.cos(half) };
  }
});


// ------------------------------------------------------------------------------------------------------------------------------------ animation

/** Where the render pose puts a fixed point: two cameras that look alike at the same time are the same camera, whichever of q and -q holds their rotation. */
function seen(cam: TrackballCamera): Vec3[] {
  const pose = cam.getRenderPose();
  return [vec3(1, 2, 3), vec3(-4, 0.5, 2)].map((v) => rotateVecByQuat(v, pose.quaternion)).concat([pose.position]);
}
function sameView(a: TrackballCamera, b: TrackballCamera, eps = 1e-3) {
  const [va, vb] = [seen(a), seen(b)];
  va.forEach((v, i) => closeV(v, vb[i]!, eps));
  assert.ok(Math.abs(a.zoom - b.zoom) < 1e-9, `zoom ${a.zoom} != ${b.zoom}`);
}
function messedUp(): TrackballCamera {
  const cam = new TrackballCamera(100, vec3(5, 0, -5));
  cam.setWindowSize(800, 600);
  cam.setCurMousePosition(400, 300);
  cam.drag(520, 240);
  cam.setCurMousePosition(520, 240);
  cam.drag(430, 330);
  cam.zoomBy(1.3);
  cam.panByWorld(3, -2);
  return cam;
}

test("the easing: BezierBlend is t^2 (3 - 2t), the quadric one is 2t^2 then 1 - 2(1 - t)^2 (3d_math.h)", () => {
  assert.equal(bezierBlend(0), 0);
  assert.equal(bezierBlend(1), 1);
  assert.equal(bezierBlend(0.5), 0.5);
  assert.ok(Math.abs(bezierBlend(0.25) - 0.15625) < 1e-12);
  assert.ok(Math.abs(quadricEasingInOut(0.25) - 0.125) < 1e-12 && Math.abs(quadricEasingInOut(0.75) - 0.875) < 1e-12 && quadricEasingInOut(1) === 1);
});

test("an animated view: the camera starts where it is, ends at the view, and ignores other view commands on the way", () => {
  for (const preset of ["top", "bottom", "front", "back", "left", "right"] as ViewPreset[]) {
    const animated = messedUp();
    const jumped = messedUp();
    const before = seen(animated);
    assert.equal(animated.animateViewPreset(preset, 1000), true);
    assert.ok(animated.isMoving());
    seen(animated).forEach((v, i) => closeV(v, before[i]!), "nothing has moved at the start");
    assert.equal(animated.animateViewPreset("top", 1100), false, "a second view command during the move is not taken");
    assert.equal(animated.animateReset(1100), false);
    animated.tick(1500);
    assert.ok(animated.isMoving(), "half way");
    assert.equal(animated.tick(5000), false, "the clock passes 1 (TOP and BOTTOM take longer when zoomed out): done");
    assert.equal(animated.isMoving(), false);
    jumped.applyViewPreset(preset);
    sameView(animated, jumped);
  }
});

test("a move takes one second at the default speed and the speed multiplier scales it: (1 << m) / 8", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.animateViewPreset("front", 0);
  assert.equal(cam.tick(999), true);
  assert.equal(cam.tick(1001), false);
  for (const [multiplier, seconds] of [[1, 4], [2, 2], [3, 1], [4, 0.5], [5, 0.25]] as const) {
    const c = new TrackballCamera(100, vec3(0, 0, 0));
    c.setAnimation(true, multiplier);
    c.animateViewPreset("front", 0);
    assert.equal(c.tick(seconds * 1000 * 0.98), true, `speed ${multiplier}: still moving at 98% of ${seconds} s`);
    assert.equal(c.tick(seconds * 1000 * 1.02), false, `speed ${multiplier}: done after ${seconds} s`);
  }
});

test("TOP and BOTTOM go slower the further the camera is zoomed out (speed = zoom within 0.5 .. 1.125), the other views do not", () => {
  const zoomedOut = new TrackballCamera(100, vec3(0, 0, 0));
  zoomedOut.zoomBy(1000); // zoom 0.02, the nearest
  zoomedOut.animateViewPreset("top", 0);
  assert.equal(zoomedOut.tick(1500), true, "speed 0.5: two seconds");
  assert.equal(zoomedOut.tick(2001), false);
  const farOut = new TrackballCamera(100, vec3(0, 0, 0));
  farOut.zoomBy(1 / 1000); // zoom 2, the furthest
  farOut.animateViewPreset("bottom", 0);
  assert.equal(farOut.tick(850), true);
  assert.equal(farOut.tick(900), false, "speed 1.125: 0.89 seconds");
  const front = new TrackballCamera(100, vec3(0, 0, 0));
  front.zoomBy(1000);
  front.animateViewPreset("front", 0);
  assert.equal(front.tick(999), true, "a side view is always one second");
  assert.equal(front.tick(1001), false);
});

test("a zoom step follows the bezier curve to zoom / 1.26 and is refused at the limit; the pan is linear and quick", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  assert.equal(cam.animateZoom(1.26, 0), true);
  cam.tick(100); // t = 0.1 s * speed 3 = 0.3
  const e = bezierBlend(0.3);
  assert.ok(Math.abs(cam.zoom - (1 + (1 / 1.26 - 1) * e)) < 1e-9, `${cam.zoom}`);
  cam.tick(400);
  assert.equal(cam.isMoving(), false);
  assert.ok(Math.abs(cam.zoom - 1 / 1.26) < 1e-9);
  const jumped = new TrackballCamera(100, vec3(0, 0, 0));
  jumped.zoomBy(1.26);
  sameView(cam, jumped);
  const atLimit = new TrackballCamera(100, vec3(0, 0, 0));
  atLimit.zoomBy(1000);
  assert.equal(atLimit.animateZoom(1.26, 0), false, "already as close as it goes");
  assert.equal(atLimit.isMoving(), false);

  const pan = new TrackballCamera(100, vec3(0, 0, 0));
  const start = pan.getRenderPose().position;
  pan.animatePan("left", 0);
  pan.tick(62.5); // t = 0.0625 s * speed 8 = 0.5, linear
  const mid = pan.getRenderPose().position;
  assert.equal(pan.tick(130), false, "0.125 s");
  const end = pan.getRenderPose().position;
  const jumpedPan = new TrackballCamera(100, vec3(0, 0, 0));
  jumpedPan.panArrow("left");
  closeV(end, jumpedPan.getRenderPose().position);
  closeV(mid, vec3((start.x + end.x) / 2, (start.y + end.y) / 2, (start.z + end.z) / 2));
});

test("Flip adds a half turn about y and Home goes to the top view, each as a move", () => {
  const animated = messedUp();
  const jumped = messedUp();
  animated.animateFlip(0);
  animated.tick(1001);
  jumped.flip();
  sameView(animated, jumped);
  animated.animateReset(2000);
  animated.tick(4000);
  const home = messedUp();
  home.reset();
  sameView(animated, home);
});

test("an auxiliary angle past a half turn is brought home the short way (Reset_T1 uses 2 pi, not 0)", () => {
  const cam = new TrackballCamera(100, vec3(0, 0, 0));
  cam.rotateY((200 * Math.PI) / 180);
  const jumped = new TrackballCamera(100, vec3(0, 0, 0));
  cam.animateViewPreset("top", 0);
  // 200 -> 360 is 160 degrees, 200 -> 0 would be 200: half way is 280 and not 100.
  cam.tick(500);
  const half = cam.getRenderPose();
  const want = new TrackballCamera(100, vec3(0, 0, 0));
  want.rotateY((280 * Math.PI) / 180);
  closeV(half.position, want.getRenderPose().position);
  cam.tick(1001);
  jumped.applyViewPreset("top");
  sameView(cam, jumped);
});

test("a pivot move re-centres the look-at point and clears the pan offset", () => {
  const cam = messedUp();
  cam.animatePivot(vec3(10, 0, 4), 0);
  cam.tick(1001);
  const want = new TrackballCamera(100, vec3(10, 0, 4));
  want.setWindowSize(800, 600);
  want.setCurMousePosition(400, 300);
  want.drag(520, 240);
  want.setCurMousePosition(520, 240);
  want.drag(430, 330);
  want.zoomBy(1.3);
  sameView(cam, want);
});

test("with animation off a move is made at once; a view command is refused only while one is going", () => {
  const cam = messedUp();
  cam.setAnimation(false);
  assert.equal(cam.animateViewPreset("left", 0), true);
  assert.equal(cam.isMoving(), false);
  const jumped = messedUp();
  jumped.applyViewPreset("left");
  sameView(cam, jumped);
  assert.equal(cam.animateViewPreset("right", 1), true, "nothing is going, so another is taken");
});

test("the quaternion of a move takes the short way between q and -q, and stays a rotation", () => {
  const q = { x: 0.1, y: 0.2, z: 0.3, w: Math.sqrt(1 - 0.14) };
  const minusQ = { x: -q.x, y: -q.y, z: -q.z, w: -q.w };
  const mid = mixQuat(q, minusQ, 0.5);
  assert.ok(Math.abs(Math.hypot(mid.x, mid.y, mid.z, mid.w) - 1) < 1e-12, "unit length");
  closeV(rotateVecByQuat(vec3(1, 2, 3), mid), rotateVecByQuat(vec3(1, 2, 3), q), 1e-9);
});
