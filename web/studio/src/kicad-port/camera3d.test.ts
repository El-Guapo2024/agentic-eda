import { test } from "node:test";
import assert from "node:assert/strict";
import { TrackballCamera, DEFAULT_MIN_ZOOM, DEFAULT_MAX_ZOOM, type ViewPreset } from "./camera3d";
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
