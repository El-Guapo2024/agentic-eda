import { test } from "node:test";
import assert from "node:assert/strict";
import { editedGraphic, emptyEdit, planGlobalEdit } from "./schGlobalEdit";
import type { SchGraphic } from "../api/schEditTypes";
import type { Schematic } from "../api/types";

const rect: SchGraphic = { id: "shp_1", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } };
const box: SchGraphic = { id: "tbox_1", shape: { type: "text_box", start: { x: 0, y: 0 }, end: { x: 10, y: 10 }, text: "hi", size_nm: 1270 } };
const area: SchGraphic = { id: "rarea_1", shape: { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }] } };
const sheet = (over: Record<string, unknown>): Schematic => ({ texts: [], graphics: [], lines: [], symbols: [], labels: [], wires: [], sheets: [], junctions: [], ...over }) as unknown as Schematic;

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

test("UI-LEFT S16: fields, labels, wires, buses, junctions and sheet pins are reached by their checkboxes, a field name filter narrows the other fields", () => {
  const field = (owner: string, name: string, extra: Record<string, unknown> = {}) => ({ id: `fld:${owner}:${name}`, owner, name, text: "x", at: [0, 0] as [number, number], vertical: false, h: "left" as const, v: "center" as const, visible: true, ...extra });
  const sch = sheet({
    symbols: [{ id: "R1", fields: [field("R1", "Reference"), field("R1", "Value"), field("R1", "MPN", { visible: false }), field("R1", "Supplier", { visible: false })] }],
    labels: [
      { id: "lbl_1", net: "SIG", at: [0, 0], scope: "local", shape: null },
      { id: "lbl_2", net: "D[0..3]", at: [0, 0], scope: "local", shape: null },
      { id: "lbl_3", net: "VCC", at: [0, 0], scope: "global", shape: "input", spin: "left" },
    ],
    wires: [
      { id: "wire_1", net: "SIG", pins: [], pts: [[0, 0], [10, 0]], bus: false },
      { id: "wire_2", net: "D", pins: [], pts: [[0, 50], [10, 50]], bus: true },
    ],
    junctions: [{ id: "jct_1", at: [5, 0] }],
    sheets: [{ id: "sh", name: "S", file: "s.kicad_sch", at: [0, 0], size: [10, 10], pins: [{ id: "shp_1", name: "IN", shape: "input", at: [0, 5] }] }],
  });
  const plan = planGlobalEdit(sch, new Set(), { ...emptyEdit(), textSize: 2_000_000, visible: true, orientation: "up", lineWidth: 300_000, junctionSize: 1_000_000, fieldNameFilter: "M*" });
  const of = (verb: string) => plan.filter((c) => c.op === "sch_edit" && c.verb === verb) as Array<Record<string, unknown>>;
  assert.deepEqual(of("edit_field").map((c) => [c.id, c.size_nm, c.visible]), [["fld:R1:Reference", 2_000_000, undefined], ["fld:R1:Value", 2_000_000, undefined], ["fld:R1:MPN", 2_000_000, true]], "Supplier is not matched by M*");
  assert.deepEqual(of("edit_label").map((c) => [c.id, c.spin]), [["lbl_1", "up"], ["lbl_2", "up"], ["lbl_3", "up"]]);
  assert.deepEqual(of("set_stroke").map((c) => [c.ids, c.width_nm ?? c.diameter_nm]), [[["wire_1", "wire_2"], 300_000], [["jct_1"], 1_000_000]]);
  assert.deepEqual(of("edit_sheet_pin").map((c) => [c.id, c.size_nm]), [["shp_1", 2_000_000]]);
  // Buses unticked: the bus, the bus label and nothing on the bus
  const noBus = planGlobalEdit(sch, new Set(), { ...emptyEdit(), kinds: { ...emptyEdit().kinds, buses: false }, orientation: "up", lineWidth: 300_000 });
  assert.deepEqual((noBus.filter((c) => c.op === "sch_edit" && c.verb === "edit_label") as Array<{ id: string }>).map((c) => c.id), ["lbl_1", "lbl_3"]);
  assert.deepEqual((noBus.find((c) => c.op === "sch_edit" && c.verb === "set_stroke") as { ids: string[] }).ids, ["wire_1"]);
});

// ---------------------------------------------------------------------------------------------- UI-LEFT-2 SS14: the dialog's spec for the backend verb

import { globalEditCmd, globalEditSpec, initialForm, initialNetFilter, nextTri, ORIENTATION_SPIN, rememberedFilters, textSizeError } from "./schGlobalEdit";

test("the dialog opens with nothing ticked and every value left unchanged; the spec then says nothing", () => {
  const f = initialForm();
  assert.ok(Object.values(f.scope).every((v) => v === false));
  const spec = globalEditSpec(f, []);
  assert.deepEqual(Object.keys(spec).filter((k) => spec[k] !== false), []);
});

test("a filter counts when its box is ticked and its text is not empty; Selected items only carries the selection", () => {
  const f = { ...initialForm(), referenceOpt: true, reference: "R*", symbolOpt: false, symbol: "Device:*", netOpt: true, net: "", typeOpt: true, type: "power" as const, selectedOnly: true };
  const spec = globalEditSpec(f, ["R1"]);
  assert.equal(spec.reference_filter, "R*");
  assert.equal(spec.symbol_filter, undefined, "box not ticked");
  assert.equal(spec.net_filter, undefined, "empty text (`!m_netFilter->GetValue().IsEmpty()`)");
  assert.equal(spec.type_filter, "power");
  assert.deepEqual([spec.selected_only, spec.selection], [true, ["R1"]]);
});

test("Orientation casts its index to SPIN_STYLE (LEFT, UP, RIGHT, BOTTOM): Right is SPIN_STYLE::LEFT", () => {
  assert.deepEqual(ORIENTATION_SPIN, ["left", "up", "right", "bottom"]);
  assert.equal(globalEditSpec({ ...initialForm(), orientation: 0 }, []).orientation, "left");
  assert.equal(globalEditSpec({ ...initialForm(), orientation: 3 }, []).orientation, "bottom");
});

test("lengths in mm, colours with their box, an unspecified colour sent as none", () => {
  const f = { ...initialForm(), textSize: "2", lineWidth: "0,25", setLineColor: true, lineColor: "#ff0000", setFillColor: true, fillColor: "", bold: true, italic: false };
  const spec = globalEditSpec(f, []);
  assert.equal(spec.text_size_nm, 2_000_000);
  assert.equal(spec.line_width_nm, 250_000);
  assert.deepEqual([spec.set_line_color, spec.line_color], [true, { r: 255, g: 0, b: 0, a: 255 }]);
  assert.deepEqual([spec.set_fill_color, spec.fill_color], [true, undefined], "COLOR4D::UNSPECIFIED");
  assert.deepEqual([spec.bold, spec.italic], [true, false]);
});

test("the text size is checked as UNIT_BINDER::Validate( 1.0, 10000.0, MILS ) says", () => {
  assert.equal(textSizeError("0.01"), "Text size must be at least 0.0254 mm.");
  assert.equal(textSizeError("300"), "Text size must be less than 254 mm.");
  assert.equal(textSizeError(""), null);
  assert.deepEqual(globalEditCmd({ ...initialForm(), textSize: "0.001" }, []), { error: "Text size must be at least 0.0254 mm." });
  const ok = globalEditCmd({ ...initialForm(), textSize: "1.27" }, []);
  assert.ok("cmd" in ok && (ok.cmd as unknown as { verb: string }).verb === "global_edit_text_graphics");
});

test("three-state boxes cycle unchecked, checked, undetermined; the net filter starts on the highlighted net", () => {
  assert.deepEqual([nextTri(false), nextTri(true), nextTri(null)], [true, null, false]);
  assert.deepEqual(initialNetFilter("", "/VCC"), { text: "/VCC", ticked: true });
  assert.deepEqual(initialNetFilter("GND", "/VCC"), { text: "GND", ticked: false });
  rememberedFilters.reference = "U*";
  assert.equal(initialForm().reference, "U*", "the session's filter comes back");
  rememberedFilters.reference = "";
});
