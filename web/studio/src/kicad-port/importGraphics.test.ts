import { test } from "node:test";
import assert from "node:assert/strict";
import { DXF_UNITS, buildImportCmd, buildSchImportCmd, defaultImportValues, defaultSchImportValues, describeInfo, graphicsFormatOf, importLayers, parseLength } from "./importGraphics";

const info = { ok: true, items: 8, counts: { line: 4, arc: 4 }, bbox_nm: [0, -30_000, 40_000, 0] as [number, number, number, number], fits: true };

function build(over: Partial<ReturnType<typeof defaultImportValues>> = {}, extra: { activeLayer?: string | null; polygonOutline?: boolean; format?: "dxf" | "svg" } = {}) {
  return buildImportCmd({ format: extra.format ?? "dxf", text: "0\nEOF\n", values: { ...defaultImportValues("mm"), ...over }, units: "mm", activeLayer: extra.activeLayer === undefined ? "F.SilkS" : extra.activeLayer, info, polygonOutline: extra.polygonOutline ?? false });
}

test("the file's extension names its format", () => {
  assert.equal(graphicsFormatOf("Board Outline.DXF"), "dxf");
  assert.equal(graphicsFormatOf("logo.svg"), "svg");
  assert.equal(graphicsFormatOf("board.kicad_pcb"), null);
  assert.equal(graphicsFormatOf("noextension"), null);
});

test("the dialog opens with KiCad's defaults in the display units", () => {
  const mm = defaultImportValues("mm");
  assert.deepEqual([mm.scale, mm.lineWidth, mm.tolerance, mm.placeAt, mm.setLayer, mm.layer, mm.group, mm.fix], ["1.0", "0.2", "1", false, true, "Dwgs.User", true, true]);
  assert.equal(defaultImportValues("in").lineWidth, "0.00787");
  assert.equal(DXF_UNITS.find((u) => u.id === defaultImportValues("mm").dxfUnits)?.label, "Millimeters");
});

test("the layer list is the board's copper and then the technical and user layers", () => {
  const l = importLayers(["F.Cu", "B.Cu"]);
  assert.deepEqual(l.slice(0, 3), ["F.Cu", "B.Cu", "B.Adhes"]);
  assert.ok(l.includes("Edge.Cuts") && l.includes("Dwgs.User") && l.includes("User.9"));
});

test("lengths take a unit suffix and a bare number is in the display units", () => {
  assert.equal(parseLength("0.2", "mm"), 200_000);
  assert.equal(parseLength("10mil", "mm"), 254_000);
  assert.equal(parseLength("1\"", "mm"), 25_400_000);
  assert.equal(parseLength("abc", "mm"), null);
});

test("Place at sets the origin over the scale, as the dialog does", () => {
  const r = build({ placeAt: true, x: "10", y: "-5", scale: "2" });
  assert.ok(r.ok && !r.interactive);
  if (!r.ok) return;
  const cmd = r.build(null);
  assert.equal(cmd.op, "import_graphics");
  assert.deepEqual([cmd.offset_x_nm, cmd.offset_y_nm, cmd.scale], [5_000_000_000, -2_500_000_000, 2000]);
});

test("interactive placement puts the picture's upper left corner at the cursor", () => {
  const r = build({ scale: "1.0" });
  assert.ok(r.ok && r.interactive);
  if (!r.ok) return;
  const cmd = r.build({ x: 50_000_000, y: 20_000_000 });
  // The box at offset 0 starts at (0, -30 mm): the offset takes it to (50 mm, 20 mm).
  assert.deepEqual([cmd.offset_x_nm, cmd.offset_y_nm], [50_000_000_000, 50_000_000_000]);
});

test("the layer is the dialog's, or the active layer when it is not checked", () => {
  const fixed = build({ layer: "Edge.Cuts" });
  assert.ok(fixed.ok && fixed.build(null).layer === "Edge.Cuts");
  const active = build({ setLayer: false }, { activeLayer: "F.SilkS" });
  assert.ok(active.ok && active.build(null).layer === "F.SilkS");
  const none = build({ setLayer: false }, { activeLayer: null });
  assert.ok(!none.ok);
});

test("fix discontinuities carries its tolerance and the group flag follows the checkbox", () => {
  const on = build({ fix: true, tolerance: "0.5", group: true });
  assert.ok(on.ok);
  if (on.ok) {
    const c = on.build(null);
    assert.deepEqual([c.fix_tolerance_nm, c.group], [500_000, true]);
  }
  const off = build({ fix: false, group: false });
  assert.ok(off.ok);
  if (off.ok) {
    const c = off.build(null);
    assert.deepEqual([c.fix_tolerance_nm, c.group], [null, false]);
  }
});

test("an outline imported on Edge.Cuts replaces the polygon outline only when the board has one and the box is checked", () => {
  const replaces = (over: Partial<ReturnType<typeof defaultImportValues>>, polygon: boolean) => {
    const r = build({ layer: "Edge.Cuts", ...over }, { polygonOutline: polygon });
    return r.ok ? r.build(null).replace_outline : undefined;
  };
  assert.equal(replaces({}, true), true);
  assert.equal(replaces({ replaceOutline: false }, true), false);
  assert.equal(replaces({}, false), false, "a board whose outline is already its Edge.Cuts shapes has no polygon to replace");
  assert.equal(replaces({ layer: "Dwgs.User" }, true), false);
});

test("a bad scale, line width, tolerance or origin is refused with a reason", () => {
  for (const [over, why] of [
    [{ scale: "0" }, /scale/],
    [{ scale: "x" }, /scale/],
    [{ scale: "-2" }, /scale/],
    [{ lineWidth: "wide" }, /line width/],
    [{ tolerance: "?" }, /tolerance/],
    [{ placeAt: true, x: "a" }, /place-at/],
  ] as const) {
    const r = build(over);
    assert.ok(!r.ok && why.test(r.error), JSON.stringify(over));
  }
  // The line width and unit are the DXF's: an SVG is not held to them.
  const svg = build({ lineWidth: "wide" }, { format: "svg" });
  assert.ok(svg.ok);
  if (svg.ok) assert.equal(svg.build(null).dxf_units, "");
});

test("the file's contents are described the way the dialog says them", () => {
  assert.equal(describeInfo(info), "8 items: 4 lines, 4 arcs");
  assert.equal(describeInfo({ ok: true, items: 1, counts: { circle: 1 } }), "1 item: 1 circle");
  assert.equal(describeInfo({ ok: true, items: 0, counts: {} }), "No graphic items found in file.");
  assert.equal(describeInfo({ ok: false, message: "This is not a DXF file" }), "This is not a DXF file");
});

// -------------------------------------------------------------------------------------------------------------------- the schematic and the Symbol Editor

function buildSch(over: Partial<ReturnType<typeof defaultSchImportValues>> = {}, extra: { editor?: "schematic" | "symbol"; format?: "dxf" | "svg"; symbol?: { libId: string; unit: number; bodyStyle: number } } = {}) {
  return buildSchImportCmd({ editor: extra.editor ?? "schematic", format: extra.format ?? "dxf", text: "0\nEOF\n", values: { ...defaultSchImportValues(), ...over }, units: "mm", symbol: extra.symbol });
}

test("the schematic's dialog opens with scale 1, interactive placement and the sheet's default line width", () => {
  const d = defaultSchImportValues();
  assert.deepEqual([d.scale, d.lineWidth, d.placeAt, d.x, d.y], ["1.0", "0", false, "0", "0"]);
  assert.equal(DXF_UNITS.find((u) => u.id === d.dxfUnits)?.label, "Millimeters");
});

test("the schematic's At sets the origin over the scale, as the board's dialog does", () => {
  const r = buildSch({ placeAt: true, x: "10", y: "-5", scale: "2", lineWidth: "0.15" });
  assert.ok(r.ok && !r.interactive);
  if (!r.ok) return;
  const cmd = r.build(null);
  assert.equal(cmd.op, "import_sch_graphics");
  assert.deepEqual([cmd.offset_x_nm, cmd.offset_y_nm, cmd.scale, cmd.line_width_nm], [5_000_000_000, -2_500_000_000, 2000, 150_000]);
});

test("interactive placement in the schematic puts the drawing's origin at the cursor", () => {
  const r = buildSch();
  assert.ok(r.ok && r.interactive);
  if (!r.ok) return;
  const cmd = r.build({ x: 50_000_000, y: -20_000_000 });
  assert.deepEqual([cmd.op, cmd.offset_x_nm, cmd.offset_y_nm], ["import_sch_graphics", 50_000_000_000, -20_000_000_000]);
});

test("the Symbol Editor's command names the symbol, the unit and the body style", () => {
  const r = buildSch({}, { editor: "symbol", symbol: { libId: "eda:Untitled", unit: 2, bodyStyle: 1 } });
  assert.ok(r.ok);
  if (!r.ok) return;
  const cmd = r.build({ x: 1_270_000, y: 2_540_000 });
  assert.equal(cmd.op, "import_symbol_graphics");
  if (cmd.op !== "import_symbol_graphics") return;
  assert.deepEqual([cmd.lib_id, cmd.unit, cmd.body_style, cmd.offset_x_nm, cmd.offset_y_nm], ["eda:Untitled", 2, 1, 1_270_000, 2_540_000]);
  assert.ok(!buildSch({}, { editor: "symbol" }).ok, "with no symbol open there is nowhere to import");
});

test("the schematic's dialog refuses a bad scale, width or origin with a reason, and an SVG is not held to the DXF's width and unit", () => {
  for (const [over, why] of [
    [{ scale: "0" }, /scale/],
    [{ scale: "x" }, /scale/],
    [{ lineWidth: "wide" }, /line width/],
    [{ lineWidth: "-1" }, /line width/],
    [{ placeAt: true, x: "a" }, /placement/],
  ] as const) {
    const r = buildSch(over);
    assert.ok(!r.ok && why.test(r.error), JSON.stringify(over));
  }
  const svg = buildSch({ lineWidth: "wide" }, { format: "svg" });
  assert.ok(svg.ok);
  if (svg.ok) {
    const c = svg.build(null);
    assert.deepEqual([c.dxf_units, c.line_width_nm], ["", 0]);
  }
});
