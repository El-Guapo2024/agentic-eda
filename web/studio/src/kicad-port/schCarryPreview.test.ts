import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibSymbol, LibrarySymbol, SchField } from "../api/types";
import { carriedFields, carriedPreview, CARRIED_IDS, corneredSymbol, libSymbolOfLibrary, needsServerDrawing, realSymbolBox } from "./schCarryPreview";
import { PREVIEW_AT, type CarriedSymbol } from "./schPlaceTool";
import type { SchPastePreview } from "./schClipboard";

const AT: [number, number] = [PREVIEW_AT[0], PREVIEW_AT[1]];

const resistor: LibSymbol = {
  graphics: [{ kind: "rectangle", start: [-1.016, 2.54], end: [1.016, -2.54], stroke_width: 0.254, fill: "none", unit: 0, body_style: 0 }],
  pins: [
    { number: "1", name: "~", electrical_type: "passive", shape: "line", at: [0, 3.81], angle_deg: 270, length_mm: 1.27, unit: 0, body_style: 0, hidden: false },
    { number: "2", name: "~", electrical_type: "passive", shape: "line", at: [0, -3.81], angle_deg: 90, length_mm: 1.27, unit: 0, body_style: 0, hidden: false },
  ],
};

const symbol = (over: Partial<CarriedSymbol> = {}): CarriedSymbol => ({ kind: "symbol", libId: "Device:R", ref: "R5", prefix: "R", value: "10k", footprint: "", unit: 1, unitCount: 1, rotDeg: 0, mirror: null, turns: [], placeAllUnits: false, oneOnly: false, ...over });

const EMPTY: SchPastePreview = { symbols: [], power_symbols: [], wires: [], labels: [], texts: [], sheets: [], no_connects: [], bus_entries: [], junctions: [], lines: [], graphics: [], groups: [], tables: [], lib_symbols: {} };

test("a carried label, text and directive label are drawn by the page alone, as GET /api/schematic lists the placed item", () => {
  const l = carriedPreview({ kind: "label", scope: "hierarchical", text: "CLK", shape: "input", spin: "left", sizeNm: null, bold: true, italic: false, autoRotate: true }, { libSymbols: {} });
  assert.deepEqual(l.labels, [{ id: CARRIED_IDS.label, net: "CLK", at: AT, scope: "hierarchical", shape: "input", spin: "left", size_nm: 1_270_000, bold: true, italic: false, auto_rotate: true }]);
  // "From the wire" draws with no spin: the painter reads it off the wire it ends on where it is
  assert.equal(carriedPreview({ kind: "label", scope: "local", text: "A", shape: "input", spin: "", sizeNm: 2_540_000, bold: false, italic: false, autoRotate: false }, { libSymbols: {} }).labels[0]!.spin, null);
  const t = carriedPreview({ kind: "text", content: "Note", sizeNm: 1_000_000, vertical: true, h: "right", v: "top" }, { libSymbols: {} });
  assert.deepEqual(t.texts, [{ id: CARRIED_IDS.text, content: "Note", at: AT, angle: 90, size_nm: 1_000_000, vertical: true, h: "right", v: "top" }]);
  const d = carriedPreview({ kind: "directive", netclass: "Power", componentClass: "", shape: "diamond", orientation: 90_000, pinLengthNm: 2_540_000 }, { libSymbols: {} });
  assert.deepEqual(d.graphics[0]!.shape, { type: "directive", at: { x: AT[0], y: AT[1] }, orientation: 90_000, shape: "diamond", pin_length_nm: 2_540_000, netclass: "Power", component_class: "" });
  // the server is asked only for what the page cannot know
  assert.deepEqual(
    [needsServerDrawing({ kind: "label", scope: "local", text: "A", shape: "input", spin: "", sizeNm: null, bold: false, italic: false, autoRotate: false }), needsServerDrawing({ kind: "label", scope: "global", text: "A", shape: "input", spin: "", sizeNm: null, bold: false, italic: false, autoRotate: false })],
    [false, true]
  );
  assert.deepEqual([needsServerDrawing({ kind: "text", content: "plain", sizeNm: 1, vertical: false, h: "left", v: "top" }), needsServerDrawing({ kind: "text", content: "${TITLE}", sizeNm: 1, vertical: false, h: "left", v: "top" })], [false, true]);
  assert.equal(needsServerDrawing(symbol()), true);
});

test("a carried symbol is drawn at once from its library drawing, turned as its turns leave it, with its unit's pins and the flags it was given", () => {
  const p = carriedPreview(symbol({ turns: ["rot_ccw"], attrs: { dnp: true } }), { libSymbols: {}, lib: resistor });
  const s = p.symbols[0]!;
  assert.deepEqual([s.id, s.lib_id, s.at, s.rot, s.mirror, s.value, s.dnp, s.pins.map((x) => x.number)], ["R5", "Device:R", AT, 90, null, "10k", true, ["1", "2"]]);
  assert.equal(s.fields, undefined, "no fields until the server's autoplacer has placed them (the painter's own rule meanwhile)");
  assert.deepEqual(p.lib_symbols["Device:R"], corneredSymbol(resistor, 1), "the chooser's drawing, in the placed frame");
  // the sheet's own drawing of the symbol wins over the chooser's
  const sheetOwn: LibSymbol = { ...resistor, graphics: [] };
  assert.equal(carriedPreview(symbol(), { libSymbols: { "Device:R": sheetOwn }, lib: resistor }).lib_symbols["Device:R"], sheetOwn);
});

test("the server's fields ride with the symbol: as they are for that very state, turned with it about the anchor for a later one", () => {
  const ref: SchField = { name: "Reference", text: "R5", at: [AT[0] + 2_540_000, AT[1] - 1_270_000], vertical: false, h: "left", v: "center", visible: true, id: "fld:R5:Reference" };
  const val: SchField = { name: "Value", text: "10k", at: [AT[0] + 2_540_000, AT[1] + 1_270_000], vertical: false, h: "left", v: "center", visible: true, id: "fld:R5:Value" };
  const was = symbol();
  const server = { item: was, preview: { ...EMPTY, symbols: [{ ...carriedPreview(was, { libSymbols: {}, lib: resistor }).symbols[0]!, fields: [ref, val] }] } };
  assert.deepEqual(carriedPreview(was, { libSymbols: {}, lib: resistor, server }).symbols[0]!.fields, [ref, val]);
  // a quarter turn counter-clockwise (`SetOrientation( SYM_ROTATE_COUNTERCLOCKWISE )`: x' = y, y' = -x) and a new value
  const turned = carriedPreview(symbol({ turns: ["rot_ccw"], value: "4k7" }), { libSymbols: {}, lib: resistor, server }).symbols[0]!.fields!;
  assert.deepEqual(turned.map((f) => [f.name, f.at[0] - AT[0], f.at[1] - AT[1], f.text]), [
    ["Reference", -1_270_000, -2_540_000, "R5"],
    ["Value", 1_270_000, -2_540_000, "4k7"],
  ]);
  // a mirror across the X axis after it (`SYM_MIRROR_X` negates y): undone by the inverse of the old orientation, then the new one applied
  assert.deepEqual(carriedFields([val], { rotDeg: 90, mirror: null }, { rotDeg: 90, mirror: "x" }, {})[0]!.at, [AT[0] + 2_540_000, AT[1] - 1_270_000]);
});

test("a carried power symbol takes the id and fields of the server's drawing, which numbers it", () => {
  const gnd: LibSymbol = { graphics: [], pins: [{ number: "1", name: "GND", electrical_type: "power_in", shape: "line", at: [0, 0], angle_deg: 270, length_mm: 0, unit: 0, body_style: 0, hidden: true }] };
  const item = { kind: "power" as const, libId: "power:GND", net: "GND", rotMillideg: 0 };
  assert.equal(carriedPreview(item, { libSymbols: {}, lib: gnd }).power_symbols[0]!.id, CARRIED_IDS.power);
  const value: SchField = { name: "Value", text: "GND", at: [AT[0], AT[1] + 5_080_000], vertical: false, h: "center", v: "center", visible: true, id: "fld:#PWR03:Value" };
  const server = { item, preview: { ...EMPTY, power_symbols: [{ id: "#PWR03", lib_id: "power:GND", at: AT, rot: 0, net: "GND", pin: { number: "1", name: "GND", kind: "power" as const }, fields: [value] }] } };
  const ps = carriedPreview({ ...item, rotMillideg: 180_000, net: "AGND" }, { libSymbols: {}, lib: gnd, server }).power_symbols[0]!;
  assert.deepEqual([ps.id, ps.rot, ps.net, ps.fields![0]!.at, ps.fields![0]!.text], ["#PWR03", 180, "AGND", [AT[0], AT[1] - 5_080_000], "AGND"]);
});

test("a library symbol the chooser loaded is drawn as a placed one's: private texts left out, a bezier as its polyline, pins and flags kept", () => {
  const lib = {
    lib_id: "X:Y",
    graphics: [
      { kind: "rectangle", unit: 0, body_style: 1, start: { x: -1, y: 1 }, end: { x: 1, y: -1 }, stroke_mm: 0.254, fill: "background" },
      { kind: "text", unit: 0, body_style: 1, text: "shown", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 },
      { kind: "text", unit: 0, body_style: 1, text: "editor only", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27, private: true },
      { kind: "bezier", unit: 0, body_style: 1, start: { x: 0, y: 0 }, c1: { x: 1, y: 2 }, c2: { x: 2, y: 2 }, end: { x: 3, y: 0 }, stroke_mm: 0.1, fill: "none" },
      { kind: "text_box", unit: 0, body_style: 1, text: "${REFERENCE}", start: { x: -2, y: 2 }, end: { x: 2, y: -2 }, angle_deg: 0, size_mm: 1.27, h_align: "left", v_align: "top", stroke_mm: 0.1, fill: "none" },
    ],
    pins: [{ number: "1", name: "A", electrical_type: "input", shape: "inverted", at: { x: -3.81, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null }],
    pin_names_hidden: false,
    pin_numbers_hidden: true,
    pin_name_offset_mm: 0.508,
    unit_count: 2,
    has_alternate_body_style: true,
  } as unknown as LibrarySymbol;
  const s = libSymbolOfLibrary(lib);
  // a text box is its border (a rectangle) and its text, numbered among the symbol's boxes, the margin resolved (`GetLegacyTextMargin`: 0.8 of the size)
  assert.deepEqual(s.graphics.map((g) => g.kind), ["rectangle", "text", "polyline", "rectangle", "text_box"]);
  const box = s.graphics[4] as Extract<LibSymbol["graphics"][number], { kind: "text_box" }>;
  assert.deepEqual([box.index, box.text, box.margin_mm, box.start, box.end], [0, "${REFERENCE}", 1.016, [-2, 2], [2, -2]]);
  assert.ok((s.graphics[2] as { pts: unknown[] }).pts.length > 4, "the curve is cut into segments");
  assert.deepEqual(s.pins[0], { number: "1", name: "A", electrical_type: "input", shape: "inverted", at: [-3.81, 0], angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false });
  assert.deepEqual([s.pin_numbers_hidden, s.pin_name_offset, s.unit_count, s.body_style_count], [true, 0.508, 2, 2]);
});

test("a symbol the chooser loaded is drawn in the frame a placed one is (`corner_symbol`): its box's top-left corner at the point it is put down by", () => {
  // Device:R: a pin above and a pin below; the box is their stub tips (2.54 up, 2.54 down) and the body's sides, snapped out to the 1.27 mm grid
  const box = realSymbolBox(resistor, 1);
  assert.deepEqual([box.x0, box.y0, box.x1, box.y1].map((v) => Math.round(v * 1000)), [-1270, -2540, 1270, 2540]);
  const c = corneredSymbol(resistor, 1);
  assert.deepEqual(c.pins.map((p) => p.at.map((v) => Math.round(v * 1000))), [
    [1270, 1270],
    [1270, -6350],
  ]);
  const r = c.graphics[0] as { start: [number, number]; end: [number, number] };
  assert.deepEqual([...r.start, ...r.end].map((v) => Math.round(v * 1000)), [254, 0, 2286, -5080]);
  // the page draws a chooser's symbol so; the sheet's or the server's drawing (already in that frame) wins once there
  assert.deepEqual(carriedPreview(symbol(), { libSymbols: {}, lib: resistor }).lib_symbols["Device:R"]!.pins[0]!.at.map((v) => Math.round(v * 1000)), [1270, 1270]);
});
