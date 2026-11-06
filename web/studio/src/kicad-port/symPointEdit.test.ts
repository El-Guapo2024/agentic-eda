import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import { eeMakePoints, RECT_RIGHT } from "./eePointEdit";
import { pinsDraggedWithEdge, setSymArcEditMode, symArcEditMode, symDrag, symEditShape, symGraphicFrom, symPointEditor } from "./symPointEdit";
import { schDrag, schEditShape, schShapeFrom, textBoxMinSize } from "./schPointEdit";
import type { SchGraphic } from "../api/schEditTypes";

const measure = (t: string, size: number) => t.length * size * 0.8;
const common = { unit: 0, body_style: 0, stroke_mm: 0.254, fill: "none" as const };

test("a symbol's rectangle in the editor's nm, y down; no radius point; a drag back to mm, y up, on the 0.1 um grid", () => {
  const g: LibrarySymbolGraphic = { kind: "rectangle", id: "r", ...common, start: { x: -2.54, y: 5.08 }, end: { x: 2.54, y: -5.08 } };
  const e = symEditShape(g, measure)!;
  assert.deepEqual(e, { kind: "rectangle", start: [-2_540_000, -5_080_000], end: [2_540_000, 5_080_000] });
  assert.equal(eeMakePoints(e).points.length, 5);
  const r = symDrag(g, { kind: "line", index: RECT_RIGHT }, [5_080_123, 0], "keep_center_adjust_angle_radius", measure)!;
  assert.deepEqual(symGraphicFrom(r.shape, g), { ...g, start: { x: -2.54, y: 5.08 }, end: { x: 5.0801, y: -5.08 } });
});

test("dragPinsOnEdge: the pins whose root is on the dragged side go along, of the rectangle's unit or common to all; not with the setting off", () => {
  const g: LibrarySymbolGraphic = { kind: "rectangle", id: "r", ...common, unit: 1, start: { x: -2.54, y: 5.08 }, end: { x: 2.54, y: -5.08 } };
  const pin = (id: string, x: number, y: number, unit: number): LibrarySymbolPin => ({ id, number: id, name: id, electrical_type: "passive", shape: "line", at: { x, y }, angle_deg: 180, length_mm: 2.54, unit, body_style: 0 }) as unknown as LibrarySymbolPin;
  // the root is 2.54 mm left of the pin's point for a pin pointing left (its tip on the right): the roots here are on the right side x = 2.54
  const pins = [pin("1", 5.08, 0, 1), pin("2", 5.08, 2.54, 0), pin("3", 5.08, 0, 2), pin("4", 5.08, 5.08, 1)];
  const rootOf = (p: LibrarySymbolPin): [number, number] => [Math.round((p.at.x - 2.54) * 1e6), Math.round(-p.at.y * 1e6)];
  const r = symDrag(g, { kind: "line", index: RECT_RIGHT }, [5_080_000, 0], "keep_center_adjust_angle_radius", measure)!;
  assert.deepEqual(pinsDraggedWithEdge(g, r, pins, rootOf, true), [{ id: "1", x: 7.62, y: 0 }, { id: "2", x: 7.62, y: 2.54 }], "pin 3 is another unit's, pin 4 is on the corner");
  assert.deepEqual(pinsDraggedWithEdge(g, r, pins, rootOf, false), []);
  const corner = symDrag(g, { kind: "point", index: 3 }, [5_080_000, 6_000_000], "keep_center_adjust_angle_radius", measure)!;
  assert.deepEqual(pinsDraggedWithEdge(g, corner, pins, rootOf, true), [], "a corner's drag takes no pin");
});

test("a symbol's circle, arc, polyline and bezier go through the point editor and back", () => {
  const c: LibrarySymbolGraphic = { kind: "circle", id: "c", ...common, center: { x: 1, y: 1 }, radius_mm: 2 };
  const e = symEditShape(c, measure)!;
  assert.deepEqual(eeMakePoints(e).points.map((p) => p.pos), [[1_000_000, -1_000_000], [3_000_000, -1_000_000]]);
  const r = symDrag(c, { kind: "point", index: 1 }, [1_000_000, -5_000_000], "keep_center_adjust_angle_radius", measure)!;
  assert.deepEqual(symGraphicFrom(r.shape, c), { ...c, radius_mm: 4 });
  const poly: LibrarySymbolGraphic = { kind: "polyline", id: "p", ...common, pts: [{ x: 0, y: 0 }, { x: 2.54, y: 0 }] };
  const ed = symPointEditor(poly, { handle: { kind: "point", index: 1 }, to: [2_540_000, -2_540_000] }, "keep_center_adjust_angle_radius", measure)!;
  assert.deepEqual(ed.graphic, { ...poly, pts: [{ x: 0, y: 0 }, { x: 2.54, y: 2.54 }] });
  assert.equal(symEditShape({ kind: "text", id: "t", unit: 0, body_style: 0, text: "x", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 } as LibrarySymbolGraphic, measure), null, "a text has no point editor");
});

test("the Symbol Editor keeps its own arc edit mode", () => {
  assert.equal(symArcEditMode(), "keep_center_adjust_angle_radius");
  assert.equal(setSymArcEditMode("cycle"), "keep_center_ends_adjust_angle");
  assert.equal(setSymArcEditMode("keep_center_adjust_angle_radius"), "keep_center_adjust_angle_radius");
});

test("the schematic's graphics: a circle's end on the radius to the right, a rule area closed, a text box's minimum height from its wrapped text", () => {
  const circle: SchGraphic = { id: "c", shape: { type: "circle", center: { x: 0, y: 0 }, radius_nm: 5_000_000 } };
  assert.deepEqual(schEditShape(circle, measure), { kind: "circle", center: [0, 0], end: [5_000_000, 0] });
  const r = schDrag(circle, { kind: "point", index: 0 }, [1_000_000, 0], "keep_center_adjust_angle_radius", measure)!;
  assert.deepEqual(schShapeFrom(r.shape, circle.shape), { type: "circle", center: { x: 1_000_000, y: 0 }, radius_nm: 4_000_000 }, "the end stays, so the radius follows the centre");
  const area: SchGraphic = { id: "a", shape: { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }] } };
  assert.deepEqual(schEditShape(area, measure), { kind: "polygon", pts: [[0, 0], [1, 0], [1, 1]], closed: true, ruleArea: true });
  const box: SchGraphic = { id: "b", shape: { type: "text_box", start: { x: 0, y: 0 }, end: { x: 20_000_000, y: 10_000_000 }, text: "one two three", size_nm: 1_270_000 } };
  const one = textBoxMinSize(box, [0, 0], [20_000_000, 10_000_000], measure);
  const narrow = textBoxMinSize(box, [0, 0], [6_000_000, 10_000_000], measure);
  assert.equal(one[0], 0);
  assert.ok(narrow[1] > one[1], `a narrower box wraps the text onto more lines (${one[1]} -> ${narrow[1]})`);
  const empty: SchGraphic = { id: "e", shape: { ...(box.shape as Extract<SchGraphic["shape"], { type: "text_box" }>), text: "" } };
  assert.deepEqual(textBoxMinSize(empty, [0, 0], [1, 1], measure), [0, 0]);
  // a drag of the bottom side up stops at the text's height
  const up = schDrag(box, { kind: "line", index: 2 }, [10_000_000, 0], "keep_center_adjust_angle_radius", measure)!;
  assert.equal((up.shape as { end: number[] }).end[1], one[1]);
});
