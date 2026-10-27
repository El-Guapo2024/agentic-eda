// Compare Symbol with Library (`eeschema.InspectionTool.diffSymbol`, `SCH_INSPECTION_TOOL::DiffSymbol` -> `LIB_SYMBOL::Compare( ..., ERC flags, reporter )`,
// eeschema/lib_symbol.cpp at 8303b2ad): the report of how the symbol the schematic draws differs from the library's.
//
// The studio keeps no separate copy of a library symbol in the schematic (it draws the library's, as `GET /api/schematic` resolves it), so the two
// sides here are the symbol the schematic is drawn from (`Schematic.lib_symbols`) and the library entry (the Symbol Editor's project entry when
// there is one -- which may carry edits not yet used by the schematic -- else the same resolved symbol). `DiffSymbol` gives the schematic's side the
// placed symbol's own fields, moved by `-symbol->GetPosition()`; so does `placed` here.
//
// What `Compare` reports under `COMPARE_FLAGS::ERC` (r6 C6-M4), in its order:
// - "Power flag differs." when `m_options` differ (normal, global power, local power);
// - "Unit count differs.";
// - the shapes (`SCH_SHAPE_T` only: a body text or text box is not a graphic item), sorted by `SCH_ITEM::cmp_items` (a `std::set`, so two equal
//   shapes are one), their count, then each pair by `SCH_SHAPE::compare` -> `SCH_ITEM::compare` (unit, body style, position) and
//   `EDA_SHAPE::Compare` (points within `EPSILON` 2 nm, the corner radius, the stroke width, its line style, the fill);
// - each schematic pin not found by number, unit and body style in the library ("Extra pin"), or found at another place: `SCH_ITEM::compare`
//   tests the type, unit, body style, `IsPrivate()` and position, and returns 0 under ERC, so a pin's name, type, shape, direction and length
//   are not compared;
// - each placed field the library lacks ("Extra field") or has at another place or privacy ("Field '%s' differs");
// - the footprint filters, keywords and pin name offset.
// "Missing pin" and "Missing field" are never printed: KiCad's second loops look the library's item up in the library itself
// (`aRhs.GetPin( .. )`, `aRhs.GetField( .. )`), so it is always found.
//
// Left: neither side's JSON carries a shape's line style or corner radius (both compare equal here), and the schematic side's keywords and
// footprint filters are compared only when the server sends them.
import type { LibGraphic, LibPin, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, LibSymbol, PinElectricalType, PinShape, SchField } from "../api/types";
import { messageTextFromValue } from "./messageText";
import { ELECTRICAL_TYPE_NAMES } from "./symPinText";
import { PIN_SHAPE_OPTIONS } from "./symPinAlternates";
import { allFields } from "./symFields";

/** The schematic's side: the resolved library symbol, with what the server may add of its options. */
export type SchematicLibSymbol = LibSymbol & { power?: boolean; power_local?: boolean; keywords?: string; footprint_filters?: string[] };

/** The placed symbol whose fields `DiffSymbol` gives the schematic's side. */
export interface PlacedSymbolFields {
  at: readonly [number, number];
  fields?: readonly SchField[];
}

/** mm (library, Y up) to KiCad's internal units (nm, Y down). */
const iu = (x: number, y: number): [number, number] => [Math.round(x * 1e6), Math.round(-y * 1e6)];
const iuLen = (v: number) => Math.round(v * 1e6);
const mm = (nm: number) => messageTextFromValue(nm, "mm");

interface Pin {
  number: string;
  name: string;
  type: PinElectricalType;
  shape: PinShape;
  pos: [number, number];
  unit: number;
  bodyStyle: number;
  hidden: boolean;
}

const pinOfResolved = (p: LibPin): Pin => ({ number: p.number, name: p.name ?? "", type: p.electrical_type, shape: p.shape, pos: iu(p.at[0], p.at[1]), unit: p.unit, bodyStyle: p.body_style, hidden: p.hidden });
const pinOfLibrary = (p: LibrarySymbolPin): Pin => ({ number: p.number, name: p.name, type: p.electrical_type, shape: p.shape, pos: iu(p.at.x, p.at.y), unit: p.unit, bodyStyle: p.body_style, hidden: p.hidden });

const shapeName = (s: PinShape) => PIN_SHAPE_OPTIONS.find((o) => o.value === s)?.label ?? s;

/** `SCH_PIN::getItemDescription`: "Pin 1 [IN, Input, Line]", "Hidden pin 1 [Passive, Line]". */
function describePin(p: Pin): string {
  const name = p.name === "~" ? "" : p.name;
  const what = [name, ELECTRICAL_TYPE_NAMES[p.type] ?? p.type, shapeName(p.shape)].filter((s) => s !== "").join(", ");
  return `${p.hidden ? "Hidden pin" : "Pin"} ${p.number} [${what}]`;
}

/** `LIB_SYMBOL::GetPin( number, unit, bodyStyle )` over `GetGraphicalPins`: a pin of unit 0 or body style 0 is in every unit or body style. */
function getPin(pins: readonly Pin[], number: string, unit: number, bodyStyle: number): Pin | undefined {
  return pins.find((p) => (!unit || !p.unit || p.unit === unit) && (!bodyStyle || !p.bodyStyle || p.bodyStyle === bodyStyle) && p.number === number);
}

// SHAPE_T: SEGMENT 0, RECTANGLE 1, ARC 2, CIRCLE 3, POLY 4, BEZIER 5
interface Shape {
  type: 1 | 2 | 3 | 4;
  unit: number;
  bodyStyle: number;
  start: [number, number];
  end: [number, number];
  mid?: [number, number];
  pts?: [number, number][];
  width: number;
  fill: string;
}

function circumcentre(a: [number, number], b: [number, number], c: [number, number]): [number, number] {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (d === 0) return [Math.round((a[0] + c[0]) / 2), Math.round((a[1] + c[1]) / 2)];
  const sq = (p: [number, number]) => p[0] * p[0] + p[1] * p[1];
  return [Math.round((sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d), Math.round((sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d)];
}

/** `EDA_SHAPE::getPosition`: an arc's centre, a polygon's first vertex, else the start. */
const position = (s: Shape): [number, number] => (s.type === 2 ? circumcentre(s.start, s.mid!, s.end) : s.type === 4 ? (s.pts![0] ?? s.start) : s.start);

function shapeOfResolved(g: LibGraphic): Shape | null {
  const base = { unit: g.unit, bodyStyle: g.body_style };
  switch (g.kind) {
    case "rectangle":
      return { ...base, type: 1, start: iu(...g.start), end: iu(...g.end), width: iuLen(g.stroke_width), fill: g.fill };
    case "polyline": {
      const pts = g.pts.map(([x, y]) => iu(x, y));
      return { ...base, type: 4, start: pts[0] ?? [0, 0], end: pts[pts.length - 1] ?? [0, 0], pts, width: iuLen(g.stroke_width), fill: g.fill };
    }
    case "circle": {
      const c = iu(...g.center);
      return { ...base, type: 3, start: c, end: [c[0] + iuLen(g.radius), c[1]], width: iuLen(g.stroke_width), fill: g.fill };
    }
    case "arc":
      return { ...base, type: 2, start: iu(...g.start), end: iu(...g.end), mid: iu(...g.mid), width: iuLen(g.stroke_width), fill: g.fill };
    case "text":
      return null;
  }
}

function shapeOfLibrary(g: LibrarySymbolGraphic): Shape | null {
  const base = { unit: g.unit, bodyStyle: g.body_style };
  switch (g.kind) {
    case "rectangle":
      return { ...base, type: 1, start: iu(g.start.x, g.start.y), end: iu(g.end.x, g.end.y), width: iuLen(g.stroke_mm), fill: g.fill };
    case "polyline": {
      const pts = g.pts.map((p) => iu(p.x, p.y));
      return { ...base, type: 4, start: pts[0] ?? [0, 0], end: pts[pts.length - 1] ?? [0, 0], pts, width: iuLen(g.stroke_mm), fill: g.fill };
    }
    case "circle": {
      const c = iu(g.center.x, g.center.y);
      return { ...base, type: 3, start: c, end: [c[0] + iuLen(g.radius_mm), c[1]], width: iuLen(g.stroke_mm), fill: g.fill };
    }
    case "arc":
      return { ...base, type: 2, start: iu(g.start.x, g.start.y), end: iu(g.end.x, g.end.y), mid: iu(g.mid.x, g.mid.y), width: iuLen(g.stroke_mm), fill: g.fill };
    case "text":
    case "text_box":
      return null;
  }
}

const EPSILON = 2;
const FILL_ORDER = ["none", "outline", "background", "color", "hatch", "reverse_hatch", "cross_hatch"];

/** `SCH_SHAPE::compare( other, ERC )`: `SCH_ITEM::compare` (unit, body style, position, exactly), then `EDA_SHAPE::Compare`. 0 is the same. */
function compareShapes(a: Shape, b: Shape): number {
  const e = (x: number, y: number) => (Math.abs(x - y) > EPSILON ? x - y : 0);
  const pt = (p: [number, number], q: [number, number]) => e(p[0], q[0]) || e(p[1], q[1]);
  if (a.unit !== b.unit) return a.unit - b.unit;
  if (a.bodyStyle !== b.bodyStyle) return a.bodyStyle - b.bodyStyle;
  const pa = position(a);
  const pb = position(b);
  if (pa[0] !== pb[0]) return pa[0] - pb[0];
  if (pa[1] !== pb[1]) return pa[1] - pb[1];
  let r = pt(a.start, b.start) || pt(a.end, b.end);
  if (r) return r;
  if (a.type !== b.type) return a.type - b.type;
  if (a.type === 2) {
    r = pt(a.mid!, b.mid!);
    if (r) return r;
  }
  if (a.type === 4) {
    if (a.pts!.length !== b.pts!.length) return a.pts!.length - b.pts!.length;
    for (let i = 0; i < a.pts!.length; i++) {
      r = pt(a.pts![i]!, b.pts![i]!);
      if (r) return r;
    }
  }
  r = e(a.width, b.width);
  if (r) return r;
  return FILL_ORDER.indexOf(a.fill) - FILL_ORDER.indexOf(b.fill);
}

/** `SCH_ITEM::cmp_items` into a `std::set`: sorted, and a shape equal to one already in is not added. */
function shapeSet(shapes: Shape[]): Shape[] {
  const sorted = [...shapes].sort(compareShapes);
  return sorted.filter((s, i) => i === 0 || compareShapes(sorted[i - 1]!, s) !== 0);
}

/** `SCH_SHAPE::GetItemDescription`. */
function describeShape(s: Shape): string {
  switch (s.type) {
    case 2: {
      const c = circumcentre(s.start, s.mid!, s.end);
      return `Arc, radius ${mm(Math.hypot(s.start[0] - c[0], s.start[1] - c[1]))}`;
    }
    case 3:
      return `Circle, radius ${mm(Math.hypot(s.end[0] - s.start[0], s.end[1] - s.start[1]))}`;
    case 1:
      return `Rectangle, width ${mm(Math.abs(s.start[0] - s.end[0]))} height ${mm(Math.abs(s.start[1] - s.end[1]))}`;
    case 4:
      return `Polyline, ${s.pts!.length} points`;
  }
}

/** `SCH_FIELD::GetItemDescription( .., false )`: "Field Value '10k'", "Field Footprint (empty)". */
const describeField = (name: string, text: string) => (text === "" ? `Field ${name} (empty)` : `Field ${name} '${text}'`);

type PowerKind = "normal" | "global" | "local";
const powerKind = (power: boolean | undefined, local: boolean | undefined): PowerKind => (!power ? "normal" : local ? "local" : "global");

/**
 * The lines `LIB_SYMBOL::Compare` reports, in its order. Empty means "No relevant differences detected."
 */
export function diffSymbols(schematic: SchematicLibSymbol, library: LibrarySymbol, placed?: PlacedSymbolFields): string[] {
  const out: string[] = [];

  // `m_options` (ENTRY_NORMAL / ENTRY_GLOBAL_POWER / ENTRY_LOCAL_POWER). A schematic side that does not say whether its power symbol is local
  // is taken to be the library's kind of power symbol.
  const libKind = powerKind(library.power, library.power_local);
  const schKind = schematic.power && schematic.power_local === undefined && libKind !== "normal" ? libKind : powerKind(schematic.power, schematic.power_local);
  if (schKind !== libKind) out.push("Power flag differs.");

  const schPins = schematic.pins.map(pinOfResolved);
  const libPins = library.pins.map(pinOfLibrary);
  const unitsOf = (pins: Pin[], graphics: Array<{ unit: number }>) => Math.max(1, ...pins.map((p) => p.unit), ...graphics.map((g) => g.unit));
  if (unitsOf(schPins, schematic.graphics) !== Math.max(unitsOf(libPins, library.graphics), library.unit_count)) out.push("Unit count differs.");

  const aShapes = shapeSet(schematic.graphics.map(shapeOfResolved).filter((s): s is Shape => s !== null));
  const bShapes = shapeSet(library.graphics.map(shapeOfLibrary).filter((s): s is Shape => s !== null));
  if (aShapes.length !== bShapes.length) {
    out.push("Graphic item count differs.");
  } else {
    aShapes.forEach((a, i) => {
      const b = bShapes[i]!;
      if (compareShapes(a, b) !== 0) out.push(`Graphic item differs: ${describeShape(a)}; ${describeShape(b)}.`);
    });
  }

  for (const p of schPins) {
    const other = getPin(libPins, p.number, p.unit, p.bodyStyle);
    if (!other) out.push(`Extra pin in schematic symbol: ${describePin(p)}.`);
    else if (p.pos[0] !== other.pos[0] || p.pos[1] !== other.pos[1]) out.push(`Pin ${p.number} differs: ${describePin(p)}; ${describePin(other)}`);
  }

  // the placed symbol's fields (`flattenedSchSymbol->SetFields( fields )`, each `Move( -symbol->GetPosition() )`): a mandatory one is found by id
  // (always in the library), a user field by name; `SCH_ITEM::compare` under ERC is the position and `IsPrivate()` (a copied field is not private)
  if (placed?.fields) {
    const libFields = allFields(library);
    const privateNames = new Set((library.fields ?? []).filter((f) => f.private).map((f) => f.name));
    for (const f of placed.fields) {
      const other = libFields.find((l) => l.name === f.name);
      if (!other) {
        out.push(`Extra field in schematic symbol: ${describeField(f.name, f.text)}.`);
        continue;
      }
      const [lx, ly] = iu(other.at.x, other.at.y);
      const moved = f.at[0] - placed.at[0] !== lx || f.at[1] - placed.at[1] !== ly;
      if (moved || privateNames.has(other.name)) out.push(`Field '${f.name}' differs: ${describeField(f.name, f.text)}; ${describeField(other.name, other.text)}.`);
    }
  }

  if (schematic.footprint_filters !== undefined) {
    const a = schematic.footprint_filters;
    const b = library.footprint_filters;
    if (a.length !== b.length) out.push("Footprint filter count differs.");
    else for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) out.push("Footprint filters differ.");
  }
  if (schematic.keywords !== undefined && schematic.keywords !== library.keywords) out.push("Symbol keywords differ.");
  if (schematic.pin_name_offset !== undefined && iuLen(schematic.pin_name_offset) !== iuLen(library.pin_name_offset_mm)) out.push("Symbol pin name offsets differ.");

  return out;
}
