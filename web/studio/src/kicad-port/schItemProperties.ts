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
import type { Cmd, LabelShape, PointXY, Schematic, SchematicLabel, SchematicSymbol, SchematicText, SchematicWire, PowerSymbol, SchJunction, SchLine, BusEntry, Sheet } from "../api/types";
import type { SchColor, SchGraphic, SchGraphicShape, SchLineStyle } from "../api/schEditTypes";
import type { LengthUnit } from "../state/units";
import { buildGrid, extractValueAndWritability, planEdit, tooSmall, type EditPlan, type GridModel } from "./propertyGrid";
import { PropertyManager, type Choice, type PropValue, type PropertyDef } from "./propertyManager";
import { colorToHex, hexToColor, isUnspecified, strokeEditCmd, UNSPECIFIED_COLOR } from "./schProperties";
import { circumcircle } from "./trackArc";

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
  | "SCH_SHEET";

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
}

/** What the getters read besides the item. */
export interface SchCtx {
  sch: Schematic;
  units: LengthUnit;
  locked: ReadonlySet<string>;
}

export function schContext(sch: Schematic, units: LengthUnit): SchCtx {
  return { sch, units, locked: new Set(sch.locked ?? []) };
}

/** The item a selection id names, or null. Same order as `propertiesTarget`. */
export function schItemOf(sch: Schematic, id: string): SchItem | null {
  const symbol = sch.symbols.find((s) => s.id === id);
  if (symbol) return { type: "SCH_SYMBOL", id, symbol };
  const power = sch.power_symbols.find((p) => p.id === id);
  if (power) return { type: "SCH_SYMBOL", id, power };
  const wire = sch.wires.find((w) => w.id === id);
  if (wire) return { type: "SCH_LINE", id, wire, lineKind: wire.bus ? "bus" : "wire" };
  const label = sch.labels.find((l) => l.id === id);
  if (label) return { type: label.scope === "global" ? "SCH_GLOBALLABEL" : label.scope === "hierarchical" ? "SCH_HIERLABEL" : "SCH_LABEL", id, label };
  const text = sch.texts.find((t) => t.id === id);
  if (text) return { type: "SCH_TEXT", id, text };
  if (sch.no_connects.some((n) => n.id === id)) return { type: "SCH_NO_CONNECT", id };
  const busEntry = sch.bus_entries.find((b) => b.id === id);
  if (busEntry) return { type: "SCH_BUS_WIRE_ENTRY", id, busEntry };
  const junction = (sch.junctions ?? []).find((j) => j.id === id);
  if (junction) return { type: "SCH_JUNCTION", id, junction };
  const line = (sch.lines ?? []).find((l) => l.id === id);
  if (line) return { type: "SCH_LINE", id, line, lineKind: "line" };
  const sheet = sch.sheets.find((s) => s.id === id);
  if (sheet) return { type: "SCH_SHEET", id, sheet };
  const graphic = (sch.graphics ?? []).find((g) => g.id === id);
  if (graphic) {
    const t = graphic.shape.type;
    return { type: t === "text_box" ? "SCH_TEXTBOX" : t === "rule_area" ? "SCH_RULE_AREA" : t === "directive" ? "SCH_DIRECTIVE_LABEL" : "SCH_SHAPE", id, graphic };
  }
  return null;
}

/** The items of a selection, in selection order; an id the sheet does not have is skipped. */
export function schItemsOf(sch: Schematic, ids: Iterable<string>): SchItem[] {
  const out: SchItem[] = [];
  for (const id of ids) {
    const item = schItemOf(sch, id);
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
    case "SCH_SHEET":
      return "Sheet";
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

/** The stroke of a wire, a bus, a bus entry or a graphic line, with the width a graphic line keeps in `width_um`. */
function strokeOf(i: SchItem): { width: number; style: SchLineStyle; color: string } {
  if (i.line) return { width: i.line.width_um, style: i.line.stroke?.style ?? "default", color: colorValue(i.line.stroke?.color) };
  const st = i.wire?.stroke ?? i.busEntry?.stroke;
  return { width: st?.width_um ?? 0, style: st?.style ?? "default", color: colorValue(st?.color) };
}

const strokeCmds = (i: SchItem, patch: { widthUm?: number; style?: SchLineStyle; color?: SchColor; diameterUm?: number }): Cmd[] => {
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
      if (s.type === "circle") next = { ...s, radius_um: v };
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
      if (s.type === "rectangle") next = { ...s, corner_radius_um: v };
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
      return which === "start" ? s.center : { x: s.center.x + s.radius_um, y: s.center.y };
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
  pm.inheritsAfter("SCH_SYMBOL", "SCH_ITEM");
  const isReal = (i: SchItem): boolean => !!i.symbol;
  /** `FilterSelectionForLockedItems`: `sch_move` leaves a locked item where it is and refuses to move nothing. */
  const unlocked = (i: SchItem, c: SchCtx): boolean => !c.locked.has(i.id);
  const move = (axis: 0 | 1) => (i: SchItem, v: PropValue, c: SchCtx): Cmd[] => {
    const at = posOf(i);
    const delta = Math.round(num(v)) - at[axis];
    return delta === 0 ? [] : [{ op: "sch_move", verb: "move", ids: [symbolTarget(i, c)], dx: axis === 0 ? delta : 0, dy: axis === 1 ? delta : 0 }];
  };
  add({ owner: "SCH_SYMBOL", name: "Position X", kind: "int", display: "coord", get: (i) => posOf(i)[0], writeable: unlocked, set: move(0) });
  add({ owner: "SCH_SYMBOL", name: "Position Y", kind: "int", display: "coord", get: (i) => posOf(i)[1], writeable: unlocked, set: move(1) });
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
  add({
    owner: "SCH_SYMBOL",
    name: "Mirror X",
    kind: "bool",
    get: (i) => i.symbol?.mirror === "x",
    available: isReal,
    set: (i, v) => (i.symbol!.mirror === "x" === Boolean(v) ? [] : [{ op: "mirror_symbol_vertical", id: i.symbol!.id, unit: i.symbol!.unit }]),
  });
  add({
    owner: "SCH_SYMBOL",
    name: "Mirror Y",
    kind: "bool",
    get: (i) => i.symbol?.mirror === "y",
    available: isReal,
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
      writeable: isReal,
      validate: (v) => (String(v).trim() === "" ? "A reference cannot be empty." : null),
      set: (i, v) => (!i.symbol || i.symbol.id === String(v).trim() ? [] : [{ op: "rename_symbol", id: i.symbol.id, new_id: String(v).trim() }]),
    },
    FIELDS
  );
  const field = (name: string, key: "value" | "footprint" | "datasheet", power?: (p: PowerSymbol) => string): void => {
    add(
      {
        owner: "SCH_SYMBOL",
        name,
        kind: "string",
        get: (i) => (i.symbol ? (i.symbol[key] ?? "") : i.power && power ? power(i.power) : ""),
        available: power ? undefined : isReal,
        writeable: isReal,
        set: (i, v) => (!i.symbol || (i.symbol[key] ?? "") === String(v) ? [] : [{ op: "edit_symbol_fields", id: i.symbol.id, [key]: String(v) } as Cmd]),
      },
      FIELDS
    );
  };
  field("Value", "value", (p) => p.net);
  add({ owner: "SCH_SYMBOL", name: "Library Link", kind: "string", get: (i) => i.symbol?.lib_id ?? i.power?.lib_id ?? "" }, FIELDS);
  // The fields a symbol has as rows are added by name by `SCH_PROPERTIES_PANEL::rebuildProperties`, sorted: Datasheet, Footprint, MPN here.
  field("Datasheet", "datasheet");
  field("Footprint", "footprint");
  add({ owner: "SCH_SYMBOL", name: "MPN", kind: "string", get: (i) => i.symbol?.mpn ?? "", available: (i) => !!i.symbol?.mpn }, FIELDS);

  const attribute = (name: string, key: "exclude_from_sim" | "exclude_from_bom" | "exclude_from_board" | "dnp"): void => {
    add(
      {
        owner: "SCH_SYMBOL",
        name,
        kind: "bool",
        get: (i) => !!i.symbol![key],
        available: isReal,
        set: (i, v) => (!!i.symbol![key] === Boolean(v) ? [] : [{ op: "set_symbol_attrs", ids: [i.symbol!.id], [key]: Boolean(v) } as Cmd]),
      },
      ATTRIBUTES
    );
  };
  attribute("Exclude From Simulation", "exclude_from_sim");
  attribute("Exclude From Bill of Materials", "exclude_from_bom");
  attribute("Exclude From Board", "exclude_from_board");
  attribute("Do not Populate", "dnp");

  // ---------------------------------------------------------------------------------------------------------- EDA_TEXT, SCH_TEXT
  pm.registerType("EDA_TEXT");
  const isTextBox = (i: SchItem): boolean => i.graphic?.shape.type === "text_box";
  const textBox = (i: SchItem): Extract<SchGraphicShape, { type: "text_box" }> => i.graphic!.shape as Extract<SchGraphicShape, { type: "text_box" }>;
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
  const boxFlag = (name: string, key: "italic" | "bold"): void => {
    add(
      {
        owner: "EDA_TEXT",
        name,
        kind: "bool",
        get: (i) => !!textBox(i)[key],
        available: isTextBox,
        set: (i, v) => (!!textBox(i)[key] === Boolean(v) ? [] : [withShape(i.graphic!, { ...textBox(i), [key]: Boolean(v) })]),
      },
      TEXT_PROPS
    );
  };
  boxFlag("Italic", "italic");
  boxFlag("Bold", "bold");
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
      get: (i) => textBox(i).h_align ?? "left",
      available: isTextBox,
      set: (i, v) => ((textBox(i).h_align ?? "left") === v ? [] : [withShape(i.graphic!, { ...textBox(i), h_align: String(v) as "left" | "center" | "right" })]),
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
      get: (i) => textBox(i).v_align ?? "top",
      available: isTextBox,
      set: (i, v) => ((textBox(i).v_align ?? "top") === v ? [] : [withShape(i.graphic!, { ...textBox(i), v_align: String(v) as "top" | "center" | "bottom" })]),
    },
    TEXT_PROPS
  );

  pm.inheritsAfter("SCH_TEXT", "SCH_ITEM");
  pm.inheritsAfter("SCH_TEXT", "EDA_TEXT");
  // "Orientation is exposed differently in schematic; mask the base for now" -- and the pen, the mirror and the width and height are not a schematic text's.
  const textSize = (owner: string): void => {
    add(
      {
        owner,
        name: "Text Size",
        kind: "int",
        display: "size",
        get: (i) => (i.text ? i.text.size_um : isTextBox(i) ? textBox(i).size_um : 0),
        // `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM`: "Don't allow text to disappear" (0.01 to 1000 mm)
        validate: (v, _i, c) => (num(v) < 10 ? tooSmall(10, "size", c.units) : num(v) > 1_000_000 ? "Value must be less than or equal to 1000 mm" : null),
        set: (i, v) => {
          const size = Math.round(num(v));
          if (i.text) return i.text.size_um === size ? [] : [{ op: "sch_edit", verb: "edit_text", id: i.id, size_um: size }];
          return isTextBox(i) && textBox(i).size_um !== size ? [withShape(i.graphic!, { ...textBox(i), size_um: size })] : [];
        },
      },
      TEXT_PROPS
    );
  };
  textSize("SCH_TEXT");

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
  // A label's size is not in this model; a text's is (`Text Size` is `SCH_TEXT`'s, and a label inherits it).
  pm.overrideAvailability("SCH_LABEL", "SCH_TEXT", "Text Size", () => false);
  pm.overrideAvailability("SCH_GLOBALLABEL", "SCH_TEXT", "Text Size", () => false);
  pm.overrideAvailability("SCH_HIERLABEL", "SCH_TEXT", "Text Size", () => false);
  pm.overrideAvailability("SCH_DIRECTIVE_LABEL", "SCH_TEXT", "Text Size", () => false);

  // The directive label (`SCH_DIRECTIVE_LABEL`): a flag with a pole, drawn from an `SchGraphic`.
  pm.inheritsAfter("SCH_DIRECTIVE_LABEL", "SCH_LABEL_BASE");
  pm.mask("SCH_DIRECTIVE_LABEL", "EDA_TEXT", "Text");
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
    get: (i) => directive(i).pin_length_um,
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => (directive(i).pin_length_um === num(v) ? [] : [withShape(i.graphic!, { ...directive(i), pin_length_um: Math.round(num(v)) })]),
  });

  // --------------------------------------------------------------------------------------------------------------- SCH_LINE
  pm.inheritsAfter("SCH_LINE", "SCH_ITEM");
  const isGraphicLine = (i: SchItem): boolean => i.lineKind === "line";
  const isWireOrBus = (i: SchItem): boolean => i.lineKind === "wire" || i.lineKind === "bus";
  const pts = (i: SchItem): Array<[number, number]> => i.wire?.pts ?? i.line?.pts ?? [];
  const lineEnd = (which: "start" | "end", axis: 0 | 1) => (i: SchItem): number => {
    const p = pts(i);
    return (which === "start" ? p[0] : p[p.length - 1])?.[axis] ?? 0;
  };
  // The end points of a wire are dragged, not typed: what they join follows (`G`); these rows show where they are.
  add({ owner: "SCH_LINE", name: "Start X", kind: "int", display: "coord", get: lineEnd("start", 0) });
  add({ owner: "SCH_LINE", name: "Start Y", kind: "int", display: "coord", get: lineEnd("start", 1) });
  add({ owner: "SCH_LINE", name: "End X", kind: "int", display: "coord", get: lineEnd("end", 0) });
  add({ owner: "SCH_LINE", name: "End Y", kind: "int", display: "coord", get: lineEnd("end", 1) });
  add({
    owner: "SCH_LINE",
    name: "Length",
    kind: "double",
    display: "size",
    get: (i) => Math.round(pts(i).slice(1).reduce((n, p, k) => n + Math.hypot(p[0] - pts(i)[k]![0], p[1] - pts(i)[k]![1]), 0)),
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
    set: (i, v) => (strokeOf(i).width === num(v) ? [] : strokeCmds(i, { widthUm: Math.round(num(v)) })),
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
    get: (i) => i.junction!.look?.diameter_um ?? 0,
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => ((i.junction!.look?.diameter_um ?? 0) === num(v) ? [] : strokeCmds(i, { diameterUm: Math.round(num(v)) })),
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
    set: (i, v) => (strokeOf(i).width === num(v) ? [] : strokeCmds(i, { widthUm: Math.round(num(v)) })),
  });
  add({ owner: "SCH_BUS_ENTRY_BASE", name: "Color", kind: "color", get: (i) => strokeOf(i).color, set: (i, v) => (strokeOf(i).color === String(v) ? [] : strokeCmds(i, { color: colorOf(String(v)) })) });

  // ------------------------------------------------------------------------------------------------------------------ SCH_SHEET
  pm.inheritsAfter("SCH_SHEET", "SCH_ITEM");
  add({
    owner: "SCH_SHEET",
    name: "Sheet Name",
    kind: "string",
    get: (i) => i.sheet!.name,
    validate: (v) => (String(v).trim() === "" ? "A sheet needs a name." : null),
    set: (i, v) => (i.sheet!.name === String(v).trim() ? [] : [{ op: "sch_edit", verb: "edit_sheet", id: i.id, name: String(v).trim() }]),
  });
  add(
    {
      owner: "SCH_SHEET",
      name: "Sheetfile",
      kind: "string",
      get: (i) => i.sheet!.file,
      validate: (v) => (String(v).trim() === "" ? "A sheet must have a valid file name." : null),
      set: (i, v) => {
        const file = String(v).trim();
        return file === i.sheet!.file || file === i.sheet!.file.replace(/\.kicad_sch$/, "") ? [] : [{ op: "sch_edit", verb: "edit_sheet", id: i.id, file }];
      },
    },
    FIELDS
  );

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
        ({ rectangle: "Rectangle", circle: "Circle", arc: "Arc", bezier: "Bezier", polygon: "Polygon", text_box: "Rectangle", rule_area: "Polygon", directive: "Polygon" })[shapeOf(i).type],
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
        validate: display === "size" && name !== "Corner Radius" ? (v, _i, c) => (num(v) < 1 ? tooSmall(1, "size", c.units) : null) : (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
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
  geometry("Radius", "size", (s) => (s.type === "circle" ? s.radius_um : 0), typeIs("circle"));
  geometry("End X", "coord", (s) => pointOf(s, "end").x, hasCorners);
  geometry("End Y", "coord", (s) => pointOf(s, "end").y, hasCorners);
  geometry("Width", "size", (s) => (s.type === "rectangle" || s.type === "text_box" ? Math.abs(s.end.x - s.start.x) : 0), typeIs("rectangle", "text_box"));
  geometry("Height", "size", (s) => (s.type === "rectangle" || s.type === "text_box" ? Math.abs(s.end.y - s.start.y) : 0), typeIs("rectangle", "text_box"));
  geometry("Corner Radius", "size", (s) => (s.type === "rectangle" ? (s.corner_radius_um ?? 0) : 0), typeIs("rectangle"));
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Width",
      kind: "int",
      display: "size",
      get: (i) => i.graphic!.width_um ?? 0,
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => ((i.graphic!.width_um ?? 0) === num(v) ? [] : [graphicCmd(i.graphic!, { width_um: Math.round(num(v)) })]),
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
  const fillAvailable = typeIs("polygon", "rectangle", "circle", "bezier");
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

  // The text box: a rectangle that holds text. A text box is not filled in this editor (`fillAvailable` says so for `SCH_TEXTBOX`), and a rectangle's shape is not a choice.
  pm.inheritsAfter("SCH_TEXTBOX", "SCH_SHAPE");
  pm.inheritsAfter("SCH_TEXTBOX", "EDA_SHAPE");
  pm.inheritsAfter("SCH_TEXTBOX", "EDA_TEXT");
  pm.mask("SCH_TEXTBOX", "EDA_SHAPE", "Shape");
  pm.mask("SCH_TEXTBOX", "EDA_SHAPE", "Corner Radius");
  pm.overrideAvailability("SCH_TEXTBOX", "EDA_SHAPE", "Fill", () => false);
  pm.overrideAvailability("SCH_TEXTBOX", "EDA_SHAPE", "Fill Color", () => false);
  textSize("SCH_TEXTBOX");

  // The rule area: a polygon with exclusion flags (`SCH_RULE_AREA`).
  pm.inheritsAfter("SCH_RULE_AREA", "SCH_SHAPE");
  pm.inheritsAfter("SCH_RULE_AREA", "SCH_ITEM");
  pm.inheritsAfter("SCH_RULE_AREA", "EDA_SHAPE");
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

  // A no-connect flag has no registration in KiCad: nothing to show.
  pm.registerType("SCH_NO_CONNECT");

  pm.rebuild();
  return pm;
}

/** The registry of the schematic editor's properties. */
export const SCH_PROPERTIES: PropertyManager<SchItem, SchCtx, Cmd> = build();

// ----------------------------------------------------------------------------------------------------------------------- the panel

/** The grid the Properties panel shows for a selection of schematic items. */
export function schGrid(sch: Schematic, ids: Iterable<string>, units: LengthUnit): GridModel {
  return buildGrid(SCH_PROPERTIES, schItemsOf(sch, ids), schContext(sch, units), schFriendlyName);
}

/** The commands an edit of one row sets on the selection, or why it is refused. */
export function schEdit(sch: Schematic, ids: Iterable<string>, name: string, value: PropValue, units: LengthUnit): EditPlan<Cmd> {
  return planEdit(SCH_PROPERTIES, schItemsOf(sch, ids), name, value, schContext(sch, units));
}

/** `extractValueAndWritability` for one named property of a selection. */
export function schRow(sch: Schematic, ids: Iterable<string>, name: string, units: LengthUnit): ReturnType<typeof extractValueAndWritability> {
  return extractValueAndWritability(SCH_PROPERTIES, schItemsOf(sch, ids), name, schContext(sch, units));
}
