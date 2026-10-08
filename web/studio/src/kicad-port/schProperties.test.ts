import { test } from "node:test";
import assert from "node:assert/strict";
import { colorToHex, graphicsEqual, hexToColor, labelEditCmd, mmToUm, propertiesTarget, sheetEditCmd, strokeEditCmd, strokeView, textEditCmd, UNSPECIFIED_COLOR, umToMm } from "./schProperties";
import type { Schematic } from "../api/types";

const sheet = (over: Partial<Schematic> = {}): Schematic =>
  ({
    lib_symbols: {},
    symbols: [{ id: "R1", lib_id: null, at: [100, 200], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] }],
    power_symbols: [{ id: "#PWR01", lib_id: "power:GND", at: [0, 0], rot: 0, net: "GND", pin: "" }],
    wires: [
      { id: "wire_a", net: "N", pins: [], pts: [[0, 0], [10, 0]], bus: false },
      { id: "wire_bus", net: "D[0..3]", pins: [], pts: [[0, 5], [10, 5]], bus: true, stroke: { width_um: 300, style: "dash", color: { r: 255, g: 0, b: 0, a: 255 } } },
    ],
    no_connects: [{ id: "nc_a", at: [1, 1], pin: "" }],
    labels: [
      { id: "lbl_a", net: "A", at: [5, 5], scope: "local", shape: null },
      { id: "lbl_g", net: "B", at: [6, 6], scope: "global", shape: "input" },
    ],
    texts: [{ id: "txt_a", content: "hello", at: [0, 0], angle: 0, size_um: 1270 }],
    title_block: null,
    bus_entries: [{ id: "bent_a", at: [2, 5], size: [2, 2] }],
    junctions: [
      { id: "jct_a", at: [5, 0] },
      { id: "jct_b", at: [7, 0], look: { diameter_um: 900, color: { r: 0, g: 0, b: 255, a: 255 } } },
    ],
    lines: [{ id: "sln_a", pts: [[0, 9], [9, 9]], width_um: 250, stroke: { style: "dot" } }],
    graphics: [{ id: "shp_a", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } }],
    sheets: [{ id: "sheet_a", name: "Power", file: "power.kicad_sch", at: [0, 0], size: [10, 10], pins: [] }],
    sheet_path: [],
    ...over,
  }) as unknown as Schematic;

test("E opens the dialog of the first selected item, and only when the selection allows it", () => {
  const s = sheet();
  assert.deepEqual(propertiesTarget(s, ["R1"]), { kind: "symbol", id: "R1" });
  assert.deepEqual(propertiesTarget(s, ["lbl_g"]), { kind: "label", id: "lbl_g" });
  assert.deepEqual(propertiesTarget(s, ["txt_a"]), { kind: "text", id: "txt_a" });
  assert.deepEqual(propertiesTarget(s, ["sheet_a"]), { kind: "sheet", id: "sheet_a" });
  assert.deepEqual(propertiesTarget(s, ["shp_a"]), { kind: "graphic", id: "shp_a" });
  // `default: if( selection.Size() > 1 ) return 0;`
  assert.equal(propertiesTarget(s, ["lbl_a", "lbl_g"]), null);
  assert.equal(propertiesTarget(s, ["R1", "txt_a"]), null);
  // a no-connect has none, and this studio's symbol dialog has no power symbol yet
  assert.equal(propertiesTarget(s, ["nc_a"]), null);
  assert.equal(propertiesTarget(s, ["#PWR01"]), null);
  assert.equal(propertiesTarget(s, []), null);
  assert.equal(propertiesTarget(s, ["gone"]), null);
});

test("lines, buses, bus entries and junctions open the stroke dialog together, graphic lines and junctions alone", () => {
  const s = sheet();
  assert.deepEqual(propertiesTarget(s, ["wire_a", "wire_bus", "bent_a", "jct_a"]), { kind: "stroke", ids: ["wire_a", "wire_bus", "bent_a", "jct_a"] });
  assert.deepEqual(propertiesTarget(s, ["jct_a", "jct_b"]), { kind: "stroke", ids: ["jct_a", "jct_b"] });
  assert.deepEqual(propertiesTarget(s, ["sln_a"]), { kind: "stroke", ids: ["sln_a"] });
  // a graphic line does not mix with a wire, and nothing else mixes with a line
  assert.equal(propertiesTarget(s, ["sln_a", "wire_a"]), null);
  assert.equal(propertiesTarget(s, ["wire_a", "lbl_a"]), null);
  // the first item decides: a label first is the default branch, which wants a single item
  assert.equal(propertiesTarget(s, ["lbl_a", "wire_a"]), null);
});

test("a label dialog sends only what changed", () => {
  const local = { id: "lbl_a", net: "A", scope: "local" as const, shape: null };
  const global = { id: "lbl_g", net: "B", scope: "global" as const, shape: "input" as const };
  assert.equal(labelEditCmd(local, "right", { text: "A", shape: "input", spin: "right" }), null, "nothing changed: nothing to send");
  assert.deepEqual(labelEditCmd(local, "right", { text: " VCC ", shape: "output", spin: "up" }), { op: "sch_edit", verb: "edit_label", id: "lbl_a", text: "VCC", spin: "up" }, "a local label has no shape to send");
  assert.deepEqual(labelEditCmd(global, "left", { text: "B", shape: "bidirectional", spin: "left" }), { op: "sch_edit", verb: "edit_label", id: "lbl_g", shape: "bidirectional" });
});

test("a text dialog sends only what changed, and keeps an angle the dialog cannot show", () => {
  const t = { id: "txt_a", content: "hello", size_um: 1270, angle: 0 };
  assert.equal(textEditCmd(t, { content: "hello", sizeUm: 1270, vertical: false }), null);
  assert.deepEqual(textEditCmd(t, { content: "bye\nnow", sizeUm: 2540, vertical: true }), { op: "sch_edit", verb: "edit_text", id: "txt_a", text: "bye\nnow", size_um: 2540, angle: 90_000 });
  assert.deepEqual(textEditCmd({ ...t, angle: 90 }, { content: "hello", sizeUm: 1270, vertical: false }), { op: "sch_edit", verb: "edit_text", id: "txt_a", angle: 0 });
  assert.equal(textEditCmd({ ...t, angle: 180 }, { content: "hello", sizeUm: 1270, vertical: false }), null, "a text at 180 degrees that is still 'horizontal' is left at 180");
  assert.equal(textEditCmd(t, { content: "hello", sizeUm: null, vertical: false }), null, "a size that is not a number is not sent");
});

test("a sheet dialog is one command: the name and file, and the page number", () => {
  const sh = { id: "sheet_a", name: "Power", file: "power.kicad_sch", page: "" };
  assert.equal(sheetEditCmd(sh, { name: "Power", file: "power.kicad_sch", page: "" }), null);
  assert.equal(sheetEditCmd(sh, { name: "Power", file: "power", page: "" }), null, "the file typed without its extension is the same file");
  assert.deepEqual(sheetEditCmd(sh, { name: "PSU", file: "power.kicad_sch", page: "" }), { op: "sch_edit", verb: "edit_sheet", id: "sheet_a", name: "PSU" });
  assert.deepEqual(sheetEditCmd(sh, { name: "Power", file: "supply", page: "" }), { op: "sch_edit", verb: "edit_sheet", id: "sheet_a", file: "supply" });
  assert.deepEqual(sheetEditCmd(sh, { name: "Power", file: "power.kicad_sch", page: "3" }), { op: "set_sheet_page", sheet: "sheet_a", page: "3" });
  assert.deepEqual(sheetEditCmd(sh, { name: "PSU", file: "power.kicad_sch", page: "3" }), {
    op: "batch",
    cmds: [
      { op: "sch_edit", verb: "edit_sheet", id: "sheet_a", name: "PSU" },
      { op: "set_sheet_page", sheet: "sheet_a", page: "3" },
    ],
  });
});

test("the stroke dialog shows what the items share, and mixed where they differ", () => {
  const s = sheet();
  const one = strokeView(s, ["wire_bus"]);
  assert.deepEqual([one.hasStroke, one.hasJunction], [true, false]);
  assert.deepEqual(one.widthUm, { mixed: false, value: 300 });
  assert.deepEqual(one.style, { mixed: false, value: "dash" });
  assert.deepEqual(one.color, { mixed: false, value: { r: 255, g: 0, b: 0, a: 255 } });
  const two = strokeView(s, ["wire_a", "wire_bus", "bent_a"]);
  assert.deepEqual(two.widthUm, { mixed: true });
  assert.deepEqual(two.style, { mixed: true });
  // a graphic line's width is its own field
  assert.deepEqual(strokeView(s, ["sln_a"]).widthUm, { mixed: false, value: 250 });
  assert.deepEqual(strokeView(s, ["sln_a"]).style, { mixed: false, value: "dot" });
  const junctions = strokeView(s, ["jct_a", "jct_b"]);
  assert.deepEqual([junctions.hasStroke, junctions.hasJunction], [false, true]);
  assert.deepEqual(junctions.diameterUm, { mixed: true });
  assert.deepEqual(strokeView(s, ["jct_b"]).diameterUm, { mixed: false, value: 900 });
  // a selection of both: the width is the lines', the diameter the junctions'
  const both = strokeView(s, ["wire_bus", "jct_b"]);
  assert.deepEqual([both.hasStroke, both.hasJunction], [true, true]);
  assert.deepEqual(both.widthUm, { mixed: false, value: 300 });
  assert.deepEqual(both.diameterUm, { mixed: false, value: 900 });
});

test("the stroke dialog sends only the fields the user touched", () => {
  assert.equal(strokeEditCmd(["wire_a"], {}), null);
  assert.deepEqual(strokeEditCmd(["wire_a", "jct_a"], { widthUm: 300, diameterUm: 900 }), { op: "sch_edit", verb: "set_stroke", ids: ["wire_a", "jct_a"], width_um: 300, diameter_um: 900 });
  assert.deepEqual(strokeEditCmd(["wire_a"], { widthUm: -5, style: "dash_dot" }), { op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], width_um: 0, style: "dash_dot" });
  assert.deepEqual(strokeEditCmd(["wire_a"], { color: { r: 0, g: 0, b: 0, a: 0 } }), { op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], color: UNSPECIFIED_COLOR });
});

test("colours and millimetres convert both ways", () => {
  assert.equal(colorToHex({ r: 255, g: 128, b: 0, a: 255 }), "#ff8000");
  assert.deepEqual(hexToColor("#0080FF"), { r: 0, g: 128, b: 255, a: 255 });
  assert.equal(hexToColor("red"), null);
  assert.equal(mmToUm("0.25"), 250);
  assert.equal(mmToUm(" 1,27 "), 1270);
  assert.equal(mmToUm(""), null);
  assert.equal(mmToUm("abc"), null);
  assert.equal(umToMm(1270), "1.27");
  assert.equal(umToMm(0), "0");
});

test("a graphic that spells out its defaults is the same graphic as one that leaves them out", () => {
  const rect = { id: "shp_a", shape: { type: "rectangle" as const, start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } };
  assert.ok(graphicsEqual(rect, { ...rect, shape: { ...rect.shape, corner_radius_um: 0 }, width_um: 0, line_style: "default", fill: "none" }));
  assert.ok(!graphicsEqual(rect, { ...rect, width_um: 250 }));
  assert.ok(!graphicsEqual(rect, { ...rect, fill: "background" }));
  const area = { id: "rarea_a", shape: { type: "rule_area" as const, pts: [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }] } };
  assert.ok(graphicsEqual(area, { ...area, shape: { ...area.shape, exclude_from_sim: false, dnp: false } }));
  assert.ok(!graphicsEqual(area, { ...area, shape: { ...area.shape, dnp: true } }));
  const box = { id: "tbox_a", shape: { type: "text_box" as const, start: { x: 0, y: 0 }, end: { x: 10, y: 10 }, text: "hi", size_um: 1270 } };
  assert.ok(graphicsEqual(box, { ...box, shape: { ...box.shape, bold: false, h_align: "left", v_align: "top", angle: 0 } }));
  assert.ok(!graphicsEqual(box, { ...box, shape: { ...box.shape, text: "ho" } }));
  // a colour of every channel zero is "no colour"
  assert.ok(graphicsEqual(box, { ...box, color: { r: 0, g: 0, b: 0, a: 0 } }));
  assert.ok(!graphicsEqual(box, { ...box, color: { r: 255, g: 0, b: 0, a: 255 } }));
});
