// The Symbol Editor's Properties panel against KiCad's own registrations (UI-LEFT Y3): for every class of a library symbol's items, the studio's rows (`symItemProperties.ts`)
// are KiCad's, in KiCad's groups and order, as the library editor frame lists them (`isLibraryEditor`: Unit, Body Style and Private shown, Locked not).
//
// KiCad's side is transcribed from the `PROPERTY_MANAGER` calls of KiCad 8303b2ad -- `EDA_ITEM_DESC`, `SCH_ITEM_DESC` (eeschema/sch_item.cpp), the SYMBOL rows of
// `SCH_SYMBOL_DESC` (sch_symbol.cpp) and `LIB_SYMBOL_DESC` (lib_symbol.cpp), `SCH_PIN_DESC` (sch_pin.cpp), `EDA_TEXT_DESC` (common/eda_text.cpp), `SCH_TEXT_DESC`,
// `EDA_SHAPE_DESC` (common/eda_shape.cpp), `SCH_SHAPE_DESC`, `SCH_TEXTBOX_DESC` and `SCH_FIELD_DESC` (sch_field.cpp) -- call for call, replayed through the ported manager.
// The behaviour of the rows (what they read and what an edit sends) is tested below the table.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd, LibrarySymbol } from "../api/types";
import { PropertyManager, type PropItem } from "./propertyManager";
import { registeredRows } from "./propertyGrid";
import { SYM_PROPERTIES, symEdit, symGrid, type SymCtx } from "./symItemProperties";
import { libFieldId } from "./symFieldLayout";

type Decl =
  | { inherits: [string, string] }
  | { add: [owner: string, name: string, group: string]; hidden?: "all" | "design" | "library" }
  | { mask: [derived: string, base: string, name: string] }
  | { replace: [owner: string, base: string, name: string, group: string] };

const TEXT = "Text Properties";
const SHAPE = "Shape Properties";

const add = (owner: string, group: string, ...names: string[]): Decl[] => names.map((name) => ({ add: [owner, name, group] }));
const hiddenFrom = (hidden: "all" | "design" | "library", owner: string, ...names: string[]): Decl[] => names.map((name) => ({ add: [owner, name, ""], hidden }));
const inherits = (derived: string, ...bases: string[]): Decl[] => bases.map((b) => ({ inherits: [derived, b] }));
const mask = (derived: string, base: string, ...names: string[]): Decl[] => names.map((n) => ({ mask: [derived, base, n] }));

const KICAD: Decl[] = [
  ...hiddenFrom("all", "EDA_ITEM", "Type"),
  // eeschema/sch_item.cpp SCH_ITEM_DESC: Locked `SetIsHiddenFromLibraryEditors`; Unit, Body Style and Private `SetIsHiddenFromDesignEditors`
  ...inherits("SCH_ITEM", "EDA_ITEM"),
  ...hiddenFrom("library", "SCH_ITEM", "Locked"),
  ...hiddenFrom("design", "SCH_ITEM", "Unit", "Body Style", "Private"),
  // eeschema/sch_symbol.cpp SCH_SYMBOL_DESC's SYMBOL rows, eeschema/lib_symbol.cpp LIB_SYMBOL_DESC
  ...add("SYMBOL", "", "Pin numbers", "Pin names"),
  ...inherits("LIB_SYMBOL", "SYMBOL"),
  ...add("LIB_SYMBOL", "Fields", "Reference", "Value", "Footprint", "Datasheet", "Keywords"),
  ...add("LIB_SYMBOL", "Symbol Definition", "Define as Power Symbol", "Define as Local Power Symbol"),
  ...add("SYMBOL", "Pin Display", "Show Pin Number", "Show Pin Name"),
  ...add("LIB_SYMBOL", "Pin Display", "Place Pin Names Inside"),
  ...add("SYMBOL", "Pin Display", "Pin Name Position Offset"),
  ...add("LIB_SYMBOL", "Attributes", "Exclude from Simulation", "Exclude from Board", "Exclude from Bill of Materials", "Exclude from Position Files"),
  ...add("LIB_SYMBOL", "Units and Body Styles", "Number of Symbol Units", "Units are Interchangeable", "Body Styles"),
  // eeschema/sch_pin.cpp SCH_PIN_DESC
  ...inherits("SCH_PIN", "SCH_ITEM"),
  ...mask("SCH_PIN", "SCH_ITEM", "Locked"),
  ...add("SCH_PIN", "", "Pin Name", "Pin Number", "Electrical Type", "Graphic Style", "Position X", "Position Y", "Orientation", "Length", "Name Text Size", "Number Text Size", "Visible"),
  // common/eda_text.cpp EDA_TEXT_DESC
  ...add("EDA_TEXT", "", "Orientation"),
  ...add("EDA_TEXT", TEXT, "Text", "Font", "Auto Thickness", "Thickness", "Italic", "Bold", "Mirrored", "Visible", "Width", "Height", "Horizontal Justification", "Vertical Justification", "Color", "Hyperlink"),
  // eeschema/sch_text.cpp SCH_TEXT_DESC
  ...inherits("SCH_TEXT", "SCH_ITEM", "EDA_TEXT"),
  ...mask("SCH_TEXT", "EDA_TEXT", "Mirrored", "Width", "Height", "Thickness"),
  ...add("SCH_TEXT", TEXT, "Text Size"),
  ...mask("SCH_TEXT", "EDA_TEXT", "Orientation"),
  // common/eda_shape.cpp EDA_SHAPE_DESC (8303b2ad)
  ...add("EDA_SHAPE", SHAPE, "Shape", "Start X", "Start Y", "Center X", "Center Y", "Radius", "End X", "End Y", "Width", "Height", "Corner Radius", "Line Width", "Line Style", "Line Color", "Angle", "Fill", "Fill Color"),
  // eeschema/sch_shape.cpp SCH_SHAPE_DESC
  ...inherits("SCH_SHAPE", "SCH_ITEM", "EDA_SHAPE"),
  ...add("SCH_SHAPE", SHAPE, "Fill Mode"),
  // eeschema/sch_textbox.cpp SCH_TEXTBOX_DESC
  ...inherits("SCH_TEXTBOX", "SCH_SHAPE", "EDA_SHAPE", "EDA_TEXT"),
  ...mask("SCH_TEXTBOX", "EDA_SHAPE", "Shape", "Corner Radius"),
  ...mask("SCH_TEXTBOX", "EDA_TEXT", "Width", "Height", "Thickness"),
  ...add("SCH_TEXTBOX", "Margins", "Margin Left", "Margin Top", "Margin Right", "Margin Bottom"),
  ...add("SCH_TEXTBOX", TEXT, "Text Size"),
  ...mask("SCH_TEXTBOX", "EDA_TEXT", "Orientation"),
  // eeschema/sch_field.cpp SCH_FIELD_DESC
  ...inherits("SCH_FIELD", "SCH_ITEM", "EDA_TEXT"),
  ...mask("SCH_FIELD", "SCH_ITEM", "Locked"),
  { replace: ["SCH_FIELD", "EDA_TEXT", "Horizontal Justification", TEXT] },
  { replace: ["SCH_FIELD", "EDA_TEXT", "Vertical Justification", TEXT] },
  ...add("SCH_FIELD", "", "Show Field Name", "Allow Autoplacement"),
  ...mask("SCH_FIELD", "EDA_TEXT", "Hyperlink", "Thickness", "Mirrored", "Width", "Height"),
  ...add("SCH_FIELD", TEXT, "Text Size"),
  ...mask("SCH_FIELD", "EDA_TEXT", "Orientation"),
];

function kicadRegistry(): PropertyManager<PropItem, unknown, unknown> {
  const pm = new PropertyManager<PropItem, unknown, unknown>();
  for (const d of KICAD) {
    if ("inherits" in d) pm.inheritsAfter(d.inherits[0], d.inherits[1]);
    else if ("mask" in d) pm.mask(d.mask[0], d.mask[1], d.mask[2]);
    else if ("replace" in d) pm.replaceProperty(d.replace[1], d.replace[2], { owner: d.replace[0], name: d.replace[2], kind: "string", get: () => "" }, d.replace[3]);
    else
      pm.addProperty(
        {
          owner: d.add[0],
          name: d.add[1],
          kind: "string",
          get: () => "",
          ...(d.hidden === "all" ? { hidden: true } : d.hidden === "design" ? { hiddenFromDesignEditors: true } : d.hidden === "library" ? { hiddenFromLibraryEditors: true } : {}),
        },
        d.add[2]
      );
  }
  pm.rebuild();
  return pm;
}

const LIB = { libraryEditor: true };

for (const type of ["SCH_PIN", "SCH_SHAPE", "SCH_TEXT", "SCH_TEXTBOX", "SCH_FIELD", "LIB_SYMBOL"]) {
  test(`${type} in the Symbol Editor: the studio's rows are KiCad's, in KiCad's groups and order`, () => {
    assert.deepEqual(registeredRows(SYM_PROPERTIES, type, LIB), registeredRows(kicadRegistry(), type, LIB));
  });
}

test("the table itself: a library pin lists Unit, Body Style and Private (hidden from the design editors) and no Locked", () => {
  const rows = registeredRows(kicadRegistry(), "SCH_PIN", LIB);
  assert.deepEqual(rows.slice(0, 3), ["/Unit", "/Body Style", "/Private"]);
  assert.ok(!rows.includes("/Locked"));
  assert.ok(registeredRows(kicadRegistry(), "SCH_PIN").every((r) => !r.endsWith("/Unit")), "a design editor does not list them");
  assert.deepEqual(
    registeredRows(kicadRegistry(), "SCH_FIELD", LIB).filter((r) => r.startsWith(TEXT)),
    [`${TEXT}/Text`, `${TEXT}/Font`, `${TEXT}/Auto Thickness`, `${TEXT}/Italic`, `${TEXT}/Bold`, `${TEXT}/Visible`, `${TEXT}/Color`, `${TEXT}/Horizontal Justification`, `${TEXT}/Vertical Justification`, `${TEXT}/Text Size`]
  );
});

// ------------------------------------------------------------------------------------------------------------------------- the rows' behaviour

const symbol = (): LibrarySymbol =>
  ({
    lib_id: "Lib:U",
    reference_prefix: "U",
    description: "",
    keywords: "amp",
    datasheet: "",
    power: false,
    in_bom: true,
    on_board: true,
    pin_numbers_hidden: false,
    pin_names_hidden: false,
    pin_name_offset_mm: 0.508,
    unit_count: 2,
    has_alternate_body_style: false,
    footprint_filters: [],
    published: false,
    fields: [{ name: "Value", text: "OPA", at: { x: 0, y: 2.54 } }, { name: "Note", text: "n", private: true }],
    graphics: [
      { kind: "rectangle", id: "r", unit: 0, body_style: 1, start: { x: -2.54, y: 2.54 }, end: { x: 2.54, y: -2.54 }, stroke_mm: 0.254, fill: "background" },
      { kind: "text", id: "t", unit: 1, body_style: 1, text: "+", at: { x: 1, y: 1 }, angle_deg: 0, size_mm: 1.27 },
    ],
    pins: [{ id: "p", number: "1", name: "IN", electrical_type: "input", shape: "line", at: { x: -5.08, y: 1.27 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null }],
  }) as unknown as LibrarySymbol;
const ctx = (s = symbol()): SymCtx => ({ sym: s, own: s, libId: s.lib_id, units: "mm" });
const row = (ids: string[], name: string, c = ctx()) => symGrid(c, ids).groups.flatMap((g) => g.rows).find((r) => r.name === name);

test("a pin's rows read the pin: KiCad's internal Y (the file's negated), the orientation and the unit by name", () => {
  assert.equal(symGrid(ctx(), ["p"]).caption, "Pin");
  assert.deepEqual([row(["p"], "Position X")!.value, row(["p"], "Position Y")!.value], [-5_080_000, -1_270_000]);
  assert.deepEqual(row(["p"], "Orientation")!.choices!.map((c) => c.label), ["Right", "Left", "Up", "Down"]);
  assert.deepEqual(row(["p"], "Unit")!.choices!.map((c) => c.label), ["All units", "A", "B"]);
  assert.equal(row(["p"], "Body Style"), undefined, "one body style: not available");
  assert.equal(row(["p"], "Locked"), undefined);
});

test("an edit of a pin row is the pin with the value, one edit_symbol_pin; the length keeps the body end (ChangeLength)", () => {
  const plan = symEdit(ctx(), ["p"], "Pin Number", "7");
  assert.ok(plan.ok);
  if (plan.ok) assert.deepEqual(plan.cmds, [{ op: "edit_symbol_pin", lib_id: "Lib:U", id: "p", pin: { ...symbol().pins[0]!, number: "7" } }]);
  const len = symEdit(ctx(), ["p"], "Length", 5_080_000);
  assert.ok(len.ok && len.cmds.length === 1);
  if (len.ok) assert.deepEqual((len.cmds[0] as Extract<Cmd, { op: "edit_symbol_pin" }>).pin.at, { x: -7.62, y: 1.27 });
  const y = symEdit(ctx(), ["p"], "Position Y", 2_540_000);
  if (y.ok) assert.deepEqual((y.cmds[0] as Extract<Cmd, { op: "edit_symbol_pin" }>).pin.at, { x: -5.08, y: -2.54 });
  assert.deepEqual(symEdit(ctx(), ["p"], "Pin Number", "1"), { ok: true, cmds: [] }, "the same value: nothing");
});

test("a text's Bold, a shape's Fill Mode: the graphic as it is with the value (replace_symbol_graphic)", () => {
  const bold = symEdit(ctx(), ["t"], "Bold", true);
  if (bold.ok) assert.deepEqual(bold.cmds, [{ op: "replace_symbol_graphic", lib_id: "Lib:U", id: "t", graphic: { ...symbol().graphics[1]!, bold: true } }]);
  assert.equal(row(["r"], "Fill Mode")!.value, "background");
  assert.equal(row(["r"], "Fill")!.value, "solid");
  const fill = symEdit(ctx(), ["r"], "Fill Mode", "outline");
  if (fill.ok) assert.equal((fill.cmds[0] as Extract<Cmd, { op: "replace_symbol_graphic" }>).graphic.kind === "rectangle" && ((fill.cmds[0] as Extract<Cmd, { op: "replace_symbol_graphic" }>).graphic as { fill: string }).fill, "outline");
  // a colour of its own (UI-LEFT-2 YY2): `FILLED_WITH_COLOR`, its colour kept beside the shape
  const color = symEdit(ctx(), ["r"], "Fill Mode", "color");
  assert.ok(color.ok && (color.cmds[0] as Extract<Cmd, { op: "replace_symbol_graphic" }>).graphic.kind === "rectangle");
  assert.deepEqual([row(["r"], "Width")!.value, row(["r"], "Height")!.value], [5_080_000, 5_080_000]);
  assert.equal(row(["r"], "Line Style")!.writable, true);
});

test("a text's Font (UI-LEFT-2 CC1): the graphic's font (SetFontProp: Default Font is none); a field keeps none here", () => {
  assert.equal(row(["t"], "Font")!.value, "Default Font");
  assert.equal(row(["t"], "Font")!.writable, true);
  const face = symEdit(ctx(), ["t"], "Font", "DejaVu Sans");
  if (face.ok) assert.deepEqual(face.cmds, [{ op: "replace_symbol_graphic", lib_id: "Lib:U", id: "t", graphic: { ...symbol().graphics[1]!, font: "DejaVu Sans" } }]);
  const faced = symbol();
  (faced.graphics[1] as { font?: string }).font = "DejaVu Sans";
  assert.equal(row(["t"], "Font", ctx(faced))!.value, "DejaVu Sans");
  const back = symEdit(ctx(faced), ["t"], "Font", "Default Font");
  if (back.ok) assert.deepEqual(back.cmds, [{ op: "replace_symbol_graphic", lib_id: "Lib:U", id: "t", graphic: symbol().graphics[1]! }]);
  assert.deepEqual(symEdit(ctx(), ["t"], "Font", "Default Font"), { ok: true, cmds: [] }, "the same font: nothing");
  // a field's face is kept beside it (UI-LEFT-2 YY2)
  assert.equal(row([libFieldId("Value")], "Font")!.writable, true);
  const ff = symEdit(ctx(), [libFieldId("Value")], "Font", "DejaVu Sans");
  if (ff.ok) assert.deepEqual(ff.cmds, [{ op: "set_symbol_field_looks", lib_id: "Lib:U", name: "Value", looks: { font: "DejaVu Sans" } }]);
});

test("fields: two selected fields edited at once are one edit_library_symbol_fields; Private is a user field's", () => {
  const ids = [libFieldId("Value"), libFieldId("Note")];
  const plan = symEdit(ctx(), ids, "Italic", true);
  assert.ok(plan.ok);
  if (plan.ok) {
    assert.equal(plan.cmds.length, 1);
    const fields = (plan.cmds[0] as Extract<Cmd, { op: "edit_library_symbol_fields" }>).fields;
    assert.deepEqual(fields.filter((f) => f.italic).map((f) => f.name), ["Value", "Note"]);
    assert.equal(fields.find((f) => f.name === "Note")!.private, true, "the private flag goes through");
  }
  assert.equal(row([libFieldId("Value")], "Private"), undefined, "a mandatory field cannot be private");
  assert.equal(row([libFieldId("Note")], "Private")!.value, true);
});

test("nothing selected: the symbol itself (getSelection's fallback), its fields, definition, pin display, attributes and units", () => {
  const grid = symGrid(ctx(), []);
  assert.equal(grid.caption, "Symbol");
  assert.deepEqual(grid.groups.map((g) => g.caption), ["Fields", "Symbol Definition", "Pin Display", "Attributes", "Units and Body Styles"]);
  assert.equal(row([], "Value")!.value, "OPA");
  const kw = symEdit(ctx(), [], "Keywords", "opamp");
  if (kw.ok) assert.equal((kw.cmds[0] as Extract<Cmd, { op: "edit_symbol_properties" }>).keywords, "opamp");
  const ref = symEdit(ctx(), [], "Reference", "IC");
  if (ref.ok) assert.equal((ref.cmds[0] as Extract<Cmd, { op: "edit_library_symbol_fields" }>).fields.find((f) => f.name === "Reference")!.text, "IC");
  const inside = symEdit(ctx({ ...symbol(), pin_name_offset_mm: 0 }), [], "Place Pin Names Inside", true);
  if (inside.ok) assert.equal((inside.cmds[0] as Extract<Cmd, { op: "edit_symbol_properties" }>).pin_name_offset_mm, 0.508, "inside with no offset is 20 mil (SetPinNamesInsideProp)");
});

// ------------------------------------------------------------------------------------------------------------------------- UI-LEFT-2 YY2: the looks kept beside an item

const looksCmd = (plan: ReturnType<typeof symEdit>) => (plan.ok ? plan.cmds.find((c) => c.op === "set_symbol_graphic_looks") : undefined) as Extract<Cmd, { op: "set_symbol_graphic_looks" }> | undefined;

test("YY2: a shape's Private, Line Style, Line Color, Corner Radius and hatched Fill are set beside it (set_symbol_graphic_looks), the rest kept", () => {
  const s = symbol();
  (s as { kicad?: unknown }).kicad = { graphic_looks: { r: { hyperlink: "x" } }, stroke_types: { r: "dash" } };
  const c = ctx(s);
  assert.equal(row(["r"], "Private", c)!.writable, true);
  assert.equal(row(["r"], "Line Style", c)!.value, "dash");
  assert.deepEqual(looksCmd(symEdit(c, ["r"], "Private", true)), { op: "set_symbol_graphic_looks", lib_id: "Lib:U", id: "r", looks: { private: true, hyperlink: "x" }, line_style: "dash", thickness_mm: null });
  assert.equal(looksCmd(symEdit(c, ["r"], "Line Style", "dot"))!.line_style, "dot");
  assert.deepEqual(looksCmd(symEdit(c, ["r"], "Line Color", "#FF0000"))!.looks.stroke_color, { r: 255, g: 0, b: 0, a: 255 });
  // the corner radius: at most half the shorter side (5.08 mm square: 2.54 mm)
  assert.equal(looksCmd(symEdit(c, ["r"], "Corner Radius", 1_000_000))!.looks.radius_mm, 1);
  const big = symEdit(c, ["r"], "Corner Radius", 3_000_000);
  assert.ok(!big.ok && /less than or equal to 2\.54/.test(big.error), JSON.stringify(big));
  // a hatch: the fill is a colour fill with its hatch beside it; Fill Color is not writable for a hatch (`isFillColorEditable`)
  const hatch = symEdit(c, ["r"], "Fill", "cross_hatch");
  assert.ok(hatch.ok);
  if (hatch.ok) {
    assert.equal(((hatch.cmds[0] as Extract<Cmd, { op: "replace_symbol_graphic" }>).graphic as { fill: string }).fill, "color");
    assert.equal(looksCmd(hatch)!.looks.hatch, "cross_hatch");
  }
  // a pin's Private has no place in KiCad's file
  assert.equal(row(["p"], "Private", c)!.writable, false);
});

test("YY2: a text's Color, Hyperlink and Auto Thickness; Body Styles' setter does nothing", () => {
  const color = symEdit(ctx(), ["t"], "Color", "#00FF00");
  if (color.ok) assert.deepEqual((color.cmds[0] as Extract<Cmd, { op: "replace_symbol_graphic" }>).graphic, { ...symbol().graphics[1]!, text_color: { r: 0, g: 255, b: 0, a: 255 } });
  assert.equal(looksCmd(symEdit(ctx(), ["t"], "Hyperlink", "https://kicad.org"))!.looks.hyperlink, "https://kicad.org");
  assert.equal(row(["t"], "Auto Thickness")!.value, true);
  // off: the pen it is drawn with, an eighth of 1.27 mm (`GetPenSizeForNormal`)
  assert.equal(looksCmd(symEdit(ctx(), ["t"], "Auto Thickness", false))!.thickness_mm, 0.1588);
});

test("YY2: an arc's start moves about its centre, the middle point recomputed (`GetArcMid`)", async () => {
  const { arcWithEnd } = await import("./symItemProperties");
  // a quarter circle of radius 2 about the origin, from (2, 0) to (0, 2) through (sqrt2, sqrt2) (file Y up)
  const arc = { kind: "arc" as const, id: "a", unit: 0, body_style: 1, start: { x: 2, y: 0 }, mid: { x: Math.SQRT2, y: Math.SQRT2 }, end: { x: 0, y: 2 }, stroke_mm: 0, fill: "none" as const };
  const moved = arcWithEnd(arc, "start", { x: 0, y: -2 });
  // the start at (0, -2): the arc now runs a half circle the other way round from start to end, its middle at (-2, 0)... or (2, 0)
  assert.deepEqual(moved.start, { x: 0, y: -2 });
  assert.equal(Math.hypot(moved.mid.x, moved.mid.y).toFixed(3), "2.000", "the middle is on the circle about the kept centre");
});
