// The schematic editor's Properties panel: which properties each kind of schematic item has, and the commands that edit them.
//
// Ported from the `PROPERTY_MANAGER` registrations of eeschema at KiCad 8303b2ad -- `SCH_ITEM_DESC` (sch_item.cpp), `SCH_SYMBOL_DESC`, `SCH_LABEL_DESC` and
// `SCH_DIRECTIVE_LABEL_DESC`, `SCH_TEXT_DESC`, `SCH_LINE_DESC`, `SCH_JUNCTION_DESC`, `SCH_BUS_ENTRY_DESC`, `SCH_SHEET_DESC`, `SCH_SHAPE_DESC`, `SCH_TEXTBOX_DESC`,
// `SCH_RULE_AREA_DESC` and `EDA_TEXT_DESC` / `EDA_SHAPE_DESC` (common/) -- with `SCH_PROPERTIES_PANEL` (eeschema/widgets/sch_properties_panel.cpp) supplying the
// fields a symbol or sheet has as rows and the way a value is set (`valueChanged`). The registrations keep KiCad's calls so `propertyManager.ts` walks the same
// classes in the same order. What this model has no place for is not registered: fonts, bold and italic of a label, a sheet's border and fill, pin names and
// numbers shown, a symbol's extra fields and its description (GAPS.md items 1 and 12). A no-connect flag has no registration in KiCad either: its grid is empty.
//
// Every setter returns commands of the verbs the schematic editor already has -- `sch_move` (move), `rotate_symbol`, `mirror_symbol`, `mirror_symbol_vertical`,
// `rename_symbol`, `edit_symbol_fields`, `set_symbol_attrs`, `sch_edit` `edit_label` / `edit_text` / `edit_sheet` / `set_stroke` / `edit_graphic` / `set_locked`.
// A grid edit sends them as one `batch`: one undo step, KiCad's one `SCH_COMMIT::Push( "Edit Properties" )`. Where an item's geometry is its own to edit (a wire's end
// points, a symbol's fields) it is read-only here: the field positions are the overlap fix's, and a wire's end points are dragged (`G`) so what they join follows.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardTable, BoardTableCell, Cmd, LabelShape, LibSymbol, PointXY, Schematic, SchematicLabel, SchematicSymbol, SchematicText, SchematicWire, PowerSymbol, SchJunction, SchLine, BusEntry, Sheet } from "../api/types";
import type { SchColor, SchGraphic, SchGraphicShape, SchLineStyle } from "../api/schEditTypes";
import type { LengthUnit } from "../state/units";
import { buildGrid, extractValueAndWritability, planEdit, tooSmall, type EditPlan, type GridModel } from "./propertyGrid";
import { PropertyManager, type Choice, type PropValue, type PropertyDef } from "./propertyManager";
import { colorToHex, hexToColor, isUnspecified, strokeEditCmd, UNSPECIFIED_COLOR } from "./schProperties";
import { inferSpin, type LabelSpin } from "./labelSpin";
import { circumcircle } from "./trackArc";
import { DEFAULT_PPI, imageSizeNm, SCALE_PPM, scaleForHeightNm, scaleForWidthNm } from "./refImage";
import { registerSchTableProperties, schTableItemOf } from "./schTableProperties";
import { defaultTextBoxMargin } from "./schTextBox";
import { unitCountOf } from "./schUnits";
import { letterSubReference } from "./unitLetter";
import { setSymbolVariantCmd, type VariantFlagKey } from "./designVariants";

// ------------------------------------------------------------------------------------------------------------------------------- items

/** The class of a schematic item (`TYPE_HASH( *item )`). */
export type SchType =
  | "SCH_SYMBOL"
  | "SCH_LINE"
  | "SCH_JUNCTION"
  | "SCH_NO_CONNECT"
  | "SCH_BUS_WIRE_ENTRY"
  | "SCH_LABEL"
  | "SCH_GLOBALLABEL"
  | "SCH_HIERLABEL"
  | "SCH_DIRECTIVE_LABEL"
  | "SCH_TEXT"
  | "SCH_TEXTBOX"
  | "SCH_SHAPE"
  | "SCH_RULE_AREA"
  | "SCH_BITMAP"
  | "SCH_SHEET"
  | "SCH_TABLE"
  | "SCH_TABLECELL";

/** What a `SCH_LINE` is (`GetLayer()`): a wire, a bus, or a graphic line on the notes layer. */
export type LineKind = "wire" | "bus" | "line";

/** One selected schematic item: the class, the id the selection holds, and the record of the sheet it stands for (only the one of its kind is set). */
export interface SchItem {
  readonly type: SchType;
  readonly id: string;
  readonly symbol?: SchematicSymbol;
  readonly power?: PowerSymbol;
  readonly wire?: SchematicWire;
  readonly line?: SchLine;
  readonly lineKind?: LineKind;
  readonly junction?: SchJunction;
  readonly busEntry?: BusEntry;
  readonly label?: SchematicLabel;
  readonly text?: SchematicText;
  readonly graphic?: SchGraphic;
  readonly sheet?: Sheet;
  /** A table (`SCH_TABLE`), or the table a cell (`SCH_TABLECELL`) is in with the cell. */
  readonly table?: BoardTable;
  readonly cell?: BoardTableCell;
}

/** What the getters read besides the item. */
export interface SchCtx {
  sch: Schematic;
  units: LengthUnit;
  locked: ReadonlySet<string>;
  /** How many items are selected: a reference names one symbol, so it is edited only on its own. */
  count: number;
  /** The current design variant (KiCad 25ec2335 `Schematic()->GetCurrentVariant()`), "" for the default: the attribute and field rows of a symbol read and set the
   * variant's values then (`GetDNPProp` / `SetDNPProp` ...); `sch` is the sheet as the variant shows it (kicad-port/designVariants.ts `withVariant`). */
  variant?: string;
}

export function schContext(sch: Schematic, units: LengthUnit, count = 1, variant = ""): SchCtx {
  return { sch, units, locked: new Set(sch.locked ?? []), count, variant };
}

/** A symbol's flag or field text set in the current variant (`SetDNP( aEnable, &CurrentSheet(), GetCurrentVariant() )`, `SetFieldText( .., variant )`), or null for the default. */
function variantEdit(c: SchCtx, id: string, change: { flag?: [VariantFlagKey, boolean]; field?: [string, string] }): Cmd[] | null {
  if (!c.variant) return null;
  const cmd = setSymbolVariantCmd(c.variant, [id], { flags: change.flag ? { [change.flag[0]]: change.flag[1] } : {}, fields: change.field ? { [change.field[0]]: change.field[1] } : {} });
  return cmd ? [cmd] : [];
}

/** The sheet's items by id, for a selection too big to search one id at a time. The first of two items of an id wins, as in `propertiesTarget`. */
interface SchIndex {
  symbol: Map<string, SchematicSymbol>;
  power: Map<string, PowerSymbol>;
  wire: Map<string, SchematicWire>;
  label: Map<string, SchematicLabel>;
  text: Map<string, SchematicText>;
  noConnect: Set<string>;
  busEntry: Map<string, BusEntry>;
  junction: Map<string, SchJunction>;
  line: Map<string, SchLine>;
  sheet: Map<string, Sheet>;
  graphic: Map<string, SchGraphic>;
  /** The sheet itself, for the tables and their cells (`schTableItemOf`). */
  sch: Schematic;
}

function firstById<T extends { id: string }>(list: readonly T[]): Map<string, T> {
  const out = new Map<string, T>();
  for (const item of list) if (!out.has(item.id)) out.set(item.id, item);
  return out;
}

function indexOf(sch: Schematic): SchIndex {
  return {
    symbol: firstById(sch.symbols),
    power: firstById(sch.power_symbols),
    wire: firstById(sch.wires),
    label: firstById(sch.labels),
    text: firstById(sch.texts),
    noConnect: new Set(sch.no_connects.map((n) => n.id)),
    busEntry: firstById(sch.bus_entries),
    junction: firstById(sch.junctions ?? []),
    line: firstById(sch.lines ?? []),
    sheet: firstById(sch.sheets),
    graphic: firstById(sch.graphics ?? []),
    sch,
  };
}

/** The item a selection id names, or null. Same order as `propertiesTarget`. */
function resolve(idx: SchIndex, id: string): SchItem | null {
  const symbol = idx.symbol.get(id);
  if (symbol) return { type: "SCH_SYMBOL", id, symbol };
  const power = idx.power.get(id);
  if (power) return { type: "SCH_SYMBOL", id, power };
  const wire = idx.wire.get(id);
  if (wire) return { type: "SCH_LINE", id, wire, lineKind: wire.bus ? "bus" : "wire" };
  const label = idx.label.get(id);
  if (label) return { type: label.scope === "global" ? "SCH_GLOBALLABEL" : label.scope === "hierarchical" ? "SCH_HIERLABEL" : "SCH_LABEL", id, label };
  const text = idx.text.get(id);
  if (text) return { type: "SCH_TEXT", id, text };
  if (idx.noConnect.has(id)) return { type: "SCH_NO_CONNECT", id };
  const busEntry = idx.busEntry.get(id);
  if (busEntry) return { type: "SCH_BUS_WIRE_ENTRY", id, busEntry };
  const junction = idx.junction.get(id);
  if (junction) return { type: "SCH_JUNCTION", id, junction };
  const line = idx.line.get(id);
  if (line) return { type: "SCH_LINE", id, line, lineKind: "line" };
  const sheet = idx.sheet.get(id);
  if (sheet) return { type: "SCH_SHEET", id, sheet };
  const graphic = idx.graphic.get(id);
  if (graphic) {
    const t = graphic.shape.type;
    return { type: t === "text_box" ? "SCH_TEXTBOX" : t === "rule_area" ? "SCH_RULE_AREA" : t === "directive" ? "SCH_DIRECTIVE_LABEL" : t === "image" ? "SCH_BITMAP" : "SCH_SHAPE", id, graphic };
  }
  return schTableItemOf(idx.sch, id);
}

/** The item a selection id names, or null. */
export function schItemOf(sch: Schematic, id: string): SchItem | null {
  return resolve(indexOf(sch), id);
}

/** The items of a selection, in selection order; an id the sheet does not have is skipped. */
export function schItemsOf(sch: Schematic, ids: Iterable<string>): SchItem[] {
  const idx = indexOf(sch);
  const out: SchItem[] = [];
  for (const id of ids) {
    const item = resolve(idx, id);
    if (item) out.push(item);
  }
  return out;
}

/** `GetFriendlyName()`: `ENUM_MAP<KICAD_T>`'s name of the class, `SCH_LINE::GetFriendlyName` for the three lines. */
export function schFriendlyName(item: SchItem): string {
  switch (item.type) {
    case "SCH_SYMBOL":
      return "Symbol";
    case "SCH_LINE":
      return item.lineKind === "wire" ? "Wire" : item.lineKind === "bus" ? "Bus" : "Graphic Line";
    case "SCH_JUNCTION":
      return "Junction";
    case "SCH_NO_CONNECT":
      return "No-Connect Flag";
    case "SCH_BUS_WIRE_ENTRY":
      return "Wire Entry";
    case "SCH_LABEL":
      return "Net Label";
    case "SCH_GLOBALLABEL":
      return "Global Label";
    case "SCH_HIERLABEL":
      return "Hierarchical Label";
    case "SCH_DIRECTIVE_LABEL":
      return "Directive Label";
    case "SCH_TEXT":
      return "Text";
    case "SCH_TEXTBOX":
      return "Text Box";
    case "SCH_SHAPE":
      return "Graphic";
    case "SCH_RULE_AREA":
      return "Rule Area";
    case "SCH_BITMAP":
      return "Image";
    case "SCH_SHEET":
      return "Sheet";
    case "SCH_TABLE":
      return "Table";
    case "SCH_TABLECELL":
      return "Table Cell";
  }
}

// ---------------------------------------------------------------------------------------------------------------------------- helpers

type PD = PropertyDef<SchItem, SchCtx, Cmd>;

const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const norm360 = (a: number): number => ((a % 360) + 360) % 360;

const LINE_STYLES: Choice[] = [
  { label: "Solid", value: "solid" },
  { label: "Dashed", value: "dash" },
  { label: "Dotted", value: "dot" },
  { label: "Dash-Dot", value: "dash_dot" },
  { label: "Dash-Dot-Dot", value: "dash_dot_dot" },
];
const WIRE_STYLES: Choice[] = [{ label: "Default", value: "default" }, ...LINE_STYLES];

const colorValue = (c: SchColor | null | undefined): string => (!c || isUnspecified(c) ? "" : colorToHex(c));
const colorOf = (text: string): SchColor => (text === "" ? UNSPECIFIED_COLOR : (hexToColor(text) ?? UNSPECIFIED_COLOR));

/** The stroke of a wire, a bus, a bus entry or a graphic line, with the width a graphic line keeps in `width_nm`. */
function strokeOf(i: SchItem): { width: number; style: SchLineStyle; color: string } {
  if (i.line) return { width: i.line.width_nm, style: i.line.stroke?.style ?? "default", color: colorValue(i.line.stroke?.color) };
  const st = i.wire?.stroke ?? i.busEntry?.stroke;
  return { width: st?.width_nm ?? 0, style: st?.style ?? "default", color: colorValue(st?.color) };
}

const strokeCmds = (i: SchItem, patch: { widthNm?: number; style?: SchLineStyle; color?: SchColor; diameterNm?: number }): Cmd[] => {
  const c = strokeEditCmd([i.id], patch);
  return c ? [c] : [];
};

/** `edit_graphic` replaces the drawn graphic: the current one with `patch` laid over it. */
function graphicCmd(g: SchGraphic, patch: Partial<Omit<SchGraphic, "id">>): Cmd {
  const { id, ...rest } = g;
  return { op: "sch_edit", verb: "edit_graphic", id, graphic: { ...rest, ...patch } };
}

const withShape = (g: SchGraphic, shape: SchGraphicShape): Cmd => graphicCmd(g, { shape });

/** The id the move and turn verbs take for a symbol: `U1#2` names one unit of a multi-unit reference, `U1` the only one. */
function symbolTarget(i: SchItem, c: SchCtx): string {
  const s = i.symbol;
  if (!s) return i.id;
  return c.sch.symbols.filter((o) => o.id === s.id).length > 1 ? `${s.id}#${s.unit}` : s.id;
}

const posOf = (i: SchItem): [number, number] => i.symbol?.at ?? i.power?.at ?? [0, 0];

// ----------------------------------------------------------------------------------------------------------------- graphic geometry

type Geometry = "Start X" | "Start Y" | "End X" | "End Y" | "Center X" | "Center Y" | "Radius" | "Width" | "Height" | "Corner Radius";

/**
 * The shape with the geometry property set to `v` (um), the way the `EDA_SHAPE` setters do it (see `shapeWith` of the board): a corner or an end point moves alone,
 * a circle's centre takes nothing with it (the radius is its own number here), `Width` and `Height` put the second corner that far from the first. null when nothing
 * changes or the property is not the shape's.
 */
export function graphicShapeWith(s: SchGraphicShape, name: Geometry, v: number): SchGraphicShape | null {
  const px = (p: PointXY, axis: 0 | 1): PointXY => (axis === 0 ? { x: v, y: p.y } : { x: p.x, y: v });
  let next: SchGraphicShape | null = null;
  switch (name) {
    case "Start X":
    case "Start Y": {
      const axis = name === "Start X" ? 0 : 1;
      if (s.type === "rectangle" || s.type === "arc" || s.type === "bezier" || s.type === "text_box") next = { ...s, start: px(s.start, axis) };
      break;
    }
    case "End X":
    case "End Y": {
      const axis = name === "End X" ? 0 : 1;
      if (s.type === "rectangle" || s.type === "arc" || s.type === "bezier" || s.type === "text_box") next = { ...s, end: px(s.end, axis) };
      break;
    }
    case "Center X":
    case "Center Y":
      if (s.type === "circle") next = { ...s, center: px(s.center, name === "Center X" ? 0 : 1) };
      break;
    case "Radius":
      if (s.type === "circle") next = { ...s, radius_nm: v };
      break;
    case "Width":
    case "Height":
      if (s.type === "rectangle" || s.type === "text_box") {
        const axis = name === "Width" ? "x" : "y";
        const dir = s.end[axis] >= s.start[axis] ? 1 : -1;
        next = { ...s, end: { ...s.end, [axis]: s.start[axis] + dir * v } };
      }
      break;
    case "Corner Radius":
      if (s.type === "rectangle") next = { ...s, corner_radius_nm: v };
      break;
  }
  return next && JSON.stringify(next) !== JSON.stringify(s) ? next : null;
}

const pointOf = (s: SchGraphicShape, which: "start" | "end"): PointXY => {
  switch (s.type) {
    case "rectangle":
    case "arc":
    case "bezier":
    case "text_box":
      return which === "start" ? s.start : s.end;
    case "circle":
      return which === "start" ? s.center : { x: s.center.x + s.radius_nm, y: s.center.y };
    default:
      return { x: 0, y: 0 };
  }
};

/** `EDA_SHAPE::GetArcAngle` in degrees. */
function arcAngle(s: Extract<SchGraphicShape, { type: "arc" }>): number {
  const c = circumcircle([s.start.x, s.start.y], [s.mid.x, s.mid.y], [s.end.x, s.end.y]);
  if (!c) return 0;
  const a = (p: PointXY): number => Math.atan2(p.y - c.cy, p.x - c.cx);
  const turn = (from: number, to: number): number => (((to - from) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  const direct = turn(a(s.start), a(s.end));
  const sweep = turn(a(s.start), a(s.mid)) <= direct ? direct : 2 * Math.PI - direct;
  return Number(((sweep * 180) / Math.PI).toFixed(6));
}

// ----------------------------------------------------------------------------------------------------------------------- the registry

const FILL_CHOICES: Choice[] = [
  { label: "None", value: "none" },
  { label: "Body outline color", value: "outline" },
  { label: "Body background color", value: "background" },
  { label: "Fill color", value: "color" },
];

const ATTRIBUTES = "Attributes";
/** A label's text with no size of its own: 50 mil (`LabelLook::text_size_nm`). */
const LABEL_SIZE_NM = 1_270_000;
const TEXT_PROPS = "Text Properties";
const SHAPE_PROPS = "Shape Properties";

function build(): PropertyManager<SchItem, SchCtx, Cmd> {
  const pm = new PropertyManager<SchItem, SchCtx, Cmd>();
  const add = (def: PD, group = "") => pm.addProperty(def, group);

  // ----------------------------------------------------------------------------------------------------------------- SCH_ITEM
  pm.registerType("SCH_ITEM");
  add({
    owner: "SCH_ITEM",
    name: "Locked",
    kind: "bool",
    get: (i, c) => c.locked.has(i.id),
    set: (i, v, c) => (c.locked.has(i.id) === Boolean(v) ? [] : [{ op: "sch_edit", verb: "set_locked", ids: [i.id], locked: Boolean(v) }]),
  });

  // ------------------------------------------------------------------------------------------------------------- SCH_SYMBOL
  // `SCH_ITEM_DESC`'s Unit, Body Style and Private are the Symbol Editor's (`SetIsHiddenFromDesignEditors`): no rows here.
  add({ owner: "SCH_ITEM", name: "Unit", kind: "string", hiddenFromDesignEditors: true, get: () => "" });
  add({ owner: "SCH_ITEM", name: "Body Style", kind: "string", hiddenFromDesignEditors: true, get: () => "" });
  add({ owner: "SCH_ITEM", name: "Private", kind: "bool", hiddenFromDesignEditors: true, get: () => false });

  // `SCH_SYMBOL_DESC`: SYMBOL's two pin rows, then the symbol's own; `LIB_SYMBOL_DESC` adds three more of SYMBOL's ("Pin Display"), which a placed symbol has too.
  pm.inheritsAfter("SCH_SYMBOL", "SYMBOL");
  pm.inheritsAfter("SCH_SYMBOL", "SCH_ITEM");
  const isReal = (i: SchItem): boolean => !!i.symbol;
  const libOf = (i: SchItem, c: SchCtx): LibSymbol | undefined => {
    const id = i.symbol?.lib_id ?? i.power?.lib_id;
    return id ? c.sch.lib_symbols[id] : undefined;
  };
  // `SCH_SYMBOL::SetShowPinNames` and kin set the placement's own copy of its library symbol; here one library symbol of a lib_id serves every placement.
  const PIN_DISPLAY_REASON = "The pin display is the library symbol's, shared by every placement of it here (one library symbol per lib_id): set it in the Symbol Editor.";
  // `hasLibPart`
  const hasLibPart = (i: SchItem, c: SchCtx): boolean => !!libOf(i, c);
  add({ owner: "SYMBOL", name: "Pin numbers", kind: "bool", get: (i, c) => !libOf(i, c)?.pin_numbers_hidden, available: hasLibPart, readOnlyReason: PIN_DISPLAY_REASON });
  add({ owner: "SYMBOL", name: "Pin names", kind: "bool", get: (i, c) => !libOf(i, c)?.pin_names_hidden, available: hasLibPart, readOnlyReason: PIN_DISPLAY_REASON });
  const PIN_DISPLAY = "Pin Display";
  add({ owner: "SYMBOL", name: "Show Pin Number", kind: "bool", get: (i, c) => !libOf(i, c)?.pin_numbers_hidden, readOnlyReason: PIN_DISPLAY_REASON }, PIN_DISPLAY);
  add({ owner: "SYMBOL", name: "Show Pin Name", kind: "bool", get: (i, c) => !libOf(i, c)?.pin_names_hidden, readOnlyReason: PIN_DISPLAY_REASON }, PIN_DISPLAY);
  // `GetPinNameOffset`, the library's `(pin_names (offset ..))` (mm in the state): KiCad's default 20 mils.
  add({ owner: "SYMBOL", name: "Pin Name Position Offset", kind: "int", display: "size", get: (i, c) => Math.round((libOf(i, c)?.pin_name_offset ?? 0.508) * 1_000_000), readOnlyReason: PIN_DISPLAY_REASON }, PIN_DISPLAY);

  /** `FilterSelectionForLockedItems`: `sch_move` leaves a locked item where it is and refuses to move nothing. */
  const unlocked = (i: SchItem, c: SchCtx): boolean => !c.locked.has(i.id);
  const move = (axis: 0 | 1) => (i: SchItem, v: PropValue, c: SchCtx): Cmd[] => {
    const at = posOf(i);
    const delta = Math.round(num(v)) - at[axis];
    return delta === 0 ? [] : [{ op: "sch_move", verb: "move", ids: [symbolTarget(i, c)], dx: axis === 0 ? delta : 0, dy: axis === 1 ? delta : 0 }];
  };
  const LOCKED = "The item is locked: unlock it to move it.";
  add({ owner: "SCH_SYMBOL", name: "Position X", kind: "int", display: "coord", get: (i) => posOf(i)[0], writeable: unlocked, readOnlyReason: LOCKED, set: move(0) });
  add({ owner: "SCH_SYMBOL", name: "Position Y", kind: "int", display: "coord", get: (i) => posOf(i)[1], writeable: unlocked, readOnlyReason: LOCKED, set: move(1) });
  add({
    owner: "SCH_SYMBOL",
    name: "Orientation",
    kind: "enum",
    choices: () => [0, 90, 180, 270].map((n) => ({ label: String(n), value: n })),
    get: (i) => norm360(Math.round((i.symbol?.rot ?? i.power?.rot ?? 0) / 90) * 90),
    set: (i, v, c) => {
      const turns = ((((Math.round(num(v) / 90) - Math.round((i.symbol?.rot ?? i.power?.rot ?? 0) / 90)) % 4) + 4) % 4) as number;
      if (turns === 0) return [];
      // `rotate_symbol` adds quarter turns to the stored angle (counter-clockwise, the sense of `R`) and leaves the mirror alone, which is what
      // `SCH_SYMBOL::SetOrientationProp` does. A power symbol has no mirror: it turns about its own anchor like any single item.
      if (i.symbol) return [{ op: "rotate_symbol", id: i.symbol.id, quarter_turns: turns, unit: i.symbol.unit }];
      return Array.from({ length: turns }, () => ({ op: "sch_move", verb: "rotate", ids: [symbolTarget(i, c)], ccw: true }) as Cmd);
    },
  });
  // KiCad's flag names: "Mirror X" is the symbol flipped about the X axis (`SYM_MIRROR_X`, the `Y` key); the state calls it `mirror: "x"`.
  const NO_MIRROR = "A power symbol is not mirrored here: it turns about its anchor.";
  add({
    owner: "SCH_SYMBOL",
    name: "Mirror X",
    kind: "bool",
    get: (i) => i.symbol?.mirror === "x",
    writeable: isReal,
    readOnlyReason: NO_MIRROR,
    set: (i, v) => (i.symbol!.mirror === "x" === Boolean(v) ? [] : [{ op: "mirror_symbol_vertical", id: i.symbol!.id, unit: i.symbol!.unit }]),
  });
  add({
    owner: "SCH_SYMBOL",
    name: "Mirror Y",
    kind: "bool",
    get: (i) => i.symbol?.mirror === "y",
    writeable: isReal,
    readOnlyReason: NO_MIRROR,
    set: (i, v) => (i.symbol!.mirror === "y" === Boolean(v) ? [] : [{ op: "mirror_symbol", id: i.symbol!.id, unit: i.symbol!.unit }]),
  });

  const FIELDS = "Fields";
  // A power symbol's reference and value are read-only: the rail it stands for is its net.
  add(
    {
      owner: "SCH_SYMBOL",
      name: "Reference",
      kind: "string",
      get: (i) => i.symbol?.id ?? i.power?.id ?? "",
      writeable: (i, c) => isReal(i) && c.count === 1,
      readOnlyReason: (i) => (i.power ? "A power symbol's reference is not edited: its value is the net it stands for." : "Two symbols cannot share a reference: rename them one at a time."),
      validate: (v) => (String(v).trim() === "" ? "A reference cannot be empty." : null),
      set: (i, v) => (!i.symbol || i.symbol.id === String(v).trim() ? [] : [{ op: "rename_symbol", id: i.symbol.id, new_id: String(v).trim() }]),
    },
    FIELDS
  );
  add(
    {
      owner: "SCH_SYMBOL",
      name: "Value",
      kind: "string",
      get: (i) => (i.symbol ? (i.symbol.value ?? "") : (i.power?.net ?? "")),
      writeable: isReal,
      readOnlyReason: "A power symbol's value is the net it stands for: edit its Value field.",
      set: (i, v, c) => (!i.symbol || (i.symbol.value ?? "") === String(v) ? [] : (variantEdit(c, i.symbol.id, { field: ["Value", String(v)] }) ?? [{ op: "edit_symbol_fields", id: i.symbol.id, value: String(v) }])),
    },
    FIELDS
  );
  add({ owner: "SCH_SYMBOL", name: "Library Link", kind: "string", get: (i) => i.symbol?.lib_id ?? i.power?.lib_id ?? "" }, FIELDS);
  add({ owner: "SCH_SYMBOL", name: "Library Description", kind: "string", get: (i, c) => libOf(i, c)?.description ?? "" }, FIELDS);
  add({ owner: "SCH_SYMBOL", name: "Keywords", kind: "string", get: (i, c) => libOf(i, c)?.keywords ?? "" }, FIELDS);
  // "Unit" (`SetUnitProp`, `multiUnit`): the units by display name (`GetUnitDisplayName`: the unit's name, else its letter). `SetUnitSelection( .. ); SetUnit( aUnit )`: the
  // `set_symbol_unit` verb, which refuses a unit already placed on the sheet (KiCad makes a second placement of it; the Symbol Unit menu swaps the two).
  add({
    owner: "SCH_SYMBOL",
    name: "Unit",
    kind: "enum",
    choices: (i, c) => {
      const lib = libOf(i, c);
      return Array.from({ length: unitCountOf(lib) }, (_, k) => ({ label: lib?.unit_names?.[String(k + 1)] ?? letterSubReference(k + 1), value: k + 1 }));
    },
    get: (i) => i.symbol?.unit ?? 1,
    available: (i, c) => isReal(i) && unitCountOf(libOf(i, c)) > 1,
    set: (i, v) => {
      const unit = Number(v);
      return !i.symbol || !Number.isFinite(unit) || unit === (i.symbol.unit ?? 1) ? [] : [{ op: "sch_edit", verb: "set_symbol_unit", id: i.symbol.id, unit: i.symbol.unit ?? 1, new_unit: unit }];
    },
  });
  // "Body Style" (`SetBodyStyleProp`, `multiBodyStyle`): "Standard" and "Alternate" for a De Morgan symbol (`GetBodyStyleDescription`), `set_body_style`.
  const BODY_STYLES = ["Standard", "Alternate"];
  add({
    owner: "SCH_SYMBOL",
    name: "Body Style",
    kind: "enum",
    choices: () => BODY_STYLES.map((label) => ({ label, value: label })),
    get: (i) => BODY_STYLES[(i.symbol?.body_style ?? 1) - 1] ?? "Standard",
    available: (i, c) => isReal(i) && (libOf(i, c)?.body_style_count ?? 1) > 1,
    set: (i, v) => {
      const style = BODY_STYLES.indexOf(String(v)) + 1;
      return style < 1 || style === (i.symbol!.body_style ?? 1) ? [] : [{ op: "sch_edit", verb: "set_body_style", ids: [i.symbol!.id], style }];
    },
  });

  const attribute = (name: string, key: "exclude_from_sim" | "exclude_from_bom" | "exclude_from_board" | "dnp"): void => {
    add(
      {
        owner: "SCH_SYMBOL",
        name,
        kind: "bool",
        get: (i) => !!i.symbol?.[key],
        writeable: isReal,
        readOnlyReason: "A power symbol has no attributes of its own here.",
        set: (i, v, c) => (!!i.symbol![key] === Boolean(v) ? [] : (variantEdit(c, i.symbol!.id, { flag: [key, Boolean(v)] }) ?? [{ op: "set_symbol_attrs", ids: [i.symbol!.id], [key]: Boolean(v) } as Cmd])),
      },
      ATTRIBUTES
    );
  };
  attribute("Exclude From Simulation", "exclude_from_sim");
  attribute("Exclude From Bill of Materials", "exclude_from_bom");
  attribute("Exclude From Board", "exclude_from_board");
  add(
    {
      owner: "SCH_SYMBOL",
      name: "Exclude From Position Files",
      kind: "bool",
      get: (i) => !!i.symbol?.exclude_from_pos_files,
      writeable: isReal,
      readOnlyReason: "A power symbol has no attributes of its own here.",
      set: (i, v, c) =>
        !!i.symbol!.exclude_from_pos_files === Boolean(v) ? [] : (variantEdit(c, i.symbol!.id, { flag: ["exclude_from_pos_files", Boolean(v)] }) ?? [{ op: "sch_edit", verb: "set_exclude_from_pos_files", ids: [i.symbol!.id], exclude: Boolean(v) }]),
    },
    ATTRIBUTES
  );
  attribute("Do not Populate", "dnp");

  // ---------------------------------------------------------------------------------------------------------- EDA_TEXT, SCH_TEXT
  // `EDA_TEXT_DESC` (common/eda_text.cpp) over a free text (`text`), a label (`label`), a text box (`graphic` text_box) and a directive label: a free text's
  // bold, italic, colour and hyperlink are its `text_attrs` (`set_text_attrs`), its size and justification `edit_text`'s; a label's size, bold and italic are
  // Label Properties' (`edit_label`), its colour its `text_attrs`; a text box's are its shape's.
  pm.registerType("EDA_TEXT");
  const isTextBox = (i: SchItem): boolean => i.graphic?.shape.type === "text_box";
  const textBox = (i: SchItem): Extract<SchGraphicShape, { type: "text_box" }> => i.graphic!.shape as Extract<SchGraphicShape, { type: "text_box" }>;
  const attrsCmd = (i: SchItem, patch: { bold?: boolean; italic?: boolean; color?: SchColor; href?: string }): Cmd => ({ op: "sch_edit", verb: "set_text_attrs", ids: [i.id], ...patch });
  const labelCmd = (i: SchItem, patch: { size_nm?: number; bold?: boolean; italic?: boolean; spin?: LabelSpin }): Cmd => ({ op: "sch_edit", verb: "edit_label", id: i.id, ...patch });
  const BOX_REASON = "A text box's font colour and hyperlink are kept as the file had them, not shown or edited here.";
  // Orientation: every schematic text masks it ("Orientation is exposed differently in schematic").
  add({ owner: "EDA_TEXT", name: "Orientation", kind: "double", display: "degree", get: () => 0 });
  add(
    {
      owner: "EDA_TEXT",
      name: "Text",
      kind: "string",
      get: (i) => i.text?.content ?? i.label?.net ?? (isTextBox(i) ? textBox(i).text : ""),
      validate: (v, i) => (String(v) === "" ? (i.label ? "Label can not be empty." : "Text can not be empty.") : null),
      set: (i, v) => {
        const text = String(v);
        if (i.text) return i.text.content === text ? [] : [{ op: "sch_edit", verb: "edit_text", id: i.id, text }];
        if (i.label) return i.label.net === text.trim() ? [] : [{ op: "sch_edit", verb: "edit_label", id: i.id, text: text.trim() }];
        if (isTextBox(i)) return textBox(i).text === text ? [] : [withShape(i.graphic!, { ...textBox(i), text })];
        return [];
      },
    },
    TEXT_PROPS
  );
  // "Font" (`GetFontProp`): eeschema's "Default Font" first. The model keeps no face for a schematic text.
  add({ owner: "EDA_TEXT", name: "Font", kind: "string", get: () => "Default Font", readOnlyReason: "The model keeps no font face for a schematic text: KiCad's default font is drawn." }, TEXT_PROPS);
  /** Bold, the way each kind keeps it. */
  const boldOf = (i: SchItem): boolean => !!(i.text?.bold ?? i.label?.bold ?? (isTextBox(i) ? textBox(i).bold : false));
  // "Auto Thickness": a schematic text has no pen of its own here -- KiCad's for its size, `GetPenSizeForBold` once bold (`EDA_TEXT::SetBold`).
  add({ owner: "EDA_TEXT", name: "Auto Thickness", kind: "bool", get: (i) => !boldOf(i), readOnlyReason: "A schematic text keeps no pen of its own here: KiCad's for its size (a bold one's is GetPenSizeForBold)." }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Thickness", kind: "int", display: "size", get: () => 0 }, TEXT_PROPS);
  const flag = (name: "Italic" | "Bold", key: "italic" | "bold"): void => {
    add(
      {
        owner: "EDA_TEXT",
        name,
        kind: "bool",
        get: (i) => !!(i.text?.[key] ?? i.label?.[key] ?? (isTextBox(i) ? textBox(i)[key] : false)),
        set: (i, v) => {
          const on = Boolean(v);
          if (i.text) return !!i.text[key] === on ? [] : [attrsCmd(i, { [key]: on })];
          if (i.label) return !!i.label[key] === on ? [] : [labelCmd(i, { [key]: on })];
          if (isTextBox(i)) return !!textBox(i)[key] === on ? [] : [withShape(i.graphic!, { ...textBox(i), [key]: on })];
          return [];
        },
      },
      TEXT_PROPS
    );
  };
  flag("Italic", "italic");
  flag("Bold", "bold");
  // A text box has the row (`SCH_TEXTBOX_DESC` masks the width, height and pen, not the mirror); no schematic text is mirrored here.
  add({ owner: "EDA_TEXT", name: "Mirrored", kind: "bool", get: () => false, readOnlyReason: "A schematic text box is not mirrored here." }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Visible", kind: "bool", get: () => true, available: () => false }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Width", kind: "int", display: "size", get: () => 0 }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Height", kind: "int", display: "size", get: () => 0 }, TEXT_PROPS);
  // A label's justification is the side of its anchor it runs to (`SCH_LABEL_BASE::SetSpinStyle`: left against the anchor when it runs right or up, right when
  // it runs left or down, and bottom); its spin, when none was set, is read off the wire it ends on.
  const spinOf = (i: SchItem, c: SchCtx): LabelSpin => i.label!.spin ?? inferSpin(c.sch.wires, i.label!.at);
  const labelHJ = (i: SchItem, c: SchCtx): "left" | "right" => (spinOf(i, c) === "right" || spinOf(i, c) === "up" ? "left" : "right");
  add(
    {
      owner: "EDA_TEXT",
      name: "Horizontal Justification",
      kind: "enum",
      choices: () => [
        { label: "Left", value: "left" },
        { label: "Center", value: "center" },
        { label: "Right", value: "right" },
      ],
      get: (i, c) => (i.text ? (i.text.h ?? "left") : i.label ? labelHJ(i, c) : isTextBox(i) ? (textBox(i).h_align ?? "left") : "left"),
      validate: (v, i) => (i.label && v === "center" ? "A label is justified left or right of its anchor (the side it runs to)." : null),
      set: (i, v, c) => {
        if (i.text) return (i.text.h ?? "left") === v ? [] : [{ op: "sch_edit", verb: "edit_text", id: i.id, h: String(v) as "left" | "center" | "right" }];
        if (i.label) {
          if (labelHJ(i, c) === v) return [];
          const vertical = spinOf(i, c) === "up" || spinOf(i, c) === "bottom";
          return [labelCmd(i, { spin: v === "left" ? (vertical ? "up" : "right") : vertical ? "bottom" : "left" })];
        }
        if (isTextBox(i)) return (textBox(i).h_align ?? "left") === v ? [] : [withShape(i.graphic!, { ...textBox(i), h_align: String(v) as "left" | "center" | "right" })];
        return [];
      },
    },
    TEXT_PROPS
  );
  add(
    {
      owner: "EDA_TEXT",
      name: "Vertical Justification",
      kind: "enum",
      choices: () => [
        { label: "Top", value: "top" },
        { label: "Center", value: "center" },
        { label: "Bottom", value: "bottom" },
      ],
      get: (i) => (i.text ? (i.text.v ?? "bottom") : i.label ? "bottom" : isTextBox(i) ? (textBox(i).v_align ?? "top") : "bottom"),
      writeable: (i) => !i.label,
      readOnlyReason: "A label sits on its anchor: its vertical justification is bottom (SetSpinStyle).",
      set: (i, v) => {
        if (i.text) return (i.text.v ?? "bottom") === v ? [] : [{ op: "sch_edit", verb: "edit_text", id: i.id, v: String(v) as "top" | "center" | "bottom" }];
        if (isTextBox(i)) return (textBox(i).v_align ?? "top") === v ? [] : [withShape(i.graphic!, { ...textBox(i), v_align: String(v) as "top" | "center" | "bottom" })];
        return [];
      },
    },
    TEXT_PROPS
  );
  add(
    {
      owner: "EDA_TEXT",
      name: "Color",
      kind: "color",
      get: (i) => colorValue(i.text?.color ?? i.label?.color),
      writeable: (i) => !!i.text || !!i.label,
      readOnlyReason: BOX_REASON,
      set: (i, v) => (colorValue(i.text?.color ?? i.label?.color) === String(v) ? [] : [attrsCmd(i, { color: colorOf(String(v)) })]),
    },
    TEXT_PROPS
  );
  add(
    {
      owner: "EDA_TEXT",
      name: "Hyperlink",
      kind: "string",
      get: (i) => i.text?.href ?? "",
      writeable: (i) => !!i.text,
      readOnlyReason: BOX_REASON,
      set: (i, v) => ((i.text!.href ?? "") === String(v).trim() ? [] : [attrsCmd(i, { href: String(v).trim() })]),
    },
    TEXT_PROPS
  );

  pm.inheritsAfter("SCH_TEXT", "SCH_ITEM");
  pm.inheritsAfter("SCH_TEXT", "EDA_TEXT");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Mirrored");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Width");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Height");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Thickness");
  // "Text Size" (`SetSchTextSize`): a free text's, a label's (Label Properties' size), a text box's.
  const textSize = (owner: string): void => {
    add(
      {
        owner,
        name: "Text Size",
        kind: "int",
        display: "size",
        get: (i) => (i.text ? i.text.size_nm : i.label ? (i.label.size_nm ?? LABEL_SIZE_NM) : isTextBox(i) ? textBox(i).size_nm : LABEL_SIZE_NM),
        writeable: (i) => !!i.text || !!i.label || isTextBox(i),
        readOnlyReason: "A directive label's text size is not modelled: it is drawn at 50 mil.",
        // `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM`: "Don't allow text to disappear" (0.01 to 1000 mm)
        validate: (v, _i, c) => (num(v) < 10_000 ? tooSmall(10_000, "size", c.units) : num(v) > 1_000_000_000 ? "Value must be less than or equal to 1000 mm" : null),
        set: (i, v) => {
          const size = Math.round(num(v));
          if (i.text) return i.text.size_nm === size ? [] : [{ op: "sch_edit", verb: "edit_text", id: i.id, size_nm: size }];
          if (i.label) return (i.label.size_nm ?? LABEL_SIZE_NM) === size ? [] : [labelCmd(i, { size_nm: size })];
          return isTextBox(i) && textBox(i).size_nm !== size ? [withShape(i.graphic!, { ...textBox(i), size_nm: size })] : [];
        },
      },
      TEXT_PROPS
    );
  };
  textSize("SCH_TEXT");
  // "Orientation is exposed differently in schematic; mask the base for now"
  pm.mask("SCH_TEXT", "EDA_TEXT", "Orientation");

  // ------------------------------------------------------------------------------------------------------------------- labels
  pm.inheritsAfter("SCH_LABEL_BASE", "SCH_TEXT");
  pm.inheritsAfter("SCH_LABEL", "SCH_LABEL_BASE");
  pm.inheritsAfter("SCH_HIERLABEL", "SCH_LABEL_BASE");
  pm.inheritsAfter("SCH_GLOBALLABEL", "SCH_LABEL_BASE");
  const hasLabelShape = (i: SchItem): boolean => !!i.label && i.label.scope !== "local";
  add({
    owner: "SCH_LABEL_BASE",
    name: "Shape",
    kind: "enum",
    choices: () => [
      { label: "Input", value: "input" },
      { label: "Output", value: "output" },
      { label: "Bidirectional", value: "bidirectional" },
      { label: "Tri-state", value: "tri_state" },
      { label: "Passive", value: "passive" },
    ],
    get: (i) => i.label?.shape ?? "input",
    available: hasLabelShape,
    set: (i, v) => (i.label!.shape === v ? [] : [{ op: "sch_edit", verb: "edit_label", id: i.id, shape: String(v) as LabelShape }]),
  });
  pm.mask("SCH_LABEL_BASE", "EDA_TEXT", "Hyperlink");

  // The directive label (`SCH_DIRECTIVE_LABEL`): a flag with a pole, drawn from an `SchGraphic`.
  pm.inheritsAfter("SCH_DIRECTIVE_LABEL", "SCH_LABEL_BASE");
  const directive = (i: SchItem): Extract<SchGraphicShape, { type: "directive" }> => i.graphic!.shape as Extract<SchGraphicShape, { type: "directive" }>;
  add({
    owner: "SCH_DIRECTIVE_LABEL",
    name: "Shape",
    kind: "enum",
    choices: () => [
      { label: "Dot", value: "dot" },
      { label: "Circle", value: "round" },
      { label: "Diamond", value: "diamond" },
      { label: "Rectangle", value: "rectangle" },
    ],
    get: (i) => directive(i).shape ?? "round",
    set: (i, v) => ((directive(i).shape ?? "round") === v ? [] : [withShape(i.graphic!, { ...directive(i), shape: String(v) as "dot" | "round" | "diamond" | "rectangle" })]),
  });
  add({
    owner: "SCH_DIRECTIVE_LABEL",
    name: "Pin length",
    kind: "int",
    display: "size",
    get: (i) => directive(i).pin_length_nm,
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => (directive(i).pin_length_nm === num(v) ? [] : [withShape(i.graphic!, { ...directive(i), pin_length_nm: Math.round(num(v)) })]),
  });
  for (const name of ["Text", "Thickness", "Italic", "Bold", "Horizontal Justification", "Vertical Justification"]) pm.mask("SCH_DIRECTIVE_LABEL", "EDA_TEXT", name);

  // --------------------------------------------------------------------------------------------------------------- SCH_LINE
  pm.inheritsAfter("SCH_LINE", "SCH_ITEM");
  const isGraphicLine = (i: SchItem): boolean => i.lineKind === "line";
  const isWireOrBus = (i: SchItem): boolean => i.lineKind === "wire" || i.lineKind === "bus";
  const pts = (i: SchItem): Array<[number, number]> => i.wire?.pts ?? i.line?.pts ?? [];
  const lineEnd = (which: "start" | "end", axis: 0 | 1) => (i: SchItem): number => {
    const p = pts(i);
    return (which === "start" ? p[0] : p[p.length - 1])?.[axis] ?? 0;
  };
  // `SCH_LINE::SetStartX` .. `SetEndY`: the end point goes where it is typed (`set_line_ends`; nothing joined to it follows, as in KiCad), then the commit's
  // clean-up merges and splits wires and puts the junctions.
  const lineEndRow = (name: string, which: "start" | "end", axis: 0 | 1): void => {
    add({
      owner: "SCH_LINE",
      name,
      kind: "int",
      display: "coord",
      get: lineEnd(which, axis),
      writeable: unlocked,
      readOnlyReason: LOCKED,
      set: (i, v) => {
        const p = pts(i);
        const at = (which === "start" ? p[0] : p[p.length - 1]) ?? [0, 0];
        const n = Math.round(num(v));
        if (at[axis] === n) return [];
        const next = axis === 0 ? { x: n, y: at[1] } : { x: at[0], y: n };
        return [{ op: "sch_edit", verb: "set_line_ends", id: i.id, [which]: next } as Cmd];
      },
    });
  };
  lineEndRow("Start X", "start", 0);
  lineEndRow("Start Y", "start", 1);
  lineEndRow("End X", "end", 0);
  lineEndRow("End Y", "end", 1);
  add({
    owner: "SCH_LINE",
    name: "Length",
    kind: "double",
    display: "size",
    get: (i) => Math.round(pts(i).slice(1).reduce((n, p, k) => n + Math.hypot(p[0] - pts(i)[k]![0], p[1] - pts(i)[k]![1]), 0)),
    readOnlyReason: "A line's length is not typed here (SetLength moves its end along it): move its end point.",
  });
  add({
    owner: "SCH_LINE",
    name: "Line Style",
    kind: "enum",
    choices: () => LINE_STYLES,
    get: (i) => (strokeOf(i).style === "default" ? "solid" : strokeOf(i).style),
    available: isGraphicLine,
    set: (i, v) => ((strokeOf(i).style === "default" ? "solid" : strokeOf(i).style) === v ? [] : strokeCmds(i, { style: String(v) as SchLineStyle })),
  });
  add({
    owner: "SCH_LINE",
    name: "Wire Style",
    kind: "enum",
    choices: () => WIRE_STYLES,
    get: (i) => strokeOf(i).style,
    available: isWireOrBus,
    set: (i, v) => (strokeOf(i).style === v ? [] : strokeCmds(i, { style: String(v) as SchLineStyle })),
  });
  add({
    owner: "SCH_LINE",
    name: "Line Width",
    kind: "int",
    display: "size",
    get: (i) => strokeOf(i).width,
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => (strokeOf(i).width === num(v) ? [] : strokeCmds(i, { widthNm: Math.round(num(v)) })),
  });
  add({
    owner: "SCH_LINE",
    name: "Color",
    kind: "color",
    get: (i) => strokeOf(i).color,
    set: (i, v) => (strokeOf(i).color === String(v) ? [] : strokeCmds(i, { color: colorOf(String(v)) })),
  });

  // --------------------------------------------------------------------------------------------------- junction, bus entry
  pm.inheritsAfter("SCH_JUNCTION", "SCH_ITEM");
  add({
    owner: "SCH_JUNCTION",
    name: "Diameter",
    kind: "int",
    display: "size",
    get: (i) => i.junction!.look?.diameter_nm ?? 0,
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => ((i.junction!.look?.diameter_nm ?? 0) === num(v) ? [] : strokeCmds(i, { diameterNm: Math.round(num(v)) })),
  });
  add({
    owner: "SCH_JUNCTION",
    name: "Color",
    kind: "color",
    get: (i) => colorValue(i.junction!.look?.color),
    set: (i, v) => (colorValue(i.junction!.look?.color) === String(v) ? [] : strokeCmds(i, { color: colorOf(String(v)) })),
  });

  pm.inheritsAfter("SCH_BUS_ENTRY_BASE", "SCH_ITEM");
  pm.inheritsAfter("SCH_BUS_WIRE_ENTRY", "SCH_BUS_ENTRY_BASE");
  add({ owner: "SCH_BUS_ENTRY_BASE", name: "Wire Style", kind: "enum", choices: () => WIRE_STYLES, get: (i) => strokeOf(i).style, set: (i, v) => (strokeOf(i).style === v ? [] : strokeCmds(i, { style: String(v) as SchLineStyle })) });
  add({
    owner: "SCH_BUS_ENTRY_BASE",
    name: "Line Width",
    kind: "int",
    display: "size",
    get: (i) => strokeOf(i).width,
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => (strokeOf(i).width === num(v) ? [] : strokeCmds(i, { widthNm: Math.round(num(v)) })),
  });
  add({ owner: "SCH_BUS_ENTRY_BASE", name: "Color", kind: "color", get: (i) => strokeOf(i).color, set: (i, v) => (strokeOf(i).color === String(v) ? [] : strokeCmds(i, { color: colorOf(String(v)) })) });

  // ------------------------------------------------------------------------------------------------------------------ SCH_SHEET
  // `SCH_SHEET_DESC`: the name, the border and background, the attributes. Its fields (Sheetfile, Sheetname, the user's) are rows the panel adds (`fieldRows`).
  pm.inheritsAfter("SCH_SHEET", "SCH_ITEM");
  add({
    owner: "SCH_SHEET",
    name: "Sheet Name",
    kind: "string",
    get: (i) => i.sheet!.name,
    validate: (v) => (String(v).trim() === "" ? "A sheet needs a name." : null),
    set: (i, v) => (i.sheet!.name === String(v).trim() ? [] : [{ op: "sch_edit", verb: "edit_sheet", id: i.id, name: String(v).trim() }]),
  });
  // A sheet's border, background and attributes are not kept by the model (a sheet file's `(stroke ..)`, `(fill ..)` and flags are not read): KiCad's defaults.
  const SHEET_LOOK = "A sheet's border, background and attributes are not kept by the model: it is drawn and written with KiCad's defaults.";
  add({ owner: "SCH_SHEET", name: "Border Width", kind: "int", display: "size", get: () => 0, readOnlyReason: SHEET_LOOK });
  add({ owner: "SCH_SHEET", name: "Border Color", kind: "color", get: () => "", readOnlyReason: SHEET_LOOK });
  add({ owner: "SCH_SHEET", name: "Background Color", kind: "color", get: () => "", readOnlyReason: SHEET_LOOK });
  for (const name of ["Exclude From Board", "Exclude From Simulation", "Exclude From Bill of Materials", "Do not Populate"]) {
    add({ owner: "SCH_SHEET", name, kind: "bool", get: () => false, readOnlyReason: SHEET_LOOK }, ATTRIBUTES);
  }

  // ----------------------------------------------------------------------------------------------- EDA_SHAPE, SCH_SHAPE and kin
  pm.registerType("EDA_SHAPE");
  const shapeOf = (i: SchItem): SchGraphicShape => i.graphic!.shape;
  const typeIs = (...types: SchGraphicShape["type"][]) => (i: SchItem): boolean => types.includes(shapeOf(i).type);
  add(
    {
      owner: "EDA_SHAPE",
      name: "Shape",
      kind: "string",
      get: (i) =>
        ({ rectangle: "Rectangle", circle: "Circle", arc: "Arc", bezier: "Bezier", polygon: "Polygon", text_box: "Rectangle", rule_area: "Polygon", directive: "Polygon", image: "Rectangle" })[shapeOf(i).type],
    },
    SHAPE_PROPS
  );
  const geometry = (name: Geometry, display: "coord" | "size", read: (s: SchGraphicShape) => number, applies: (i: SchItem) => boolean): void => {
    add(
      {
        owner: "EDA_SHAPE",
        name,
        kind: "int",
        display,
        get: (i) => read(shapeOf(i)),
        available: applies,
        validate: display === "size" && name !== "Corner Radius" ? (v, _i, c) => (num(v) < 1_000 ? tooSmall(1_000, "size", c.units) : null) : (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
        set: (i, v) => {
          const next = graphicShapeWith(shapeOf(i), name, Math.round(num(v)));
          return next ? [withShape(i.graphic!, next)] : [];
        },
      },
      SHAPE_PROPS
    );
  };
  const hasCorners = typeIs("rectangle", "arc", "bezier", "text_box");
  geometry("Start X", "coord", (s) => pointOf(s, "start").x, hasCorners);
  geometry("Start Y", "coord", (s) => pointOf(s, "start").y, hasCorners);
  geometry("Center X", "coord", (s) => pointOf(s, "start").x, typeIs("circle"));
  geometry("Center Y", "coord", (s) => pointOf(s, "start").y, typeIs("circle"));
  geometry("Radius", "size", (s) => (s.type === "circle" ? s.radius_nm : 0), typeIs("circle"));
  geometry("End X", "coord", (s) => pointOf(s, "end").x, hasCorners);
  geometry("End Y", "coord", (s) => pointOf(s, "end").y, hasCorners);
  geometry("Width", "size", (s) => (s.type === "rectangle" || s.type === "text_box" ? Math.abs(s.end.x - s.start.x) : 0), typeIs("rectangle", "text_box"));
  geometry("Height", "size", (s) => (s.type === "rectangle" || s.type === "text_box" ? Math.abs(s.end.y - s.start.y) : 0), typeIs("rectangle", "text_box"));
  geometry("Corner Radius", "size", (s) => (s.type === "rectangle" ? (s.corner_radius_nm ?? 0) : 0), typeIs("rectangle"));
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Width",
      kind: "int",
      display: "size",
      get: (i) => i.graphic!.width_nm ?? 0,
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => ((i.graphic!.width_nm ?? 0) === num(v) ? [] : [graphicCmd(i.graphic!, { width_nm: Math.round(num(v)) })]),
    },
    SHAPE_PROPS
  );
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Style",
      kind: "enum",
      choices: () => WIRE_STYLES,
      get: (i) => i.graphic!.line_style ?? "default",
      set: (i, v) => ((i.graphic!.line_style ?? "default") === v ? [] : [graphicCmd(i.graphic!, { line_style: String(v) as SchLineStyle })]),
    },
    SHAPE_PROPS
  );
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Color",
      kind: "color",
      get: (i) => colorValue(i.graphic!.color),
      set: (i, v) => (colorValue(i.graphic!.color) === String(v) ? [] : [graphicCmd(i.graphic!, { color: colorOf(String(v)) })]),
    },
    SHAPE_PROPS
  );
  add({ owner: "EDA_SHAPE", name: "Angle", kind: "double", display: "degree", get: (i) => arcAngle(shapeOf(i) as Extract<SchGraphicShape, { type: "arc" }>), available: typeIs("arc") }, SHAPE_PROPS);
  // `fillAvailable`: a polygon, a rectangle, a circle or a Bezier -- which a text box (a rectangle) and a rule area (a polygon) are too.
  const fillAvailable = typeIs("polygon", "rectangle", "circle", "bezier", "text_box", "rule_area");
  add(
    {
      owner: "EDA_SHAPE",
      name: "Fill",
      kind: "enum",
      choices: () => FILL_CHOICES,
      get: (i) => i.graphic!.fill ?? "none",
      available: fillAvailable,
      set: (i, v) => ((i.graphic!.fill ?? "none") === v ? [] : [graphicCmd(i.graphic!, { fill: String(v) as SchGraphic["fill"] })]),
    },
    SHAPE_PROPS
  );
  add(
    {
      owner: "EDA_SHAPE",
      name: "Fill Color",
      kind: "color",
      get: (i) => colorValue(i.graphic!.fill_color),
      available: fillAvailable,
      // "isFillColorEditable": the colour only counts for a shape filled with a colour
      writeable: (i) => (i.graphic!.fill ?? "none") === "color",
      set: (i, v) => (colorValue(i.graphic!.fill_color) === String(v) ? [] : [graphicCmd(i.graphic!, { fill_color: colorOf(String(v)) })]),
    },
    SHAPE_PROPS
  );

  pm.inheritsAfter("SCH_SHAPE", "SCH_ITEM");
  pm.inheritsAfter("SCH_SHAPE", "EDA_SHAPE");
  // "Fill Mode" (`FILL_T`): a symbol's shapes only (`isSymbolItem`, the Symbol Editor's); a shape on the sheet has the Fill row.
  add({ owner: "SCH_SHAPE", name: "Fill Mode", kind: "enum", choices: () => FILL_CHOICES, get: (i) => i.graphic!.fill ?? "none", available: () => false }, SHAPE_PROPS);

  // The text box: a rectangle that holds text. A text box is not filled in this editor (`fillAvailable` says so for `SCH_TEXTBOX`), and a rectangle's shape is not a choice.
  pm.inheritsAfter("SCH_TEXTBOX", "SCH_SHAPE");
  pm.inheritsAfter("SCH_TEXTBOX", "EDA_SHAPE");
  pm.inheritsAfter("SCH_TEXTBOX", "EDA_TEXT");
  pm.mask("SCH_TEXTBOX", "EDA_SHAPE", "Shape");
  pm.mask("SCH_TEXTBOX", "EDA_SHAPE", "Corner Radius");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Width");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Height");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Thickness");
  // `fillAvailable` gives a schematic text box its fill rows (only a board's text box and table cell are left without); not edited here.
  pm.overrideWriteability("SCH_TEXTBOX", "EDA_SHAPE", "Fill", () => false, "A text box's fill is not edited here.");
  pm.overrideWriteability("SCH_TEXTBOX", "EDA_SHAPE", "Fill Color", () => false, "A text box's fill is not edited here.");
  // "Margins": the model keeps one margin for the four sides (`margin_nm`; 0 is `GetLegacyTextMargin`), so the rows show it, read-only.
  const MARGINS = "Margins";
  const margin = (i: SchItem): number => textBox(i).margin_nm || defaultTextBoxMargin(textBox(i).size_nm, i.graphic!.width_nm ?? 0);
  for (const side of ["Left", "Top", "Right", "Bottom"]) {
    add({ owner: "SCH_TEXTBOX", name: `Margin ${side}`, kind: "int", display: "size", get: margin, readOnlyReason: "The model keeps one margin for the four sides of a text box: set it in Text Box Properties." }, MARGINS);
  }
  textSize("SCH_TEXTBOX");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Orientation");

  // The rule area: a polygon with exclusion flags (`SCH_RULE_AREA`).
  pm.inheritsAfter("SCH_RULE_AREA", "SCH_SHAPE");
  pm.inheritsAfter("SCH_RULE_AREA", "SCH_ITEM");
  pm.inheritsAfter("SCH_RULE_AREA", "EDA_SHAPE");
  pm.overrideWriteability("SCH_RULE_AREA", "EDA_SHAPE", "Fill", () => false, "A rule area is drawn as an outline here: its fill is not edited.");
  pm.overrideWriteability("SCH_RULE_AREA", "EDA_SHAPE", "Fill Color", () => false, "A rule area is drawn as an outline here: its fill is not edited.");
  const rule = (i: SchItem): Extract<SchGraphicShape, { type: "rule_area" }> => i.graphic!.shape as Extract<SchGraphicShape, { type: "rule_area" }>;
  const ruleFlag = (name: string, key: "exclude_from_board" | "exclude_from_sim" | "exclude_from_bom" | "dnp"): void => {
    add(
      {
        owner: "SCH_RULE_AREA",
        name,
        kind: "bool",
        get: (i) => !!rule(i)[key],
        set: (i, v) => (!!rule(i)[key] === Boolean(v) ? [] : [withShape(i.graphic!, { ...rule(i), [key]: Boolean(v) })]),
      },
      ATTRIBUTES
    );
  };
  ruleFlag("Exclude From Board", "exclude_from_board");
  ruleFlag("Exclude From Simulation", "exclude_from_sim");
  ruleFlag("Exclude From Bill of Materials", "exclude_from_bom");
  ruleFlag("Do not Populate", "dnp");

  // The image (`SCH_BITMAP_DESC`): its position, scale and size. The scale and the size move the image about its centre (no transform origin offset here).
  pm.inheritsAfter("SCH_BITMAP", "SCH_ITEM");
  const image = (i: SchItem): Extract<SchGraphicShape, { type: "image" }> => i.graphic!.shape as Extract<SchGraphicShape, { type: "image" }>;
  const imagePos = (axis: 0 | 1) => (i: SchItem, v: PropValue, c: SchCtx): Cmd[] => {
    const delta = Math.round(num(v)) - (axis === 0 ? image(i).at.x : image(i).at.y);
    return delta === 0 || c.locked.has(i.id) ? [] : [{ op: "sch_move", verb: "move", ids: [i.id], dx: axis === 0 ? delta : 0, dy: axis === 1 ? delta : 0 }];
  };
  add({ owner: "SCH_BITMAP", name: "Position X", kind: "int", display: "coord", get: (i) => image(i).at.x, writeable: unlocked, set: imagePos(0) });
  add({ owner: "SCH_BITMAP", name: "Position Y", kind: "int", display: "coord", get: (i) => image(i).at.y, writeable: unlocked, set: imagePos(1) });
  const IMAGE_PROPS = "Image Properties";
  /** `edit_graphic` of the image with another scale: no `data` (the pixels stay), and none of the state's `px`, `ppi`, `hash`, which the command does not know. */
  const rescale = (i: SchItem, scalePpm: number): Cmd[] =>
    scalePpm <= 0 || scalePpm === (image(i).scale_ppm ?? SCALE_PPM) ? [] : [{ op: "sch_edit", verb: "edit_graphic", id: i.id, graphic: { shape: { type: "image", at: image(i).at, scale_ppm: scalePpm } } }];
  add(
    {
      owner: "SCH_BITMAP",
      name: "Scale",
      kind: "double",
      get: (i) => (image(i).scale_ppm ?? SCALE_PPM) / SCALE_PPM,
      validate: (v) => (num(v) <= 0 ? "Scale must be a positive number." : null),
      set: (i, v) => rescale(i, Math.round(num(v) * SCALE_PPM)),
    },
    IMAGE_PROPS
  );
  const imageSide = (name: "Width" | "Height"): void => {
    add(
      {
        owner: "SCH_BITMAP",
        name,
        kind: "int",
        display: "size",
        get: (i) => imageSizeNm(image(i).px ?? [0, 0], image(i).ppi ?? DEFAULT_PPI, image(i).scale_ppm ?? SCALE_PPM)[name === "Width" ? 0 : 1],
        validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1_000, "size", c.units) : null),
        set: (i, v) => {
          const want = Math.round(num(v));
          const px = image(i).px ?? [0, 0];
          const ppi = image(i).ppi ?? DEFAULT_PPI;
          return want <= 0 ? [] : rescale(i, name === "Width" ? scaleForWidthNm(px, ppi, want) : scaleForHeightNm(px, ppi, want));
        },
      },
      IMAGE_PROPS
    );
  };
  // "Transform Offset X/Y" (`SetTransformOriginOffset*`): the point the image is scaled and turned about, relative to its centre -- not kept by the model.
  for (const axis of ["X", "Y"]) {
    add({ owner: "SCH_BITMAP", name: `Transform Offset ${axis}`, kind: "int", display: "size", get: () => 0, readOnlyReason: "An image is scaled about its centre here: the model keeps no transform origin offset." }, IMAGE_PROPS);
  }
  imageSide("Width");
  imageSide("Height");

  // A no-connect flag has no registration in KiCad: nothing to show.
  pm.registerType("SCH_NO_CONNECT");

  // The sheet's tables and their cells (`SCH_TABLE_DESC`, `SCH_TABLECELL_DESC`).
  registerSchTableProperties(pm, { add });

  pm.rebuild();
  return pm;
}

/** The registry of the schematic editor's properties. */
export const SCH_PROPERTIES: PropertyManager<SchItem, SchCtx, Cmd> = build();

// ----------------------------------------------------------------------------------------------------------------------- field rows

/** `SCH_PROPERTIES_PANEL::m_currentSymbolFieldNames` / `m_currentSheetFieldNames`: the fields of the symbols and sheets selected now, by name. */
const currentSymbolFields = new Set<string>();
const currentSheetFields = new Set<string>();

/** A symbol's fields by name (`GetFields()`, none private here): the state's, else the four a symbol always has. A power symbol's are its Reference and Value. */
function symbolFieldNames(i: SchItem): string[] {
  if (i.symbol) return i.symbol.fields?.map((f) => f.name) ?? ["Reference", "Value", "Footprint", "Datasheet"];
  return i.power?.fields?.map((f) => f.name) ?? ["Reference", "Value"];
}

const sheetFieldNames = (i: SchItem): string[] => i.sheet!.fields?.map((f) => f.name) ?? ["Sheetname", "Sheetfile"];

/** The text of the field `name` of a symbol (`SCH_SYMBOL_FIELD_PROPERTY::getter`: "" when it has none). */
function symbolFieldText(i: SchItem, name: string): string {
  const f = (i.symbol?.fields ?? i.power?.fields)?.find((x) => x.name === name);
  if (f) return f.text;
  if (i.symbol && name === "Footprint") return i.symbol.footprint ?? "";
  if (i.symbol && name === "Datasheet") return i.symbol.datasheet ?? "";
  return "";
}

/** `SCH_SYMBOL_FIELD_PROPERTY`: a field of the selected symbols as a row of "Fields"; setting it on a symbol that lacks the field adds it (`AddField`). */
function symbolFieldProperty(name: string): PD {
  return {
    owner: "SCH_SYMBOL",
    name,
    kind: "string",
    get: (i) => symbolFieldText(i, name),
    available: () => currentSymbolFields.has(name),
    writeable: (i) => !!i.symbol,
    readOnlyReason: "A power symbol's fields are its Reference and Value.",
    set: (i, v, c) => {
      const s = i.symbol!;
      const text = String(v);
      if (symbolFieldText(i, name) === text) return [];
      // a field's text in the current variant (`SCH_SYMBOL_FIELD_PROPERTY::setter` -> `SetFieldText( .., GetCurrentVariant() )`); the Reference is no variant's
      const inVariant = name === "Reference" ? null : variantEdit(c, s.id, { field: [name, text] });
      if (inVariant) return inVariant;
      const f = s.fields?.find((x) => x.name === name);
      if (f?.id) return [{ op: "sch_edit", verb: "edit_field", id: f.id, text }];
      if (name === "Footprint" || name === "Datasheet") return [{ op: "edit_symbol_fields", id: s.id, [name.toLowerCase()]: text } as Cmd];
      return [{ op: "sch_edit", verb: "add_user_field", id: s.id, name, text }];
    },
  };
}

/** `SCH_SHEET_FIELD_PROPERTY`: a field of the selected sheets as a row: the Sheetname, the Sheetfile (the field's own text, not the root-relative key: B3-M1) and the user's. */
function sheetFieldProperty(name: string): PD {
  const text = (i: SchItem): string => i.sheet!.fields?.find((f) => f.name === name)?.text ?? (name === "Sheetname" ? i.sheet!.name : name === "Sheetfile" ? i.sheet!.file : "");
  return {
    owner: "SCH_SHEET",
    name,
    kind: "string",
    get: text,
    available: () => currentSheetFields.has(name),
    // A sheet whose file is in a folder of its own is not renamed here: `edit_sheet` takes a bare file name.
    writeable: (i) => name === "Sheetname" || (name === "Sheetfile" && !i.sheet!.file.includes("/")),
    readOnlyReason: name === "Sheetfile" ? "The file of a sheet in a folder of its own is not renamed here: the verb takes a bare file name." : "A sheet's own fields are kept as the file had them, not edited here.",
    validate: (v) => (String(v).trim() !== "" ? null : name === "Sheetfile" ? "A sheet must have a valid file name." : "A sheet needs a name."),
    set: (i, v) => {
      const next = String(v).trim();
      if (next === text(i) || (name === "Sheetfile" && next === text(i).replace(/\.kicad_sch$/, ""))) return [];
      return [{ op: "sch_edit", verb: "edit_sheet", id: i.id, ...(name === "Sheetname" ? { name: next } : { file: next }) }];
    },
  };
}

/**
 * `SCH_PROPERTIES_PANEL::rebuildProperties`: the field names of the selected symbols and sheets become the current ones, and a name no class registers yet
 * gets a field row ("Fields"), which stays registered (shown while a selection has the field), as KiCad's do.
 */
export function syncFieldRows(items: readonly SchItem[]): void {
  currentSymbolFields.clear();
  currentSheetFields.clear();
  for (const i of items) {
    if (i.type === "SCH_SYMBOL") for (const n of symbolFieldNames(i)) currentSymbolFields.add(n);
    else if (i.type === "SCH_SHEET") for (const n of sheetFieldNames(i)) currentSheetFields.add(n);
  }
  // `std::set<wxString>`: in code-unit order
  for (const name of [...currentSymbolFields].sort()) if (!SCH_PROPERTIES.getProperty("SCH_SYMBOL", name)) SCH_PROPERTIES.addProperty(symbolFieldProperty(name), "Fields");
  for (const name of [...currentSheetFields].sort()) if (!SCH_PROPERTIES.getProperty("SCH_SHEET", name)) SCH_PROPERTIES.addProperty(sheetFieldProperty(name), "Fields");
}

// ----------------------------------------------------------------------------------------------------------------------- the panel

/** The grid the Properties panel shows for a selection of schematic items. */
export function schGrid(sch: Schematic, ids: Iterable<string>, units: LengthUnit, variant = ""): GridModel {
  const items = schItemsOf(sch, ids);
  syncFieldRows(items);
  return buildGrid(SCH_PROPERTIES, items, schContext(sch, units, items.length, variant), schFriendlyName);
}

/** The commands an edit of one row sets on the selection, or why it is refused. `variant`: the current design variant, whose values the symbol rows set. */
export function schEdit(sch: Schematic, ids: Iterable<string>, name: string, value: PropValue, units: LengthUnit, variant = ""): EditPlan<Cmd> {
  const items = schItemsOf(sch, ids);
  syncFieldRows(items);
  return planEdit(SCH_PROPERTIES, items, name, value, schContext(sch, units, items.length, variant));
}

/** `extractValueAndWritability` for one named property of a selection. */
export function schRow(sch: Schematic, ids: Iterable<string>, name: string, units: LengthUnit): ReturnType<typeof extractValueAndWritability> {
  const items = schItemsOf(sch, ids);
  syncFieldRows(items);
  return extractValueAndWritability(SCH_PROPERTIES, items, name, schContext(sch, units, items.length));
}
