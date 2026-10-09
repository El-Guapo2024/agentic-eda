import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_STACK,
  SCENE_FROM_KICAD,
  VRML_UNIT_MM,
  flippedFrame,
  footprintZ,
  inferKind,
  kindShown,
  mat4Identity,
  mat4Mul,
  mat4RotationX,
  mat4RotationY,
  mat4RotationZ,
  mat4Translation,
  modelPlacementMatrix,
  modelUrl,
  modelWorldMatrix,
  modelWorldMatrixKicadFile,
  plainModel,
  sameModels,
  transformPoint,
  type Vec3Tuple,
} from "./model3d";

const EPS = 1e-9;
function close(got: readonly number[], want: readonly number[], eps = EPS) {
  assert.equal(got.length, want.length);
  for (let i = 0; i < want.length; i++) assert.ok(Math.abs(got[i]! - want[i]!) < eps, `[${got.join(", ")}] != [${want.join(", ")}] (index ${i})`);
}

test("matrix basics: multiplication applies the right operand first, rotations are right-handed", () => {
  close(transformPoint(mat4Identity(), [1, 2, 3]), [1, 2, 3]);
  close(transformPoint(mat4RotationZ(Math.PI / 2), [1, 0, 0]), [0, 1, 0]);
  close(transformPoint(mat4RotationX(Math.PI / 2), [0, 1, 0]), [0, 0, 1]);
  close(transformPoint(mat4RotationY(Math.PI / 2), [0, 0, 1]), [1, 0, 0]);
  const turnThenMove = mat4Mul(mat4Translation(5, 0, 0), mat4RotationZ(Math.PI / 2));
  close(transformPoint(turnThenMove, [1, 0, 0]), [5, 1, 0]);
  const moveThenTurn = mat4Mul(mat4RotationZ(Math.PI / 2), mat4Translation(5, 0, 0));
  close(transformPoint(moveThenTurn, [1, 0, 0]), [0, 6, 0]);
});

test("the scene's frame is KiCad's turned a quarter about x: (x, y up, z up) -> (x, z, -y)", () => {
  close(transformPoint([...SCENE_FROM_KICAD], [1, 2, 3]), [1, 3, -2]);
  // A proper rotation: no mirror, so the handedness the scene was built in is kept.
  const [m0, m1, m2, , m4, m5, m6, , m8, m9, m10] = SCENE_FROM_KICAD;
  const det = m0! * (m5! * m10! - m6! * m9!) - m4! * (m1! * m10! - m2! * m9!) + m8! * (m1! * m6! - m2! * m5!);
  assert.ok(Math.abs(det - 1) < EPS);
});

test("a model's own placement is offset * Rz(-z) * Ry(-y) * Rx(-x) * scale, scale first (render_3d_opengl.cpp)", () => {
  close(transformPoint(modelPlacementMatrix(plainModel("a.step")), [1, 2, 3]), [1, 2, 3]);
  close(transformPoint(modelPlacementMatrix({ offset: [1, 2, 0.5], scale: [1, 1, 1], rotate: [0, 0, 0] }), [0, 0, 0]), [1, 2, 0.5]);
  close(transformPoint(modelPlacementMatrix({ offset: [0, 0, 0], scale: [2, 3, 4], rotate: [0, 0, 0] }), [1, 1, 1]), [2, 3, 4]);
  // `(rotate (xyz 0 0 90))` turns the model clockwise (Rz(-90)): +x goes to -y.
  close(transformPoint(modelPlacementMatrix({ offset: [0, 0, 0], scale: [1, 1, 1], rotate: [0, 0, 90] }), [1, 0, 0]), [0, -1, 0]);
  close(transformPoint(modelPlacementMatrix({ offset: [0, 0, 0], scale: [1, 1, 1], rotate: [90, 0, 0] }), [0, 1, 0]), [0, 0, -1]);
  close(transformPoint(modelPlacementMatrix({ offset: [0, 0, 0], scale: [1, 1, 1], rotate: [0, 90, 0] }), [0, 0, 1]), [-1, 0, 0]);
  // Scale, then rotate, then move: a vertex (1, 0, 0) of a model scaled 2 in x, turned 90 about z, lifted by (1, 0, 0) is at (1, -2, 0).
  close(transformPoint(modelPlacementMatrix({ offset: [1, 0, 0], scale: [2, 1, 1], rotate: [0, 0, 90] }), [1, 0, 0]), [1, -2, 0]);
  // The three rotations apply x first, then y, then z (the matrix is Rz * Ry * Rx).
  const xyz = transformPoint(modelPlacementMatrix({ offset: [0, 0, 0], scale: [1, 1, 1], rotate: [90, 90, 90] }), [0, 1, 0]);
  const byHand = transformPoint(mat4Mul(mat4RotationZ(-Math.PI / 2), mat4Mul(mat4RotationY(-Math.PI / 2), mat4RotationX(-Math.PI / 2))), [0, 1, 0]);
  close(xyz, byHand);
});

test("a footprint sits on the copper: +-(thickness / 2 + copper), from the stack-up", () => {
  assert.ok(Math.abs(footprintZ(DEFAULT_STACK, false) - 0.835) < EPS);
  assert.ok(Math.abs(footprintZ(DEFAULT_STACK, true) + 0.835) < EPS);
  assert.ok(Math.abs(footprintZ({ boardThicknessMm: 1, copperMm: 0.07 }, false) - 0.57) < EPS);
});

const at = (xMm: number, yMm: number, rotDeg = 0, bottom = false) => ({ xMm, yMm, rotDeg, bottom });
const plain = { offset: [0, 0, 0] as Vec3Tuple, scale: [1, 1, 1] as Vec3Tuple, rotate: [0, 0, 0] as Vec3Tuple };

test("a top footprint: the model's origin is on the footprint, up on the copper; offset lifts and moves it (board y is down, the model's y is up)", () => {
  close(transformPoint(modelWorldMatrix(at(10, 20), plain), [0, 0, 0]), [10, 0.835, 20]);
  // A vertex 1 mm up the model's y is 1 mm up the board (toward smaller board y): world z 19.
  close(transformPoint(modelWorldMatrix(at(10, 20), plain), [0, 1, 0]), [10, 0.835, 19]);
  close(transformPoint(modelWorldMatrix(at(10, 20), { ...plain, offset: [1, 2, 0.5] }), [0, 0, 0]), [11, 1.335, 18]);
});

test("the footprint's rotation is the studio's: clockwise on screen, the matrix to_board rotates pads by", () => {
  // to_board: (x, y) -> (x cos - y sin, x sin + y cos), board y down. 90 degrees takes +x to +y (down the screen).
  close(transformPoint(modelWorldMatrix(at(10, 20, 90), plain), [1, 0, 0]), [10, 0.835, 21]);
  close(transformPoint(modelWorldMatrix(at(10, 20, 90), plain), [0, 1, 0]), [11, 0.835, 20], 1e-9);
  // Any angle: the model's footprint-frame point (x, -y) lands where to_board puts the pad at (x, y).
  for (const deg of [0, 30, 45, 90, 135, 180, 270, 359]) {
    const rad = (deg * Math.PI) / 180;
    const [px, py] = [3, -2];
    const want = [10 + (px * Math.cos(rad) - py * Math.sin(rad)), 20 + (px * Math.sin(rad) + py * Math.cos(rad))];
    const got = transformPoint(modelWorldMatrix(at(10, 20, deg), plain), [px, -py, 0]);
    close([got[0], got[2]], want, 1e-9);
  }
});

test("a bottom footprint: the model is turned over, mirrored in x like the pads, and sits under the copper", () => {
  close(transformPoint(modelWorldMatrix(at(10, 20, 0, true), plain), [0, 0, 0]), [10, -0.835, 20]);
  // A vertex 1 mm to the right and 0.5 mm up is 1 mm to the LEFT on the board and 0.5 mm further below.
  close(transformPoint(modelWorldMatrix(at(10, 20, 0, true), plain), [1, 0, 0.5]), [9, -1.335, 20]);
  // to_board mirrors x of a bottom pad and then rotates: the model follows it at every angle.
  for (const deg of [0, 30, 90, 180, 270]) {
    const rad = (deg * Math.PI) / 180;
    const [px, py] = [3, -2];
    const [mx, my] = [-px, py];
    const want = [10 + (mx * Math.cos(rad) - my * Math.sin(rad)), 20 + (mx * Math.sin(rad) + my * Math.cos(rad))];
    const got = transformPoint(modelWorldMatrix(at(10, 20, deg, true), plain), [px, -py, 0]);
    close([got[0], got[2]], want, 1e-9);
  }
});

test("the studio's chain and KiCad's chain for the board file agree once a bottom footprint's model carries the half turn the writer gives it", () => {
  const models = [
    plain,
    { offset: [1, 2, 0.5] as Vec3Tuple, scale: [1, 1, 1] as Vec3Tuple, rotate: [0, 0, 0] as Vec3Tuple },
    { offset: [-1.5, 0.25, 3] as Vec3Tuple, scale: [0.5, 1, 2] as Vec3Tuple, rotate: [90, 0, 30] as Vec3Tuple },
    { offset: [0, 0, 0] as Vec3Tuple, scale: [1, 1, 1] as Vec3Tuple, rotate: [10, 20, 30] as Vec3Tuple },
  ];
  for (const model of models) {
    for (const rot of [0, 45, 90, 200]) {
      // Top: KiCad's file stores the footprint's angle as the negative of the studio's, the model as it is.
      const top = modelWorldMatrix(at(7, -4, rot), model);
      const topFile = modelWorldMatrixKicadFile({ xMm: 7, yMm: -4, orientDeg: -rot, flipped: false }, model);
      close(top, topFile, 1e-9);
      // Bottom: the file's model is the studio's with the half turn (Model3d::flipped_frame), and KiCad turns it by Ry(pi) Rz(pi).
      const bottom = modelWorldMatrix(at(7, -4, rot, true), model);
      const bottomFile = modelWorldMatrixKicadFile({ xMm: 7, yMm: -4, orientDeg: -rot, flipped: true }, { ...model, ...flippedFrame(model) });
      close(bottom, bottomFile, 1e-9);
    }
  }
});

test("the half turn is its own inverse and matches the board writer's numbers", () => {
  const m = { offset: [1, 2, 3] as Vec3Tuple, rotate: [10, 20, 30] as Vec3Tuple };
  assert.deepEqual(flippedFrame(m), { offset: [-1, -2, 3], rotate: [10, 20, -150] });
  assert.deepEqual(flippedFrame(flippedFrame(m)), m);
  assert.deepEqual(flippedFrame({ offset: [0, 0, 0], rotate: [0, 0, 0] }), { offset: [-0, -0, 0], rotate: [0, 0, 180] }, "0 - 180 is 180, as the Rust side has it");
  assert.deepEqual(flippedFrame(flippedFrame({ offset: [0, 0, 0], rotate: [0, 0, 0] })).rotate, [0, 0, 0]);
});

test("a model in VRML units: 0.1 inch", () => {
  assert.equal(VRML_UNIT_MM, 2.54);
  // KiCad's R_0603 model is +-0.31496 VRML units across: 1.6 mm.
  assert.ok(Math.abs(0.31496063 * 2 * VRML_UNIT_MM - 1.6) < 1e-6);
});

test("the through-hole, SMD and virtual rows gate a model by its kind", () => {
  const all = { tht: true, smd: true, virtual: true };
  assert.ok(kindShown("smd", all) && kindShown("tht", all) && kindShown("virtual", all));
  assert.equal(kindShown("smd", { ...all, smd: false }), false);
  assert.equal(kindShown("tht", { ...all, smd: false }), true);
  assert.equal(kindShown("virtual", { ...all, virtual: false }), false);
  assert.equal(inferKind([{ th: false }, { th: true }]), "tht");
  assert.equal(inferKind([{ th: false }]), "smd");
  assert.equal(inferKind(undefined), "smd");
});

test("a model's URL is its name percent-encoded: the server decodes it back to the path the footprint wrote", () => {
  assert.equal(modelUrl("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R 0603,1.step"), "/api/3dmodel?name=%24%7BKICAD10_3DMODEL_DIR%7D%2FResistor_SMD.3dshapes%2FR%200603%2C1.step");
  assert.equal(decodeURIComponent(modelUrl("a/b+c.wrl").split("=")[1]!), "a/b+c.wrl");
});

test("two model lists are the same when every model places and shows alike", () => {
  const a = [plainModel("x.step")];
  assert.ok(sameModels(a, [plainModel("x.step")]));
  assert.ok(sameModels(undefined, undefined));
  assert.equal(sameModels(a, undefined), false);
  assert.equal(sameModels(a, [{ ...plainModel("x.step"), rotate: [0, 0, 90] }]), false);
  assert.equal(sameModels(a, [{ ...plainModel("x.step"), opacity: 0.5 }]), false);
  assert.equal(sameModels(a, [plainModel("x.step"), plainModel("y.step")]), false);
});
