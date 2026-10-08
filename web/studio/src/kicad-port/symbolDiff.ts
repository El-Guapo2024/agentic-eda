// Compare Symbol with Library (`eeschema.InspectionTool.diffSymbol`, `SCH_INSPECTION_TOOL::DiffSymbol` -> `LIB_SYMBOL::Compare( ..., ERC flags, reporter )`,
// eeschema/lib_symbol.cpp at 8303b2ad): the report of how the symbol the schematic draws differs from the library's.
//
// The studio keeps no separate copy of a library symbol in the schematic (it draws the library's, as `GET /api/schematic` resolves it), so the two
// sides here are the symbol the schematic is drawn from (`Schematic.lib_symbols`) and the library entry (the Symbol Editor's project entry when
// there is one -- which may carry edits not yet used by the schematic -- else the same resolved symbol). Fields are not compared: a placed symbol's
// reference, value, footprint and datasheet belong to the instance.
import type { LibGraphic, LibPin, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, LibSymbol } from "../api/types";

interface Pin {
  number: string;
  name: string;
  type: string;
  shape: string;
  x: number;
  y: number;
  angle: number;
  length: number;
  unit: number;
  bodyStyle: number;
}

const round = (v: number) => Math.round(v * 1e4) / 1e4; // 0.1 um: the file's resolution

const pinOfResolved = (p: LibPin): Pin => ({ number: p.number, name: p.name ?? "", type: p.electrical_type, shape: p.shape, x: round(p.at[0]), y: round(p.at[1]), angle: round(p.angle_deg), length: round(p.length_mm), unit: p.unit, bodyStyle: p.body_style || 1 });
const pinOfLibrary = (p: LibrarySymbolPin): Pin => ({ number: p.number, name: p.name, type: p.electrical_type, shape: p.shape, x: round(p.at.x), y: round(p.at.y), angle: round(p.angle_deg), length: round(p.length_mm), unit: p.unit, bodyStyle: p.body_style || 1 });

/** `SCH_PIN::GetItemDescription`: "Pin 1 (IN) [passive, line, (x, y), length]". */
function describePin(p: Pin): string {
  return `Pin ${p.number}${p.name && p.name !== "~" ? ` (${p.name})` : ""} [${p.type}, ${p.shape}, (${p.x}, ${p.y}) mm, length ${p.length} mm]`;
}

const pinKey = (p: Pin) => `${p.number}|${p.unit}|${p.bodyStyle}`;
const pinSame = (a: Pin, b: Pin) => a.name === b.name && a.type === b.type && a.shape === b.shape && a.x === b.x && a.y === b.y && a.angle === b.angle && a.length === b.length;

const pts = (list: ReadonlyArray<readonly [number, number]>) => list.map(([x, y]) => `(${round(x)}, ${round(y)})`).join(" ");
const fillOfResolved = (g: Exclude<LibGraphic, { kind: "text" }>): string => ("fill" in g ? String(g.fill) : "none");

/** A graphic item as one comparable line; `unit` and `body style` are part of what makes two items the same. */
function graphicOfResolved(g: LibGraphic): string {
  const where = `unit ${g.unit}, style ${g.body_style || 1}`;
  switch (g.kind) {
    case "rectangle":
      return `Rectangle ${pts([g.start, g.end])} stroke ${round(g.stroke_width)} fill ${fillOfResolved(g)} [${where}]`;
    case "polyline":
      return `Polyline ${pts(g.pts)} stroke ${round(g.stroke_width)} fill ${fillOfResolved(g)} [${where}]`;
    case "circle":
      return `Circle ${pts([g.center])} radius ${round(g.radius)} stroke ${round(g.stroke_width)} fill ${fillOfResolved(g)} [${where}]`;
    case "arc":
      return `Arc ${pts([g.start, g.mid, g.end])} stroke ${round(g.stroke_width)} fill ${fillOfResolved(g)} [${where}]`;
    case "text":
      return `Text '${g.content}' ${pts([g.at])} ${round(g.angle_deg)} deg size ${round(g.size_mm)} [${where}]`;
  }
}

function graphicOfLibrary(g: LibrarySymbolGraphic): string {
  const where = `unit ${g.unit}, style ${g.body_style || 1}`;
  const p = (a: { x: number; y: number }) => [a.x, a.y] as const;
  switch (g.kind) {
    case "rectangle":
      return `Rectangle ${pts([p(g.start), p(g.end)])} stroke ${round(g.stroke_mm)} fill ${g.fill} [${where}]`;
    case "polyline":
      return `Polyline ${pts(g.pts.map(p))} stroke ${round(g.stroke_mm)} fill ${g.fill} [${where}]`;
    case "circle":
      return `Circle ${pts([p(g.center)])} radius ${round(g.radius_mm)} stroke ${round(g.stroke_mm)} fill ${g.fill} [${where}]`;
    case "arc":
      return `Arc ${pts([p(g.start), p(g.mid), p(g.end)])} stroke ${round(g.stroke_mm)} fill ${g.fill} [${where}]`;
    case "text":
      return `Text '${g.text}' ${pts([p(g.at)])} ${round(g.angle_deg)} deg size ${round(g.size_mm)} [${where}]`;
  }
}

/**
 * The lines `LIB_SYMBOL::Compare` reports, in its order: power flag, unit count, graphic items (count, then one by one in a fixed order), pins the
 * schematic's symbol has that the library's lacks or has differently, pins it is missing. Empty means "No relevant differences detected."
 */
export function diffSymbols(schematic: LibSymbol & { power?: boolean }, library: LibrarySymbol): string[] {
  const out: string[] = [];

  if (!!schematic.power !== library.power) out.push("Power flag differs.");

  const schPins = schematic.pins.map(pinOfResolved);
  const libPins = library.pins.map(pinOfLibrary);
  const unitsOf = (pins: Pin[], graphics: Array<{ unit: number }>) => Math.max(1, ...pins.map((p) => p.unit), ...graphics.map((g) => g.unit));
  if (unitsOf(schPins, schematic.graphics) !== Math.max(unitsOf(libPins, library.graphics), library.unit_count)) out.push("Unit count differs.");

  const schShapes = schematic.graphics.map(graphicOfResolved).sort();
  const libShapes = library.graphics.map(graphicOfLibrary).sort();
  if (schShapes.length !== libShapes.length) {
    out.push("Graphic item count differs.");
  } else {
    schShapes.forEach((a, i) => {
      if (a !== libShapes[i]) out.push(`Graphic item differs: ${a}; ${libShapes[i]}.`);
    });
  }

  const libByKey = new Map(libPins.map((p) => [pinKey(p), p]));
  for (const p of schPins) {
    const other = libByKey.get(pinKey(p));
    if (!other) out.push(`Extra pin in schematic symbol: ${describePin(p)}.`);
    else if (!pinSame(p, other)) out.push(`Pin ${p.number} differs: ${describePin(p)}; ${describePin(other)}`);
  }
  const schKeys = new Set(schPins.map(pinKey));
  for (const p of libPins) if (!schKeys.has(pinKey(p))) out.push(`Missing pin in schematic symbol: ${describePin(p)}.`);

  return out;
}
