import { test } from "node:test";
import assert from "node:assert/strict";
import {
  angleFieldText,
  angleFieldValue,
  fromDisplay,
  fromDisplayAbs,
  fromDisplayAngle,
  fromDisplayRel,
  IDENTITY_TRANSFORMS,
  pcbOriginTransforms,
  sanitizeDisplayOrigin,
  statusBarCoords,
  toDisplay,
  toDisplayAbs,
  toDisplayAngle,
  toDisplayRel,
  userOrigin,
  type CoordType,
} from "./originTransforms";

const aux = { x: 10_000_000, y: 20_000_000 };
const grid = { x: -5_000_000, y: 7_500_000 };

test("GetUserOrigin: the page's (0, 0), the drill/place file origin or the grid origin; an origin the board lacks is (0, 0)", () => {
  assert.deepEqual(userOrigin("page", aux, grid), { x: 0, y: 0 });
  assert.deepEqual(userOrigin("aux", aux, grid), aux);
  assert.deepEqual(userOrigin("grid", aux, grid), grid);
  assert.deepEqual(userOrigin("aux", null, grid), { x: 0, y: 0 });
  assert.deepEqual(userOrigin("grid", aux, undefined), { x: 0, y: 0 });
});

test("ToDisplayAbs: the value less the user origin, negated on an inverted axis; FromDisplayAbs undoes it", () => {
  const xf = pcbOriginTransforms("aux", false, false, aux, grid);
  assert.equal(toDisplay(xf, 12_000_000, "abs_x"), 2_000_000);
  assert.equal(toDisplay(xf, 15_000_000, "abs_y"), -5_000_000);
  const inv = pcbOriginTransforms("aux", true, true, aux, grid);
  assert.equal(toDisplay(inv, 12_000_000, "abs_x"), -2_000_000);
  assert.equal(toDisplay(inv, 15_000_000, "abs_y"), 5_000_000);
  for (const t of ["abs_x", "abs_y", "rel_x", "rel_y", "not"] as CoordType[]) {
    for (const x of [xf, inv, pcbOriginTransforms("grid", true, false, aux, grid)]) {
      for (const v of [0, 1, -3_000_000, 12_345_678]) assert.equal(fromDisplay(x, toDisplay(x, v, t), t), v, `${t} ${v}`);
    }
  }
  // the origin itself shows as 0, never -0
  assert.ok(Object.is(toDisplay(inv, aux.x, "abs_x"), 0));
  assert.deepEqual(toDisplayAbs(inv, { x: 11_000_000, y: 19_000_000 }), { x: -1_000_000, y: 1_000_000 });
  assert.deepEqual(fromDisplayAbs(inv, { x: -1_000_000, y: 1_000_000 }), { x: 11_000_000, y: 19_000_000 });
});

test("ToDisplayRel: only the axis direction, whatever the origin; NOT_A_COORD is left alone", () => {
  const xf = pcbOriginTransforms("grid", true, false, aux, grid);
  assert.equal(toDisplay(xf, 3_000_000, "rel_x"), -3_000_000);
  assert.equal(toDisplay(xf, 3_000_000, "rel_y"), 3_000_000);
  assert.equal(toDisplay(xf, 3_000_000, "not"), 3_000_000);
  assert.ok(Object.is(toDisplay(xf, 0, "rel_x"), 0), "`displayValue != 0`: no -0");
  assert.deepEqual(toDisplayRel(xf, { x: 1, y: 2 }), { x: -1, y: 2 });
  assert.deepEqual(fromDisplayRel(xf, { x: -1, y: 2 }), { x: 1, y: 2 });
  // the base class (the schematic's frames) and KiCad's defaults change nothing
  assert.equal(toDisplay(IDENTITY_TRANSFORMS, 42, "abs_y"), 42);
  assert.deepEqual(pcbOriginTransforms("page", false, false, aux, grid), IDENTITY_TRANSFORMS);
});

test("the EDA_ANGLE overloads: a relative angle is negated while Y is not inverted; an absolute one never", () => {
  const down = IDENTITY_TRANSFORMS;
  const up = pcbOriginTransforms("page", false, true, null, null);
  assert.equal(toDisplayAngle(down, 30, "rel_y"), -30);
  assert.equal(toDisplayAngle(down, 30, "rel_x"), -30);
  assert.equal(toDisplayAngle(down, 30, "abs_y"), 30);
  assert.equal(toDisplayAngle(up, 30, "rel_y"), 30);
  assert.equal(fromDisplayAngle(down, -30, "rel_y"), 30);
  assert.equal(fromDisplayAngle(up, 30, "rel_y"), 30);
});

test("UNIT_BINDER::SetAngleValue on a REL_Y field shows minus the angle whatever the Y axis, and GetAngleValue reads it back", () => {
  for (const invertY of [false, true]) {
    const xf = pcbOriginTransforms("aux", false, invertY, aux, grid);
    assert.equal(angleFieldText(xf, 30, "rel_y"), -30, `invertY ${invertY}`);
    assert.equal(angleFieldValue(xf, -30, "rel_y"), 30, `invertY ${invertY}`);
  }
});

test("UpdateStatusBar: X and Y from the user origin, dx and dy from the local origin turned by the axes, polar from the local origin untransformed", () => {
  const xf = pcbOriginTransforms("grid", true, true, aux, grid);
  const s = statusBarCoords(xf, { x: 0, y: 10_000_000 }, { x: 3_000_000, y: 6_000_000 }, false);
  // 0 - (-5) mm = 5 mm, X increasing left: -5 mm; 10 - 7.5 mm = 2.5 mm, Y increasing up: -2.5 mm
  assert.equal(s.x, -5_000_000);
  assert.equal(s.y, -2_500_000);
  assert.equal(s.dx, 3_000_000);
  assert.equal(s.dy, -4_000_000);
  assert.equal(s.dist, 5_000_000);
  assert.equal(s.polar, null);
  const p = statusBarCoords(xf, { x: 0, y: 10_000_000 }, { x: 3_000_000, y: 6_000_000 }, true);
  assert.equal(p.polar!.r, 5_000_000);
  // ( dx, dy ) = ( -3, 4 ) mm in board coordinates (Y down): atan2( -4, -3 ) = -126.87 degrees
  assert.ok(Math.abs(p.polar!.theta + 126.8698976) < 1e-6);
  assert.equal(p.x, s.x, "field 2 is the cursor's X and Y with polar coordinates too");
});

test("pcb_display.origin_mode: the name or the enum's number; anything else is the page", () => {
  assert.equal(sanitizeDisplayOrigin("grid"), "grid");
  assert.equal(sanitizeDisplayOrigin(1), "aux");
  assert.equal(sanitizeDisplayOrigin(2), "grid");
  assert.equal(sanitizeDisplayOrigin(3), "page");
  assert.equal(sanitizeDisplayOrigin("drill"), "page");
  assert.equal(sanitizeDisplayOrigin(undefined), "page");
});
