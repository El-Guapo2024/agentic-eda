import { test } from "node:test";
import assert from "node:assert/strict";
import { colorToHex, graphicsEqual, hexToColor, labelEditCmd, labelFieldDisplayName, newLabelField, ownLabelFields, mmToNm, powerEditCmd, propertiesTarget, sheetEditCmd, strokeEditCmd, strokeView, textEditCmd, UNSPECIFIED_COLOR, nmToMm } from "./schProperties";
import type { Schematic } from "../api/types";

const sheet = (over: Partial<Schematic> = {}): Schematic =>
  ({
    lib_symbols: {},
    symbols: [{ id: "R1", lib_id: null, at: [100_000, 200_000], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] }],
    power_symbols: [{ id: "#PWR01", lib_id: "power:GND", at: [0, 0], rot: 0, net: "GND", pin: "" }],
    wires: [
      { id: "wire_a", net: "N", pins: [], pts: [[0, 0], [10_000, 0]], bus: false },
      { id: "wire_bus", net: "D[0..3]", pins: [], pts: [[0, 5000], [10_000, 5000]], bus: true, stroke: { width_nm: 300_000, style: "dash", color: { r: 255, g: 0, b: 0, a: 255 } } },
    ],
    no_connects: [{ id: "nc_a", at: [1, 1], pin: "" }],
    labels: [
      { id: "lbl_a", net: "A", at: [5000, 5000], scope: "local", shape: null },
      { id: "lbl_g", net: "B", at: [6000, 6000], scope: "global", shape: "input" },
    ],
    texts: [{ id: "txt_a", content: "hello", at: [0, 0], angle: 0, size_nm: 1_270_000 }],
    title_block: null,
    bus_entries: [{ id: "bent_a", at: [2000, 5000], size: [2000, 2000] }],
    junctions: [
      { id: "jct_a", at: [5000, 0] },
      { id: "jct_b", at: [7000, 0], look: { diameter_nm: 900, color: { r: 0, g: 0, b: 255, a: 255 } } },
    ],
    lines: [{ id: "sln_a", pts: [[0, 9000], [9000, 9000]], width_nm: 250_000, stroke: { style: "dot" } }],
    graphics: [{ id: "shp_a", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10_000, y: 10_000 } } }],
    sheets: [{ id: "sheet_a", name: "Power", file: "power.kicad_sch", at: [0, 0], size: [10_000, 10_000], pins: [] }],
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
  // a no-connect has none; a power symbol is a symbol and has the Symbol Properties dialog
  assert.equal(propertiesTarget(s, ["nc_a"]), null);
  assert.deepEqual(propertiesTarget(s, ["#PWR01"]), { kind: "power", id: "#PWR01" });
  assert.equal(propertiesTarget(s, ["#PWR01", "R1"]), null, "one item only");
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
  const t = { id: "txt_a", content: "hello", size_nm: 1_270_000, angle: 0 };
  assert.equal(textEditCmd(t, { content: "hello", sizeNm: 1_270_000, vertical: false }), null);
  assert.deepEqual(textEditCmd(t, { content: "bye\nnow", sizeNm: 2_540_000, vertical: true }), { op: "sch_edit", verb: "edit_text", id: "txt_a", text: "bye\nnow", size_nm: 2_540_000, angle: 90_000 });
  assert.deepEqual(textEditCmd({ ...t, angle: 90 }, { content: "hello", sizeNm: 1_270_000, vertical: false }), { op: "sch_edit", verb: "edit_text", id: "txt_a", angle: 0 });
  assert.equal(textEditCmd({ ...t, angle: 180 }, { content: "hello", sizeNm: 1_270_000, vertical: false }), null, "a text at 180 degrees that is still 'horizontal' is left at 180");
  assert.equal(textEditCmd(t, { content: "hello", sizeNm: null, vertical: false }), null, "a size that is not a number is not sent");
});

test("a text dialog sends the justification when it differs from how the text lies (left and bottom for a text that was never justified)", () => {
  const t = { id: "txt_a", content: "hello", size_nm: 1_270_000, angle: 0 };
  // nothing picked differently
  assert.equal(textEditCmd(t, { content: "hello", sizeNm: 1_270_000, vertical: false, h: "left", v: "bottom" }), null);
  assert.deepEqual(textEditCmd(t, { content: "hello", sizeNm: 1_270_000, vertical: false, h: "center", v: "bottom" }), { op: "sch_edit", verb: "edit_text", id: "txt_a", h: "center" });
  assert.deepEqual(textEditCmd(t, { content: "hello", sizeNm: 1_270_000, vertical: false, h: "right", v: "top" }), { op: "sch_edit", verb: "edit_text", id: "txt_a", h: "right", v: "top" });
  // a text the backend says is centred and vertical
  const centred = { ...t, angle: 90, vertical: true, h: "center" as const, v: "center" as const };
  assert.equal(textEditCmd(centred, { content: "hello", sizeNm: 1_270_000, vertical: true, h: "center", v: "center" }), null);
  assert.deepEqual(textEditCmd(centred, { content: "hello", sizeNm: 1_270_000, vertical: false, h: "center", v: "center" }), { op: "sch_edit", verb: "edit_text", id: "txt_a", angle: 0 });
  // a legacy angle of 270 is a vertical text (KeepUpright)
  assert.equal(textEditCmd({ ...t, angle: 270 }, { content: "hello", sizeNm: 1_270_000, vertical: true }), null);
});

test("a label dialog sends the Auto check when it changed, and the spin only while Auto is off", () => {
  const global = { id: "lbl_g", net: "B", scope: "global" as const, shape: "input" as const };
  const local = { id: "lbl_a", net: "A", scope: "local" as const, shape: null };
  // Auto turned on: the flag is sent, the spin picked is not (the connection decides)
  assert.deepEqual(labelEditCmd(global, "left", { text: "B", shape: "input", spin: "up", autoRotate: true }), { op: "sch_edit", verb: "edit_label", id: "lbl_g", auto_rotate: true });
  // Auto turned off with a spin picked: both
  assert.deepEqual(labelEditCmd({ ...global, auto_rotate: true }, "left", { text: "B", shape: "input", spin: "up", autoRotate: false }), { op: "sch_edit", verb: "edit_label", id: "lbl_g", auto_rotate: false, spin: "up" });
  // Auto already on and left on: nothing to send, whatever the radio shows
  assert.equal(labelEditCmd({ ...global, auto_rotate: true }, "left", { text: "B", shape: "input", spin: "up", autoRotate: true }), null);
  // a local label has no such check
  assert.equal(labelEditCmd(local, "right", { text: "A", shape: "input", spin: "right", autoRotate: true }), null);
  assert.deepEqual(labelEditCmd(local, "right", { text: "A", shape: "input", spin: "up", autoRotate: true }), { op: "sch_edit", verb: "edit_label", id: "lbl_a", spin: "up" });
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
  // the dialog sets the page of the sheet path it shows (the sheet in view's path, then the sheet)
  assert.deepEqual(sheetEditCmd(sh, { name: "Power", file: "power.kicad_sch", page: "4" }, ["outer", "sheet_a"]), { op: "set_sheet_page", sheet: "sheet_a", page: "4", path: ["outer", "sheet_a"] });
});

test("the stroke dialog shows what the items share, and mixed where they differ", () => {
  const s = sheet();
  const one = strokeView(s, ["wire_bus"]);
  assert.deepEqual([one.hasStroke, one.hasJunction], [true, false]);
  assert.deepEqual(one.widthNm, { mixed: false, value: 300_000 });
  assert.deepEqual(one.style, { mixed: false, value: "dash" });
  assert.deepEqual(one.color, { mixed: false, value: { r: 255, g: 0, b: 0, a: 255 } });
  const two = strokeView(s, ["wire_a", "wire_bus", "bent_a"]);
  assert.deepEqual(two.widthNm, { mixed: true });
  assert.deepEqual(two.style, { mixed: true });
  // a graphic line's width is its own field
  assert.deepEqual(strokeView(s, ["sln_a"]).widthNm, { mixed: false, value: 250_000 });
  assert.deepEqual(strokeView(s, ["sln_a"]).style, { mixed: false, value: "dot" });
  const junctions = strokeView(s, ["jct_a", "jct_b"]);
  assert.deepEqual([junctions.hasStroke, junctions.hasJunction], [false, true]);
  assert.deepEqual(junctions.diameterNm, { mixed: true });
  assert.deepEqual(strokeView(s, ["jct_b"]).diameterNm, { mixed: false, value: 900 });
  // a selection of both: the width is the lines', the diameter the junctions'
  const both = strokeView(s, ["wire_bus", "jct_b"]);
  assert.deepEqual([both.hasStroke, both.hasJunction], [true, true]);
  assert.deepEqual(both.widthNm, { mixed: false, value: 300_000 });
  assert.deepEqual(both.diameterNm, { mixed: false, value: 900 });
});

test("the stroke dialog sends only the fields the user touched", () => {
  assert.equal(strokeEditCmd(["wire_a"], {}), null);
  assert.deepEqual(strokeEditCmd(["wire_a", "jct_a"], { widthNm: 300_000, diameterNm: 900 }), { op: "sch_edit", verb: "set_stroke", ids: ["wire_a", "jct_a"], width_nm: 300_000, diameter_nm: 900 });
  assert.deepEqual(strokeEditCmd(["wire_a"], { widthNm: -5000, style: "dash_dot" }), { op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], width_nm: 0, style: "dash_dot" });
  assert.deepEqual(strokeEditCmd(["wire_a"], { color: { r: 0, g: 0, b: 0, a: 0 } }), { op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], color: UNSPECIFIED_COLOR });
});

test("colours and millimetres convert both ways", () => {
  assert.equal(colorToHex({ r: 255, g: 128, b: 0, a: 255 }), "#ff8000");
  assert.deepEqual(hexToColor("#0080FF"), { r: 0, g: 128, b: 255, a: 255 });
  assert.equal(hexToColor("red"), null);
  assert.equal(mmToNm("0.25"), 250_000);
  assert.equal(mmToNm(" 1,27 "), 1_270_000);
  assert.equal(mmToNm(""), null);
  assert.equal(mmToNm("abc"), null);
  assert.equal(nmToMm(1_270_000), "1.27");
  assert.equal(nmToMm(0), "0");
});

test("a graphic that spells out its defaults is the same graphic as one that leaves them out", () => {
  const rect = { id: "shp_a", shape: { type: "rectangle" as const, start: { x: 0, y: 0 }, end: { x: 10_000, y: 10_000 } } };
  assert.ok(graphicsEqual(rect, { ...rect, shape: { ...rect.shape, corner_radius_nm: 0 }, width_nm: 0, line_style: "default", fill: "none" }));
  assert.ok(!graphicsEqual(rect, { ...rect, width_nm: 250_000 }));
  assert.ok(!graphicsEqual(rect, { ...rect, fill: "background" }));
  const area = { id: "rarea_a", shape: { type: "rule_area" as const, pts: [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }] } };
  assert.ok(graphicsEqual(area, { ...area, shape: { ...area.shape, exclude_from_sim: false, dnp: false } }));
  assert.ok(!graphicsEqual(area, { ...area, shape: { ...area.shape, dnp: true } }));
  const box = { id: "tbox_a", shape: { type: "text_box" as const, start: { x: 0, y: 0 }, end: { x: 10_000, y: 10_000 }, text: "hi", size_nm: 1_270_000 } };
  assert.ok(graphicsEqual(box, { ...box, shape: { ...box.shape, bold: false, h_align: "left", v_align: "top", angle: 0 } }));
  assert.ok(!graphicsEqual(box, { ...box, shape: { ...box.shape, text: "ho" } }));
  // a colour of every channel zero is "no colour"
  assert.ok(graphicsEqual(box, { ...box, color: { r: 0, g: 0, b: 0, a: 0 } }));
  assert.ok(!graphicsEqual(box, { ...box, color: { r: 255, g: 0, b: 0, a: 255 } }));
});

test("a power symbol's dialog sends the new Value and the Show checks that changed, as one command", () => {
  const power = { id: "#PWR01", net: "GND", fields: [{ name: "Reference", visible: false }, { name: "Value", visible: true }] };
  const same = { net: "GND", showReference: false, showValue: true };
  assert.equal(powerEditCmd(power, same), null, "nothing changed: nothing to send");
  assert.deepEqual(powerEditCmd(power, { ...same, net: " AGND " }), { op: "sch_edit", verb: "edit_field", id: "fld:#PWR01:Value", text: "AGND" });
  assert.deepEqual(powerEditCmd(power, { ...same, showValue: false }), { op: "sch_edit", verb: "edit_field", id: "fld:#PWR01:Value", visible: false });
  assert.deepEqual(powerEditCmd(power, { ...same, showReference: true }), { op: "sch_edit", verb: "edit_field", id: "fld:#PWR01:Reference", visible: true });
  // the text and the Show check of the Value are one edit; the Reference is another, and two edits are one command
  assert.deepEqual(powerEditCmd(power, { net: "VCC", showReference: true, showValue: false }), {
    op: "batch",
    cmds: [
      { op: "sch_edit", verb: "edit_field", id: "fld:#PWR01:Value", text: "VCC", visible: false },
      { op: "sch_edit", verb: "edit_field", id: "fld:#PWR01:Reference", visible: true },
    ],
  });
  // a backend that sent no fields has no Show to change
  assert.equal(powerEditCmd({ id: "#PWR01", net: "GND" }, { net: "GND", showReference: true, showValue: true }), null);
});

test("Label Properties' grid of a label's own fields: OnAddField's name and look, the net class stored as Netclass, sent only when it changed", () => {
  // OnAddField: a net class first, "Field" once the label has a field that is not one; the look of the last row, else shown and italic
  assert.deepEqual(newLabelField([], null), { name: "Netclass", text: "", visible: true, italic: true });
  assert.deepEqual(newLabelField([], false), { name: "Netclass", text: "", visible: false, italic: false }, "after a global label's Sheet References");
  assert.deepEqual(newLabelField([{ name: "Netclass", text: "Power", visible: true, italic: true }], null).name, "Netclass");
  assert.deepEqual(newLabelField([{ name: "Note", text: "x", visible: false, italic: false }], null), { name: "Field", text: "", visible: false, italic: false });
  assert.equal(labelFieldDisplayName("Netclass"), "Net Class");
  assert.equal(labelFieldDisplayName("Intersheetrefs"), "Sheet References");
  const isr = { name: "Intersheetrefs", text: "${INTERSHEET_REFS}", shown: "[2]", visible: true, italic: false, intersheet_refs: true, at: [0, 0] as [number, number], vertical: false, h: "left" as const, size_nm: 1_270_000 };
  const nc = { ...isr, name: "Netclass", text: "Power", shown: "Power", italic: true, intersheet_refs: false };
  const global = { id: "lbl_g", net: "G", scope: "global" as const, shape: "input" as const, fields: [isr, nc] };
  assert.deepEqual(ownLabelFields(global), [{ name: "Netclass", text: "Power", visible: true, italic: true }], "the Sheet References are not the label's own");
  // the grid shows "Net Class": the same rows are no change, an edited one is sent stored as Netclass
  const shown = [{ name: "Net Class", text: "Power", visible: true, italic: true }];
  assert.equal(labelEditCmd(global, "right", { text: "G", shape: "input", spin: "right", fields: shown }), null);
  assert.deepEqual(labelEditCmd(global, "right", { text: "G", shape: "input", spin: "right", fields: [{ ...shown[0]!, text: "HV" }] }), { op: "sch_edit", verb: "edit_label", id: "lbl_g", fields: [{ name: "Netclass", text: "HV", visible: true, italic: true }] });
});
