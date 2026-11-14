// Compare Symbol with Library (`eeschema.InspectionTool.diffSymbol`, `SCH_INSPECTION_TOOL::DiffSymbol` -> `LIB_SYMBOL::Compare( ..., SymbolCompareFlags(), reporter )`,
// KiCad 25ec2335 eeschema/lib_symbol.cpp, schematic_settings.cpp): the report of how the symbol the schematic draws differs from the library's.
//
// The studio keeps no separate copy of a library symbol in the schematic (it draws the library's, as `GET /api/schematic` resolves it), so the two
// sides here are the symbol the schematic is drawn from (`Schematic.lib_symbols`) and the library entry (the Symbol Editor's project entry when
// there is one -- which may carry edits not yet used by the schematic -- else the same resolved symbol). `DiffSymbol` gives the schematic's side the
// placed symbol's own fields, moved by `-symbol->GetPosition()`; so does `placed` here.
//
// What `Compare` reports, in its order, with the flags Schematic Setup > Compare Symbol with Library gives (`SYMBOL_PARITY_SETTINGS`, UI-LEFT-3 B3-9):
// - "Power flag differs." when `m_options` differ (normal, global power, local power);
// - "Unit count differs.";
// - the shapes (`SCH_SHAPE_T` only: a body text or text box is not a graphic item), sorted by `SCH_ITEM::cmp_items` (a `std::set`, so two equal
//   shapes are one), their count, then each pair by `SCH_SHAPE::compare` -> `SCH_ITEM::compare` (unit, body style, position) and
//   `EDA_SHAPE::Compare` (points within `EPSILON` 2 IU, 200 nm, the corner radius, the stroke width, its line style, the fill);
// - each schematic pin not found by number, unit and body style in the library ("Extra pin"), or found but different (`PIN_COMPARISON_VIEW::Compare`:
//   the number, place, length, direction, shape and type; the hidden flag with "Compare pin name/number visibilities"; the name and number text sizes; the
//   alternates with "Compare alternate pin functions"; a pin's name is not compared), and each library pin the schematic's symbol lacks ("Missing pin");
// - each placed field the library lacks ("Extra field", with "Check for extra fields"), and of one it has: its text but the Reference's ("Compare
//   non-reference field text"), its font, text size and styles ("... sizes and styles": `TEXT_ATTRIBUTES::Compare`'s font, size, angle, justification,
//   italic, bold and colour), its visibility and name-shown flags ("Compare field visibilities"), its privacy, its place ("Compare field positions"); then
//   each library user field the placed symbol lacks ("Missing field", "Check for missing fields");
// - the footprint filters, keywords and pin name offset; the show pin names and numbers flags ("Compare pin name/number visibilities"); the attribute
//   flags the boxes of the page name (UI-LEFT-3 S3-22: a shape's line style and corner radius, a pin's text sizes, a field's font and the position-file flag
//   are compared too, and the keywords and footprint filters always).
//
// "Do not populate settings differ." is never reported: a library symbol holds no DNP of its own (`SCH_IO_KICAD_SEXPR_PARSER::parseLibSymbol` reads no `dnp`, so
// `LIB_SYMBOL::m_DNP` is false on both sides). **Differs**: a placed field's pen width (`TEXT_ATTRIBUTES::m_StrokeWidth`) is not kept by the studio, so the text
// styles do not compare it; a server built before these were sent leaves the schematic's side of each out (compared equal).
import type { LibGraphic, LibPin, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, LibSymbol, PinAlternate, PinElectricalType, PinShape, SchField, SymbolParitySettings } from "../api/types";
import { fieldLooksOf, graphicStateOf, isUnspecified } from "./symLooks";
import { DEFAULT_SYMBOL_PARITY } from "./schSetup";
import { messageTextFromValue } from "./messageText";
import { ELECTRICAL_TYPE_NAMES } from "./symPinText";
import { PIN_SHAPE_OPTIONS } from "./symPinAlternates";
import { allFields, isMandatoryField } from "./symFields";
import { libBezierPointsNm } from "./itemBoxes";

/** The schematic's side: the resolved library symbol, with what the server may add of its options. */
export type SchematicLibSymbol = LibSymbol & { power?: boolean; power_local?: boolean; keywords?: string; footprint_filters?: string[]; in_bom?: boolean; on_board?: boolean; exclude_from_pos_files?: boolean };

/** The placed symbol whose fields `DiffSymbol` gives the schematic's side. */
export interface PlacedSymbolFields {
  at: readonly [number, number];
  fields?: readonly SchField[];
}

/** mm (library, Y up) to the schematic's internal units, Y down: `SCH_IU_PER_MM` is 1e4, so one IU is 100 nm (base_units.h), and `EDA_SHAPE::Compare`'s
 *  `EPSILON` of 2 is 200 nm (r7 A7-L1: this counted in nanometres). */
const SCH_IU_PER_MM = 1e4;
const NM_PER_IU = 1e6 / SCH_IU_PER_MM;
const iu = (x: number, y: number): [number, number] => [Math.round(x * SCH_IU_PER_MM), Math.round(-y * SCH_IU_PER_MM)];
const iuLen = (v: number) => Math.round(v * SCH_IU_PER_MM);
/** A length in IU as the message writes it (`messageTextFromValue` takes nanometres). */
const mm = (iuValue: number) => messageTextFromValue(iuValue * NM_PER_IU, "mm");

interface Pin {
  number: string;
  name: string;
  type: PinElectricalType;
  shape: PinShape;
  pos: [number, number];
  unit: number;
  bodyStyle: number;
  hidden: boolean;
  /** IU, and the direction in degrees. */
  length: number;
  angle: number;
  /** By name, as `std::map<wxString, PIN_ALTERNATE>` holds them. */
  alternates: PinAlternate[];
  /** IU; null where the side does not say (compared equal). */
  numberSize: number | null;
  nameSize: number | null;
}

const byName = (alts: readonly PinAlternate[] | undefined) => [...(alts ?? [])].sort((x, y) => (x.name < y.name ? -1 : x.name > y.name ? 1 : 0));
const sizeIu = (mmValue: number | null | undefined, fallback: number | null): number | null => (mmValue === undefined ? fallback : iuLen(mmValue ?? 1.27));
const pinOfResolved = (p: LibPin): Pin => ({ number: p.number, name: p.name ?? "", type: p.electrical_type, shape: p.shape, pos: iu(p.at[0], p.at[1]), unit: p.unit, bodyStyle: p.body_style, hidden: p.hidden, length: iuLen(p.length_mm), angle: ((Math.round(p.angle_deg) % 360) + 360) % 360, alternates: byName(p.alternates), numberSize: sizeIu(p.number_size_mm, null), nameSize: sizeIu(p.name_size_mm, null) });
// (a library pin with no size of its own has KiCad's 50 mil, `schIUScale.MilsToIU( DEFAULT_PINNUM_SIZE )`)
const pinOfLibrary = (p: LibrarySymbolPin): Pin => ({ number: p.number, name: p.name, type: p.electrical_type, shape: p.shape, pos: iu(p.at.x, p.at.y), unit: p.unit, bodyStyle: p.body_style, hidden: p.hidden, length: iuLen(p.length_mm), angle: ((Math.round(p.angle_deg) % 360) + 360) % 360, alternates: byName(p.alternates), numberSize: iuLen(p.number_size_mm ?? 1.27), nameSize: iuLen(p.name_size_mm ?? 1.27) });

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
  /** `LINE_STYLE`: default -1, solid 0, dash 1, dot 2, dash-dot 3, dash-dot-dot 4; null where the side does not say (compared equal). */
  lineStyle: number | null;
  /** A rectangle's corner radius, IU (`m_cornerRadius`); null where the side does not say. */
  radius: number | null;
}

/** `LINE_STYLE`'s values by the file's words (`STROKE_PARAMS::Format`). */
const LINE_STYLES: Record<string, number> = { default: -1, solid: 0, dash: 1, dot: 2, dash_dot: 3, dash_dot_dot: 4 };
const lineStyleOf = (s: string | null | undefined): number => LINE_STYLES[s ?? "default"] ?? -1;

function circumcentre(a: [number, number], b: [number, number], c: [number, number]): [number, number] {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (d === 0) return [Math.round((a[0] + c[0]) / 2), Math.round((a[1] + c[1]) / 2)];
  const sq = (p: [number, number]) => p[0] * p[0] + p[1] * p[1];
  return [Math.round((sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d), Math.round((sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d)];
}

/** `EDA_SHAPE::getPosition`: an arc's centre, a polygon's first vertex, else the start. */
const position = (s: Shape): [number, number] => (s.type === 2 ? circumcentre(s.start, s.mid!, s.end) : s.type === 4 ? (s.pts![0] ?? s.start) : s.start);

function shapeOfResolved(g: LibGraphic): Shape | null {
  // the line style and corner radius the server sends (`LibConnectivity::shape_looks`), absent the default
  const looks = g as { line_style?: string; corner_radius_mm?: number };
  const base = { unit: g.unit, bodyStyle: g.body_style, lineStyle: lineStyleOf(looks.line_style), radius: g.kind === "rectangle" ? iuLen(looks.corner_radius_mm ?? 0) : null };
  switch (g.kind) {
    case "rectangle":
      return { ...base, type: 1, start: iu(...g.start), end: iu(...g.end), width: iuLen(g.stroke_width), fill: g.fill };
    case "polyline": {
      // a `SHAPE_T::POLY` keeps its points in `m_poly`; the parser leaves `m_start` / `m_end` at (0, 0), so `TEST_PT( m_start ..)` ties and the
      // order falls to the vertex count, then the vertices (r7 A7-L1)
      const pts = g.pts.map(([x, y]) => iu(x, y));
      return { ...base, type: 4, start: [0, 0], end: [0, 0], pts, width: iuLen(g.stroke_width), fill: g.fill };
    }
    case "circle": {
      const c = iu(...g.center);
      return { ...base, type: 3, start: c, end: [c[0] + iuLen(g.radius), c[1]], width: iuLen(g.stroke_width), fill: g.fill };
    }
    case "arc":
      return { ...base, type: 2, start: iu(...g.start), end: iu(...g.end), mid: iu(...g.mid), width: iuLen(g.stroke_width), fill: g.fill };
    // a text box's text, as the library's text box is not a shape here (its border is the `rectangle` beside it)
    case "text":
    case "text_box":
      return null;
  }
}

function shapeOfLibrary(g: LibrarySymbolGraphic, sym: LibrarySymbol): Shape | null {
  // the line style and corner radius the Symbol Editor keeps by the graphic's id (`stroke_types`, `graphic_looks`)
  const state = g.id ? graphicStateOf(sym, g.id) : null;
  const base = { unit: g.unit, bodyStyle: g.body_style, lineStyle: lineStyleOf(state?.lineStyle), radius: g.kind === "rectangle" ? iuLen(state?.looks.radius_mm ?? 0) : null };
  switch (g.kind) {
    case "rectangle":
      return { ...base, type: 1, start: iu(g.start.x, g.start.y), end: iu(g.end.x, g.end.y), width: iuLen(g.stroke_mm), fill: g.fill };
    case "polyline": {
      const pts = g.pts.map((p) => iu(p.x, p.y));
      return { ...base, type: 4, start: [0, 0], end: [0, 0], pts, width: iuLen(g.stroke_mm), fill: g.fill };
    }
    case "circle": {
      const c = iu(g.center.x, g.center.y);
      return { ...base, type: 3, start: c, end: [c[0] + iuLen(g.radius_mm), c[1]], width: iuLen(g.stroke_mm), fill: g.fill };
    }
    case "arc":
      return { ...base, type: 2, start: iu(g.start.x, g.start.y), end: iu(g.end.x, g.end.y), mid: iu(g.mid.x, g.mid.y), width: iuLen(g.stroke_mm), fill: g.fill };
    // the schematic's side draws a library bezier as the polyline it is cut into (`LibrarySymbolGraphic::to_engine_graphics`), so that is what it is compared as
    case "bezier": {
      const pts = libBezierPointsNm(g).map(([x, y]) => [Math.round(x / NM_PER_IU), Math.round(y / NM_PER_IU)] as [number, number]);
      return { ...base, type: 4, start: [0, 0], end: [0, 0], pts, width: iuLen(g.stroke_mm), fill: g.fill };
    }
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
  if (a.type === 1 && a.radius !== null && b.radius !== null && a.radius !== b.radius) return a.radius - b.radius;
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
  if (a.lineStyle !== null && b.lineStyle !== null && a.lineStyle !== b.lineStyle) return a.lineStyle - b.lineStyle;
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


type PowerKind = "normal" | "global" | "local";
const powerKind = (power: boolean | undefined, local: boolean | undefined): PowerKind => (!power ? "normal" : local ? "local" : "global");

/**
 * The lines `LIB_SYMBOL::Compare` reports, in its order. Empty means "No relevant differences detected."
 */
export function diffSymbols(schematic: SchematicLibSymbol, library: LibrarySymbol, placed?: PlacedSymbolFields, flags: SymbolParitySettings = DEFAULT_SYMBOL_PARITY): string[] {
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
  const bShapes = shapeSet(library.graphics.map((g) => shapeOfLibrary(g, library)).filter((s): s is Shape => s !== null));
  if (aShapes.length !== bShapes.length) {
    out.push("Graphic item count differs.");
  } else {
    aShapes.forEach((a, i) => {
      const b = bShapes[i]!;
      if (compareShapes(a, b) !== 0) out.push(`Graphic item differs: ${describeShape(a)}; ${describeShape(b)}.`);
    });
  }

  // the pins (`SCH_PIN::compare` -> `PIN_COMPARISON_VIEW::Compare` under `SymbolCompareFlags`, whose UNIT bit is off): the number, place, length, direction,
  // graphic shape and electrical type, the hidden flag with "Compare pin name/number visibilities", the alternates with "Compare alternate pin functions"
  for (const p of schPins) {
    const other = getPin(libPins, p.number, p.unit, p.bodyStyle);
    if (!other) out.push(`Extra pin in schematic symbol: ${describePin(p)}.`);
    else if (comparePins(p, other, flags) !== 0) out.push(`Pin ${p.number} differs: ${describePin(p)}; ${describePin(other)}`);
  }
  // KiCad 25ec2335 looks each library pin up in the schematic's symbol (`GetPin( .. )` of `this`)
  for (const p of libPins) {
    if (!getPin(schPins, p.number, p.unit, p.bodyStyle)) out.push(`Missing pin in schematic symbol: ${describePin(p)}.`);
  }

  // the placed symbol's fields (`flattenedSchSymbol->SetFields( fields )`, each `Move( -symbol->GetPosition() )`): a mandatory one is found by id
  // (always in the library), a user field by name; each check as `SymbolCompareFlags` says (a copied field is not private)
  if (placed?.fields) {
    const libFields = allFields(library);
    const privateNames = new Set((library.fields ?? []).filter((f) => f.private).map((f) => f.name));
    for (const f of placed.fields) {
      const other = libFields.find((l) => l.name === f.name);
      if (!other) {
        if (flags.extra_fields) out.push(`Extra field in schematic symbol: ${f.name}.`);
        continue;
      }
      if (flags.field_texts && f.name !== "Reference" && f.text !== other.text) out.push(`Field '${f.name}' differs: ${f.text}; ${other.text}.`);
      if (flags.field_styles) {
        // the font (`GetFont()`: none is the default font, "KiCad Font" the stroke font), the text size in IU (absent is 50 mil), then `TEXT_ATTRIBUTES::Compare`:
        // the font's name, the size, the angle, the justification, italic, bold and the colour (the pen width the studio does not keep for a placed field)
        const looks = fieldLooksOf(library, other.name);
        const fontOf = (font: string | null | undefined) => (font ? font : "");
        const sameFont = fontOf(f.font) === fontOf(looks.font);
        const sameSize = Math.round((f.size_nm ?? 1_270_000) / NM_PER_IU) === iuLen(other.size_mm);
        const colorKey = (c: { r: number; g: number; b: number; a: number } | null | undefined) => (isUnspecified(c) ? "" : `${c!.r},${c!.g},${c!.b},${c!.a}`);
        const sameAngle = f.local_vertical === undefined || f.local_vertical === (Math.round(other.angle_deg) % 180 === 90);
        const sameJustify = (f.local_h === undefined || f.local_h === other.h_align) && (f.local_v === undefined || f.local_v === other.v_align);
        if (!sameFont) out.push(`Field '${f.name}' fonts differ.`);
        if (!sameSize) out.push(`Field '${f.name}' text sizes differ.`);
        if (!sameFont || !sameSize || !sameAngle || !sameJustify || !!f.italic !== other.italic || !!f.bold !== other.bold || colorKey(f.color) !== colorKey(looks.color)) out.push(`Field '${f.name}' text styles differ.`);
      }
      if (flags.field_visibilities) {
        if (f.visible !== !other.hidden) out.push(`Field '${f.name}' visibility flags differ.`);
        if (!!f.name_shown !== other.show_name) out.push(`Field '${f.name}' name shown flags differ.`);
      }
      if (privateNames.has(other.name)) out.push(`Field '${f.name}' privacy flags differ.`);
      if (flags.field_positions) {
        const [lx, ly] = iu(other.at.x, other.at.y);
        // the placed field's offset (nanometres) in IU: the position exactly, at 100 nm
        if (Math.round((f.at[0] - placed.at[0]) / NM_PER_IU) !== lx || Math.round((f.at[1] - placed.at[1]) / NM_PER_IU) !== ly) out.push(`Field '${f.name}' positions differ.`);
      }
    }
    // "Check for missing fields": a library field the placed symbol lacks (a mandatory field is on every symbol in KiCad, so only a user field can be missing)
    if (flags.missing_fields) {
      for (const l of libFields) if (!isMandatoryField(l.name) && !placed.fields.some((f) => f.name === l.name)) out.push(`Missing field in schematic symbol: ${l.name}.`);
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
  if (flags.pin_visibilities) {
    if (!!schematic.pin_names_hidden !== library.pin_names_hidden) out.push("Show pin names settings differ.");
    if (!!schematic.pin_numbers_hidden !== library.pin_numbers_hidden) out.push("Show pin numbers settings differ.");
  }
  // the attribute flags, each when the schematic's side says it (a server built before them may not send them); "Do not populate settings differ." never: a library
  // symbol has no DNP of its own (`parseLibSymbol` reads no `dnp`)
  if (flags.exclude_from_bom && schematic.in_bom !== undefined && schematic.in_bom !== library.in_bom) out.push("Exclude from bill of materials settings differ.");
  if (flags.exclude_from_board && schematic.on_board !== undefined && schematic.on_board !== library.on_board) out.push("Exclude from board settings differ.");
  if (flags.exclude_from_pos_files && schematic.exclude_from_pos_files !== undefined && schematic.exclude_from_pos_files !== !!library.exclude_from_pos_files) out.push("Exclude from position files settings differ.");

  return out;
}

/** `PIN_COMPARISON_VIEW::Compare` with `SymbolCompareFlags` (UNIT off): 0 is the same. */
function comparePins(a: Pin, b: Pin, flags: SymbolParitySettings): number {
  if (a.number !== b.number) return a.number < b.number ? -1 : 1;
  if (a.pos[0] !== b.pos[0]) return a.pos[0] - b.pos[0];
  if (a.pos[1] !== b.pos[1]) return a.pos[1] - b.pos[1];
  if (a.length !== b.length) return a.length - b.length;
  if (a.angle !== b.angle) return a.angle - b.angle;
  if (a.shape !== b.shape) return a.shape < b.shape ? -1 : 1;
  if (a.type !== b.type) return a.type < b.type ? -1 : 1;
  if (flags.pin_visibilities && a.hidden !== b.hidden) return a.hidden ? 1 : -1;
  // `numberTextSize`, `nameTextSize` (a side that does not say is taken to be the other's)
  if (a.numberSize !== null && b.numberSize !== null && a.numberSize !== b.numberSize) return a.numberSize - b.numberSize;
  if (a.nameSize !== null && b.nameSize !== null && a.nameSize !== b.nameSize) return a.nameSize - b.nameSize;
  if (!flags.pin_alt_functions) return 0;
  if (a.alternates.length !== b.alternates.length) return a.alternates.length - b.alternates.length;
  for (let i = 0; i < a.alternates.length; i++) {
    const x = a.alternates[i]!;
    const y = b.alternates[i]!;
    if (x.name !== y.name || x.electrical_type !== y.electrical_type || x.shape !== y.shape) return 1;
  }
  return 0;
}
