import { test } from "node:test";
import assert from "node:assert/strict";
import type { SchGraphic } from "../api/schEditTypes";
import { schPolyIsClosed, schPolyStrokePoints } from "./schPolyline";
import { schAddCorner, schCornerEntries, schEditShape, schRemoveCorner } from "./schPointEdit";

const MM = 1_000_000;
// A filled polyline of the sheet as `parseSchPolyLine` reads it (an open chain), and a rule area.
const poly: SchGraphic = { id: "p", shape: { type: "polygon", pts: [{ x: 0, y: 0 }, { x: 10 * MM, y: 0 }, { x: 10 * MM, y: 10 * MM }, { x: 0, y: 10 * MM }] }, fill: "color", fill_color: { r: 0, g: 0, b: 255, a: 255 } };
const area: SchGraphic = { id: "r", shape: { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: 10 * MM, y: 0 }, { x: 10 * MM, y: 10 * MM }, { x: 0, y: 10 * MM }] }, line_style: "dash" };
const tol = 8 * 25_400; // `ToWorld( EDIT_POINT::POINT_SIZE )` at 1 px = 1 mil

test("SS21: a polyline is stroked open, a rule area closed (`SCH_PAINTER::draw( SCH_SHAPE )`, `EDA_SHAPE::IsClosed`)", () => {
  assert.equal(schPolyIsClosed("polygon"), false);
  assert.equal(schPolyIsClosed("rule_area"), true);
  const pts: [number, number][] = [[0, 0], [10, 0], [10, 10], [0, 10]];
  assert.deepEqual(schPolyStrokePoints(pts, false), pts, "the chain's points: no side from the last back to the first");
  assert.deepEqual(schPolyStrokePoints(pts, true), [...pts, [0, 0]], "a closed chain's closing segment ends on the first point");
  assert.deepEqual(schPolyStrokePoints([[1, 1]], false), [], "no segment, nothing drawn");
  // a chain whose last point repeats its first reads closed on the page, and is still the open chain the file holds
  assert.deepEqual(schPolyStrokePoints([...pts, [0, 0]], false), [...pts, [0, 0]]);
  const e = schEditShape(poly, () => 0);
  assert.deepEqual(e?.kind === "polygon" && { closed: e.closed, ruleArea: e.ruleArea }, { closed: false, ruleArea: undefined }, "the point editor's shape is the open chain");
});

test("SS21: Create Corner on the sheet is the Symbol Editor's `addCorner`: an open polyline's closing side is no candidate, a rule area's is", () => {
  // on the side from the last vertex back to the first: no corner for the polyline (`addCornerCondition`'s `HitTest` of the open chain), one for the rule area
  const closingSide: [number, number] = [0, 5 * MM];
  assert.equal(schCornerEntries(poly, closingSide, tol).canAdd, false);
  assert.equal(schCornerEntries(area, closingSide, tol).canAdd, true);
  const a = schAddCorner(area, closingSide);
  assert.deepEqual(a?.type === "rule_area" && a.pts, [{ x: 0, y: 0 }, { x: 10 * MM, y: 0 }, { x: 10 * MM, y: 10 * MM }, { x: 0, y: 10 * MM }, { x: 0, y: 5 * MM }], "after the closing side's start");
  // on a side of the chain: after that side's start (`poly.Insert( closestLineStart + 1, cursor )`), the other properties kept
  assert.equal(schCornerEntries(poly, [10 * MM + 1000, 4 * MM], tol).canAdd, true);
  const p = schAddCorner(poly, [10 * MM, 4 * MM]);
  assert.deepEqual(p, { type: "polygon", pts: [{ x: 0, y: 0 }, { x: 10 * MM, y: 0 }, { x: 10 * MM, y: 4 * MM }, { x: 10 * MM, y: 10 * MM }, { x: 0, y: 10 * MM }] });
  // the nearest side of the open chain only: near the closing side's middle the corner goes on the nearest real side (`numPoints -= 1`)
  const q = schAddCorner(poly, [MM, 6 * MM]);
  assert.deepEqual(q?.type === "polygon" && q.pts.map((v) => [v.x, v.y]), [[0, 0], [10 * MM, 0], [10 * MM, 10 * MM], [MM, 6 * MM], [0, 10 * MM]]);
  const r = schAddCorner(area, [MM, 6 * MM]);
  assert.deepEqual(r?.type === "rule_area" && r.pts.map((v) => [v.x, v.y]), [[0, 0], [10 * MM, 0], [10 * MM, 10 * MM], [0, 10 * MM], [MM, 6 * MM]], "the rule area's closing side is nearer");
  // anything else has no corners
  const rect: SchGraphic = { id: "x", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: MM, y: MM } } };
  assert.deepEqual(schCornerEntries(rect, [0, 0], tol), { canAdd: false, removeIndex: -1 });
  assert.equal(schAddCorner(rect, [0, 0]), null);
});

test("SS21: Remove Corner on the sheet takes the edited point -- the point editor's vertex under the cursor -- and keeps two points (a rule area three)", () => {
  // the vertex under the cursor, within POINT_SIZE
  assert.equal(schCornerEntries(poly, [10 * MM + 50_000, 10 * MM - 50_000], tol).removeIndex, 2);
  // a side's middle is an edit point too (an `EDIT_LINE`), but no vertex: no Remove Corner there
  assert.equal(schCornerEntries(poly, [10 * MM, 5 * MM], tol).removeIndex, -1);
  assert.deepEqual(schRemoveCorner(poly, 2), { type: "polygon", pts: [{ x: 0, y: 0 }, { x: 10 * MM, y: 0 }, { x: 0, y: 10 * MM }] });
  const two: SchGraphic = { ...poly, shape: { type: "polygon", pts: [{ x: 0, y: 0 }, { x: MM, y: 0 }] } };
  assert.equal(schCornerEntries(two, [0, 0], tol).removeIndex, -1, "a polyline of two points keeps them");
  assert.equal(schRemoveCorner(two, 0), null);
  const tri: SchGraphic = { ...area, shape: { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: MM, y: 0 }, { x: 0, y: MM }] } };
  assert.equal(schCornerEntries(tri, [0, 0], tol).removeIndex, -1, "a rule area of three points keeps them");
  // the first vertex of a chain that repeats it at the end: the repeat becomes the new first point (`poly.SetPoint( last - 1, poly.GetPoint( 0 ) )`)
  const loop: SchGraphic = { ...poly, shape: { type: "polygon", pts: [{ x: 0, y: 0 }, { x: 10 * MM, y: 0 }, { x: 10 * MM, y: 10 * MM }, { x: 0, y: 0 }] } };
  assert.equal(schCornerEntries(loop, [0, 0], tol).removeIndex, 0);
  assert.deepEqual(schRemoveCorner(loop, 0), { type: "polygon", pts: [{ x: 10 * MM, y: 0 }, { x: 10 * MM, y: 10 * MM }, { x: 10 * MM, y: 0 }] });
});
