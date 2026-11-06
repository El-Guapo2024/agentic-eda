import { test } from "node:test";
import assert from "node:assert/strict";
import { addCorner, canAddCorner, eeDrag, eeFindHandle, eeMakePoints, MIL_NM, pinsOnEdge, removableCorner, removeCorner, RECT_BOT, RECT_CENTER, RECT_RADIUS, RECT_RIGHT, RECT_TOP, type EeEditShape } from "./eePointEdit";

const MM = 1_000_000;
const rect: EeEditShape = { kind: "rectangle", start: [10 * MM, 20 * MM], end: [0, 0], radius: 0 };

test("a rectangle's points: the four corners, the centre and the radius point (round), and a handle in the middle of each side; it is normalised first", () => {
  const e = eeMakePoints(rect);
  assert.deepEqual(e.points.map((p) => p.pos), [[0, 0], [10 * MM, 0], [0, 20 * MM], [10 * MM, 20 * MM], [5 * MM, 10 * MM], [10 * MM, 0]]);
  assert.equal(e.points[RECT_RADIUS]!.circle, true);
  assert.deepEqual(e.lines.map((l) => [l.a, l.b]), [[0, 1], [1, 3], [3, 2], [2, 0]]);
  assert.ok(e.lines.every((l) => l.center && !l.draw));
  // `FindPoint`: the points first, in their order (the top-right corner before the radius point on it), then the middles of the sides
  assert.deepEqual(eeFindHandle(e, [10 * MM + 10, 5], 100), { kind: "point", index: 1 });
  assert.deepEqual(eeFindHandle(e, [5 * MM, 20 * MM - 50], 100), { kind: "line", index: RECT_BOT });
  assert.equal(eeFindHandle(e, [3 * MM, 3 * MM], 100), null);
  // a symbol's rectangle holds no radius: no radius point
  assert.equal(eeMakePoints({ kind: "rectangle", start: [0, 0], end: [MM, MM] }).points.length, 5);
});

test("PinEditedCorner: a corner drags its two sides along and stays 1 mil inside the opposite corner; the centre moves the rectangle", () => {
  const r = eeDrag(rect, { kind: "point", index: 0 }, [-2 * MM, -3 * MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(r.shape, { ...rect, start: [-2 * MM, -3 * MM], end: [10 * MM, 20 * MM] });
  assert.equal(r.edge, undefined, "a corner's drag moves no pin");
  const past = eeDrag(rect, { kind: "point", index: 0 }, [50 * MM, 50 * MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(past.shape, { ...rect, start: [10 * MM - MIL_NM, 20 * MM - MIL_NM], end: [10 * MM, 20 * MM] });
  const moved = eeDrag(rect, { kind: "point", index: RECT_CENTER }, [6 * MM, 12 * MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(moved.shape, { ...rect, start: [MM, 2 * MM], end: [11 * MM, 22 * MM] });
});

test("a side moves perpendicular only (its other coordinate is not read) and says which side and how far, for the pins on it", () => {
  const r = eeDrag(rect, { kind: "line", index: RECT_RIGHT }, [14 * MM, 3 * MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(r.shape, { ...rect, start: [0, 0], end: [14 * MM, 20 * MM] });
  assert.deepEqual(r.edge, { seg: [[10 * MM, 0], [10 * MM, 20 * MM]], delta: [4 * MM, 0] });
  const top = eeDrag(rect, { kind: "line", index: RECT_TOP }, [MM, 30 * MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(top.shape, { ...rect, start: [0, 20 * MM - MIL_NM], end: [10 * MM, 20 * MM] }, "not past the bottom");
  // `dragPinsOnEdge`: the pins whose root is on the old side, its ends left out
  const pins = [
    { id: "on", root: [10 * MM, 5 * MM] as [number, number] },
    { id: "corner", root: [10 * MM, 0] as [number, number] },
    { id: "off", root: [9 * MM, 5 * MM] as [number, number] },
  ];
  assert.deepEqual(pinsOnEdge(r.edge!, pins), ["on"]);
});

test("the radius point slides along the top side between the corner and half the shorter side", () => {
  const r = eeDrag(rect, { kind: "point", index: RECT_RADIUS }, [8 * MM, 4 * MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(r.shape, { ...rect, start: [0, 0], end: [10 * MM, 20 * MM], radius: 2 * MM });
  const far = eeDrag(rect, { kind: "point", index: RECT_RADIUS }, [-50 * MM, 0], "keep_center_adjust_angle_radius")!;
  assert.equal((far.shape as { radius: number }).radius, 5 * MM, "at most half the 10 mm width");
});

test("a text box keeps GetMinSize and its centre and radius points do nothing", () => {
  const box: EeEditShape = { kind: "text_box", start: [0, 0], end: [10 * MM, 5 * MM], minSize: [0, 3 * MM] };
  const r = eeDrag(box, { kind: "line", index: RECT_BOT }, [5 * MM, MM], "keep_center_adjust_angle_radius")!;
  assert.deepEqual(r.shape, { ...box, end: [10 * MM, 3 * MM] });
  assert.deepEqual(eeDrag(box, { kind: "point", index: RECT_CENTER }, [MM, MM], "keep_center_adjust_angle_radius")!.shape, box);
});

test("a circle: the centre moves alone (the end stays, the radius follows), the end sets the radius", () => {
  const c: EeEditShape = { kind: "circle", center: [0, 0], end: [5 * MM, 0] };
  assert.deepEqual(eeMakePoints(c).points.map((p) => p.pos), [[0, 0], [5 * MM, 0]]);
  assert.deepEqual(eeDrag(c, { kind: "point", index: 0 }, [MM, 0], "keep_center_adjust_angle_radius")!.shape, { ...c, center: [MM, 0] });
  assert.deepEqual(eeDrag(c, { kind: "point", index: 1 }, [0, 7 * MM], "keep_center_adjust_angle_radius")!.shape, { ...c, end: [0, 7 * MM] });
});

test("a bezier: four points, the two indicator lines are no handles, a drag sets one point", () => {
  const b: EeEditShape = { kind: "bezier", start: [0, 0], c1: [MM, -MM], c2: [2 * MM, -MM], end: [3 * MM, 0] };
  const e = eeMakePoints(b);
  assert.equal(e.points.length, 4);
  assert.deepEqual(e.lines.map((l) => [l.a, l.b, l.center, l.draw]), [[0, 1, false, true], [2, 3, false, true]]);
  assert.equal(eeFindHandle(e, [MM / 2, -MM / 2], 100), null);
  assert.deepEqual(eeDrag(b, { kind: "point", index: 2 }, [2 * MM, -3 * MM], "keep_center_adjust_angle_radius")!.shape, { ...b, c2: [2 * MM, -3 * MM] });
});

test("an arc: start, middle, end and centre; the end moves round the centre in the default mode", () => {
  const R = 10 * MM;
  const M = Math.round(R / Math.SQRT2);
  const a: EeEditShape = { kind: "arc", start: [R, 0], mid: [M, M], end: [0, R] };
  const e = eeMakePoints(a);
  assert.deepEqual(e.lines.map((l) => [l.a, l.b]), [[3, 0], [3, 2]]);
  const r = eeDrag(a, { kind: "point", index: 2 }, [-2 * R, 0], "keep_center_ends_adjust_angle")!;
  const end = (r.shape as { end: number[] }).end;
  assert.ok(Math.abs(end[0]! + R) <= 5 && Math.abs(end[1]!) <= 5, `the end on the radius, at the cursor's angle (${end})`);
});

test("a polyline: a point per vertex and a side handle per side, the last back to the first; a side moves both its ends", () => {
  const poly: EeEditShape = { kind: "polygon", pts: [[0, 0], [10 * MM, 0], [10 * MM, 10 * MM]] };
  const e = eeMakePoints(poly);
  assert.deepEqual(e.lines.map((l) => [l.a, l.b]), [[0, 1], [1, 2], [2, 0]]);
  assert.deepEqual(eeDrag(poly, { kind: "point", index: 1 }, [12 * MM, MM], "keep_center_adjust_angle_radius")!.shape, { ...poly, pts: [[0, 0], [12 * MM, MM], [10 * MM, 10 * MM]] });
  // the middle of the first side (5, 0) to (5, -2): both ends go up 2 mm
  assert.deepEqual(eeDrag(poly, { kind: "line", index: 0 }, [5 * MM, -2 * MM], "keep_center_adjust_angle_radius")!.shape, { ...poly, pts: [[0, -2 * MM], [10 * MM, -2 * MM], [10 * MM, 10 * MM]] });
});

test("Add Corner: on the outline, after the nearest side's start (an open chain's closing side is none); Remove Corner keeps two points, a rule area three", () => {
  const poly = { kind: "polygon" as const, pts: [[0, 0], [10 * MM, 0], [10 * MM, 10 * MM]] as [number, number][] };
  assert.equal(canAddCorner(poly, [5 * MM, 100], 200), true);
  assert.equal(canAddCorner(poly, [5 * MM, 5 * MM + 300], 200), false, "the closing side of an open chain is not on it");
  assert.equal(canAddCorner({ ...poly, ruleArea: true }, [5 * MM, 5 * MM], 200), true);
  assert.deepEqual(addCorner(poly, [10 * MM, 4 * MM]).pts, [[0, 0], [10 * MM, 0], [10 * MM, 4 * MM], [10 * MM, 10 * MM]]);
  // a point near the closing side of an open chain goes in after the nearest real side
  assert.deepEqual(addCorner(poly, [4 * MM, 5 * MM]).pts.length, 4);
  assert.equal(removableCorner(poly, [10 * MM, 0]), 1);
  assert.equal(removableCorner(poly, [3, 3]), -1);
  assert.deepEqual(removeCorner(poly, 1)!.pts, [[0, 0], [10 * MM, 10 * MM]]);
  assert.equal(removeCorner({ kind: "polygon", pts: [[0, 0], [MM, 0]] }, 0), null);
  assert.equal(removableCorner({ ...poly, ruleArea: true }, [0, 0]), -1);
  // the first vertex of a chain closed by a repeated point: the repeat takes the new first
  const closed = { kind: "polygon" as const, pts: [[0, 0], [MM, 0], [MM, MM], [0, 0]] as [number, number][] };
  assert.deepEqual(removeCorner(closed, 0)!.pts, [[MM, 0], [MM, MM], [MM, 0]]);
});
