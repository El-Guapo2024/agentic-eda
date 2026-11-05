// The board editor's Properties panel against KiCad's own registrations: for every item class the panel lists, the rows the studio's registry gives
// (`registeredRows`: every property that is not hidden from the panel, in its group and display order) are the rows KiCad's give.
//
// KiCad's side is a table transcribed from the `PROPERTY_MANAGER` calls of KiCad 8303b2ad -- `BOARD_ITEM_DESC` (pcbnew/board_item.cpp),
// `BOARD_CONNECTED_ITEM_DESC` (board_connected_item.cpp), `FOOTPRINT_DESC` (footprint.cpp), `PAD_DESC` (pad.cpp), `TRACK_VIA_DESC` (pcb_track.cpp),
// `EDA_TEXT_DESC` (common/eda_text.cpp), `PCB_TEXT_DESC` (pcb_text.cpp), `PCB_FIELD_DESC` (pcb_field.cpp), `EDA_SHAPE_DESC` (common/eda_shape.cpp, with
// KiCad 25ec2335's line endings after "Fill Color"), `PCB_SHAPE_DESC` (pcb_shape.cpp), `DIMENSION_DESC` and its five subclasses (pcb_dimension.cpp) and
// `PCB_GROUP_DESC` (pcb_group.cpp) -- call for call, in the order each file makes them, and replayed through the same port of the manager
// (`propertyManager.ts`, whose walk has tests of its own). A hidden property (`SetIsHiddenFromPropertiesManager`) is in the table so the walk sees what
// KiCad's sees, and is not a row. Zones, text boxes, tables, barcodes, points and the KiCad 10 items are not in this table.
import { test } from "node:test";
import assert from "node:assert/strict";
import { PropertyManager, type PropItem } from "./propertyManager";
import { registeredRows } from "./propertyGrid";
import { PCB_PROPERTIES } from "./pcbProperties";

type Decl =
  | { inherits: [string, string] }
  | { add: [owner: string, name: string, group?: string]; hidden?: boolean }
  | { replace: [base: string, old: string, owner: string, name: string, group?: string] }
  | { mask: [derived: string, base: string, name: string] };

const TEXT = "Text Properties";
const SHAPE = "Shape Properties";
const DIM = "Dimension Properties";

const add = (owner: string, group: string, ...names: string[]): Decl[] => names.map((name) => ({ add: [owner, name, group] }));
const hidden = (owner: string, name: string): Decl => ({ add: [owner, name, ""], hidden: true });
const inherits = (derived: string, ...bases: string[]): Decl[] => bases.map((b) => ({ inherits: [derived, b] }));
const mask = (derived: string, base: string, ...names: string[]): Decl[] => names.map((n) => ({ mask: [derived, base, n] }));

const KICAD: Decl[] = [
  // common/eda_item.cpp EDA_ITEM_DESC
  hidden("EDA_ITEM", "Type"),
  // pcbnew/board_item.cpp BOARD_ITEM_DESC
  ...inherits("BOARD_ITEM", "EDA_ITEM"),
  hidden("BOARD_ITEM", "Parent"),
  ...add("BOARD_ITEM", "", "Position X", "Position Y", "Layer", "Locked"),
  // pcbnew/board_connected_item.cpp BOARD_CONNECTED_ITEM_DESC
  ...inherits("BOARD_CONNECTED_ITEM", "BOARD_ITEM"),
  { replace: ["BOARD_ITEM", "Layer", "BOARD_CONNECTED_ITEM", "Layer"] },
  ...add("BOARD_CONNECTED_ITEM", "", "Net"),
  hidden("BOARD_CONNECTED_ITEM", "Net Class"),
  hidden("BOARD_CONNECTED_ITEM", "NetClass"),
  hidden("BOARD_CONNECTED_ITEM", "NetName"),
  ...add("BOARD_CONNECTED_ITEM", "Teardrops", "Enable Teardrops", "Best Length Ratio", "Max Length", "Best Width Ratio", "Max Width", "Curved Teardrops", "Prefer Zone Connections", "Allow Teardrops To Span Two Tracks", "Max Width Ratio"),
  // pcbnew/footprint.cpp FOOTPRINT_DESC
  ...inherits("FOOTPRINT", "BOARD_ITEM", "BOARD_ITEM_CONTAINER"),
  { replace: ["BOARD_ITEM", "Layer", "FOOTPRINT", "Layer"] },
  ...add("FOOTPRINT", "", "Orientation"),
  ...add("FOOTPRINT", "Fields", "Reference"),
  ...add("FOOTPRINT", "Footprint Properties", "Library Link", "Library Description", "Keywords", "Component Class"),
  ...add("FOOTPRINT", "Attributes", "Not in Schematic", "Exclude From Position Files", "Exclude From Bill of Materials", "Do not Populate"),
  ...add("FOOTPRINT", "Overrides", "Exempt From Courtyard Requirement", "Clearance Override", "Solderpaste Margin Override", "Solderpaste Margin Ratio Override", "Zone Connection Style"),
  // pcbnew/pad.cpp PAD_DESC
  ...inherits("PAD", "BOARD_ITEM", "BOARD_CONNECTED_ITEM"),
  ...mask("PAD", "BOARD_CONNECTED_ITEM", "Layer"),
  ...mask("PAD", "BOARD_ITEM", "Locked"),
  ...add("PAD", "", "Orientation"),
  ...add("PAD", "Pad Properties", "Pad Type", "Pad Shape", "Pad Number", "Pin Name", "Pin Type", "Size X", "Size Y", "Corner Radius Ratio", "Corner Radius Size", "Hole Shape", "Hole Size X", "Hole Size Y"),
  ...add("PAD", "Post-machining Properties", "Top Post-machining", "Top Post-machining Size", "Top Counterbore Depth", "Top Countersink Angle", "Bottom Post-machining", "Bottom Post-machining Size", "Bottom Counterbore Depth", "Bottom Countersink Angle"),
  ...add("PAD", "Backdrill Properties", "Backdrill Mode", "Bottom Backdrill Size", "Bottom Backdrill Must-Cut", "Top Backdrill Size", "Top Backdrill Must-Cut"),
  ...add("PAD", "Pad Properties", "Fabrication Property", "Copper Layers", "Pad To Die Length", "Pad To Die Delay"),
  ...add("PAD", "Overrides", "Clearance Override", "Soldermask Margin Override", "Solderpaste Margin Override", "Solderpaste Margin Ratio Override", "Zone Connection Style", "Thermal Relief Spoke Width", "Thermal Relief Spoke Angle", "Thermal Relief Gap"),
  // pcbnew/pcb_track.cpp TRACK_VIA_DESC
  ...inherits("PCB_TRACK", "BOARD_CONNECTED_ITEM"),
  ...add("PCB_TRACK", "", "Width"),
  { replace: ["BOARD_ITEM", "Position X", "PCB_TRACK", "Start X"] },
  { replace: ["BOARD_ITEM", "Position Y", "PCB_TRACK", "Start Y"] },
  ...add("PCB_TRACK", "", "End X", "End Y"),
  ...add("PCB_TRACK", "Technical Layers", "Soldermask", "Soldermask Margin Override"),
  ...inherits("PCB_ARC", "PCB_TRACK"),
  ...inherits("PCB_VIA", "BOARD_CONNECTED_ITEM"),
  ...mask("PCB_VIA", "BOARD_CONNECTED_ITEM", "Layer"),
  ...add("PCB_VIA", "Via Properties", "Diameter", "Hole", "Layer Top", "Layer Bottom", "Via Type", "Front tenting", "Back tenting", "Front covering", "Back covering", "Front plugging", "Back plugging", "Capping", "Filling"),
  ...add("PCB_VIA", "Backdrill", "Backdrill Mode", "Bottom Backdrill Size", "Bottom Backdrill Must-Cut", "Top Backdrill Size", "Top Backdrill Must-Cut"),
  ...add("PCB_VIA", "Post-machining", "Front Post-machining", "Front Post-machining Size", "Front Post-machining Depth", "Front Post-machining Angle", "Back Post-machining", "Back Post-machining Size", "Back Post-machining Depth", "Back Post-machining Angle"),
  // common/eda_text.cpp EDA_TEXT_DESC
  ...add("EDA_TEXT", "", "Orientation"),
  ...add("EDA_TEXT", TEXT, "Text", "Font", "Auto Thickness", "Thickness", "Italic", "Bold", "Mirrored", "Visible", "Width", "Height", "Horizontal Justification", "Vertical Justification", "Color", "Hyperlink"),
  // pcbnew/pcb_text.cpp PCB_TEXT_DESC
  ...inherits("PCB_TEXT", "BOARD_ITEM", "EDA_TEXT"),
  ...mask("PCB_TEXT", "EDA_TEXT", "Color"),
  ...add("PCB_TEXT", TEXT, "Knockout", "Keep Upright"),
  ...mask("PCB_TEXT", "EDA_TEXT", "Hyperlink"),
  // pcbnew/pcb_field.cpp PCB_FIELD_DESC
  ...inherits("PCB_FIELD", "BOARD_ITEM", "PCB_TEXT", "EDA_TEXT"),
  hidden("PCB_FIELD", "Name"),
  ...mask("PCB_FIELD", "EDA_TEXT", "Hyperlink", "Color"),
  // common/eda_shape.cpp EDA_SHAPE_DESC (8303b2ad), then the line endings of KiCad 25ec2335
  ...add("EDA_SHAPE", SHAPE, "Shape", "Start X", "Start Y", "Center X", "Center Y", "Radius", "End X", "End Y", "Width", "Height", "Corner Radius", "Line Width", "Line Style", "Line Color", "Angle", "Fill", "Fill Color"),
  ...add("EDA_SHAPE", SHAPE, "Start Shape", "Start Length", "Start Width", "Start Stroke Width", "End Shape", "End Length", "End Width", "End Stroke Width"),
  // pcbnew/pcb_shape.cpp PCB_SHAPE_DESC
  ...inherits("PCB_SHAPE", "BOARD_CONNECTED_ITEM", "EDA_SHAPE"),
  { replace: ["BOARD_CONNECTED_ITEM", "Layer", "PCB_SHAPE", "Layer"] },
  ...mask("PCB_SHAPE", "EDA_SHAPE", "Line Color", "Fill Color"),
  ...add("PCB_SHAPE", "Pad Primitives", "Number Box", "Thermal Spoke Template"),
  ...add("PCB_SHAPE", "Technical Layers", "Soldermask", "Soldermask Margin Override"),
  // pcbnew/pcb_dimension.cpp DIMENSION_DESC and the five kinds
  ...inherits("PCB_DIMENSION_BASE", "PCB_TEXT", "BOARD_ITEM", "EDA_TEXT"),
  ...mask("PCB_DIMENSION_BASE", "EDA_TEXT", "Orientation"),
  ...add("PCB_DIMENSION_BASE", DIM, "Prefix", "Suffix", "Override Text", "Text", "Units", "Units Format", "Precision", "Suppress Trailing Zeroes", "Arrow Direction"),
  ...add("PCB_DIMENSION_BASE", TEXT, "Keep Aligned with Dimension", "Orientation"),
  ...inherits("PCB_DIM_ALIGNED", "BOARD_ITEM", "EDA_TEXT", "PCB_TEXT", "PCB_DIMENSION_BASE"),
  ...add("PCB_DIM_ALIGNED", DIM, "Crossbar Height", "Extension Line Overshoot"),
  ...inherits("PCB_DIM_ORTHOGONAL", "BOARD_ITEM", "EDA_TEXT", "PCB_TEXT", "PCB_DIMENSION_BASE", "PCB_DIM_ALIGNED"),
  ...inherits("PCB_DIM_RADIAL", "BOARD_ITEM", "EDA_TEXT", "PCB_TEXT", "PCB_DIMENSION_BASE"),
  ...add("PCB_DIM_RADIAL", DIM, "Leader Length"),
  ...inherits("PCB_DIM_LEADER", "BOARD_ITEM", "EDA_TEXT", "PCB_TEXT", "PCB_DIMENSION_BASE"),
  ...add("PCB_DIM_LEADER", DIM, "Text Frame"),
  ...inherits("PCB_DIM_CENTER", "BOARD_ITEM", "EDA_TEXT", "PCB_TEXT", "PCB_DIMENSION_BASE"),
  // pcbnew/pcb_group.cpp PCB_GROUP_DESC
  ...inherits("PCB_GROUP", "BOARD_ITEM", "EDA_GROUP"),
  ...mask("PCB_GROUP", "BOARD_ITEM", "Position X", "Position Y", "Layer"),
  ...add("EDA_GROUP", "Group Properties", "Name"),
];

/** The table replayed through the manager. */
function kicadRegistry(): PropertyManager<PropItem, unknown, unknown> {
  const pm = new PropertyManager<PropItem, unknown, unknown>();
  for (const d of KICAD) {
    if ("inherits" in d) pm.inheritsAfter(d.inherits[0], d.inherits[1]);
    else if ("mask" in d) pm.mask(d.mask[0], d.mask[1], d.mask[2]);
    else if ("replace" in d) {
      const [base, old, owner, name, group] = d.replace;
      pm.replaceProperty(base, old, { owner, name, kind: "string", get: () => "" }, group ?? "");
    } else {
      const [owner, name, group] = d.add;
      pm.addProperty({ owner, name, kind: "string", get: () => "", ...(d.hidden ? { hidden: true } : {}) }, group ?? "");
    }
  }
  pm.rebuild();
  return pm;
}

const CLASSES = ["FOOTPRINT", "PAD", "PCB_TRACK", "PCB_ARC", "PCB_VIA", "PCB_TEXT", "PCB_FIELD", "PCB_SHAPE", "PCB_DIM_ALIGNED", "PCB_DIM_ORTHOGONAL", "PCB_DIM_RADIAL", "PCB_DIM_LEADER", "PCB_DIM_CENTER", "PCB_GROUP"];

for (const type of CLASSES) {
  test(`${type}: the studio's rows are KiCad's, in KiCad's groups and order`, () => {
    assert.deepEqual(registeredRows(PCB_PROPERTIES, type), registeredRows(kicadRegistry(), type));
  });
}

test("the table itself: a pad's rows as KiCad 8303b2ad lists them (the walk puts BOARD_ITEM's and BOARD_CONNECTED_ITEM's first)", () => {
  const rows = registeredRows(kicadRegistry(), "PAD");
  assert.deepEqual(rows.slice(0, 4), ["/Position X", "/Position Y", "/Net", "/Orientation"]);
  assert.ok(rows.includes("Teardrops/Enable Teardrops") && rows.includes("Overrides/Thermal Relief Gap"));
  assert.equal(rows.filter((r) => r.startsWith("Pad Properties/")).length, 16);
});
