import { test } from "node:test";
import assert from "node:assert/strict";
import { editedGraphic, emptyEdit, planGlobalEdit } from "./schGlobalEdit";
import type { SchGraphic } from "../api/schEditTypes";
import type { Schematic } from "../api/types";

const rect: SchGraphic = { id: "shp_1", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } };
const box: SchGraphic = { id: "tbox_1", shape: { type: "text_box", start: { x: 0, y: 0 }, end: { x: 10, y: 10 }, text: "hi", size_um: 1270 } };
const area: SchGraphic = { id: "rarea_1", shape: { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }] } };
const sheet = (over: Partial<Schematic>): Schematic => ({ texts: [], graphics: [], lines: [], ...over }) as unknown as Schematic;

test("a property left unchanged touches nothing", () => {
  assert.equal(editedGraphic(rect, emptyEdit()), null);
});

test("line width, style and fill reach a shape; a text box also takes its text properties", () => {
  const spec = { ...emptyEdit(), lineWidth: 300, lineStyle: "dash" as const, fill: "background" as const, textSize: 2000, bold: true, hAlign: "center" as const };
  const r = editedGraphic(rect, spec)!;
  assert.deepEqual([r.width_um, r.line_style, r.fill], [300, "dash", "background"]);
  const b = editedGraphic(box, spec)!;
  assert.equal(b.shape.type === "text_box" && b.shape.size_um, 2000);
  assert.equal(b.shape.type === "text_box" && b.shape.bold, true);
  assert.equal(b.shape.type === "text_box" && b.shape.h_align, "center");
});

test("the kinds a request leaves out are not reached", () => {
  const spec = { ...emptyEdit(), lineWidth: 300, kinds: { texts: true, textBoxes: false, shapes: false, ruleAreas: true, lines: true } };
  assert.equal(editedGraphic(rect, spec), null);
  assert.equal(editedGraphic(box, spec), null);
  assert.ok(editedGraphic(area, spec));
});

test("an arc has no fill to set", () => {
  const arc: SchGraphic = { id: "shp_a", shape: { type: "arc", start: { x: 0, y: 0 }, mid: { x: 5, y: 5 }, end: { x: 10, y: 0 } } };
  assert.equal(editedGraphic(arc, { ...emptyEdit(), fill: "outline" }), null);
});

test("selected-only restricts the edit; texts and lines are replaced to change their size and width", () => {
  const sch = sheet({
    texts: [{ id: "txt_1", content: "a", at: [100, 200], angle: 90, size_um: 1270 }, { id: "txt_2", content: "b", at: [0, 0], angle: 0, size_um: 1270 }],
    graphics: [rect, area],
    lines: [{ id: "sln_1", pts: [[0, 0], [10, 0]], width_um: 0 }],
  });
  const all = planGlobalEdit(sch, new Set(), { ...emptyEdit(), textSize: 2000, lineWidth: 250 });
  assert.equal(all.filter((c) => c.op === "delete_sch_text").length, 2);
  assert.equal(all.filter((c) => c.op === "sch_edit").length, 2);
  assert.deepEqual(all.find((c) => c.op === "add_sch_line"), { op: "add_sch_line", pts: [{ x: 0, y: 0 }, { x: 10, y: 0 }], width_um: 250 });
  const some = planGlobalEdit(sch, new Set(["txt_2", "shp_1"]), { ...emptyEdit(), selectedOnly: true, textSize: 2000, lineWidth: 250 });
  assert.deepEqual(some.filter((c) => c.op === "delete_sch_text"), [{ op: "delete_sch_text", id: "txt_2" }]);
  assert.equal(some.filter((c) => c.op === "sch_edit").length, 1);
  assert.equal(some.some((c) => c.op === "add_sch_line"), false, "the line is not selected");
  assert.deepEqual(some.find((c) => c.op === "add_sch_text"), { op: "add_sch_text", content: "b", at: { x: 0, y: 0 }, angle_millideg: 0, size_um: 2000 });
});

test("a text already of the size is left alone", () => {
  const sch = sheet({ texts: [{ id: "txt_1", content: "a", at: [0, 0], angle: 0, size_um: 2000 }] });
  assert.deepEqual(planGlobalEdit(sch, new Set(), { ...emptyEdit(), textSize: 2000 }), []);
});
