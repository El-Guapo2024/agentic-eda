import { test } from "node:test";
import assert from "node:assert/strict";
import { applyExtrusionTransform, bodyDrawn, bodyMaterial3d, bodyZRange, defaultBodyColor, kiRound, pinMaterial3d, pinZRange, PIN_PROTRUSION_MM, rotatePoint, type Polygon } from "./extrudedBody3d";
import { DEFAULT_STACK, footprintZ } from "./model3d";
import { glShininess } from "./material3d";

const MM = 1e6;
const square: Polygon[] = [[[[0, 0], [2 * MM, 0], [2 * MM, MM], [0, MM]]]];
const plain = { rotate: [0, 0, 0] as [number, number, number], scale: [1, 1, 1] as [number, number, number], offset: [0, 0, 0] as [number, number, number] };

test("KiROUND is half away from zero, and RotatePoint turns counter-clockwise on the screen with the quarter turns exact", () => {
  assert.deepEqual([kiRound(2.5), kiRound(-2.5), kiRound(2.4), kiRound(-2.6)], [3, -3, 2, -3]);
  // y down: +x turned by 90 goes to -y (`pt = VECTOR2I( *pY, -*pX )`)
  assert.deepEqual(rotatePoint([10, 0], [0, 0], 90), [0, -10]);
  assert.deepEqual(rotatePoint([10, 0], [0, 0], -90), [0, 10], "-90 is 270");
  assert.deepEqual(rotatePoint([10, 5], [10, 0], 180), [10, -5], "about the centre");
  assert.deepEqual(rotatePoint([10, 0], [0, 0], 450), [0, -10], "normalised");
  // any other angle rounded to the nanometre: 30 degrees of (1 mm, 0) is (866025.4, -500000)
  assert.deepEqual(rotatePoint([MM, 0], [0, 0], 30), [866025, -500000]);
  assert.deepEqual(rotatePoint([MM, 0], [0, 0], 45), [707107, -707107], "45 degrees uses sqrt(1/2), as EDA_ANGLE::Sin does");
});

test("ApplyExtrusionTransform turns, then scales about the footprint, then moves, in the board's frame", () => {
  assert.deepEqual(applyExtrusionTransform(square, plain, [0, 0]), square, "nothing to do");
  const turned = applyExtrusionTransform(square, { ...plain, rotate: [0, 0, 90] }, [0, 0]);
  assert.deepEqual(turned[0]![0], [[0, 0], [0, -2 * MM], [MM, -2 * MM], [MM, 0]]);
  const scaled = applyExtrusionTransform(square, { ...plain, scale: [2, 0.5, 3] }, [MM, 0]);
  assert.deepEqual(scaled[0]![0], [[-MM, 0], [3 * MM, 0], [3 * MM, MM / 2], [-MM, MM / 2]], "about the footprint's position, z not used");
  const moved = applyExtrusionTransform(square, { ...plain, offset: [0.5, -0.25, 9] }, [0, 0]);
  assert.deepEqual(moved[0]![0]![0], [MM / 2, -MM / 4], "the offset in millimetres, x and y");
  // the order: the offset is not turned (it is applied last)
  const all = applyExtrusionTransform([[[[MM, 0]]]], { rotate: [0, 0, 90], scale: [2, 2, 1], offset: [1, 0, 0] }, [0, 0]);
  assert.deepEqual(all[0]![0]![0], [MM, -2 * MM]);
  // holes go with their outline
  const withHole: Polygon[] = [[square[0]![0]!, [[MM / 4, MM / 4], [MM / 2, MM / 4], [MM / 2, MM / 2]]]];
  assert.equal(applyExtrusionTransform(withHole, { ...plain, offset: [1, 0, 0] }, [0, 0])[0]!.length, 2);
});

test("the body stands on the mounting surface past its standoff, up on the front and down on the back, its thickness scaled by z", () => {
  const body = { height: 2 * MM, standoff: MM / 2, scale: [1, 1, 2] as [number, number, number], offset: [0, 0, 0.1] as [number, number, number] };
  const top = footprintZ(DEFAULT_STACK, false);
  const [lo, hi] = bodyZRange(body, DEFAULT_STACK, false);
  assert.ok(Math.abs(lo - (top + 0.5 + 0.1)) < 1e-9 && Math.abs(hi - (lo + 1.5 * 2)) < 1e-9, `${lo} ${hi}`);
  const back = footprintZ(DEFAULT_STACK, true);
  const [blo, bhi] = bodyZRange(body, DEFAULT_STACK, true);
  assert.ok(Math.abs(bhi - (back - 0.5 - 0.1)) < 1e-9 && Math.abs(blo - (bhi - 3)) < 1e-9, `${blo} ${bhi}`);
});

test("the pins run from a millimetre past the far side to the standoff, and only when the body stands off the board", () => {
  assert.equal(pinZRange({ standoff: 0 }, DEFAULT_STACK, false), null);
  const [lo, hi] = pinZRange({ standoff: MM }, DEFAULT_STACK, false)!;
  assert.ok(Math.abs(lo - (footprintZ(DEFAULT_STACK, true) - PIN_PROTRUSION_MM)) < 1e-9 && Math.abs(hi - (footprintZ(DEFAULT_STACK, false) + 1)) < 1e-9);
  const [blo, bhi] = pinZRange({ standoff: MM }, DEFAULT_STACK, true)!;
  assert.ok(Math.abs(bhi - (footprintZ(DEFAULT_STACK, false) + 1)) < 1e-9 && Math.abs(blo - (footprintZ(DEFAULT_STACK, true) - 1)) < 1e-9);
});

test("the materials are GetMaterialProps' and the colours GetDefaultColor's", () => {
  assert.deepEqual(defaultBodyColor("plastic"), [0.2, 0.2, 0.2, 1]);
  assert.deepEqual(defaultBodyColor("copper"), [0.72, 0.45, 0.2, 1]);
  const plastic = bodyMaterial3d("plastic", [0.5, 0.5, 0.5, 1]);
  assert.deepEqual([plastic.ambient, plastic.specular, plastic.shininess, plastic.transparency], [{ r: 0.05, g: 0.05, b: 0.05 }, { r: 0.4, g: 0.4, b: 0.4 }, 0.3 * 128, 0]);
  const metal = bodyMaterial3d("metal", [0.2, 0.4, 0.6, 0.5]);
  assert.ok(Math.abs(metal.specular.b - (0.6 * 0.5 + 0.4)) < 1e-12 && Math.abs(metal.ambient.r - 0.03) < 1e-12 && metal.transparency === 0.5);
  assert.equal(bodyMaterial3d("matte", [1, 1, 1, 1]).shininess, 0.05 * 128);
  assert.equal(glShininess(bodyMaterial3d("matte", [1, 1, 1, 1])), 128, "OglSetMaterial makes 128 of it");
  const pin = pinMaterial3d({ r: 0.7, g: 0.6, b: 0 });
  assert.deepEqual([pin.diffuse, pin.specular, pin.shininess, pin.transparency], [{ r: 0.75, g: 0.75, b: 0.75 }, { r: 0.85, g: 0.85, b: 0.85 }, 0.6 * 128, 0]);
  assert.ok(Math.abs(pin.ambient.r - 0.07) < 1e-12, "the copper's ambient");
});

test("a body is drawn when it is shown and has an outline", () => {
  assert.equal(bodyDrawn({ show: true, outline: square }), true);
  assert.equal(bodyDrawn({ show: false, outline: square }), false);
  assert.equal(bodyDrawn({ show: true, outline: [] }), false);
});
