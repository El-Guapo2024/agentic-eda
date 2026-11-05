// The schematic editor's Properties panel against KiCad's own registrations: for every item class the panel lists, the studio's rows (`registeredRows`) are
// KiCad's, in KiCad's groups and order.
//
// KiCad's side is a table transcribed from the `PROPERTY_MANAGER` calls of KiCad 8303b2ad -- `EDA_ITEM_DESC` (common/eda_item.cpp), `SCH_ITEM_DESC`
// (eeschema/sch_item.cpp), `SCH_SYMBOL_DESC` (sch_symbol.cpp), the three rows `LIB_SYMBOL_DESC` (lib_symbol.cpp) gives `SYMBOL` and so every placed symbol,
// `EDA_TEXT_DESC` (common/eda_text.cpp), `SCH_TEXT_DESC` (sch_text.cpp), `SCH_LABEL_DESC` and `SCH_DIRECTIVE_LABEL_DESC` (sch_label.cpp), `SCH_LINE_DESC`,
// `SCH_JUNCTION_DESC`, `SCH_BUS_ENTRY_DESC`, `SCH_SHEET_DESC`, `EDA_SHAPE_DESC` (common/eda_shape.cpp; the schematic has no line endings here), `SCH_SHAPE_DESC`,
// `SCH_TEXTBOX_DESC`, `SCH_RULE_AREA_DESC` and `SCH_BITMAP_DESC` -- call for call, replayed through the ported manager. The rows `SCH_PROPERTIES_PANEL::rebuildProperties`
// adds for a symbol's and a sheet's fields are tested apart (they depend on the selection). Tables, groups and the library editors' classes are not here.
import { test } from "node:test";
import assert from "node:assert/strict";
import { PropertyManager, type PropItem } from "./propertyManager";
import { registeredRows } from "./propertyGrid";
import { SCH_PROPERTIES } from "./schItemProperties";

type Decl = { inherits: [string, string] } | { add: [owner: string, name: string, group: string]; hidden?: boolean } | { mask: [derived: string, base: string, name: string] };

const TEXT = "Text Properties";
const SHAPE = "Shape Properties";
const ATTRS = "Attributes";

const add = (owner: string, group: string, ...names: string[]): Decl[] => names.map((name) => ({ add: [owner, name, group] }));
const hidden = (owner: string, ...names: string[]): Decl[] => names.map((name) => ({ add: [owner, name, ""], hidden: true }));
const inherits = (derived: string, ...bases: string[]): Decl[] => bases.map((b) => ({ inherits: [derived, b] }));
const mask = (derived: string, base: string, ...names: string[]): Decl[] => names.map((n) => ({ mask: [derived, base, n] }));

const KICAD: Decl[] = [
  ...hidden("EDA_ITEM", "Type"),
  // eeschema/sch_item.cpp SCH_ITEM_DESC: Unit, Body Style and Private are hidden from the design editors
  ...inherits("SCH_ITEM", "EDA_ITEM"),
  ...add("SCH_ITEM", "", "Locked"),
  ...hidden("SCH_ITEM", "Unit", "Body Style", "Private"),
  // eeschema/sch_symbol.cpp SCH_SYMBOL_DESC (with SYMBOL's "Pin numbers" and "Pin names")
  ...inherits("SCH_SYMBOL", "SYMBOL", "SCH_ITEM"),
  ...add("SCH_SYMBOL", "", "Position X", "Position Y", "Orientation", "Mirror X", "Mirror Y"),
  ...add("SYMBOL", "", "Pin numbers", "Pin names"),
  ...add("SCH_SYMBOL", "Fields", "Reference", "Value", "Library Link", "Library Description", "Keywords"),
  ...add("SCH_SYMBOL", "", "Unit", "Body Style"),
  ...add("SCH_SYMBOL", ATTRS, "Exclude From Simulation", "Exclude From Bill of Materials", "Exclude From Board", "Exclude From Position Files", "Do not Populate"),
  // eeschema/lib_symbol.cpp LIB_SYMBOL_DESC: SYMBOL's own pin display rows
  ...add("SYMBOL", "Pin Display", "Show Pin Number", "Show Pin Name", "Pin Name Position Offset"),
  // common/eda_text.cpp EDA_TEXT_DESC
  ...add("EDA_TEXT", "", "Orientation"),
  ...add("EDA_TEXT", TEXT, "Text", "Font", "Auto Thickness", "Thickness", "Italic", "Bold", "Mirrored", "Visible", "Width", "Height", "Horizontal Justification", "Vertical Justification", "Color", "Hyperlink"),
  // eeschema/sch_text.cpp SCH_TEXT_DESC
  ...inherits("SCH_TEXT", "SCH_ITEM", "EDA_TEXT"),
  ...mask("SCH_TEXT", "EDA_TEXT", "Mirrored", "Width", "Height", "Thickness"),
  ...add("SCH_TEXT", TEXT, "Text Size"),
  ...mask("SCH_TEXT", "EDA_TEXT", "Orientation"),
  // eeschema/sch_label.cpp SCH_LABEL_DESC, SCH_DIRECTIVE_LABEL_DESC
  ...inherits("SCH_LABEL_BASE", "SCH_TEXT"),
  ...inherits("SCH_LABEL", "SCH_LABEL_BASE"),
  ...inherits("SCH_HIERLABEL", "SCH_LABEL_BASE"),
  ...inherits("SCH_GLOBALLABEL", "SCH_LABEL_BASE"),
  ...add("SCH_LABEL_BASE", "", "Shape"),
  ...mask("SCH_LABEL_BASE", "EDA_TEXT", "Hyperlink"),
  ...inherits("SCH_DIRECTIVE_LABEL", "SCH_LABEL_BASE"),
  ...add("SCH_DIRECTIVE_LABEL", "", "Shape", "Pin length"),
  ...mask("SCH_DIRECTIVE_LABEL", "EDA_TEXT", "Text", "Thickness", "Italic", "Bold", "Horizontal Justification", "Vertical Justification"),
  // eeschema/sch_line.cpp SCH_LINE_DESC
  ...inherits("SCH_LINE", "SCH_ITEM"),
  ...add("SCH_LINE", "", "Start X", "Start Y", "End X", "End Y", "Length", "Line Style", "Wire Style", "Line Width", "Color"),
  // eeschema/sch_junction.cpp, sch_bus_entry.cpp
  ...inherits("SCH_JUNCTION", "SCH_ITEM"),
  ...add("SCH_JUNCTION", "", "Diameter", "Color"),
  ...inherits("SCH_BUS_ENTRY_BASE", "SCH_ITEM"),
  ...inherits("SCH_BUS_WIRE_ENTRY", "SCH_BUS_ENTRY_BASE"),
  ...add("SCH_BUS_ENTRY_BASE", "", "Wire Style", "Line Width", "Color"),
  // eeschema/sch_sheet.cpp SCH_SHEET_DESC
  ...inherits("SCH_SHEET", "SCH_ITEM"),
  ...add("SCH_SHEET", "", "Sheet Name", "Border Width", "Border Color", "Background Color"),
  ...add("SCH_SHEET", ATTRS, "Exclude From Board", "Exclude From Simulation", "Exclude From Bill of Materials", "Do not Populate"),
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
  // eeschema/sch_rule_area.cpp SCH_RULE_AREA_DESC
  ...inherits("SCH_RULE_AREA", "SCH_SHAPE", "SCH_ITEM", "EDA_SHAPE"),
  ...add("SCH_RULE_AREA", ATTRS, "Exclude From Board", "Exclude From Simulation", "Exclude From Bill of Materials", "Do not Populate"),
  // eeschema/sch_bitmap.cpp SCH_BITMAP_DESC
  ...inherits("SCH_BITMAP", "SCH_ITEM"),
  ...add("SCH_BITMAP", "", "Position X", "Position Y"),
  ...add("SCH_BITMAP", "Image Properties", "Scale", "Transform Offset X", "Transform Offset Y", "Width", "Height"),
];

function kicadRegistry(): PropertyManager<PropItem, unknown, unknown> {
  const pm = new PropertyManager<PropItem, unknown, unknown>();
  for (const d of KICAD) {
    if ("inherits" in d) pm.inheritsAfter(d.inherits[0], d.inherits[1]);
    else if ("mask" in d) pm.mask(d.mask[0], d.mask[1], d.mask[2]);
    else pm.addProperty({ owner: d.add[0], name: d.add[1], kind: "string", get: () => "", ...(d.hidden ? { hidden: true } : {}) }, d.add[2]);
  }
  pm.rebuild();
  return pm;
}

/** The studio's rows of a class without the field rows the panel adds for a selection (`syncFieldRows`, tested in schItemProperties.test.ts). */
const studioRows = (type: string): string[] => registeredRows(SCH_PROPERTIES, type);

const CLASSES = ["SCH_SYMBOL", "SCH_TEXT", "SCH_LABEL", "SCH_GLOBALLABEL", "SCH_HIERLABEL", "SCH_DIRECTIVE_LABEL", "SCH_LINE", "SCH_JUNCTION", "SCH_BUS_WIRE_ENTRY", "SCH_SHEET", "SCH_SHAPE", "SCH_TEXTBOX", "SCH_RULE_AREA", "SCH_BITMAP"];

for (const type of CLASSES) {
  test(`${type}: the studio's rows are KiCad's, in KiCad's groups and order`, () => {
    assert.deepEqual(studioRows(type), registeredRows(kicadRegistry(), type));
  });
}

test("the table itself: a placed symbol's rows as KiCad 8303b2ad lists them (SYMBOL's first, then SCH_ITEM's Locked, then the symbol's own)", () => {
  const rows = registeredRows(kicadRegistry(), "SCH_SYMBOL");
  assert.deepEqual(rows.slice(0, 3), ["/Pin numbers", "/Pin names", "/Locked"]);
  assert.deepEqual(rows.slice(-3), ["Pin Display/Show Pin Number", "Pin Display/Show Pin Name", "Pin Display/Pin Name Position Offset"]);
  assert.deepEqual(registeredRows(kicadRegistry(), "SCH_LABEL").filter((r) => r.startsWith(TEXT)), [
    `${TEXT}/Text`,
    `${TEXT}/Font`,
    `${TEXT}/Auto Thickness`,
    `${TEXT}/Italic`,
    `${TEXT}/Bold`,
    `${TEXT}/Visible`,
    `${TEXT}/Horizontal Justification`,
    `${TEXT}/Vertical Justification`,
    `${TEXT}/Color`,
    `${TEXT}/Text Size`,
  ]);
});
