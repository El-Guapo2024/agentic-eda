// Import Graphics (`DIALOG_IMPORT_GRAPHICS`, pcbnew/import_gfx/dialog_import_graphics.cpp, and `DRAWING_TOOL::PlaceImportedGraphics`): what the dialog asks for, and the
// `import_graphics` command it makes of the answers. Pure: no store, no DOM; unit-tested in importGraphics.test.ts. The reader of the file and the items it
// becomes are the server's (`crates/kicad/src/{dxf_import,svg_import}.rs`, `crates/ops/src/import_graphics.rs`).
import type { ImportGraphicsCmd, ImportSchGraphicsCmd, ImportSymbolGraphicsCmd, PointXY } from "../api/types";
import { umFrom, type LengthUnit } from "../state/units";

/** A length typed in the dialog ("1.5", "0.2mm", "10mil", `0.01"`), in micrometres; a bare number is in the display units. Null when it is not a length. */
export function parseLength(text: string, units: LengthUnit): number | null {
  const m = /^\s*(-?\d*\.?\d+)\s*(mm|mil|in|")?\s*$/i.exec(text);
  if (!m) return null;
  const unit: LengthUnit = m[2] ? (m[2] === '"' ? "in" : (m[2].toLowerCase() as LengthUnit)) : units;
  return Math.round(umFrom(parseFloat(m[1]!), unit));
}

export type GraphicsFormat = "dxf" | "svg";

/** `GRAPHICS_IMPORT_MGR::GetPluginByExt`: the format a file name's extension names. */
export function graphicsFormatOf(fileName: string): GraphicsFormat | null {
  const ext = fileName.includes(".") ? fileName.slice(fileName.lastIndexOf(".") + 1).toLowerCase() : "";
  return ext === "dxf" || ext === "svg" ? ext : null;
}

/** "DXF default units:" -- `dxfUnitsMap`, in the order its keys (`DXF_IMPORT_UNITS`) sort in. */
export const DXF_UNITS: ReadonlyArray<{ id: string; label: string }> = [
  { id: "inch", label: "Inches" },
  { id: "feet", label: "Feet" },
  { id: "mm", label: "Millimeters" },
  { id: "cm", label: "Centimeter" },
  { id: "mils", label: "Mils" },
];

/**
 * The layers the dialog's "Layer:" box offers (`PCB_LAYER_BOX_SELECTOR`): the board's copper layers, then the technical and user layers in KiCad's order.
 */
export function importLayers(copper: readonly string[]): string[] {
  const technical = ["B.Adhes", "F.Adhes", "B.Paste", "F.Paste", "B.SilkS", "F.SilkS", "B.Mask", "F.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "B.CrtYd", "F.CrtYd", "B.Fab", "F.Fab"];
  const user = Array.from({ length: 9 }, (_, i) => `User.${i + 1}`);
  return [...copper, ...technical, ...user];
}

/** What the dialog's controls hold (strings are what is typed; lengths are in the display units). */
export interface ImportValues {
  /** "Import scale:" */
  scale: string;
  /** "DXF default line width:" */
  lineWidth: string;
  /** "DXF default units:" (an id of DXF_UNITS) */
  dxfUnits: string;
  /** "Place at:" -- unchecked is interactive placement. */
  placeAt: boolean;
  x: string;
  y: string;
  /** "Layer:" -- unchecked uses the Board Editor's active layer. */
  setLayer: boolean;
  layer: string;
  /** "Group imported items" */
  group: boolean;
  /** "Fix discontinuities" and its "Tolerance:" */
  fix: boolean;
  tolerance: string;
  /** The studio's own option: the imported Edge.Cuts become the board's outline (see `ImportGraphics::replace_outline`). */
  replaceOutline: boolean;
}

/** The dialog as it opens: the defaults of `dialog_import_graphics_base.cpp` (the DXF unit is millimetres, the board's own). */
export function defaultImportValues(units: LengthUnit): ImportValues {
  const len = (mm: number) => String(units === "mm" ? mm : units === "mil" ? Number((mm / 0.0254).toFixed(2)) : Number((mm / 25.4).toFixed(5)));
  return { scale: "1.0", lineWidth: len(0.2), dxfUnits: "mm", placeAt: false, x: len(0), y: len(0), setLayer: true, layer: "Dwgs.User", group: true, fix: true, tolerance: len(1), replaceOutline: true };
}

/** `POST /api/import_graphics/read`'s reply (`crates/cli/src/graphics_api.rs`). */
export interface GraphicsInfo {
  ok: boolean;
  message?: string;
  items?: number;
  counts?: Record<string, number>;
  messages?: Array<{ text: string; count: number }>;
  /** The picture's box in micrometres at the scale asked, with no offset: x0, y0, x1, y1. */
  bbox_um?: [number, number, number, number] | null;
  width_mm?: number;
  height_mm?: number;
  fits?: boolean;
}

/** "Found 12 items: 4 lines, 4 arcs, ..." */
export function describeInfo(info: GraphicsInfo): string {
  if (!info.ok) return info.message ?? "The file cannot be read.";
  const names: Array<[string, string, string]> = [
    ["line", "line", "lines"],
    ["arc", "arc", "arcs"],
    ["circle", "circle", "circles"],
    ["curve", "curve", "curves"],
    ["polygon", "polygon", "polygons"],
    ["text", "text", "texts"],
  ];
  const parts = names.filter(([k]) => (info.counts?.[k] ?? 0) > 0).map(([k, one, many]) => `${info.counts![k]} ${info.counts![k] === 1 ? one : many}`);
  const items = info.items ?? 0;
  if (items === 0) return "No graphic items found in file.";
  return `${items} ${items === 1 ? "item" : "items"}: ${parts.join(", ")}`;
}

/** A float as the command carries it: six decimals at most, so it never prints in an exponent. */
const r6 = (v: number): number => Math.round(v * 1e6) / 1e6;

export interface BuiltImport {
  ok: true;
  /** The command, for where the picture goes: `at` is where the cursor is (the placement is interactive when "Place at" is off). */
  build: (at: PointXY | null) => ImportGraphicsCmd;
  /** The command asks for a placement at the cursor (`Place at` off). */
  interactive: boolean;
}

/**
 * The command the dialog makes (`TransferDataFromWindow`): the scale and the offset the way KiCad computes them -- the origin typed in "Place at" is
 * divided by the scale (`VECTOR2D origin( m_xOrigin.GetDoubleValue() / xscale, ... )`, so a scaled import is placed at origin / scale, as it is there). The
 * interactive placement puts the picture's upper left corner at the cursor (`PlaceImportedGraphics` moves the group so its top left item is there).
 */
export function buildImportCmd(args: {
  format: GraphicsFormat;
  text: string;
  values: ImportValues;
  units: LengthUnit;
  activeLayer: string | null;
  info: GraphicsInfo | null;
  /** The board's outline is the intent's polygon (so an outline imported on Edge.Cuts can replace it). */
  polygonOutline: boolean;
}): BuiltImport | { ok: false; error: string } {
  const v = args.values;
  const scale = Number(v.scale.trim());
  if (!(Number.isFinite(scale) && scale > 0)) return { ok: false, error: "The import scale must be a number greater than zero." };
  const layer = v.setLayer ? v.layer : (args.activeLayer ?? "");
  if (!layer) return { ok: false, error: "Please select a valid layer." };
  let lineWidthUm = 200;
  let fixUm: number | null = null;
  if (args.format === "dxf") {
    const w = parseLength(v.lineWidth, args.units);
    if (w == null || w < 0) return { ok: false, error: "The DXF default line width is not a length." };
    lineWidthUm = w;
  }
  if (v.fix) {
    const t = parseLength(v.tolerance, args.units);
    if (t == null || t < 0) return { ok: false, error: "The tolerance is not a length." };
    fixUm = t;
  }
  let originUm: PointXY = { x: 0, y: 0 };
  if (v.placeAt) {
    const x = parseLength(v.x, args.units);
    const y = parseLength(v.y, args.units);
    if (x == null || y == null) return { ok: false, error: "The place-at coordinates are not lengths." };
    originUm = { x, y };
  }
  const replace = v.replaceOutline && layer === "Edge.Cuts" && args.polygonOutline;
  const base = {
    op: "import_graphics" as const,
    format: args.format,
    text: args.text,
    layer,
    dxf_units: args.format === "dxf" ? v.dxfUnits : "",
    scale: r6(scale),
    line_width_um: lineWidthUm,
    group: v.group,
    fix_tolerance_um: fixUm,
    replace_outline: replace,
  };
  if (v.placeAt) {
    // The importer's offset, in nanometres: the typed origin over the scale (micrometres to nanometres is a thousand).
    const cmd: ImportGraphicsCmd = { ...base, offset_x_nm: Math.round((originUm.x * 1000) / scale), offset_y_nm: Math.round((originUm.y * 1000) / scale) };
    return { ok: true, interactive: false, build: () => cmd };
  }
  return {
    ok: true,
    interactive: true,
    build: (at) => {
      // The picture's box at offset 0 is the server's `bbox_um`; the offset that takes its upper left corner to the cursor.
      const box = args.info?.bbox_um;
      const left = box ? box[0] : 0;
      const top = box ? box[1] : 0;
      const to = at ?? { x: 0, y: 0 };
      return { ...base, offset_x_nm: (to.x - left) * 1000, offset_y_nm: (to.y - top) * 1000 };
    },
  };
}

// -------------------------------------------------------------------------------------------------------------------- the schematic and the Symbol Editor

/**
 * What the Import Graphics dialog of the schematic editor and the Symbol Editor holds (`DIALOG_IMPORT_GFX_SCH`): the scale, the DXF's default line width and units, and where it goes --
 * "Interactive placement" (`placeAt` off) or "At" X, Y. It has no layer, no group and no discontinuities to fix.
 */
export interface SchImportValues {
  scale: string;
  lineWidth: string;
  dxfUnits: string;
  placeAt: boolean;
  x: string;
  y: string;
}

/** The dialog as it opens (`DIALOG_IMPORT_GFX_SCH`): scale 1, interactive placement, the DXF width 0 (the sheet's default width) and millimetres. */
export function defaultSchImportValues(): SchImportValues {
  return { scale: "1.0", lineWidth: "0", dxfUnits: "mm", placeAt: false, x: "0", y: "0" };
}

export interface BuiltSchImport {
  ok: true;
  /** The command, for where the picture goes: `at` is where the cursor is (the placement is interactive when "At" is off). */
  build: (at: PointXY | null) => ImportSchGraphicsCmd | ImportSymbolGraphicsCmd;
  /** The command asks for a placement at the cursor. */
  interactive: boolean;
}

/**
 * The command the dialog makes (`DIALOG_IMPORT_GFX_SCH::TransferDataFromWindow`): the same arithmetic as the board's -- the origin typed in "At" is divided by the scale. The interactive
 * placement of `SCH_DRAWING_TOOLS::ImportGraphics` moves the items by the cursor position (`delta = cursorPos`), so the drawing's own origin lands on the cursor.
 */
export function buildSchImportCmd(args: {
  editor: "schematic" | "symbol";
  format: GraphicsFormat;
  text: string;
  values: SchImportValues;
  units: LengthUnit;
  /** The symbol being edited and the unit and body style the new items are on (the Symbol Editor's). */
  symbol?: { libId: string; unit: number; bodyStyle: number };
}): BuiltSchImport | { ok: false; error: string } {
  const v = args.values;
  const scale = Number(v.scale.trim());
  if (!(Number.isFinite(scale) && scale > 0)) return { ok: false, error: "The import scale must be a number greater than zero." };
  let lineWidthUm = 0;
  if (args.format === "dxf") {
    const w = v.lineWidth.trim() === "" ? 0 : parseLength(v.lineWidth, args.units);
    if (w == null || w < 0) return { ok: false, error: "The DXF default line width is not a length." };
    lineWidthUm = w;
  }
  if (args.editor === "symbol" && !args.symbol) return { ok: false, error: "There is no symbol open to import into." };
  let originUm: PointXY = { x: 0, y: 0 };
  if (v.placeAt) {
    const x = parseLength(v.x, args.units);
    const y = parseLength(v.y, args.units);
    if (x == null || y == null) return { ok: false, error: "The placement coordinates are not lengths." };
    originUm = { x, y };
  }
  const make = (offsetXnm: number, offsetYnm: number): ImportSchGraphicsCmd | ImportSymbolGraphicsCmd => {
    const base = { format: args.format, text: args.text, dxf_units: args.format === "dxf" ? v.dxfUnits : "", scale: r6(scale), offset_x_nm: offsetXnm, offset_y_nm: offsetYnm, line_width_um: lineWidthUm };
    if (args.editor === "symbol") {
      const sym = args.symbol!;
      return { op: "import_symbol_graphics", lib_id: sym.libId, unit: sym.unit, body_style: sym.bodyStyle, ...base };
    }
    return { op: "import_sch_graphics", ...base };
  };
  if (v.placeAt) {
    // The importer's offset, in nanometres: the typed origin over the scale (micrometres to nanometres is a thousand).
    const cmd = make(Math.round((originUm.x * 1000) / scale), Math.round((originUm.y * 1000) / scale));
    return { ok: true, interactive: false, build: () => cmd };
  }
  return {
    ok: true,
    interactive: true,
    build: (at) => {
      const to = at ?? { x: 0, y: 0 };
      return make(Math.round(to.x * 1000), Math.round(to.y * 1000));
    },
  };
}
