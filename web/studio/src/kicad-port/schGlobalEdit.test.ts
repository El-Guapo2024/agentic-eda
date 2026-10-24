import { test } from "node:test";
import assert from "node:assert/strict";
import { editedGraphic, emptyEdit, planGlobalEdit } from "./schGlobalEdit";
import type { SchGraphic } from "../api/schEditTypes";
import type { Schematic } from "../api/types";

const rect: SchGraphic = { id: "shp_1", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } };
const box: SchGraphic = { id: "tbox_1", shape: { type: "text_box", start: { x: 0, y: 0 }, end: { x: 10, y: 10 }, text: "hi", size_nm: 1270 } };
const area: SchGraphic = { id: "rarea_1", shape: { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }] } };
const sheet = (over: Partial<Schematic>): Schematic => ({ texts: [], graphics: [], lines: [], ...over }) as unknown as Schematic;

test("a property left unchanged touches nothing", () => {
  assert.equal(editedGraphic(rect, emptyEdit()), null);
});

test("line width, style and fill reach a shape; a text box also takes its text properties", () => {
  const spec = { ...emptyEdit(), lineWidth: 300, lineStyle: "dash" as const, fill: "background" as const, textSize: 2000, bold: true, hAlign: "center" as const };
  const r = editedGraphic(rect, spec)!;
  assert.deepEqual([r.width_nm, r.line_style, r.fill], [300, "dash", "background"]);
  const b = editedGraphic(box, spec)!;
  assert.equal(b.shape.type === "text_box" && b.shape.size_nm, 2000);
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

test("selected-only restricts the edit; a text's size is edited in place and a line is replaced to change its width", () => {
  const sch = sheet({
    texts: [{ id: "txt_1", content: "a", at: [100, 200], angle: 90, size_nm: 1270 }, { id: "txt_2", content: "b", at: [0, 0], angle: 0, size_nm: 1270 }],
    graphics: [rect, area],
    lines: [{ id: "sln_1", pts: [[0, 0], [10, 0]], width_nm: 0 }],
  });
  const all = planGlobalEdit(sch, new Set(), { ...emptyEdit(), textSize: 2000, lineWidth: 250 });
  const verb = (cmds: typeof all, v: string) => cmds.filter((c) => c.op === "sch_edit" && c.verb === v);
  assert.deepEqual(verb(all, "edit_text"), [{ op: "sch_edit", verb: "edit_text", id: "txt_1", size_nm: 2000 }, { op: "sch_edit", verb: "edit_text", id: "txt_2", size_nm: 2000 }]);
  assert.equal(all.some((c) => c.op === "delete_sch_text" || c.op === "add_sch_text"), false, "a text is not deleted and put back: it would lose its id and its justification");
  assert.equal(verb(all, "edit_graphic").length, 2);
  assert.deepEqual(all.find((c) => c.op === "add_sch_line"), { op: "add_sch_line", pts: [{ x: 0, y: 0 }, { x: 10, y: 0 }], width_nm: 250 });
  const some = planGlobalEdit(sch, new Set(["txt_2", "shp_1"]), { ...emptyEdit(), selectedOnly: true, textSize: 2000, lineWidth: 250 });
  assert.deepEqual(verb(some, "edit_text"), [{ op: "sch_edit", verb: "edit_text", id: "txt_2", size_nm: 2000 }]);
  assert.equal(verb(some, "edit_graphic").length, 1);
  assert.equal(some.some((c) => c.op === "add_sch_line"), false, "the line is not selected");
});

test("a text already of the size is left alone", () => {
  const sch = sheet({ texts: [{ id: "txt_1", content: "a", at: [0, 0], angle: 0, size_nm: 2000 }] });
  assert.deepEqual(planGlobalEdit(sch, new Set(), { ...emptyEdit(), textSize: 2000 }), []);
});
