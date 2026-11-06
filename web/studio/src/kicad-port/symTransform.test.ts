// SYMBOL_EDITOR_EDIT_TOOL::Rotate and ::Mirror on every kind of item of a library symbol, against KiCad 8303b2ad's transforms of each class (symTransform.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import type { FieldRow } from "./symFields";
import { arcCenter, graphicPosition, mirrorGraphic, mirrorPin, mirrorSelection, rotateGraphic, rotatePin, rotateSelection, rotateField, mirrorLoneField, transformPivot } from "./symTransform";

const O = { x: 0, y: 0 };
const pin = (x: number, y: number, angle: number): LibrarySymbolPin => ({ id: "p", number: "1", name: "A", electrical_type: "input", shape: "line", at: { x, y }, angle_deg: angle, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
const text = (x: number, y: number, angle: number, h?: "left" | "center" | "right", v?: "top" | "center" | "bottom"): LibrarySymbolGraphic => ({ kind: "text", id: "t", unit: 0, body_style: 1, text: "T", at: { x, y }, angle_deg: angle, size_mm: 1.27, ...(h ? { h_align: h } : {}), ...(v ? { v_align: v } : {}) });
const field = (angle: number, h: "left" | "center" | "right", v: "top" | "center" | "bottom" = "center"): FieldRow => ({ name: "Value", text: "V", at: { x: 2.54, y: 1.27 }, angle_deg: angle, size_mm: 1.27, hidden: false, show_name: false, do_not_autoplace: false, bold: false, italic: false, h_align: h, v_align: v });

test("SCH_PIN::RotatePin: the position turns about the centre, the orientation with it (Right -> Up counter-clockwise)", () => {
  assert.deepEqual([rotatePin(pin(2.54, 0, 0), O, true).at, rotatePin(pin(2.54, 0, 0), O, true).angle_deg], [{ x: 0, y: 2.54 }, 90]);
  assert.deepEqual([rotatePin(pin(2.54, 0, 0), O, false).at, rotatePin(pin(2.54, 0, 0), O, false).angle_deg], [{ x: 0, y: -2.54 }, 270]);
  // Down turns to Right counter-clockwise, Left to Up clockwise
  assert.equal(rotatePin(pin(0, 0, 270), O, true).angle_deg, 0);
  assert.equal(rotatePin(pin(0, 0, 180), O, false).angle_deg, 90);
});

test("SCH_PIN::MirrorHorizontallyPin swaps Right and Left, MirrorVerticallyPin Up and Down; the other two stay", () => {
  const h = mirrorPin(pin(2.54, 1.27, 0), "h", 1.27);
  assert.deepEqual([h.at, h.angle_deg], [{ x: 0, y: 1.27 }, 180]);
  assert.equal(mirrorPin(pin(0, 0, 90), "h", 0).angle_deg, 90, "an Up pin mirrored left-right still points up");
  const v = mirrorPin(pin(2.54, 1.27, 90), "v", 0);
  assert.deepEqual([v.at, v.angle_deg], [{ x: 2.54, y: -1.27 }, 270]);
  assert.equal(mirrorPin(pin(0, 0, 180), "v", 0).angle_deg, 180);
});

test("EDA_SHAPE::rotate: every point of a rectangle, polyline, circle, arc and bezier turns about the centre", () => {
  const rect: LibrarySymbolGraphic = { kind: "rectangle", unit: 1, body_style: 1, start: { x: -1, y: -2 }, end: { x: 1, y: 2 }, stroke_mm: 0.254, fill: "none" };
  assert.deepEqual(rotateGraphic(rect, O, true), { ...rect, start: { x: 2, y: -1 }, end: { x: -2, y: 1 } });
  const poly: LibrarySymbolGraphic = { kind: "polyline", unit: 1, body_style: 1, pts: [{ x: 0, y: 0 }, { x: 2.54, y: 0 }], stroke_mm: 0, fill: "none" };
  assert.deepEqual((rotateGraphic(poly, { x: 1.27, y: 0 }, false) as { pts: unknown }).pts, [{ x: 1.27, y: 1.27 }, { x: 1.27, y: -1.27 }]);
  const circle: LibrarySymbolGraphic = { kind: "circle", unit: 1, body_style: 1, center: { x: 1, y: 0 }, radius_mm: 1, stroke_mm: 0, fill: "none" };
  assert.deepEqual((rotateGraphic(circle, O, true) as { center: unknown }).center, { x: 0, y: 1 });
  const bez: LibrarySymbolGraphic = { kind: "bezier", unit: 1, body_style: 1, start: { x: 0, y: 0 }, c1: { x: 1, y: 2 }, c2: { x: 3, y: 2 }, end: { x: 4, y: 0 }, stroke_mm: 0, fill: "none" };
  assert.deepEqual(rotateGraphic(bez, O, true), { ...bez, start: { x: 0, y: 0 }, c1: { x: -2, y: 1 }, c2: { x: -2, y: 3 }, end: { x: 0, y: 4 } });
});

test("EDA_SHAPE::flip: the points mirror, and an arc's start and end swap so it still runs the same way round", () => {
  const arc: LibrarySymbolGraphic = { kind: "arc", unit: 1, body_style: 1, start: { x: 1, y: 0 }, mid: { x: 0.7071, y: 0.7071 }, end: { x: 0, y: 1 }, stroke_mm: 0, fill: "none" };
  assert.deepEqual(mirrorGraphic(arc, "h", 0), { ...arc, start: { x: 0, y: 1 }, mid: { x: -0.7071, y: 0.7071 }, end: { x: -1, y: 0 } });
  const bez: LibrarySymbolGraphic = { kind: "bezier", unit: 1, body_style: 1, start: { x: 0, y: 0 }, c1: { x: 1, y: 2 }, c2: { x: 3, y: 2 }, end: { x: 4, y: 0 }, stroke_mm: 0, fill: "none" };
  assert.deepEqual(mirrorGraphic(bez, "v", 1), { ...bez, start: { x: 0, y: 2 }, c1: { x: 1, y: 0 }, c2: { x: 3, y: 0 }, end: { x: 4, y: 2 } });
});

test("SCH_TEXT::Rotate: the position turns, a vertical text flips its horizontal justification, and the angle toggles either way", () => {
  const h = rotateGraphic(text(2.54, 0, 0, "left", "top"), O, true) as Extract<LibrarySymbolGraphic, { kind: "text" }>;
  assert.deepEqual([h.at, h.angle_deg, h.h_align, h.v_align], [{ x: 0, y: 2.54 }, 90, "left", "top"]);
  const cw = rotateGraphic(text(2.54, 0, 0, "left"), O, false) as Extract<LibrarySymbolGraphic, { kind: "text" }>;
  assert.deepEqual([cw.angle_deg, cw.h_align], [90, "left"], "clockwise too: Rotate90( false ) whichever way");
  const v = rotateGraphic(text(0, 0, 90, "left"), O, true) as Extract<LibrarySymbolGraphic, { kind: "text" }>;
  assert.deepEqual([v.angle_deg, v.h_align], [0, "right"]);
  assert.equal((rotateGraphic(text(0, 0, 90), O, true) as { h_align?: string }).h_align, "center");
});

test("SCH_TEXT::MirrorHorizontally / MirrorVertically of a symbol's text: the position mirrors and the justification across the mirror flips", () => {
  const t = (g: LibrarySymbolGraphic) => g as Extract<LibrarySymbolGraphic, { kind: "text" }>;
  // left-right: a horizontal text's horizontal justification, a vertical text's vertical one
  const a = t(mirrorGraphic(text(2.54, 1, 0, "left", "top"), "h", 0));
  assert.deepEqual([a.at, a.h_align, a.v_align], [{ x: -2.54, y: 1 }, "right", "top"]);
  const b = t(mirrorGraphic(text(2.54, 1, 90, "left", "top"), "h", 0));
  assert.deepEqual([b.at, b.h_align, b.v_align], [{ x: -2.54, y: 1 }, "left", "bottom"]);
  // top-bottom: a horizontal text's vertical justification, a vertical text's horizontal one
  const c = t(mirrorGraphic(text(2.54, 1, 0, "left", "top"), "v", 0));
  assert.deepEqual([c.at, c.h_align, c.v_align], [{ x: 2.54, y: -1 }, "left", "bottom"]);
  const d = t(mirrorGraphic(text(2.54, 1, 90, "left", "top"), "v", 0));
  assert.deepEqual([d.at, d.h_align, d.v_align], [{ x: 2.54, y: -1 }, "right", "top"]);
  // centred stays centred
  assert.equal(t(mirrorGraphic(text(1, 1, 0), "h", 0)).h_align, "center");
});

test("SCH_TEXTBOX: Rotate turns the box and toggles the text's angle; Mirror flips the box and the horizontal justification of a text running across the mirror", () => {
  const box: LibrarySymbolGraphic = { kind: "text_box", unit: 1, body_style: 1, text: "x", start: { x: 0, y: 0 }, end: { x: 4, y: -2 }, angle_deg: 0, size_mm: 1.27, h_align: "left", v_align: "top", stroke_mm: 0, fill: "none" };
  const r = rotateGraphic(box, O, true) as Extract<LibrarySymbolGraphic, { kind: "text_box" }>;
  assert.deepEqual([r.start, r.end, r.angle_deg, r.h_align, r.v_align], [{ x: 0, y: 0 }, { x: 2, y: 4 }, 90, "left", "top"]);
  const h = mirrorGraphic(box, "h", 0) as Extract<LibrarySymbolGraphic, { kind: "text_box" }>;
  assert.deepEqual([h.start, h.end, h.h_align], [{ x: 0, y: 0 }, { x: -4, y: -2 }, "right"]);
  assert.equal((mirrorGraphic(box, "v", 0) as { h_align: string }).h_align, "left", "a horizontal text mirrored top-bottom keeps its justification");
  assert.equal((mirrorGraphic({ ...box, angle_deg: 90 }, "v", 0) as { h_align: string }).h_align, "right");
});

test("SCH_FIELD::Rotate: horizontal becomes vertical (left and right swap clockwise), vertical horizontal (they swap counter-clockwise); the position turns", () => {
  assert.deepEqual([rotateField(field(0, "left"), O, true).angle_deg, rotateField(field(0, "left"), O, true).h_align, rotateField(field(0, "left"), O, true).at], [90, "left", { x: -1.27, y: 2.54 }]);
  assert.equal(rotateField(field(0, "left"), O, false).h_align, "right");
  assert.deepEqual([rotateField(field(90, "left"), O, true).angle_deg, rotateField(field(90, "left"), O, true).h_align], [0, "right"]);
  assert.equal(rotateField(field(90, "right"), O, false).h_align, "right");
});

test("Mirror of a field selected alone flips its justification only; with other items it mirrors its position", () => {
  assert.deepEqual(mirrorLoneField(field(0, "left", "top"), "v"), field(0, "left", "bottom"));
  assert.deepEqual(mirrorLoneField(field(0, "left", "top"), "h"), field(0, "right", "top"));
  const alone = mirrorSelection({ pins: [], graphics: [], fields: [field(0, "left")], centre: null }, "h")!;
  assert.deepEqual(alone.fields[0]!.at, { x: 2.54, y: 1.27 }, "it stays where it is");
  const both = mirrorSelection({ pins: [pin(0, 0, 0)], graphics: [], fields: [field(0, "left")], centre: { x: 1.27, y: 0 } }, "h")!;
  assert.deepEqual([both.fields[0]!.at, both.fields[0]!.h_align, both.pins[0]!.at, both.pins[0]!.angle_deg], [{ x: 0, y: 1.27 }, "left", { x: 2.54, y: 0 }, 180]);
});

test("the pivot: a lone item's GetPosition() (an arc's centre, a polygon's first vertex, a text box's start), else the selection's half-grid centre", () => {
  const arc: LibrarySymbolGraphic = { kind: "arc", unit: 1, body_style: 1, start: { x: 3, y: 1 }, mid: { x: 2, y: 2 }, end: { x: 1, y: 1 }, stroke_mm: 0, fill: "none" };
  assert.deepEqual(graphicPosition(arc), { x: 2, y: 1 });
  assert.deepEqual(arcCenter({ x: 3, y: 1 }, { x: 2, y: 2 }, { x: 1, y: 1 }), { x: 2, y: 1 });
  const poly: LibrarySymbolGraphic = { kind: "polyline", unit: 1, body_style: 1, pts: [{ x: 5, y: 6 }, { x: 0, y: 0 }], stroke_mm: 0, fill: "none" };
  assert.deepEqual(transformPivot({ pins: [], graphics: [poly], fields: [], centre: { x: 9, y: 9 } }), { x: 5, y: 6 });
  assert.deepEqual(transformPivot({ pins: [pin(1, 1, 0)], graphics: [poly], fields: [], centre: { x: 9, y: 9 } }), { x: 9, y: 9 });
  // a lone arc rotated about its own centre stays on its circle
  const r = rotateSelection({ pins: [], graphics: [arc], fields: [], centre: null }, true)!;
  assert.deepEqual(r.graphics[0], { ...arc, start: { x: 2, y: 2 }, mid: { x: 1, y: 1 }, end: { x: 2, y: 0 } });
});

test("a mixed selection turns and mirrors as one about the same point", () => {
  const sel = { pins: [pin(5.08, 0, 180)], graphics: [text(0, 2.54, 0, "left")], fields: [field(0, "center")], centre: { x: 0, y: 0 } };
  const r = rotateSelection(sel, false)!;
  assert.deepEqual([r.pins[0]!.at, r.pins[0]!.angle_deg], [{ x: 0, y: -5.08 }, 90]);
  assert.deepEqual((r.graphics[0] as { at: unknown }).at, { x: 2.54, y: 0 });
  assert.deepEqual(r.fields[0]!.at, { x: 1.27, y: -2.54 });
  const m = mirrorSelection(sel, "v")!;
  assert.deepEqual([m.pins[0]!.at, (m.graphics[0] as { at: unknown }).at, m.fields[0]!.at], [{ x: 5.08, y: 0 }, { x: 0, y: -2.54 }, { x: 2.54, y: -1.27 }]);
});
