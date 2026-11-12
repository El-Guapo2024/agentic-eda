// KiCad's `PROPERTY_MANAGER` registrations of the board item classes at KiCad 8303b2ad, transcribed call for call (moved here from pcbPropertiesKicad.test.ts so that the
// Footprint Editor's Properties pane, kicad-port/fpItemProperties.ts, is tested against the same table: KiCad's footprint editor runs `PCB_PROPERTIES_PANEL` on the
// same classes, UI-LEFT Y3) -- `BOARD_ITEM_DESC` (pcbnew/board_item.cpp), `BOARD_CONNECTED_ITEM_DESC` (board_connected_item.cpp), `FOOTPRINT_DESC` (footprint.cpp),
// `PAD_DESC` (pad.cpp), `TRACK_VIA_DESC` (pcb_track.cpp), `EDA_TEXT_DESC` (common/eda_text.cpp), `PCB_TEXT_DESC` (pcb_text.cpp), `PCB_FIELD_DESC` (pcb_field.cpp),
// `EDA_SHAPE_DESC` (common/eda_shape.cpp, with KiCad 25ec2335's line endings after "Fill Color"), `PCB_SHAPE_DESC` (pcb_shape.cpp), `DIMENSION_DESC` and its five
// subclasses (pcb_dimension.cpp) and `PCB_GROUP_DESC` (pcb_group.cpp). A hidden property (`SetIsHiddenFromPropertiesManager`) is in the table so the walk sees what
// KiCad's sees, and is not a row; `library` marks one `SetIsHiddenFromLibraryEditors` (the Footprint Editor's panel leaves it out).
//
// Pure: no React, no DOM.
import { PropertyManager, type PropItem } from "./propertyManager";

export type Decl =
  | { inherits: [string, string] }
  | { add: [owner: string, name: string, group?: string]; hidden?: boolean; library?: boolean }
  | { replace: [base: string, old: string, owner: string, name: string, group?: string]; hidden?: boolean }
  | { mask: [derived: string, base: string, name: string] };

const TEXT = "Text Properties";
const SHAPE = "Shape Properties";
const DIM = "Dimension Properties";

const add = (owner: string, group: string, ...names: string[]): Decl[] => names.map((name) => ({ add: [owner, name, group] }));
const hidden = (owner: string, name: string): Decl => ({ add: [owner, name, ""], hidden: true });
const inherits = (derived: string, ...bases: string[]): Decl[] => bases.map((b) => ({ inherits: [derived, b] }));
const mask = (derived: string, base: string, ...names: string[]): Decl[] => names.map((n) => ({ mask: [derived, base, n] }));

export const KICAD_PCB_REGISTRATIONS: Decl[] = [
  // common/eda_item.cpp EDA_ITEM_DESC
  hidden("EDA_ITEM", "Type"),
  // pcbnew/board_item.cpp BOARD_ITEM_DESC
  ...inherits("BOARD_ITEM", "EDA_ITEM"),
  hidden("BOARD_ITEM", "Parent"),
  ...add("BOARD_ITEM", "", "Position X", "Position Y", "Layer", "Locked"),
  // pcbnew/board_connected_item.cpp BOARD_CONNECTED_ITEM_DESC
  ...inherits("BOARD_CONNECTED_ITEM", "BOARD_ITEM"),
  { replace: ["BOARD_ITEM", "Layer", "BOARD_CONNECTED_ITEM", "Layer"] },
  { add: ["BOARD_CONNECTED_ITEM", "Net", ""], library: true },
  hidden("BOARD_CONNECTED_ITEM", "Net Class"),
  hidden("BOARD_CONNECTED_ITEM", "NetClass"),
  hidden("BOARD_CONNECTED_ITEM", "NetName"),
  ...add("BOARD_CONNECTED_ITEM", "Teardrops", "Enable Teardrops", "Best Length Ratio", "Max Length", "Best Width Ratio", "Max Width", "Curved Teardrops", "Prefer Zone Connections", "Allow Teardrops To Span Two Tracks", "Max Width Ratio"),
  // pcbnew/footprint.cpp FOOTPRINT_DESC
  ...inherits("FOOTPRINT", "BOARD_ITEM", "BOARD_ITEM_CONTAINER"),
  { replace: ["BOARD_ITEM", "Layer", "FOOTPRINT", "Layer"] },
  ...add("FOOTPRINT", "", "Orientation"),
  ...add("FOOTPRINT", "Fields", "Reference"),
  ...add("FOOTPRINT", "Footprint Properties", "Library Link", "Library Description", "Keywords"),
  { add: ["FOOTPRINT", "Component Class", "Footprint Properties"], library: true },
  ...add("FOOTPRINT", "Attributes", "Not in Schematic", "Exclude From Position Files", "Exclude From Bill of Materials", "Do not Populate"),
  ...add("FOOTPRINT", "Overrides", "Exempt From Courtyard Requirement", "Clearance Override", "Solderpaste Margin Override", "Solderpaste Margin Ratio Override", "Zone Connection Style"),
  // pcbnew/pad.cpp PAD_DESC
  ...inherits("PAD", "BOARD_ITEM", "BOARD_CONNECTED_ITEM"),
  ...mask("PAD", "BOARD_CONNECTED_ITEM", "Layer"),
  ...mask("PAD", "BOARD_ITEM", "Locked"),
  ...add("PAD", "", "Orientation"),
  ...add("PAD", "Pad Properties", "Pad Type", "Pad Shape", "Pad Number"),
  { add: ["PAD", "Pin Name", "Pad Properties"], library: true },
  { add: ["PAD", "Pin Type", "Pad Properties"], library: true },
  ...add("PAD", "Pad Properties", "Size X", "Size Y", "Corner Radius Ratio", "Corner Radius Size", "Hole Shape", "Hole Size X", "Hole Size Y"),
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
  // pcbnew/zone.cpp ZONE_DESC (the position and the layer replaced by hidden ones: "they aren't useful in current form")
  ...inherits("ZONE", "BOARD_CONNECTED_ITEM"),
  { replace: ["BOARD_ITEM", "Position X", "ZONE", "Position X"], hidden: true },
  { replace: ["BOARD_ITEM", "Position Y", "ZONE", "Position Y"], hidden: true },
  { replace: ["BOARD_CONNECTED_ITEM", "Layer", "ZONE", "Layer"], hidden: true },
  ...add("ZONE", "", "Priority", "Name"),
  ...add("ZONE", "Keepout", "Keep Out Tracks", "Keep Out Vias", "Keep Out Pads", "Keep Out Zone Fills", "Keep Out Footprints"),
  ...add("ZONE", "Placement", "Enable", "Source Type", "Source Name"),
  ...add("ZONE", "Fill Style", "Fill Mode", "Hatch Orientation", "Hatch Width", "Hatch Gap", "Hatch Minimum Hole Ratio", "Smoothing Effort", "Smoothing Amount", "Remove Islands", "Minimum Island Area"),
  ...add("ZONE", "Electrical", "Clearance", "Minimum Width", "Pad Connections", "Thermal Relief Gap", "Thermal Relief Spoke Width"),
  // pcbnew/pcb_textbox.cpp PCB_TEXTBOX_DESC ("Filled" is not a property of EDA_SHAPE's -- its is "Fill" -- so that mask masks nothing, as in KiCad)
  ...inherits("PCB_TEXTBOX", "PCB_SHAPE", "EDA_SHAPE", "EDA_TEXT"),
  ...mask("PCB_TEXTBOX", "EDA_SHAPE", "Shape", "Start X", "Start Y", "End X", "End Y", "Width", "Height", "Line Width", "Line Style", "Filled", "Line Color", "Corner Radius"),
  ...mask("PCB_TEXTBOX", "EDA_TEXT", "Color"),
  ...mask("PCB_TEXTBOX", "PCB_SHAPE", "Soldermask", "Soldermask Margin Override"),
  ...add("PCB_TEXTBOX", TEXT, "Knockout"),
  ...add("PCB_TEXTBOX", "Border Properties", "Border", "Border Style", "Border Width"),
  ...add("PCB_TEXTBOX", "Margins", "Margin Left", "Margin Top", "Margin Right", "Margin Bottom"),
  // 8303b2ad masks "Hyperlink" on PCB_TEXT here (`Mask( TYPE_HASH( PCB_TEXT ), ..)`, which leaves the text box's row); KiCad 25ec2335 masks it on PCB_TEXTBOX
  ...mask("PCB_TEXTBOX", "EDA_TEXT", "Hyperlink"),
  // pcbnew/pcb_table.cpp PCB_TABLE_DESC; KiCad 25ec2335 pcb_generated_table.cpp PCB_GENERATED_TABLE_DESC and pcb_drill_chart.cpp PCB_DRILL_CHART_DESC
  ...inherits("PCB_TABLE", "BOARD_ITEM", "BOARD_ITEM_CONTAINER"),
  ...add("PCB_TABLE", "", "Start X", "Start Y"),
  ...add("PCB_TABLE", "Table Properties", "External Border", "Header Border", "Border Width", "Border Style", "Border Color", "Row Separators", "Cell Separators", "Separators Width", "Separators Style", "Separators Color"),
  ...inherits("PCB_GENERATED_TABLE", "PCB_TABLE"),
  ...add("PCB_GENERATED_TABLE", "Table Generator", "Units", "Decimal Places"),
  ...inherits("PCB_DRILL_CHART", "PCB_GENERATED_TABLE"),
  ...add("PCB_DRILL_CHART", "Drill Chart Properties", "Show Totals"),
  // pcbnew/pcb_tablecell.cpp PCB_TABLECELL_DESC
  ...inherits("PCB_TABLECELL", "BOARD_ITEM", "BOARD_CONNECTED_ITEM", "PCB_TEXTBOX", "PCB_SHAPE", "EDA_SHAPE", "EDA_TEXT"),
  ...mask("PCB_TABLECELL", "BOARD_ITEM", "Position X", "Position Y"),
  ...mask("PCB_TABLECELL", "PCB_SHAPE", "Layer", "Soldermask", "Soldermask Margin Override"),
  ...mask("PCB_TABLECELL", "EDA_SHAPE", "Corner Radius"),
  ...mask("PCB_TABLECELL", "BOARD_CONNECTED_ITEM", "Net"),
  ...mask("PCB_TABLECELL", "PCB_TEXTBOX", "Border", "Border Style", "Border Width"),
  ...mask("PCB_TABLECELL", "EDA_SHAPE", "Start X", "Start Y", "End X", "End Y", "Shape", "Width", "Height", "Line Width", "Line Style", "Line Color"),
  ...mask("PCB_TABLECELL", "EDA_TEXT", "Orientation", "Hyperlink", "Color"),
  ...add("PCB_TABLECELL", "Table", "Column Width", "Row Height"),
  // pcbnew/pcb_reference_image.cpp PCB_REFERENCE_IMAGE_DESC
  ...inherits("PCB_REFERENCE_IMAGE", "BOARD_ITEM"),
  { replace: ["BOARD_ITEM", "Layer", "PCB_REFERENCE_IMAGE", "Associated Layer"] },
  ...add("PCB_REFERENCE_IMAGE", "Image Properties", "Scale", "Transform Offset X", "Transform Offset Y", "Width", "Height"),
  // pcbnew/pcb_point.cpp PCB_POINT_DESC, pcbnew/pcb_barcode.cpp PCB_BARCODE_DESC (the same at KiCad 25ec2335)
  ...inherits("PCB_POINT", "BOARD_ITEM"),
  ...add("PCB_POINT", "", "Size"),
  ...inherits("PCB_BARCODE", "BOARD_ITEM"),
  ...add("PCB_BARCODE", "Barcode Properties", "Text", "Show Text", "Text Size", "Width", "Height", "Orientation", "Barcode Type", "Error Correction", "Knockout", "Margin X", "Margin Y"),
  // KiCad 25ec2335: pcbnew/pcb_grid_item.cpp PCB_GRID_ITEM_DESC (its Layer made unavailable, `OverrideAvailability`), pcbnew/pcb_drill_map.cpp PCB_DRILL_MAP_DESC,
  // pcbnew/generators/pcb_via_stitch.cpp PCB_VIA_STITCH_DESC (its `InheritsAfter( PCB_GENERATOR )` adds nothing: `PCB_GENERATOR_DESC` is compiled only with GENERATOR_ORDER)
  ...inherits("PCB_GRID_ITEM", "BOARD_ITEM"),
  ...add("PCB_GRID_ITEM", "", "Grid Type", "Orientation", "Extent X", "Extent Y", "Spacing X", "Spacing Y", "Radius Extent", "Radius Spacing", "Phi Extent", "Phi Spacing", "Priority", "Tick Interval", "Affects Cursor Snap", "Affects Routing", "Affects Placement"),
  ...inherits("PCB_DRILL_MAP", "BOARD_ITEM"),
  ...add("PCB_DRILL_MAP", "Drill Map Properties", "Offset X", "Offset Y", "Symbol Size"),
  ...mask("PCB_DRILL_MAP", "BOARD_ITEM", "Position X", "Position Y"),
  ...add("PCB_DRILL_MAP", "Drill Map Properties", "Hole Span", "Outline Slots", "Guide Cross"),
  ...inherits("PCB_VIA_STITCH", "BOARD_ITEM"),
  ...add("PCB_VIA_STITCH", "", "Size", "Drill", "Pitch", "Layout", "Mode", "Seed", "Net", "Guarded Net"),
  ...mask("PCB_VIA_STITCH", "BOARD_ITEM", "Layer"),
];

/** The table replayed through the manager. */
export function kicadPcbRegistry(): PropertyManager<PropItem, unknown, unknown> {
  const pm = new PropertyManager<PropItem, unknown, unknown>();
  for (const d of KICAD_PCB_REGISTRATIONS) {
    if ("inherits" in d) pm.inheritsAfter(d.inherits[0], d.inherits[1]);
    else if ("mask" in d) pm.mask(d.mask[0], d.mask[1], d.mask[2]);
    else if ("replace" in d) {
      const [base, old, owner, name, group] = d.replace;
      pm.replaceProperty(base, old, { owner, name, kind: "string", get: () => "", ...(d.hidden ? { hidden: true } : {}) }, group ?? "");
    } else {
      const [owner, name, group] = d.add;
      pm.addProperty({ owner, name, kind: "string", get: () => "", ...(d.hidden ? { hidden: true } : {}), ...(d.library ? { hiddenFromLibraryEditors: true } : {}) }, group ?? "");
    }
  }
  pm.rebuild();
  return pm;
}
