import { test } from "node:test";
import assert from "node:assert/strict";
import { ARROW_SIZE, CAMERA_DISTANCE, SPHERES, SPHERE_RADIUS, VIEW_OF, gizmoNdc, gizmoSize, intersectDist, pickSphere, toScene } from "./navigator3d";
import { CONTEXT_MENU_3D, contextAction3D } from "./actions3d";

test("SPHERES_GIZMO: six spheres 1.6 out on the axes, 0.4 across, the camera 4.4 away; the positive ones lettered and bright", () => {
  assert.equal(ARROW_SIZE, 1.6);
  assert.equal(SPHERE_RADIUS, 0.4);
  assert.ok(Math.abs(CAMERA_DISTANCE - 4.4) < 1e-12);
  assert.deepEqual(SPHERES.map((s) => s.id), ["right", "left", "back", "front", "top", "bottom"]);
  assert.deepEqual(SPHERES.map((s) => s.label), ["X", "", "Y", "", "Z", ""]);
  assert.deepEqual(SPHERES[0]!.color, { x: 0.9, y: 0, z: 0 });
  assert.deepEqual(SPHERES[3]!.color, { x: 0, y: 0.4, z: 0 });
  // KiCad's +Z (top) is the scene's up, its +Y (back) the scene's -z.
  assert.deepEqual(toScene(SPHERES[4]!.position), { x: 0, y: 1.6, z: 0 });
  assert.deepEqual(toScene(SPHERES[2]!.position), { x: 0, y: 0, z: -1.6 });
  assert.deepEqual(Object.values(VIEW_OF), ["right", "left", "back", "front", "top", "bottom"]);
});

test("handleMouseInput: a square of a eighth of the height at (4, 4) from the bottom left, in its own -1 .. 1", () => {
  assert.equal(gizmoSize(800), 100);
  // A click at the square's centre: 4 + 50 from the left, 800 - 54 from the top.
  assert.deepEqual(gizmoNdc(54, 800 - 54, 800), { x: 0, y: 0 });
  assert.deepEqual(gizmoNdc(4, 796, 800), { x: -1, y: -1 });
  assert.equal(gizmoNdc(200, 700, 800), null, "outside the square");
  assert.equal(gizmoNdc(54, 54, 800), null, "the top-left corner is not it");
});

test("updateSelection: the ray hits the sphere nearest along it", () => {
  // Looking down from above at the origin (KiCad's frame: from +Z), a ray at the top sphere's centre hits it before the bottom one behind it.
  assert.equal(pickSphere({ x: 0, y: 0, z: 10 }, { x: 0, y: 0, z: -1 })!.id, "top");
  assert.equal(pickSphere({ x: 1.6, y: 0, z: 10 }, { x: 0, y: 0, z: -1 })!.id, "right");
  assert.equal(pickSphere({ x: 1, y: 1, z: 10 }, { x: 0, y: 0, z: -1 }), null);
  assert.equal(intersectDist({ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 5, y: 0.3, z: 0 }, 0.4), 5);
  assert.equal(intersectDist({ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 5, y: 0.5, z: 0 }, 0.4), -1);
});

test("EDA_3D_CONTROLLER::Init: the right-click menu and what each entry does", () => {
  const names = (items: typeof CONTEXT_MENU_3D): string[] => items.flatMap((i) => ("action" in i ? [i.action] : "submenu" in i ? [`[${i.submenu}]`, ...names(i.items)] : ["-"]));
  assert.deepEqual(names(CONTEXT_MENU_3D), [
    "common.Control.zoomInCenter",
    "common.Control.zoomOutCenter",
    "-",
    "3DViewer.Control.viewTop",
    "3DViewer.Control.viewBottom",
    "3DViewer.Control.viewRight",
    "3DViewer.Control.viewLeft",
    "3DViewer.Control.viewFront",
    "3DViewer.Control.viewBack",
    "-",
    "[Rotate Board]",
    "3DViewer.Control.rotateXclockwise",
    "3DViewer.Control.rotateXcounterclockwise",
    "-",
    "3DViewer.Control.rotateYclockwise",
    "3DViewer.Control.rotateYcounterclockwise",
    "-",
    "3DViewer.Control.rotateZclockwise",
    "3DViewer.Control.rotateZcounterclockwise",
    "3DViewer.Control.flipView",
    "[Move Board]",
    "3DViewer.Control.moveLeft",
    "3DViewer.Control.moveRight",
    "3DViewer.Control.moveUp",
    "3DViewer.Control.moveDown",
  ]);
  assert.deepEqual(contextAction3D("3DViewer.Control.viewBack"), { kind: "viewPreset", preset: "back" });
  assert.deepEqual(contextAction3D("3DViewer.Control.rotateYcounterclockwise"), { kind: "rotate", axis: "y", dir: "ccw" });
  assert.deepEqual(contextAction3D("3DViewer.Control.moveUp"), { kind: "pan", direction: "up" });
  assert.deepEqual(contextAction3D("common.Control.zoomOutCenter"), { kind: "zoomOut" });
  assert.deepEqual(contextAction3D("3DViewer.Control.flipView"), { kind: "flipSwitch" });
  assert.equal(contextAction3D("nope"), null);
});
