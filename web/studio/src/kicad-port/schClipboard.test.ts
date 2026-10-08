import { test } from "node:test";
import assert from "node:assert/strict";
import type { Schematic } from "../api/types";
import { newIdsAfter, pasteOffset, PASTE_SPECIAL_OPTIONS, previewIds, shiftPreview, shiftShape, withPaste, type SchPastePreview } from "./schClipboard";

const sym = (id: string, at: [number, number], unit = 1) => ({ id, lib_id: "Device:R", at, rot: 0, mirror: null, unit, body_style: 0, value: "10k", mpn: null, package: null, footprint: null, datasheet: null, pins: [] });

function sheet(over: Partial<Schematic> = {}): Schematic {
  return {
    lib_symbols: { "Device:C": { graphics: [], pins: [] } },
    symbols: [sym("R1", [0, 0])],
    power_symbols: [],
    wires: [{ id: "wire_a", net: "N", pins: [], pts: [[0, 0], [1000, 0]], bus: false }],
    no_connects: [],
    labels: [],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [],
    sheet_path: [],
    ...over,
  };
}

function preview(): SchPastePreview {
  return {
    symbols: [sym("R2", [100, 200])],
    power_symbols: [{ id: "#PWR02", lib_id: "power:GND", at: [300, 400], rot: 0, net: "GND", pin: { number: "1", name: null, kind: "ground" } }],
    wires: [{ id: "wire_b", net: "", pins: [], pts: [[10, 20], [30, 20], [30, 50]], bus: false }],
    labels: [{ id: "lbl_b", net: "SDA", at: [5, 6], scope: "local", shape: null }],
    texts: [{ id: "txt_b", content: "n", at: [7, 8], angle: 0, size_um: 1270 }],
    no_connects: [{ id: "nc_b", at: [9, 9] }],
    bus_entries: [{ id: "bent_b", at: [1, 1], size: [2540, 2540] }],
    junctions: [{ id: "jct_b", at: [2, 2] }],
    lines: [{ id: "sln_b", pts: [[0, 0], [5, 5]], width_um: 0 }],
    graphics: [{ id: "shp_b", shape: { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10, y: 10 } } }],
    lib_symbols: { "Device:R": { graphics: [], pins: [] }, "Device:C": { graphics: [{ kind: "text", content: "other", at: [0, 0], angle_deg: 0, size_mm: 1, unit: 0, body_style: 0 }], pins: [] } },
  };
}

test("pasteOffset: the shift that lands the anchor on the cursor", () => {
  assert.deepEqual(pasteOffset([100_330, 50_800], [102_870, 48_260]), [2_540, -2_540]);
  assert.deepEqual(pasteOffset([5, 5], [5, 5]), [0, 0]);
});

test("shiftPreview moves every kind of item and nothing else", () => {
  const p = shiftPreview(preview(), 1000, -500);
  assert.deepEqual(p.symbols[0]!.at, [1100, -300]);
  assert.deepEqual(p.power_symbols[0]!.at, [1300, -100]);
  assert.deepEqual(p.wires[0]!.pts, [[1010, -480], [1030, -480], [1030, -450]]);
  assert.deepEqual(p.labels[0]!.at, [1005, -494]);
  assert.deepEqual(p.texts[0]!.at, [1007, -492]);
  assert.deepEqual(p.no_connects[0]!.at, [1009, -491]);
  assert.deepEqual(p.bus_entries[0]!.at, [1001, -499]);
  assert.deepEqual(p.bus_entries[0]!.size, [2540, 2540], "a size is not a place");
  assert.deepEqual(p.junctions[0]!.at, [1002, -498]);
  assert.deepEqual(p.lines[0]!.pts, [[1000, -500], [1005, -495]]);
  assert.deepEqual(p.graphics[0]!.shape, { type: "rectangle", start: { x: 1000, y: -500 }, end: { x: 1010, y: -490 } });
  assert.deepEqual(preview().symbols[0]!.at, [100, 200], "the original is left alone");
});

test("shiftShape knows every shape's points", () => {
  const at = { x: 1, y: 2 };
  assert.deepEqual(shiftShape({ type: "circle", center: at, radius_um: 5 }, 10, 10), { type: "circle", center: { x: 11, y: 12 }, radius_um: 5 });
  assert.deepEqual(shiftShape({ type: "arc", start: at, mid: at, end: at }, 1, 1), { type: "arc", start: { x: 2, y: 3 }, mid: { x: 2, y: 3 }, end: { x: 2, y: 3 } });
  assert.deepEqual(shiftShape({ type: "bezier", start: at, c1: at, c2: at, end: at }, 1, 1), { type: "bezier", start: { x: 2, y: 3 }, c1: { x: 2, y: 3 }, c2: { x: 2, y: 3 }, end: { x: 2, y: 3 } });
  assert.deepEqual(shiftShape({ type: "polygon", pts: [at, at] }, 1, 1), { type: "polygon", pts: [{ x: 2, y: 3 }, { x: 2, y: 3 }] });
  assert.deepEqual(shiftShape({ type: "rule_area", pts: [at], dnp: true }, 1, 1), { type: "rule_area", pts: [{ x: 2, y: 3 }], dnp: true });
  assert.deepEqual(shiftShape({ type: "directive", at, pin_length_um: 2540, netclass: "HV" }, 1, 1), { type: "directive", at: { x: 2, y: 3 }, pin_length_um: 2540, netclass: "HV" });
  assert.deepEqual(shiftShape({ type: "text_box", start: at, end: at, text: "t", size_um: 1270 }, 1, 1), { type: "text_box", start: { x: 2, y: 3 }, end: { x: 2, y: 3 }, text: "t", size_um: 1270 });
});

test("withPaste draws the paste on the sheet: new items after the old, library symbols the sheet has stay as they are", () => {
  const sch = sheet();
  const drawn = withPaste(sch, preview(), 1000, 0);
  assert.deepEqual(drawn.symbols.map((s) => s.id), ["R1", "R2"]);
  assert.deepEqual(drawn.symbols[1]!.at, [1100, 200]);
  assert.equal(drawn.wires.length, 2);
  assert.equal(drawn.junctions!.length, 1);
  assert.equal(drawn.graphics!.length, 1);
  assert.deepEqual(Object.keys(drawn.lib_symbols).sort(), ["Device:C", "Device:R"]);
  assert.equal(drawn.lib_symbols["Device:C"]!.graphics.length, 0, "the sheet's own Device:C wins");
  assert.equal(sch.symbols.length, 1, "the sheet itself is not changed");
});

test("previewIds lists every pasted item, for the canvas to draw as selected", () => {
  assert.deepEqual(previewIds(preview()).sort(), ["#PWR02", "R2", "bent_b", "jct_b", "lbl_b", "nc_b", "shp_b", "sln_b", "txt_b", "wire_b"].sort());
});

test("newIdsAfter: what the paste made, and the reference a unit joined", () => {
  const before = sheet({ symbols: [sym("U1", [0, 0], 1)] });
  const after = sheet({
    symbols: [sym("U1", [0, 0], 1), sym("U1", [500, 0], 2), sym("R7", [900, 0])],
    wires: [{ id: "wire_a", net: "N", pins: [], pts: [[0, 0], [1000, 0]], bus: false }, { id: "wire_new", net: "N", pins: [], pts: [[0, 0], [0, 1000]], bus: false }],
    labels: [{ id: "lbl_new", net: "X", at: [0, 0], scope: "local", shape: null }],
  });
  assert.deepEqual(newIdsAfter(before, after).sort(), ["R7", "U1", "lbl_new", "wire_new"].sort(), "unit 2 of U1 is new, so the reference U1 is selected");
  const same = newIdsAfter(after, after);
  assert.deepEqual(same, [], "nothing new when nothing changed");
});

test("Paste Special offers KiCad's three options in its order", () => {
  assert.deepEqual(PASTE_SPECIAL_OPTIONS.map((o) => o.mode), ["unique", "keep", "remove"]);
  assert.match(PASTE_SPECIAL_OPTIONS[1]!.label, /Keep existing reference designators/);
});
