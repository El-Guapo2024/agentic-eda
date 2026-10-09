import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd, Schematic } from "../api/types";
import type { SchGraphic } from "../api/schEditTypes";
import { graphicShapeWith, schEdit, schFriendlyName, schGrid, schItemOf, schItemsOf, schRow, SCH_PROPERTIES } from "./schItemProperties";

// ---------------------------------------------------------------------------------------------------------------------------- fixture

const graphic = (id: string, shape: SchGraphic["shape"], over: Partial<SchGraphic> = {}): SchGraphic => ({ id, shape, ...over });

const sheet = (): Schematic =>
  ({
    lib_symbols: {},
    symbols: [
      { id: "R1", lib_id: "Device:R", at: [100_000, 50_000], rot: 90, mirror: "x", unit: 1, body_style: 1, value: "10k", mpn: null, package: "0603", footprint: "R_0603", datasheet: "", pins: [], dnp: false },
      { id: "U1", lib_id: "MCU:STM32", at: [200_000, 50_000], rot: 0, mirror: null, unit: 1, body_style: 1, value: "STM32", mpn: "STM32F103", package: null, footprint: null, datasheet: "http://x", pins: [], dnp: true, exclude_from_bom: true },
      { id: "U2", lib_id: "Amp:Dual", at: [10_000, 10_000], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] },
      { id: "U2", lib_id: "Amp:Dual", at: [10_000, 40_000], rot: 0, mirror: null, unit: 2, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] },
    ],
    power_symbols: [{ id: "#PWR01", lib_id: "power:GND", at: [5_000, 5_000], rot: 180, net: "GND", pin: "1" }],
    wires: [
      { id: "wire_a", net: "N", pins: [], pts: [[0, 0], [10_000, 0], [10_000, 5_000]], bus: false, stroke: { width_um: 300, style: "dash", color: { r: 255, g: 0, b: 0, a: 255 } } },
      { id: "wire_bus", net: "D[0..3]", pins: [], pts: [[0, 5_000], [10_000, 5_000]], bus: true },
    ],
    no_connects: [{ id: "nc_a", at: [1, 1], pin: "" }],
    labels: [
      { id: "lbl_a", net: "A", at: [5, 5], scope: "local", shape: null },
      { id: "lbl_g", net: "B", at: [6, 6], scope: "global", shape: "input" },
      { id: "lbl_h", net: "C", at: [7, 7], scope: "hierarchical", shape: "output" },
    ],
    texts: [{ id: "txt_a", content: "hello", at: [0, 0], angle: 0, size_um: 1270 }],
    title_block: null,
    bus_entries: [{ id: "bent_a", at: [2, 5], size: [2, 2], stroke: { width_um: 250 } }],
    junctions: [
      { id: "jct_a", at: [5, 0] },
      { id: "jct_b", at: [7, 0], look: { diameter_um: 900, color: { r: 0, g: 0, b: 255, a: 255 } } },
    ],
    lines: [{ id: "sln_a", pts: [[0, 9_000], [9_000, 9_000]], width_um: 250, stroke: { style: "dot" } }],
    graphics: [
      graphic("shp_rect", { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10_000, y: 5_000 }, corner_radius_um: 100 }, { width_um: 200, fill: "color", fill_color: { r: 0, g: 255, b: 0, a: 255 } }),
      graphic("shp_circle", { type: "circle", center: { x: 20_000, y: 20_000 }, radius_um: 3_000 }),
      graphic("shp_arc", { type: "arc", start: { x: 0, y: 0 }, mid: { x: 5_000, y: -3_000 }, end: { x: 10_000, y: 0 } }),
      graphic("shp_bez", { type: "bezier", start: { x: 0, y: 0 }, c1: { x: 1, y: 1 }, c2: { x: 2, y: 1 }, end: { x: 3, y: 0 } }),
      graphic("shp_poly", { type: "polygon", pts: [{ x: 0, y: 0 }, { x: 5, y: 0 }, { x: 5, y: 5 }] }),
      graphic("shp_box", { type: "text_box", start: { x: 0, y: 0 }, end: { x: 20_000, y: 10_000 }, text: "note", size_um: 1270, bold: false, italic: true, h_align: "center", v_align: "top" }),
      graphic("shp_rule", { type: "rule_area", pts: [{ x: 0, y: 0 }, { x: 5, y: 0 }, { x: 5, y: 5 }], dnp: true }),
      graphic("shp_dir", { type: "directive", at: { x: 1, y: 1 }, shape: "diamond", pin_length_um: 2_540, netclass: "Fast" }),
    ],
    locked: ["R1", "wire_bus"],
    sheets: [{ id: "sheet_a", name: "Power", file: "power.kicad_sch", at: [0, 0], size: [10, 10], pins: [] }],
    sheet_path: [],
  }) as unknown as Schematic;

const names = (s: Schematic, ids: string[]): Array<[string, string[]]> => schGrid(s, ids, "mm").groups.map((g) => [g.caption, g.rows.map((r) => r.name)]);
const rowOf = (s: Schematic, ids: string[], name: string) => schGrid(s, ids, "mm").groups.flatMap((g) => g.rows).find((r) => r.name === name);
const edit = (s: Schematic, ids: string[], name: string, value: string | number | boolean): Cmd[] => {
  const plan = schEdit(s, ids, name, value, "mm");
  assert.equal(plan.ok, true, plan.ok ? "" : plan.error);
  return plan.ok ? plan.cmds : [];
};

// ---------------------------------------------------------------------------------------------------------------- item resolution

test("a selection id names the kind of item it is", () => {
  const s = sheet();
  const kinds = Object.fromEntries(
    ["R1", "#PWR01", "wire_a", "wire_bus", "lbl_a", "lbl_g", "lbl_h", "txt_a", "nc_a", "bent_a", "jct_a", "sln_a", "sheet_a", "shp_rect", "shp_box", "shp_rule", "shp_dir"].map((id) => [id, schItemOf(s, id)?.type])
  );
  assert.deepEqual(kinds, {
    R1: "SCH_SYMBOL",
    "#PWR01": "SCH_SYMBOL",
    wire_a: "SCH_LINE",
    wire_bus: "SCH_LINE",
    lbl_a: "SCH_LABEL",
    lbl_g: "SCH_GLOBALLABEL",
    lbl_h: "SCH_HIERLABEL",
    txt_a: "SCH_TEXT",
    nc_a: "SCH_NO_CONNECT",
    bent_a: "SCH_BUS_WIRE_ENTRY",
    jct_a: "SCH_JUNCTION",
    sln_a: "SCH_LINE",
    sheet_a: "SCH_SHEET",
    shp_rect: "SCH_SHAPE",
    shp_box: "SCH_TEXTBOX",
    shp_rule: "SCH_RULE_AREA",
    shp_dir: "SCH_DIRECTIVE_LABEL",
  });
  assert.equal(schItemOf(s, "nope"), null);
  assert.deepEqual(schItemsOf(s, ["R1", "nope", "lbl_a"]).map((i) => i.id), ["R1", "lbl_a"]);
});

test("the caption is the friendly name of a single item", () => {
  const s = sheet();
  const caption = (id: string) => schGrid(s, [id], "mm").caption;
  assert.deepEqual(["R1", "wire_a", "wire_bus", "sln_a", "lbl_a", "lbl_g", "lbl_h", "shp_dir", "txt_a", "shp_box", "shp_rect", "shp_rule", "sheet_a", "nc_a", "bent_a", "jct_a"].map(caption), [
    "Symbol",
    "Wire",
    "Bus",
    "Graphic Line",
    "Net Label",
    "Global Label",
    "Hierarchical Label",
    "Directive Label",
    "Text",
    "Text Box",
    "Graphic",
    "Rule Area",
    "Sheet",
    "No-Connect Flag",
    "Wire Entry",
    "Junction",
  ]);
  assert.equal(schGrid(s, ["R1", "U1"], "mm").caption, "2 objects selected");
  assert.equal(schFriendlyName(schItemOf(s, "sln_a")!), "Graphic Line");
});

// ------------------------------------------------------------------------------------------------------------------ property lists

test("a symbol: where it is and how it is turned, its fields, and its attributes", () => {
  const s = sheet();
  assert.deepEqual(names(s, ["R1"]), [
    ["Basic Properties", ["Locked", "Position X", "Position Y", "Orientation", "Mirror X", "Mirror Y"]],
    ["Fields", ["Reference", "Value", "Library Link", "Datasheet", "Footprint"]],
    ["Attributes", ["Exclude From Simulation", "Exclude From Bill of Materials", "Exclude From Board", "Do not Populate"]],
  ]);
  assert.deepEqual(names(s, ["U1"])[1], ["Fields", ["Reference", "Value", "Library Link", "Datasheet", "Footprint", "MPN"]], "an MPN, when the part has one");
});

test("a power symbol is a symbol without a mirror, a value of its own to edit, or attributes", () => {
  const s = sheet();
  assert.deepEqual(names(s, ["#PWR01"]), [
    ["Basic Properties", ["Locked", "Position X", "Position Y", "Orientation"]],
    ["Fields", ["Reference", "Value", "Library Link"]],
  ]);
  assert.equal(rowOf(s, ["#PWR01"], "Value")!.value, "GND");
  assert.equal(rowOf(s, ["#PWR01"], "Value")!.writable, false);
  assert.equal(rowOf(s, ["#PWR01"], "Reference")!.writable, false);
  assert.equal(rowOf(s, ["#PWR01"], "Orientation")!.value, 180);
});

test("wires, buses and lines: the end points, the length and the stroke", () => {
  const s = sheet();
  assert.deepEqual(names(s, ["wire_a"]), [["Basic Properties", ["Locked", "Start X", "Start Y", "End X", "End Y", "Length", "Wire Style", "Line Width", "Color"]]]);
  assert.deepEqual(names(s, ["wire_bus"]), names(s, ["wire_a"]));
  assert.deepEqual(names(s, ["sln_a"]), [["Basic Properties", ["Locked", "Start X", "Start Y", "End X", "End Y", "Length", "Line Style", "Line Width", "Color"]]], "a graphic line has a line style, a wire a wire style");
  assert.equal(rowOf(s, ["wire_a"], "Length")!.value, 15_000, "the whole run: 10 mm along and 5 mm up");
  assert.equal(rowOf(s, ["wire_a"], "Start X")!.writable, false, "an end point is dragged, not typed");
});

test("a junction, a bus entry and a no-connect flag", () => {
  const s = sheet();
  assert.deepEqual(names(s, ["jct_a"]), [["Basic Properties", ["Locked", "Diameter", "Color"]]]);
  assert.deepEqual(names(s, ["bent_a"]), [["Basic Properties", ["Locked", "Wire Style", "Line Width", "Color"]]]);
  assert.deepEqual(names(s, ["nc_a"]), [], "KiCad registers nothing for a no-connect flag: its grid is empty");
});

test("labels and text", () => {
  const s = sheet();
  assert.deepEqual(names(s, ["lbl_a"]), [["Basic Properties", ["Locked"]], ["Text Properties", ["Text"]]]);
  assert.deepEqual(names(s, ["lbl_g"]), [["Basic Properties", ["Locked", "Shape"]], ["Text Properties", ["Text"]]]);
  assert.deepEqual(names(s, ["lbl_h"]), names(s, ["lbl_g"]));
  assert.deepEqual(names(s, ["txt_a"]), [["Basic Properties", ["Locked"]], ["Text Properties", ["Text", "Text Size"]]]);
  assert.deepEqual(names(s, ["shp_dir"]), [["Basic Properties", ["Locked", "Shape", "Pin length"]]], "a directive label has no text of its own");
});

test("a sheet, and the shapes a sheet is drawn with", () => {
  const s = sheet();
  assert.deepEqual(names(s, ["sheet_a"]), [["Basic Properties", ["Locked", "Sheet Name"]], ["Fields", ["Sheetfile"]]]);
  const shape = (id: string) => Object.fromEntries(names(s, [id]))["Shape Properties"];
  assert.deepEqual(shape("shp_rect"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Width", "Height", "Corner Radius", "Line Width", "Line Style", "Line Color", "Fill", "Fill Color"]);
  assert.deepEqual(shape("shp_circle"), ["Shape", "Center X", "Center Y", "Radius", "Line Width", "Line Style", "Line Color", "Fill", "Fill Color"]);
  assert.deepEqual(shape("shp_arc"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Line Width", "Line Style", "Line Color", "Angle"]);
  assert.deepEqual(shape("shp_bez"), ["Shape", "Start X", "Start Y", "End X", "End Y", "Line Width", "Line Style", "Line Color", "Fill", "Fill Color"]);
  assert.deepEqual(shape("shp_poly"), ["Shape", "Line Width", "Line Style", "Line Color", "Fill", "Fill Color"]);
});

test("a text box has its text rows before its shape rows, and neither a fill nor a corner radius", () => {
  assert.deepEqual(names(sheet(), ["shp_box"]), [
    ["Basic Properties", ["Locked"]],
    ["Text Properties", ["Text", "Italic", "Bold", "Horizontal Justification", "Vertical Justification", "Text Size"]],
    ["Shape Properties", ["Start X", "Start Y", "End X", "End Y", "Width", "Height", "Line Width", "Line Style", "Line Color"]],
  ]);
});

test("a rule area has its attributes and the polygon's look, unfilled (this editor draws it as an outline)", () => {
  assert.deepEqual(names(sheet(), ["shp_rule"]), [
    ["Basic Properties", ["Locked"]],
    ["Attributes", ["Exclude From Board", "Exclude From Simulation", "Exclude From Bill of Materials", "Do not Populate"]],
    ["Shape Properties", ["Shape", "Line Width", "Line Style", "Line Color"]],
  ]);
});

test("the registry has no property twice for one class", () => {
  for (const type of ["SCH_SYMBOL", "SCH_LINE", "SCH_JUNCTION", "SCH_BUS_WIRE_ENTRY", "SCH_LABEL", "SCH_GLOBALLABEL", "SCH_HIERLABEL", "SCH_DIRECTIVE_LABEL", "SCH_TEXT", "SCH_TEXTBOX", "SCH_SHAPE", "SCH_RULE_AREA", "SCH_SHEET"]) {
    const seen = new Map<string, string>();
    for (const p of SCH_PROPERTIES.getProperties(type)) {
      const owner = seen.get(p.name);
      // A name two classes register (a label's Shape and a directive label's) is one row: the one the item has.
      assert.ok(owner === undefined || p.name === "Shape", `${type}: ${p.name} from ${owner} and ${p.owner}`);
      seen.set(p.name, p.owner);
    }
  }
});

// -------------------------------------------------------------------------------------------------------------------------- values

test("values: a symbol's orientation and mirror, a wire's stroke, a junction's look, a label's shape", () => {
  const s = sheet();
  assert.equal(rowOf(s, ["R1"], "Orientation")!.value, 90);
  assert.equal(rowOf(s, ["R1"], "Mirror X")!.value, true);
  assert.equal(rowOf(s, ["R1"], "Mirror Y")!.value, false);
  assert.equal(rowOf(s, ["R1"], "Locked")!.value, true);
  assert.equal(rowOf(s, ["U1"], "Do not Populate")!.value, true);
  assert.equal(rowOf(s, ["U1"], "Exclude From Bill of Materials")!.value, true);
  assert.equal(rowOf(s, ["wire_a"], "Wire Style")!.value, "dash");
  assert.equal(rowOf(s, ["wire_a"], "Line Width")!.value, 300);
  assert.equal(rowOf(s, ["wire_a"], "Color")!.value, "#ff0000");
  assert.equal(rowOf(s, ["wire_bus"], "Color")!.value, "", "no colour of its own: the layer's");
  assert.equal(rowOf(s, ["sln_a"], "Line Style")!.value, "dot");
  assert.equal(rowOf(s, ["jct_b"], "Diameter")!.value, 900);
  assert.equal(rowOf(s, ["lbl_g"], "Shape")!.value, "input");
  assert.equal(rowOf(s, ["txt_a"], "Text Size")!.value, 1270);
  assert.equal(rowOf(s, ["shp_circle"], "Radius")!.value, 3_000);
  assert.equal(rowOf(s, ["shp_rect"], "Width")!.value, 10_000);
  assert.equal(rowOf(s, ["shp_rect"], "Fill")!.value, "color");
  assert.equal(rowOf(s, ["shp_rect"], "Fill Color")!.writable, true);
  assert.equal(rowOf(s, ["shp_circle"], "Fill Color")!.writable, false, "a colour counts only for a fill with a colour");
  const angle = rowOf(s, ["shp_arc"], "Angle")!;
  assert.ok(typeof angle.value === "number" && angle.value > 0 && angle.value < 180);
});

test("several items: the shared value or none", () => {
  const s = sheet();
  assert.equal(rowOf(s, ["R1", "U1"], "Locked")!.value, null, "R1 is locked, U1 is not");
  assert.equal(rowOf(s, ["U1", "U2"], "Orientation")!.value, 0);
  assert.equal(rowOf(s, ["R1", "U1"], "Orientation")!.value, null);
  assert.deepEqual(names(s, ["R1", "lbl_a"]), [["Basic Properties", ["Locked"]]], "a symbol and a label share the lock");
  assert.deepEqual(names(s, ["wire_a", "wire_bus"]), names(s, ["wire_a"]));
  assert.deepEqual(names(s, ["lbl_a", "txt_a"]), [["Basic Properties", ["Locked"]], ["Text Properties", ["Text"]]], "labels and texts share the text");
  assert.equal(schRow(s, ["wire_a", "shp_rect"], "Wire Style", "mm"), null);
});

// ---------------------------------------------------------------------------------------------------------------------------- edits

test("a symbol is moved, turned, mirrored and renamed through the verbs the editor has", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["U1"], "Position X", 201_000), [{ op: "sch_move", verb: "move", ids: ["U1"], dx: 1_000, dy: 0 }]);
  assert.deepEqual(edit(s, ["U1"], "Position Y", 49_000), [{ op: "sch_move", verb: "move", ids: ["U1"], dx: 0, dy: -1_000 }]);
  assert.deepEqual(edit(s, ["U1"], "Position X", 200_000), []);
  assert.deepEqual(edit(s, ["U1"], "Orientation", 270), [{ op: "rotate_symbol", id: "U1", quarter_turns: 3, unit: 1 }]);
  assert.deepEqual(edit(s, ["R1"], "Orientation", 270), [{ op: "rotate_symbol", id: "R1", quarter_turns: 2, unit: 1 }]);
  assert.deepEqual(edit(s, ["R1"], "Orientation", 0), [{ op: "rotate_symbol", id: "R1", quarter_turns: 3, unit: 1 }]);
  assert.deepEqual(edit(s, ["U1"], "Orientation", 0), []);
  assert.deepEqual(edit(s, ["U1"], "Mirror X", true), [{ op: "mirror_symbol_vertical", id: "U1", unit: 1 }]);
  assert.deepEqual(edit(s, ["U1"], "Mirror Y", true), [{ op: "mirror_symbol", id: "U1", unit: 1 }]);
  assert.deepEqual(edit(s, ["R1"], "Mirror X", true), [], "already mirrored about X");
  assert.deepEqual(edit(s, ["R1"], "Mirror X", false), [{ op: "mirror_symbol_vertical", id: "R1", unit: 1 }]);
  assert.deepEqual(edit(s, ["U1"], "Reference", "U7"), [{ op: "rename_symbol", id: "U1", new_id: "U7" }]);
});

test("a reference is edited on one symbol only: two of them would be one name", () => {
  const s = sheet();
  assert.equal(rowOf(s, ["U1"], "Reference")!.writable, true);
  assert.equal(rowOf(s, ["U1", "R1"], "Reference")!.writable, false);
  assert.deepEqual(edit(s, ["U1", "R1"], "Reference", "X9"), []);
  assert.equal(rowOf(s, ["U1", "R1"], "Value")!.writable, true, "a value can be the same on many");
});

test("a locked symbol cannot be moved from the grid, as it cannot be moved by hand", () => {
  const s = sheet();
  assert.equal(rowOf(s, ["R1"], "Position X")!.writable, false);
  assert.deepEqual(edit(s, ["R1"], "Position X", 1), []);
  assert.equal(rowOf(s, ["R1"], "Reference")!.writable, true, "its fields are still its own");
});

test("one unit of a symbol with several is addressed with its unit", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["U2"], "Position X", 11_000), [{ op: "sch_move", verb: "move", ids: ["U2#1"], dx: 1_000, dy: 0 }]);
});

test("a power symbol is moved and turned like a single item", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["#PWR01"], "Position Y", 6_000), [{ op: "sch_move", verb: "move", ids: ["#PWR01"], dx: 0, dy: 1_000 }]);
  assert.deepEqual(edit(s, ["#PWR01"], "Orientation", 270), [{ op: "sch_move", verb: "rotate", ids: ["#PWR01"], ccw: true }]);
  assert.deepEqual(edit(s, ["#PWR01"], "Orientation", 0), [
    { op: "sch_move", verb: "rotate", ids: ["#PWR01"], ccw: true },
    { op: "sch_move", verb: "rotate", ids: ["#PWR01"], ccw: true },
  ]);
});

test("a symbol's fields and attributes", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["R1"], "Value", "22k"), [{ op: "edit_symbol_fields", id: "R1", value: "22k" }]);
  assert.deepEqual(edit(s, ["R1"], "Footprint", "R_0805"), [{ op: "edit_symbol_fields", id: "R1", footprint: "R_0805" }]);
  assert.deepEqual(edit(s, ["R1"], "Datasheet", "http://r"), [{ op: "edit_symbol_fields", id: "R1", datasheet: "http://r" }]);
  assert.deepEqual(edit(s, ["R1"], "Value", "10k"), []);
  assert.deepEqual(edit(s, ["R1"], "Do not Populate", true), [{ op: "set_symbol_attrs", ids: ["R1"], dnp: true }]);
  assert.deepEqual(edit(s, ["U1"], "Do not Populate", true), []);
  assert.deepEqual(edit(s, ["R1"], "Exclude From Board", true), [{ op: "set_symbol_attrs", ids: ["R1"], exclude_from_board: true }]);
  assert.deepEqual(edit(s, ["U1"], "Library Link", "x"), [], "the library link is read-only");
  const empty = schEdit(s, ["R1"], "Reference", "  ", "mm");
  assert.equal(empty.ok, false);
});

test("the lock goes through the schematic's lock verb", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["U1"], "Locked", true), [{ op: "sch_edit", verb: "set_locked", ids: ["U1"], locked: true }]);
  assert.deepEqual(edit(s, ["R1"], "Locked", false), [{ op: "sch_edit", verb: "set_locked", ids: ["R1"], locked: false }]);
  assert.deepEqual(edit(s, ["R1", "U1", "lbl_a"], "Locked", true), [
    { op: "sch_edit", verb: "set_locked", ids: ["U1"], locked: true },
    { op: "sch_edit", verb: "set_locked", ids: ["lbl_a"], locked: true },
  ]);
});

test("wires, lines, bus entries and junctions are restyled with set_stroke", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["wire_a"], "Wire Style", "dot"), [{ op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], style: "dot" }]);
  assert.deepEqual(edit(s, ["wire_a"], "Wire Style", "dash"), []);
  assert.deepEqual(edit(s, ["wire_a"], "Line Width", 400), [{ op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], width_um: 400 }]);
  assert.deepEqual(edit(s, ["wire_a"], "Color", "#0000ff"), [{ op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], color: { r: 0, g: 0, b: 255, a: 255 } }]);
  assert.deepEqual(edit(s, ["wire_a"], "Color", ""), [{ op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], color: { r: 0, g: 0, b: 0, a: 0 } }], "no colour is the layer's");
  assert.deepEqual(edit(s, ["sln_a"], "Line Style", "dash_dot"), [{ op: "sch_edit", verb: "set_stroke", ids: ["sln_a"], style: "dash_dot" }]);
  assert.deepEqual(edit(s, ["sln_a"], "Line Width", 250), []);
  assert.deepEqual(edit(s, ["bent_a"], "Wire Style", "solid"), [{ op: "sch_edit", verb: "set_stroke", ids: ["bent_a"], style: "solid" }]);
  assert.deepEqual(edit(s, ["jct_a"], "Diameter", 800), [{ op: "sch_edit", verb: "set_stroke", ids: ["jct_a"], diameter_um: 800 }]);
  assert.deepEqual(edit(s, ["jct_b"], "Color", "#000000"), [{ op: "sch_edit", verb: "set_stroke", ids: ["jct_b"], color: { r: 0, g: 0, b: 0, a: 255 } }]);
  // The three wires, one verb each: the grid sends one command per item, the caller makes them one undo step.
  assert.deepEqual(edit(s, ["wire_a", "wire_bus"], "Wire Style", "solid"), [
    { op: "sch_edit", verb: "set_stroke", ids: ["wire_a"], style: "solid" },
    { op: "sch_edit", verb: "set_stroke", ids: ["wire_bus"], style: "solid" },
  ]);
});

test("a label, a text and a sheet", () => {
  const s = sheet();
  assert.deepEqual(edit(s, ["lbl_a"], "Text", "CLK"), [{ op: "sch_edit", verb: "edit_label", id: "lbl_a", text: "CLK" }]);
  assert.deepEqual(edit(s, ["lbl_g"], "Shape", "output"), [{ op: "sch_edit", verb: "edit_label", id: "lbl_g", shape: "output" }]);
  assert.equal(schEdit(s, ["lbl_a"], "Text", "", "mm").ok, false, "Label can not be empty.");
  assert.deepEqual(edit(s, ["txt_a"], "Text", "bye"), [{ op: "sch_edit", verb: "edit_text", id: "txt_a", text: "bye" }]);
  assert.deepEqual(edit(s, ["txt_a"], "Text Size", 2000), [{ op: "sch_edit", verb: "edit_text", id: "txt_a", size_um: 2000 }]);
  assert.equal(schEdit(s, ["txt_a"], "Text Size", 5, "mm").ok, false, "text must not disappear: 0.01 mm at least");
  assert.deepEqual(edit(s, ["lbl_a", "txt_a"], "Text", "same"), [
    { op: "sch_edit", verb: "edit_label", id: "lbl_a", text: "same" },
    { op: "sch_edit", verb: "edit_text", id: "txt_a", text: "same" },
  ]);
  assert.deepEqual(edit(s, ["sheet_a"], "Sheet Name", "Supply"), [{ op: "sch_edit", verb: "edit_sheet", id: "sheet_a", name: "Supply" }]);
  assert.deepEqual(edit(s, ["sheet_a"], "Sheetfile", "supply"), [{ op: "sch_edit", verb: "edit_sheet", id: "sheet_a", file: "supply" }]);
  assert.deepEqual(edit(s, ["sheet_a"], "Sheetfile", "power"), [], "the file it already shows, with or without the extension");
});

test("a drawn graphic is replaced with the edit laid over it", () => {
  const s = sheet();
  const [move] = edit(s, ["shp_rect"], "Start X", 1_000);
  assert.deepEqual(move, {
    op: "sch_edit",
    verb: "edit_graphic",
    id: "shp_rect",
    graphic: { shape: { type: "rectangle", start: { x: 1_000, y: 0 }, end: { x: 10_000, y: 5_000 }, corner_radius_um: 100 }, width_um: 200, fill: "color", fill_color: { r: 0, g: 255, b: 0, a: 255 } },
  });
  const shapeOf = (cmd: Cmd | undefined) => ((cmd as unknown as { graphic: { shape: unknown } }).graphic.shape);
  assert.deepEqual(shapeOf(edit(s, ["shp_rect"], "Width", 12_000)[0]), { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 12_000, y: 5_000 }, corner_radius_um: 100 });
  assert.deepEqual(shapeOf(edit(s, ["shp_rect"], "Corner Radius", 200)[0]), { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10_000, y: 5_000 }, corner_radius_um: 200 });
  assert.deepEqual(shapeOf(edit(s, ["shp_circle"], "Radius", 4_000)[0]), { type: "circle", center: { x: 20_000, y: 20_000 }, radius_um: 4_000 });
  assert.deepEqual(shapeOf(edit(s, ["shp_circle"], "Center X", 21_000)[0]), { type: "circle", center: { x: 21_000, y: 20_000 }, radius_um: 3_000 });
  assert.deepEqual(shapeOf(edit(s, ["shp_arc"], "End Y", 1_000)[0]), { type: "arc", start: { x: 0, y: 0 }, mid: { x: 5_000, y: -3_000 }, end: { x: 10_000, y: 1_000 } });
  assert.equal(edit(s, ["shp_rect"], "Width", 10_000).length, 0);
  const style = edit(s, ["shp_rect"], "Line Style", "dash")[0] as unknown as { graphic: Record<string, unknown> };
  assert.equal(style.graphic.line_style, "dash");
  assert.deepEqual(style.graphic.shape, { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 10_000, y: 5_000 }, corner_radius_um: 100 });
  const fill = edit(s, ["shp_circle"], "Fill", "background")[0] as unknown as { graphic: Record<string, unknown> };
  assert.equal(fill.graphic.fill, "background");
  const color = edit(s, ["shp_rect"], "Fill Color", "#ff0000")[0] as unknown as { graphic: Record<string, unknown> };
  assert.deepEqual(color.graphic.fill_color, { r: 255, g: 0, b: 0, a: 255 });
});

test("a text box, a rule area and a directive label are edited through their own graphic", () => {
  const s = sheet();
  const g = (cmd: Cmd | undefined) => (cmd as unknown as { graphic: { shape: Record<string, unknown> } }).graphic.shape;
  assert.equal(g(edit(s, ["shp_box"], "Text", "new")[0])!.text, "new");
  assert.equal(g(edit(s, ["shp_box"], "Bold", true)[0])!.bold, true);
  assert.equal(g(edit(s, ["shp_box"], "Horizontal Justification", "right")[0])!.h_align, "right");
  assert.equal(g(edit(s, ["shp_box"], "Vertical Justification", "bottom")[0])!.v_align, "bottom");
  assert.equal(g(edit(s, ["shp_box"], "Text Size", 2000)[0])!.size_um, 2000);
  assert.deepEqual(g(edit(s, ["shp_box"], "Width", 15_000)[0])!.end, { x: 15_000, y: 10_000 });
  assert.equal(g(edit(s, ["shp_rule"], "Exclude From Board", true)[0])!.exclude_from_board, true);
  assert.deepEqual(edit(s, ["shp_rule"], "Do not Populate", true), [], "already set");
  assert.equal(g(edit(s, ["shp_dir"], "Shape", "dot")[0])!.shape, "dot");
  assert.equal(g(edit(s, ["shp_dir"], "Pin length", 5_080)[0])!.pin_length_um, 5_080);
  assert.equal(g(edit(s, ["shp_dir"], "Pin length", 5_080)[0])!.netclass, "Fast", "the rest of the directive is as it was");
});

test("graphicShapeWith follows the EDA_SHAPE setters and refuses a property the shape does not have", () => {
  const rect = { type: "rectangle", start: { x: 10, y: 10 }, end: { x: 0, y: 0 } } as const;
  assert.deepEqual(graphicShapeWith(rect, "Width", 4), { ...rect, end: { x: 6, y: 0 } }, "corners keep their order: the end is left of the start");
  assert.equal(graphicShapeWith(rect, "Radius", 4), null);
  assert.equal(graphicShapeWith({ type: "polygon", pts: [] }, "Start X", 4), null);
});
