import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdShape, PadCustom } from "../api/types";
import {
  NO_CORNERS,
  PAD_SHAPE_CHOICES,
  PRIMITIVE_KINDS,
  checkPrimitive,
  checkShape,
  choiceOf,
  chooseShape,
  customPadBox,
  customPadExtent,
  defaultIpcRoundingRatio,
  describePrimitive,
  enabledCorners,
  hasSizeY,
  maxChamferRatio,
  maxCornerRatio,
  newPrimitive,
  polygonWith,
  primitiveFields,
  primitivesWith,
  radiusOf,
  setChamferRatio,
  setCornerRatio,
  setFilled,
  setPrimitiveField,
  shapeEdit,
  shapeFormOf,
  shapeNameOf,
  shapePatch,
  sizeOf,
  trapezoidCorners,
  trapezoidDelta,
  type PadShapeSource,
} from "./padShape";

const pad = (over: Partial<PadShapeSource> = {}): PadShapeSource => ({ shape: "rect", size: [1000, 600], w: 1000, h: 600, ...over });
const fmt = (nm: number) => `${(nm / 1e6).toFixed(2)} mm`;

test("the selector has KiCad's nine choices in its order and its words", () => {
  assert.deepEqual(
    PAD_SHAPE_CHOICES.map((c) => c.label),
    ["Circular", "Oval", "Rectangular", "Trapezoidal", "Rounded rectangle", "Chamfered rectangle", "Chamfered with other corners rounded", "Custom (circular base)", "Custom (rectangular base)"]
  );
  assert.deepEqual(PAD_SHAPE_CHOICES.map((c) => shapeNameOf(c.value)), ["circle", "oval", "rect", "trapezoid", "round_rect", "chamfered_rect", "chamfered_rect", "custom", "custom"]);
});

test("a pad opens on the choice that shows its shape: a chamfered rectangle with a corner ratio is the mixed one, a custom pad has its base", () => {
  assert.equal(choiceOf(pad()), "rect");
  assert.equal(choiceOf(pad({ shape: undefined, round: true, w: 800, h: 800 })), "circle", "an older backend says round and square");
  assert.equal(choiceOf(pad({ shape: undefined, round: true, w: 800, h: 400 })), "oval");
  assert.equal(choiceOf(pad({ shape: "chamfered_rect", ratio: 0 })), "chamfered_rect");
  assert.equal(choiceOf(pad({ shape: "chamfered_rect", ratio: 0.25 })), "chamfered_round_rect");
  const custom = (anchor: PadCustom["anchor"]): PadCustom => ({ anchor, zone: "outline", primitives: [] });
  assert.equal(choiceOf(pad({ shape: "custom", custom: custom("rect") })), "custom_rect");
  assert.equal(choiceOf(pad({ shape: "custom", custom: custom("circle") })), "custom_circle");
  assert.equal(choiceOf(pad({ shape: "trapezoid" })), "trapezoid");
});

test("the form reads a trapezoid's axis from the delta that is set and a custom pad's knockout and shapes", () => {
  assert.deepEqual([shapeFormOf(pad({ shape: "trapezoid", delta: [400, 0] })).trapAxis, shapeFormOf(pad({ shape: "trapezoid", delta: [400, 0] })).trapDelta], ["horizontal", 400]);
  assert.deepEqual([shapeFormOf(pad({ shape: "trapezoid", delta: [0, 300] })).trapAxis, shapeFormOf(pad({ shape: "trapezoid", delta: [0, 300] })).trapDelta], ["vertical", 300]);
  assert.equal(shapeFormOf(pad({ shape: "trapezoid", delta: null })).trapAxis, "horizontal", "no delta: the first axis, a delta of nothing");
  const form = shapeFormOf(pad({ shape: "custom", custom: { anchor: "rect", zone: "convex_hull", primitives: [newPrimitive("segment", 1000, 600)] } }));
  assert.equal(form.knockout, "convex_hull");
  assert.equal(form.primitives.length, 1);
  assert.deepEqual(shapeFormOf(pad({ shape: "chamfered_rect", chamfer_ratio: 0.3, chamfer_corners: { ...NO_CORNERS, top_left: true } })).corners, { ...NO_CORNERS, top_left: true });
});

test("choosing a chamfered rectangle gives a 20% chamfer and one corner, and the rounded one the IPC corner ratio too (onPadShapeSelection)", () => {
  const form = { ...shapeFormOf(pad({ ratio: 0 })), chamferRatio: 0 };
  const chamfered = chooseShape(form, "chamfered_rect", 90).form;
  assert.equal(chamfered.chamferRatio, 0.2);
  assert.deepEqual(chamfered.corners, { ...NO_CORNERS, top_left: true }, "usual for some SMD pads");
  // A pad that already has corners and a ratio keeps them.
  const kept = chooseShape({ ...form, chamferRatio: 0.35, corners: { ...NO_CORNERS, bottom_right: true } }, "chamfered_rect", 90).form;
  assert.deepEqual([kept.chamferRatio, kept.corners], [0.35, { ...NO_CORNERS, bottom_right: true }]);
  const mixed = chooseShape({ ...form, ratio: 0 }, "chamfered_round_rect", 90).form;
  assert.deepEqual([mixed.chamferRatio, mixed.ratio], [0.2, 0.25], "the IPC ratio: 25%, the radius capped at 0.25 mm");
  // The IPC cap: a 2 mm pad's corner radius is 0.25 mm, 12.5%.
  assert.equal(defaultIpcRoundingRatio([4_000_000, 2_000_000]), 0.125);
  assert.equal(defaultIpcRoundingRatio([800_000, 600_000]), 0.25);
  assert.equal(chooseShape({ ...form, ratio: 0 }, "round_rect", 90).form.ratio, 0.25);
});

test("the spoke angle follows a circle: 45 degrees on the way in, back to 90 on the way out, and anything else is left alone", () => {
  const rect = shapeFormOf(pad());
  assert.equal(chooseShape(rect, "circle", 90).spokeAngleDeg, 45);
  assert.equal(chooseShape(rect, "circle", 30).spokeAngleDeg, 30, "an angle the user set stays");
  const circle = shapeFormOf(pad({ shape: "circle", size: [800, 800], w: 800, h: 800 }));
  assert.equal(chooseShape(circle, "rect", 45).spokeAngleDeg, 90);
  assert.equal(chooseShape(circle, "rect", 20).spokeAngleDeg, 20);
  assert.equal(chooseShape(rect, "oval", 90).spokeAngleDeg, 90);
});

test("a circle and a circular base have one size and the others two", () => {
  assert.deepEqual(["circle", "custom_circle"].map((c) => hasSizeY(c as never)), [false, false]);
  assert.deepEqual(["oval", "rect", "trapezoid", "round_rect", "chamfered_rect", "custom_rect"].map((c) => hasSizeY(c as never)), [true, true, true, true, true, true]);
  assert.deepEqual(sizeOf({ ...shapeFormOf(pad()), choice: "circle" }), [1000, 1000]);
  assert.deepEqual(sizeOf(shapeFormOf(pad())), [1000, 600]);
});

test("the chamfer's limit is what the rounded corners leave, and half the side when two neighbouring corners are cut (getMaxChamferRatio)", () => {
  const base = { ...shapeFormOf(pad({ shape: "chamfered_rect", ratio: 0 })), choice: "chamfered_rect" as const };
  assert.equal(maxChamferRatio(base), 1, "no corner or one: the whole side");
  assert.equal(maxChamferRatio({ ...base, corners: { ...NO_CORNERS, top_left: true } }), 1);
  assert.equal(maxChamferRatio({ ...base, corners: { ...NO_CORNERS, top_left: true, top_right: true } }), 1, "the C++ takes the larger of the baseline and 50%");
  const mixed = { ...base, choice: "chamfered_round_rect" as const, ratio: 0.4 };
  assert.ok(Math.abs(maxChamferRatio(mixed) - 0.6) < 1e-12, "1 - the corner ratio");
  assert.equal(maxChamferRatio({ ...mixed, ratio: 0.8, corners: { ...NO_CORNERS, top_left: true, top_right: true } }), 0.5, "two neighbours: at least 50%");
  assert.equal(maxCornerRatio({ ...base, chamferRatio: 0.3 }), 0.7, "the side less the chamfer");
  assert.equal(maxCornerRatio({ ...base, choice: "round_rect" }), 0.5);
});

test("a chamfer over half the side leaves the neighbours of a cut corner uncut (updateAllowedPadChamferCorners)", () => {
  const form = { ...shapeFormOf(pad()), choice: "chamfered_rect" as const, corners: { ...NO_CORNERS, top_left: true }, chamferRatio: 0.3 };
  assert.deepEqual(Object.values(enabledCorners(form)), [true, true, true, true]);
  const big = { ...form, chamferRatio: 0.6 };
  assert.deepEqual(enabledCorners(big), { top_left: true, top_right: false, bottom_right: true, bottom_left: false }, "the neighbours of the top left are the top right and the bottom left");
  // Typing a size past half clears the checkboxes one by one in the dialog's order: the top left goes (its neighbour the top right is cut), which frees the top right.
  const clamped = setChamferRatio({ ...form, corners: { ...NO_CORNERS, top_left: true, top_right: true } }, 0.7);
  assert.equal(clamped.chamferRatio, 0.7, "the limit with no rounded corners is the whole side");
  assert.deepEqual(clamped.corners, { ...NO_CORNERS, top_right: true });
  assert.equal(setChamferRatio(form, -5).chamferRatio, 0);
  assert.equal(setChamferRatio(form, 3).chamferRatio, 1, "cut to the limit");
  const rounded = { ...form, choice: "chamfered_round_rect" as const, ratio: 0.3 };
  assert.equal(setChamferRatio(rounded, 0.9).chamferRatio, 0.7, "what the rounded corners leave");
  assert.ok(Math.abs(setCornerRatio({ ...form, chamferRatio: 0.7 }, 0.5).ratio - 0.3) < 1e-12, "the corner size is limited to what the chamfer leaves");
  assert.equal(setCornerRatio({ ...form, choice: "round_rect" }, 0.9).ratio, 0.5);
});

test("a trapezoid's delta is on one axis and never more than the pad is wide there (transferDataToPad)", () => {
  const trap = { ...shapeFormOf(pad({ shape: "trapezoid", size: [2000, 1000], w: 2000, h: 1000 })), choice: "trapezoid" as const };
  assert.deepEqual(trapezoidDelta({ ...trap, trapAxis: "horizontal", trapDelta: 400 }), { delta: [400, 0], clamped: false });
  assert.deepEqual(trapezoidDelta({ ...trap, trapAxis: "vertical", trapDelta: -500 }), { delta: [0, -500], clamped: false });
  // delta.x past the height (1000) and delta.y past the width (2000): cut to the size less 2 um.
  assert.deepEqual(trapezoidDelta({ ...trap, trapAxis: "horizontal", trapDelta: 1200 }), { delta: [998, 0], clamped: true });
  assert.deepEqual(trapezoidDelta({ ...trap, trapAxis: "horizontal", trapDelta: -1200 }), { delta: [-998, 0], clamped: true });
  assert.deepEqual(trapezoidDelta({ ...trap, trapAxis: "vertical", trapDelta: 2500 }), { delta: [0, 1998], clamped: true });
  assert.equal(checkShape({ ...trap, trapAxis: "horizontal", trapDelta: 1200 }), "Error: (trapezoid delta is too large)");
  assert.equal(checkShape({ ...trap, trapAxis: "horizontal", trapDelta: 400 }), null);
});

test("OK sends the keys the shape needs: a rounded rectangle's ratio, a chamfered one's ratio and corners with no rounding, a trapezoid's delta, a custom pad's whole shape", () => {
  const form = shapeFormOf(pad());
  assert.deepEqual(shapeEdit({ ...form, choice: "rect" }), { shape: "rect", size: [1000, 600] });
  assert.deepEqual(shapeEdit({ ...form, choice: "circle" }), { shape: "circle", size: [1000, 1000] });
  assert.deepEqual(shapeEdit({ ...form, choice: "round_rect", ratio: 0.3 }), { shape: "round_rect", size: [1000, 600], roundrect_ratio: 0.3 });
  assert.deepEqual(shapeEdit({ ...form, choice: "chamfered_rect", chamferRatio: 0.25, corners: { ...NO_CORNERS, top_left: true, bottom_right: true } }), {
    shape: "chamfered_rect",
    size: [1000, 600],
    roundrect_ratio: 0,
    chamfer_ratio: 0.25,
    chamfer_corners: { ...NO_CORNERS, top_left: true, bottom_right: true },
  });
  assert.equal(shapeEdit({ ...form, choice: "chamfered_round_rect", ratio: 0.2, chamferRatio: 0.25 }).roundrect_ratio, 0.2);
  assert.deepEqual(shapeEdit({ ...form, choice: "trapezoid", trapAxis: "vertical", trapDelta: 300 }).trapezoid_delta, [0, 300]);
  const prim = newPrimitive("segment", 1000, 600);
  assert.deepEqual(shapeEdit({ ...form, choice: "custom_rect", knockout: "convex_hull", primitives: [prim] }).custom, { anchor: "rect", zone: "convex_hull", primitives: [prim] });
  assert.equal(shapeEdit({ ...form, choice: "custom_circle" }).custom?.anchor, "circle");
});

test("the patch is only what the form changed: nothing for a pad left as it is, the whole new shape for one that was turned into another", () => {
  const was = pad({ shape: "round_rect", ratio: 0.25 });
  assert.deepEqual(shapePatch(shapeFormOf(was), was), {});
  // Only the ratio moved.
  assert.deepEqual(shapePatch({ ...shapeFormOf(was), ratio: 0.4 }, was), { roundrect_ratio: 0.4 });
  // A rectangle made a trapezoid: the shape and its delta.
  const rect = pad();
  const trap = { ...shapeFormOf(rect), choice: "trapezoid" as const, trapDelta: 300 };
  assert.deepEqual(shapePatch(trap, rect), { shape: "trapezoid", trapezoid_delta: [300, 0] });
  // A rectangle made a chamfered one: the shape, the explicit no-rounding, the ratio and the corners.
  const chamfered = chooseShape(shapeFormOf(rect), "chamfered_rect", 90).form;
  assert.deepEqual(shapePatch(chamfered, rect), { shape: "chamfered_rect", roundrect_ratio: 0, chamfer_ratio: 0.2, chamfer_corners: { ...NO_CORNERS, top_left: true } });
  // A custom pad whose one basic shape is edited sends the custom shape whole.
  const custom: PadCustom = { anchor: "circle", zone: "outline", primitives: [newPrimitive("segment", 1000, 600)] };
  const was2 = pad({ shape: "custom", custom });
  const form2 = shapeFormOf(was2);
  assert.deepEqual(shapePatch(form2, was2), {});
  const edited = { ...form2, primitives: primitivesWith(form2.primitives, { replace: [0, { shape: setPrimitiveField(form2.primitives[0]!.shape, "end.x", 5000) }] }) };
  assert.equal(shapePatch(edited, was2).custom?.primitives.length, 1);
  assert.equal((shapePatch(edited, was2).custom?.primitives[0]?.shape as { end: { x: number } }).end.x, 5000);
  assert.deepEqual(Object.keys(shapePatch({ ...form2, knockout: "convex_hull" }, was2)), ["custom"]);
});

test("a new basic shape is drawn over the pad and is a shape that can be drawn", () => {
  for (const { value } of PRIMITIVE_KINDS) {
    const prim = newPrimitive(value, 1000, 600);
    assert.equal(prim.shape.kind, value);
    assert.equal(checkPrimitive(prim), null, value);
    assert.ok(primitiveFields(prim.shape).length >= 1);
  }
  assert.equal((newPrimitive("circle", 1000, 600).shape as { filled: boolean }).filled, true);
  assert.equal(newPrimitive("segment", 1000, 600).shape.filled, false);
  assert.deepEqual(PRIMITIVE_KINDS.map((k) => k.label), ["Line", "Arc", "Circle", "Rectangle", "Polygon", "Bezier"]);
});

test("the numbers a basic shape is edited by are the dialog's: start, midpoint, end, center, radius, control points and the line width", () => {
  const labels = (kind: CmdShape["kind"]) => primitiveFields(newPrimitive(kind, 1000, 600).shape).map((f) => f.label);
  assert.deepEqual(labels("segment"), ["Start X", "Start Y", "End X", "End Y", "Line width"]);
  assert.deepEqual(labels("arc"), ["Start X", "Start Y", "Midpoint X", "Midpoint Y", "End X", "End Y", "Line width"]);
  assert.deepEqual(labels("circle"), ["Center X", "Center Y", "Radius", "Line width"]);
  assert.deepEqual(labels("bezier").slice(4, 6), ["Control point 2 X", "Control point 2 Y"]);
  assert.deepEqual(labels("polygon"), ["Line width"], "a polygon's points are edited one by one");
});

test("setting a number moves that point only, the radius moves the circle's end along x, the width cannot go below nothing", () => {
  const seg = newPrimitive("segment", 1000, 600).shape;
  const moved = setPrimitiveField(seg, "end.y", 123.6) as { start: { x: number; y: number }; end: { x: number; y: number } };
  assert.deepEqual(moved.end, { x: (seg as { end: { x: number } }).end.x, y: 124 }, "whole micrometres");
  assert.deepEqual(moved.start, { x: 0, y: 0 }, "the other point is as it was");
  assert.equal((seg as { end: { y: number } }).end.y, 0, "the shape it came from is not touched");
  const circle = newPrimitive("circle", 1000, 600).shape as Extract<CmdShape, { kind: "circle" }>;
  const bigger = setPrimitiveField(circle, "radius", 900) as Extract<CmdShape, { kind: "circle" }>;
  assert.equal(radiusOf(bigger), 900);
  assert.deepEqual(bigger.center, circle.center);
  assert.equal(setPrimitiveField(seg, "stroke_width", -50).stroke_width, 0);
  assert.equal(setFilled(seg, true).filled, false, "a line is never filled");
  assert.equal(setFilled(circle, false).filled, false);
  assert.equal(setFilled({ ...circle, filled: false }, true).filled, true);
});

test("a polygon keeps at least three points and says what is wrong with a shape that cannot be drawn, in the verb's words", () => {
  const poly = newPrimitive("polygon", 1000, 600).shape as Extract<CmdShape, { kind: "polygon" }>;
  assert.equal(poly.pts.length, 3);
  assert.equal((polygonWith(poly, { add: { x: 1, y: 2 } }) as typeof poly).pts.length, 4);
  assert.equal((polygonWith(poly, { remove: 0 }) as typeof poly).pts.length, 3, "three is the least");
  assert.deepEqual((polygonWith(poly, { set: [1, { x: 9, y: 9 }] }) as typeof poly).pts[1], { x: 9, y: 9 });
  assert.equal(checkPrimitive({ shape: { ...poly, pts: poly.pts.slice(0, 2) } }), "a polygon needs at least three points");
  assert.equal(checkPrimitive({ shape: { ...(newPrimitive("circle", 1000, 600).shape as CmdShape & { kind: "circle" }), end: { x: 0, y: 0 } } }), "a circle needs a radius");
  assert.equal(checkPrimitive({ shape: { ...(newPrimitive("segment", 1000, 600).shape as CmdShape & { kind: "segment" }), end: { x: 0, y: 0 } } }), "a line needs a length");
  const arc = newPrimitive("arc", 1000, 600).shape as Extract<CmdShape, { kind: "arc" }>;
  assert.equal(checkPrimitive({ shape: { ...arc, mid: arc.start } }), "an arc needs three different points");
  const form = { ...shapeFormOf(pad()), choice: "custom_circle" as const, primitives: [newPrimitive("segment", 1000, 600), { shape: { ...poly, pts: [] } }] };
  assert.equal(checkShape(form), "Error: Basic shape 2: a polygon needs at least three points");
});

test("the list of basic shapes describes each one and adds, replaces, duplicates and deletes (the list is replaced whole)", () => {
  const a = newPrimitive("segment", 1_000_000, 600_000);
  const b = newPrimitive("circle", 1_000_000, 600_000);
  assert.equal(describePrimitive(a, fmt), "Line (0.00 mm, 0.00 mm) to (0.60 mm, 0.00 mm), width 0.20 mm");
  assert.equal(describePrimitive(b, fmt), "Circle at (0.00 mm, 0.00 mm), radius 0.30 mm, width 0.20 mm, filled");
  assert.match(describePrimitive({ ...a, proxy: true }, fmt), /\(not copper\)$/);
  let list = primitivesWith([], { add: a });
  list = primitivesWith(list, { add: b });
  assert.deepEqual(list.map((p) => p.shape.kind), ["segment", "circle"]);
  list = primitivesWith(list, { duplicate: 0 });
  assert.deepEqual(list.map((p) => p.shape.kind), ["segment", "segment", "circle"]);
  assert.notEqual(list[0]!.shape, list[1]!.shape, "a copy, not the same shape twice");
  list = primitivesWith(list, { replace: [1, newPrimitive("arc", 1_000_000, 600_000)] });
  assert.deepEqual(list.map((p) => p.shape.kind), ["segment", "arc", "circle"]);
  list = primitivesWith(list, { remove: 0 });
  assert.deepEqual(list.map((p) => p.shape.kind), ["arc", "circle"]);
});

test("a chamfer is checked in the dialog's words: too large for the corners cut, or negative", () => {
  const form = { ...shapeFormOf(pad()), choice: "chamfered_rect" as const, corners: { ...NO_CORNERS, top_left: true, top_right: true } };
  assert.equal(checkShape({ ...form, chamferRatio: 0.4 }), null);
  assert.equal(checkShape({ ...form, chamferRatio: -0.1 }), "Error: (negative corner chamfer is not allowed)");
  const mixed = { ...form, choice: "chamfered_round_rect" as const, ratio: 0.5, corners: { ...NO_CORNERS, top_left: true }, chamferRatio: 0.8 };
  assert.equal(checkShape(mixed), "Error: (corner chamfer is too large)", "the rounded corners leave 50% of the side");
});

test("a custom pad reaches as far as its anchor and its copper shapes, each as wide as its stroke, and not as far as a proxy item", () => {
  const prim = (shape: CmdShape, proxy = false) => ({ shape, proxy });
  const seg = (a: [number, number], b: [number, number], w: number): CmdShape => ({ kind: "segment", layer: "", stroke_width: w, filled: false, start: { x: a[0], y: a[1] }, end: { x: b[0], y: b[1] } });
  const custom: PadCustom = { anchor: "circle", zone: "outline", primitives: [prim(seg([0, 0], [2000, 0], 200)), prim(seg([0, 0], [0, -9000], 200), true)] };
  // The anchor is a circle of the pad's width, the line goes 2 mm out and is 0.2 mm wide; the proxy line is not copper.
  assert.deepEqual(customPadExtent({ size: [800, 800], custom }), [-400, -400, 2100, 400]);
  const rect: PadCustom = { ...custom, anchor: "rect" };
  assert.deepEqual(customPadExtent({ size: [800, 500], custom: rect }), [-400, -250, 2100, 250], "a rectangular base is the pad's own size");
  assert.deepEqual(customPadExtent({ size: [800, 500], custom: null }), [-400, -400, 400, 400], "no data: the circle base");
  // A circle reaches its radius and its stroke; an arc reaches the side of its circle that its sweep passes.
  const ring: CmdShape = { kind: "circle", layer: "", stroke_width: 100, filled: false, center: { x: 1000, y: 0 }, end: { x: 1500, y: 0 } };
  assert.deepEqual(customPadExtent({ size: [100, 100], custom: { ...custom, primitives: [prim(ring)] } }), [-50, -550, 1550, 550]);
  const arc: CmdShape = { kind: "arc", layer: "", stroke_width: 0, filled: false, start: { x: 0, y: 1000 }, mid: { x: 1000, y: 0 }, end: { x: 0, y: -1000 } };
  // Through (1000, 0) between (0, 1000) and (0, -1000): the circle is centred on the origin with radius 1000, and the arc takes its right side.
  assert.deepEqual(customPadExtent({ size: [100, 100], custom: { ...custom, primitives: [prim(arc)] } }).map(Math.round), [-50, -1000, 1000, 1000]);
  const opposite: CmdShape = { ...arc, mid: { x: -1000, y: 0 } };
  assert.deepEqual(customPadExtent({ size: [100, 100], custom: { ...custom, primitives: [prim(opposite)] } }).map(Math.round), [-1000, -1000, 50, 1000], "the other way round, the left side");
});

test("a custom pad's box on the footprint is moved by its offset and turned with the pad", () => {
  const custom: PadCustom = {
    anchor: "circle",
    zone: "outline",
    primitives: [{ shape: { kind: "segment", layer: "", stroke_width: 0, filled: false, start: { x: 0, y: 0 }, end: { x: 2000, y: 0 } } }],
  };
  const at = { x: 10_000, y: 5_000 };
  const base = { size: [400, 400] as [number, number], custom, at, offset: { x: 0, y: 0 } };
  assert.deepEqual(customPadBox({ ...base, rot: 0 }).map(Math.round), [9_800, 4_800, 12_000, 5_200]);
  // Turned a quarter: the arm points along +y (the canvas is y-down, so a positive angle runs clockwise on the screen).
  assert.deepEqual(customPadBox({ ...base, rot: 90_000 }).map(Math.round), [9_800, 4_800, 10_200, 7_000]);
  assert.deepEqual(customPadBox({ ...base, rot: 0, offset: { x: 100, y: -100 } }).map(Math.round), [9_900, 4_700, 12_100, 5_100]);
});

test("a trapezoid's corners are KiCad's: delta y changes the length of the horizontal sides, delta x the vertical ones", () => {
  // 2 mm by 1 mm, delta (0, 0.8 mm): the bottom side (y +0.5) is 2.8 mm long and the top one 1.2 mm.
  assert.deepEqual(trapezoidCorners(2000, 1000, [0, 800]), [[-1400, 500], [1400, 500], [600, -500], [-600, -500]]);
  // delta (0.4 mm, 0): the left side (x -1) is 1.4 mm long, the right one 0.6 mm.
  assert.deepEqual(trapezoidCorners(2000, 1000, [400, 0]), [[-1000, 700], [1000, 300], [1000, -300], [-1000, -700]]);
  // No delta: the rectangle.
  assert.deepEqual(trapezoidCorners(2000, 1000, [0, 0]), [[-1000, 500], [1000, 500], [1000, -500], [-1000, -500]]);
});
