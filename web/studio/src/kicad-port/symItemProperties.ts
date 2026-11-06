// The Symbol Editor's Properties panel (UI-LEFT Y3): `SCH_PROPERTIES_PANEL` in the symbol editor frame (eeschema/widgets/sch_properties_panel.cpp at KiCad 8303b2ad),
// with the rows KiCad's `PROPERTY_MANAGER` registrations give a library symbol's items -- `SCH_ITEM_DESC` (sch_item.cpp: Unit, Body Style and Private are hidden from the
// design editors, shown here; Locked is hidden from the library editors), `SCH_PIN_DESC` (sch_pin.cpp), `EDA_TEXT_DESC` (common/eda_text.cpp), `SCH_TEXT_DESC`, `EDA_SHAPE_DESC`
// (common/eda_shape.cpp), `SCH_SHAPE_DESC` (its Fill Mode is a symbol item's), `SCH_TEXTBOX_DESC`, `SCH_FIELD_DESC` (sch_field.cpp) and `LIB_SYMBOL_DESC` (lib_symbol.cpp, with
// SYMBOL's rows; a placed symbol's "Pin numbers" and "Pin names" are not available on a library symbol). With nothing selected the panel shows the symbol itself
// (`SCH_PROPERTIES_PANEL::getSelection`: the frame's `GetCurSymbol()` as the fallback selection).
//
// The registrations keep KiCad's calls, in KiCad's order, so `propertyManager.ts` walks the classes the way `PROPERTY_MANAGER` does and `propertyGrid.ts` builds the grid of the
// library editors (`isLibraryEditor`). The rows are the schematic panel's (`schItemProperties.ts`) for the same classes; the getters read the library symbol and the setters
// return the Symbol Editor's verbs -- `edit_symbol_pin`, `replace_symbol_graphic`, the fields as one `edit_library_symbol_fields` and `edit_symbol_properties` -- which an edit
// sends as ONE `batch` on the editor's own undo stack (`SCH_COMMIT::Push( "Edit Properties" )`). What a library symbol here has no place for (a font, a colour, a hyperlink,
// a shape's corner radius, line style or private flag, a named body style) is read-only, with the reason as the row's tooltip.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). Lengths are nm in the grid (the panel's unit), mm in the symbol; Y is KiCad's internal Y, the file's negated
// (eeschema's `ORIGIN_TRANSFORMS` is the identity: Pin Properties shows the same, PARITY-symedit E7-S3).
import type { Cmd, LibraryFill, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, MmPoint, SymbolPropertiesFields } from "../api/types";
import type { LengthUnit } from "../state/units";
import { buildGrid, planEdit, positiveInt, tooSmall, type EditPlan, type GridModel } from "./propertyGrid";
import { PropertyManager, type Choice, type PropValue, type PropertyDef } from "./propertyManager";
import { PIN_SHAPE_OPTIONS, PIN_TYPE_OPTIONS } from "./symPinAlternates";
import { changePinLength } from "./symPinSync";
import { allFields, isMandatoryField, wireField, type FieldRow } from "./symFields";
import { libFieldId, libFieldName, unitLetters } from "./symFieldLayout";
import { arcCenter } from "./symTransform";
import { textBoxMarginMm } from "./symTextBox";
import { bodyStyleCount, bodyStyleDescription, isMultiBodyStyle } from "./symBodyStyles";

// ------------------------------------------------------------------------------------------------------------------------------- items

/** The class of a library item (`TYPE_HASH( *item )`). */
export type SymType = "LIB_SYMBOL" | "SCH_PIN" | "SCH_SHAPE" | "SCH_TEXT" | "SCH_TEXTBOX" | "SCH_FIELD";

/** One item of the panel's selection: the class and what it stands for (only the one of its kind is set). The symbol itself has the id "". */
export interface SymItem {
  readonly type: SymType;
  readonly id: string;
  readonly pin?: LibrarySymbolPin;
  readonly graphic?: LibrarySymbolGraphic;
  readonly field?: FieldRow;
}

/** What the getters read besides the item. */
export interface SymCtx {
  /** The symbol the editor draws (a derived symbol's flat one). */
  sym: LibrarySymbol;
  /** The entry itself (a derived symbol's own; else the same as `sym`): its fields and properties are what an edit changes. */
  own: LibrarySymbol;
  libId: string;
  units: LengthUnit;
}

/** A field changed by a row: the edit gathers them into one `edit_library_symbol_fields` (the verb takes the whole list). */
export type FieldChange = { op: "__field"; field: FieldRow };
export type SymCmd = Cmd | FieldChange;

/** The item a selection id names, or null. */
export function symItemOf(sym: LibrarySymbol, id: string): SymItem | null {
  const pin = sym.pins.find((p) => p.id === id);
  if (pin) return { type: "SCH_PIN", id, pin };
  const graphic = sym.graphics.find((g) => g.id === id);
  if (graphic) return { type: graphic.kind === "text" ? "SCH_TEXT" : graphic.kind === "text_box" ? "SCH_TEXTBOX" : "SCH_SHAPE", id, graphic };
  const name = libFieldName(id);
  if (name) {
    const field = allFields(sym).find((f) => f.name === name);
    if (field) return { type: "SCH_FIELD", id, field };
  }
  return null;
}

/** `getSelection`: the selected items, in selection order -- or, with nothing selected (or nothing the symbol has), the symbol itself. */
export function symItemsOf(sym: LibrarySymbol, ids: Iterable<string>): SymItem[] {
  const out: SymItem[] = [];
  for (const id of ids) {
    const item = symItemOf(sym, id);
    if (item) out.push(item);
  }
  return out.length > 0 ? out : [{ type: "LIB_SYMBOL", id: "" }];
}

/** `GetFriendlyName()`: "Pin", the shape's name (`EDA_SHAPE::getFriendlyName`), "Text", "Text Box", "Field", "Symbol". */
export function symFriendlyName(item: SymItem): string {
  switch (item.type) {
    case "SCH_PIN":
      return "Pin";
    case "SCH_TEXT":
      return "Text";
    case "SCH_TEXTBOX":
      return "Text Box";
    case "SCH_FIELD":
      return "Field";
    case "LIB_SYMBOL":
      return "Symbol";
    case "SCH_SHAPE":
      switch (item.graphic?.kind) {
        case "rectangle":
          return "Rectangle";
        case "circle":
          return "Circle";
        case "arc":
          return "Arc";
        case "bezier":
          return "Curve";
        default:
          return "Polygon";
      }
  }
}

// ------------------------------------------------------------------------------------------------------------------------------- helpers

type PD = PropertyDef<SymItem, SymCtx, SymCmd>;

const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const TEXT_PROPS = "Text Properties";
const SHAPE_PROPS = "Shape Properties";
/** nm in the grid to the 0.1 um the file keeps, in mm. */
const mmOf = (nm: number): number => Math.round(nm / 100) / 1e4;
const nmOf = (mm: number): number => Math.round(mm * 1e6);

const pinCmd = (c: SymCtx, p: LibrarySymbolPin): Cmd => ({ op: "edit_symbol_pin", lib_id: c.libId, id: p.id!, pin: p });
const graphicCmd = (c: SymCtx, g: LibrarySymbolGraphic): Cmd => ({ op: "replace_symbol_graphic", lib_id: c.libId, id: g.id!, graphic: g });
/** The pin with `patch`, or nothing when it already is. */
const pinWith = (i: SymItem, c: SymCtx, patch: Partial<LibrarySymbolPin>): SymCmd[] => {
  const p = i.pin!;
  const next = { ...p, ...patch };
  return JSON.stringify(next) === JSON.stringify(p) ? [] : [pinCmd(c, next)];
};
/** The graphic with `patch` (of its own kind), or nothing when it already is. */
const graphicWith = (i: SymItem, c: SymCtx, patch: Record<string, unknown>): SymCmd[] => {
  const g = i.graphic!;
  const next = { ...g, ...patch } as LibrarySymbolGraphic;
  return JSON.stringify(next) === JSON.stringify(g) ? [] : [graphicCmd(c, next)];
};
const fieldWith = (i: SymItem, patch: Partial<FieldRow>): SymCmd[] => {
  const f = i.field!;
  const next = { ...f, ...patch };
  return JSON.stringify(next) === JSON.stringify(f) ? [] : [{ op: "__field", field: next }];
};

/** The symbol's properties as `edit_symbol_properties` takes them (all of them), from the entry, with `patch`. */
function propertiesOf(s: LibrarySymbol): SymbolPropertiesFields {
  return {
    reference_prefix: s.reference_prefix ?? "",
    description: s.description ?? "",
    keywords: s.keywords ?? "",
    datasheet: s.datasheet ?? "",
    power: !!s.power,
    in_bom: s.in_bom ?? true,
    on_board: s.on_board ?? true,
    pin_numbers_hidden: !!s.pin_numbers_hidden,
    pin_names_hidden: !!s.pin_names_hidden,
    pin_name_offset_mm: s.pin_name_offset_mm ?? 0.508,
    unit_count: s.unit_count ?? 1,
    has_alternate_body_style: !!s.has_alternate_body_style,
    footprint_filters: s.footprint_filters ?? [],
    units_locked: !!s.units_locked,
    power_local: !!s.power_local,
    exclude_from_sim: !!s.exclude_from_sim,
    exclude_from_pos_files: !!s.exclude_from_pos_files,
  };
}
const symbolWith = (c: SymCtx, patch: Partial<SymbolPropertiesFields>): SymCmd[] => {
  const before = propertiesOf(c.own);
  const next = { ...before, ...patch };
  return JSON.stringify(next) === JSON.stringify(before) ? [] : [{ op: "edit_symbol_properties", lib_id: c.libId, ...next } as Cmd];
};

/** The unit's name as `GetUnitDisplayName( unit, false )` gives it: the symbol's own name for it, else its letter. */
function unitName(s: LibrarySymbol, unit: number): string {
  const names = (s as { kicad?: { unit_names?: Record<string, string> } }).kicad?.unit_names;
  return names?.[String(unit)] ?? unitLetters(unit);
}

/** `EDA_SHAPE::GetShape`'s `ENUM_MAP<SHAPE_T>`: the shape kinds and their names. */
const SHAPE_CHOICES: Choice[] = [
  { label: "Segment", value: "segment" },
  { label: "Rectangle", value: "rectangle" },
  { label: "Arc", value: "arc" },
  { label: "Circle", value: "circle" },
  { label: "Polygon", value: "polyline" },
  { label: "Bezier", value: "bezier" },
];
/** `ENUM_MAP<LINE_STYLE>`. */
const LINE_STYLES: Choice[] = [
  { label: "Solid", value: "solid" },
  { label: "Dashed", value: "dash" },
  { label: "Dotted", value: "dot" },
  { label: "Dash-Dot", value: "dash_dot" },
  { label: "Dash-Dot-Dot", value: "dash_dot_dot" },
];
/** `ENUM_MAP<UI_FILL_MODE>`: EDA_SHAPE's "Fill". */
const UI_FILL: Choice[] = [
  { label: "None", value: "none" },
  { label: "Solid", value: "solid" },
  { label: "Hatch", value: "hatch" },
  { label: "Reverse Hatch", value: "reverse_hatch" },
  { label: "Cross-hatch", value: "cross_hatch" },
];
/** `ENUM_MAP<FILL_T>`: SCH_SHAPE's "Fill Mode". */
const FILL_MODES: Choice[] = [
  { label: "None", value: "none" },
  { label: "Body outline color", value: "outline" },
  { label: "Body background color", value: "background" },
  { label: "Fill color", value: "color" },
];
const H_JUSTIFY: Choice[] = [
  { label: "Left", value: "left" },
  { label: "Center", value: "center" },
  { label: "Right", value: "right" },
];
const V_JUSTIFY: Choice[] = [
  { label: "Top", value: "top" },
  { label: "Center", value: "center" },
  { label: "Bottom", value: "bottom" },
];
/** `ENUM_MAP<PIN_ORIENTATION>`: Right, Left, Up, Down -- the file's angles 0, 180, 90, 270. */
const ORIENTATIONS: Choice[] = [
  { label: "Right", value: 0 },
  { label: "Left", value: 180 },
  { label: "Up", value: 90 },
  { label: "Down", value: 270 },
];

/** The shapes of `EDA_SHAPE`'s start / end rows: their start and end (a text box's corners, a bezier's ends). */
const startOf = (g: LibrarySymbolGraphic): MmPoint | null => ("start" in g ? g.start : null);
const endOf = (g: LibrarySymbolGraphic): MmPoint | null => ("end" in g ? g.end : null);

/** `EDA_SHAPE::GetArcAngle`: the arc's sweep in degrees, counter-clockwise on screen from start to end through mid. */
function arcSweepDeg(g: Extract<LibrarySymbolGraphic, { kind: "arc" }>): number {
  const c = arcCenter(g.start, g.mid, g.end);
  const a = (p: MmPoint) => Math.atan2(p.y - c.y, p.x - c.x);
  const norm = (v: number) => ((v % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  const toMid = norm(a(g.mid) - a(g.start));
  const toEnd = norm(a(g.end) - a(g.start));
  return ((toMid <= toEnd ? toEnd : toEnd - 2 * Math.PI) * 180) / Math.PI;
}

const READ_ONLY = {
  font: "A library symbol's field keeps no font face here: KiCad's default font is drawn.",
  pen: "A library symbol's text keeps its file's pen as it was; it is not edited here.",
  color: "A library symbol's text, text box or shape keeps no colour here.",
  link: "A library symbol's text keeps no hyperlink here.",
  privateItem: "A library pin, shape or text keeps no Private flag here.",
  corner: "A library rectangle has no corner radius here.",
  lineStyle: "A shape's line style is kept as the file had it; it is not edited here.",
  arcEnds: "An arc's ends are dragged on the canvas: the property would move one end about a stale centre.",
  fieldUnit: "A field of a library symbol is on every unit and body style.",
  bodyStyles: "Only the standard and the De Morgan body styles are modelled; they are chosen in Symbol Properties.",
};

// ------------------------------------------------------------------------------------------------------------------------------- the registry

function build(): PropertyManager<SymItem, SymCtx, SymCmd> {
  const pm = new PropertyManager<SymItem, SymCtx, SymCmd>();
  const add = (def: PD, group = "") => pm.addProperty(def, group);

  // ----------------------------------------------------------------------------------------------------------------- EDA_ITEM, SCH_ITEM
  pm.registerType("EDA_ITEM");
  add({ owner: "EDA_ITEM", name: "Type", kind: "string", hidden: true, get: (i) => i.type });
  pm.inheritsAfter("SCH_ITEM", "EDA_ITEM");
  add({ owner: "SCH_ITEM", name: "Locked", kind: "bool", hiddenFromLibraryEditors: true, get: () => false });
  const unitOf = (i: SymItem): number => i.pin?.unit ?? i.graphic?.unit ?? 0;
  const styleOf = (i: SymItem): number => i.pin?.body_style ?? i.graphic?.body_style ?? 0;
  add({
    owner: "SCH_ITEM",
    name: "Unit",
    kind: "enum",
    hiddenFromDesignEditors: true,
    available: (_i, c) => (c.sym.unit_count ?? 1) > 1,
    choices: (_i, c) => [{ label: "All units", value: 0 }, ...Array.from({ length: c.sym.unit_count ?? 1 }, (_, k) => ({ label: unitName(c.sym, k + 1), value: k + 1 }))],
    get: unitOf,
    writeable: (i) => i.type !== "SCH_FIELD",
    readOnlyReason: (i) => (i.type === "SCH_FIELD" ? READ_ONLY.fieldUnit : null),
    set: (i, v, c) => (i.pin ? pinWith(i, c, { unit: num(v) }) : i.graphic ? graphicWith(i, c, { unit: num(v) }) : []),
  });
  add({
    owner: "SCH_ITEM",
    name: "Body Style",
    kind: "enum",
    hiddenFromDesignEditors: true,
    available: (_i, c) => isMultiBodyStyle(c.sym),
    // `SCH_ITEM`'s Body Style choices: "All body styles", then each style by `GetBodyStyleDescription`
    choices: (_i, c) => [{ label: "All body styles", value: 0 }, ...Array.from({ length: bodyStyleCount(c.sym) }, (_, k) => ({ label: bodyStyleDescription(c.sym, k + 1), value: k + 1 }))],
    get: styleOf,
    writeable: (i) => i.type !== "SCH_FIELD",
    readOnlyReason: (i) => (i.type === "SCH_FIELD" ? READ_ONLY.fieldUnit : null),
    set: (i, v, c) => (i.pin ? pinWith(i, c, { body_style: num(v) }) : i.graphic ? graphicWith(i, c, { body_style: num(v) }) : []),
  });
  add({
    owner: "SCH_ITEM",
    name: "Private",
    kind: "bool",
    hiddenFromDesignEditors: true,
    get: (i) => !!i.field?.private,
    writeable: (i) => i.type === "SCH_FIELD",
    readOnlyReason: (i) => (i.type === "SCH_FIELD" ? null : READ_ONLY.privateItem),
    set: (i, v) => (i.field ? fieldWith(i, { private: Boolean(v) }) : []),
  });

  // ----------------------------------------------------------------------------------------------------------------- SYMBOL, LIB_SYMBOL
  pm.inheritsAfter("LIB_SYMBOL", "SYMBOL");
  // `SCH_SYMBOL_DESC`'s two rows of SYMBOL are a placed symbol's (`hasLibPart`): not on a library symbol
  add({ owner: "SYMBOL", name: "Pin numbers", kind: "bool", available: () => false, get: () => false });
  add({ owner: "SYMBOL", name: "Pin names", kind: "bool", available: () => false, get: () => false });
  const FIELDS = "Fields";
  const fieldText = (c: SymCtx, name: string): string => allFields(c.own).find((f) => f.name === name)?.text ?? "";
  const symbolField = (name: "Reference" | "Value" | "Footprint" | "Datasheet"): void => {
    add(
      {
        owner: "LIB_SYMBOL",
        name,
        kind: "string",
        get: (_i, c) => fieldText(c, name),
        set: (_i, v, c) => {
          const f = allFields(c.own).find((x) => x.name === name);
          return !f || f.text === String(v) ? [] : [{ op: "__field", field: { ...f, text: String(v) } }];
        },
      },
      FIELDS
    );
  };
  symbolField("Reference");
  symbolField("Value");
  symbolField("Footprint");
  symbolField("Datasheet");
  add({ owner: "LIB_SYMBOL", name: "Keywords", kind: "string", get: (_i, c) => c.own.keywords ?? "", set: (_i, v, c) => symbolWith(c, { keywords: String(v) }) }, FIELDS);
  const DEF = "Symbol Definition";
  add({ owner: "LIB_SYMBOL", name: "Define as Power Symbol", kind: "bool", get: (_i, c) => !!c.own.power, set: (_i, v, c) => symbolWith(c, Boolean(v) ? { power: true, power_local: false } : { power: false, power_local: false }) }, DEF);
  add(
    {
      owner: "LIB_SYMBOL",
      name: "Define as Local Power Symbol",
      kind: "bool",
      get: (_i, c) => !!c.own.power && !!c.own.power_local,
      // `SetLocalPowerSymbolProp`: local; off, a global power symbol if it is one
      set: (_i, v, c) => symbolWith(c, Boolean(v) ? { power: true, power_local: true } : { power_local: false }),
    },
    DEF
  );
  const PIN_DISPLAY = "Pin Display";
  add({ owner: "SYMBOL", name: "Show Pin Number", kind: "bool", get: (_i, c) => !c.sym.pin_numbers_hidden, set: (_i, v, c) => symbolWith(c, { pin_numbers_hidden: !v }) }, PIN_DISPLAY);
  add({ owner: "SYMBOL", name: "Show Pin Name", kind: "bool", get: (_i, c) => !c.sym.pin_names_hidden, set: (_i, v, c) => symbolWith(c, { pin_names_hidden: !v }) }, PIN_DISPLAY);
  add(
    {
      owner: "LIB_SYMBOL",
      name: "Place Pin Names Inside",
      kind: "bool",
      get: (_i, c) => (c.sym.pin_name_offset_mm ?? 0.508) !== 0,
      // `SetPinNamesInsideProp`: inside with no offset is `DEFAULT_PIN_NAME_OFFSET` (20 mil); outside is 0
      set: (_i, v, c) => {
        const offset = c.own.pin_name_offset_mm ?? 0.508;
        if (v && offset === 0) return symbolWith(c, { pin_name_offset_mm: 0.508 });
        if (!v) return symbolWith(c, { pin_name_offset_mm: 0 });
        return [];
      },
    },
    PIN_DISPLAY
  );
  add(
    {
      owner: "SYMBOL",
      name: "Pin Name Position Offset",
      kind: "int",
      display: "size",
      get: (_i, c) => nmOf(c.sym.pin_name_offset_mm ?? 0.508),
      validate: (v, _i, c) => positiveInt(v, "size", c.units),
      set: (_i, v, c) => symbolWith(c, { pin_name_offset_mm: mmOf(num(v)) }),
    },
    PIN_DISPLAY
  );
  const ATTRS = "Attributes";
  add({ owner: "LIB_SYMBOL", name: "Exclude from Simulation", kind: "bool", get: (_i, c) => !!c.own.exclude_from_sim, set: (_i, v, c) => symbolWith(c, { exclude_from_sim: Boolean(v) }) }, ATTRS);
  add({ owner: "LIB_SYMBOL", name: "Exclude from Board", kind: "bool", get: (_i, c) => !(c.own.on_board ?? true), set: (_i, v, c) => symbolWith(c, { on_board: !v }) }, ATTRS);
  add({ owner: "LIB_SYMBOL", name: "Exclude from Bill of Materials", kind: "bool", get: (_i, c) => !(c.own.in_bom ?? true), set: (_i, v, c) => symbolWith(c, { in_bom: !v }) }, ATTRS);
  add({ owner: "LIB_SYMBOL", name: "Exclude from Position Files", kind: "bool", get: (_i, c) => !!c.own.exclude_from_pos_files, set: (_i, v, c) => symbolWith(c, { exclude_from_pos_files: Boolean(v) }) }, ATTRS);
  const UNITS = "Units and Body Styles";
  add(
    {
      owner: "LIB_SYMBOL",
      name: "Number of Symbol Units",
      kind: "int",
      get: (_i, c) => c.sym.unit_count ?? 1,
      validate: (v) => (num(v) < 1 ? "Value must be greater than or equal to 1" : null),
      // `SetUnitCount( aUnits, true )`: the items of the units dropped go, a new unit is a copy of unit 1 (the verb's `set_unit_count`); a derived symbol's are its root's
      writeable: (_i, c) => !c.own.extends,
      readOnlyReason: "A derived symbol's units are its root symbol's.",
      set: (_i, v, c) => symbolWith(c, { unit_count: Math.round(num(v)), units_locked: Math.round(num(v)) > 1 ? !!c.own.units_locked : false }),
    },
    UNITS
  );
  add({ owner: "LIB_SYMBOL", name: "Units are Interchangeable", kind: "bool", get: (_i, c) => !c.own.units_locked, set: (_i, v, c) => symbolWith(c, { units_locked: !v && (c.own.unit_count ?? 1) > 1 }) }, UNITS);
  add(
    {
      owner: "LIB_SYMBOL",
      name: "Body Styles",
      kind: "enum",
      available: (_i, c) => isMultiBodyStyle(c.sym),
      choices: (_i, c) => Array.from({ length: bodyStyleCount(c.sym) }, (_, k) => ({ label: bodyStyleDescription(c.sym, k + 1), value: bodyStyleDescription(c.sym, k + 1) })),
      // `GetBodyStyleProp`: `GetBodyStyleDescription( 1, false )`; its setter does nothing ("primarily for display purposes")
      get: (_i, c) => bodyStyleDescription(c.sym, 1),
      readOnlyReason: READ_ONLY.bodyStyles,
    },
    UNITS
  );

  // ----------------------------------------------------------------------------------------------------------------- SCH_PIN
  pm.inheritsAfter("SCH_PIN", "SCH_ITEM");
  // "Lock state is inherited from parent symbol (no independent locking of child items)"
  pm.mask("SCH_PIN", "SCH_ITEM", "Locked");
  add({ owner: "SCH_PIN", name: "Pin Name", kind: "string", get: (i) => i.pin!.name, set: (i, v, c) => pinWith(i, c, { name: String(v) }) });
  add({ owner: "SCH_PIN", name: "Pin Number", kind: "string", get: (i) => i.pin!.number, set: (i, v, c) => pinWith(i, c, { number: String(v) }) });
  add({
    owner: "SCH_PIN",
    name: "Electrical Type",
    kind: "enum",
    choices: () => PIN_TYPE_OPTIONS.map((o) => ({ label: o.label, value: o.value })),
    get: (i) => i.pin!.electrical_type,
    set: (i, v, c) => pinWith(i, c, { electrical_type: String(v) as LibrarySymbolPin["electrical_type"] }),
  });
  add({
    owner: "SCH_PIN",
    name: "Graphic Style",
    kind: "enum",
    choices: () => PIN_SHAPE_OPTIONS.map((o) => ({ label: o.label, value: o.value })),
    get: (i) => i.pin!.shape,
    set: (i, v, c) => pinWith(i, c, { shape: String(v) as LibrarySymbolPin["shape"] }),
  });
  add({ owner: "SCH_PIN", name: "Position X", kind: "int", display: "coord", get: (i) => nmOf(i.pin!.at.x), set: (i, v, c) => pinWith(i, c, { at: { x: mmOf(num(v)), y: i.pin!.at.y } }) });
  add({ owner: "SCH_PIN", name: "Position Y", kind: "int", display: "coord", get: (i) => nmOf(-i.pin!.at.y) || 0, set: (i, v, c) => pinWith(i, c, { at: { x: i.pin!.at.x, y: mmOf(-num(v)) || 0 } }) });
  add({
    owner: "SCH_PIN",
    name: "Orientation",
    kind: "enum",
    choices: () => ORIENTATIONS,
    get: (i) => ((Math.round(i.pin!.angle_deg) % 360) + 360) % 360,
    set: (i, v, c) => pinWith(i, c, { angle_deg: num(v) }),
  });
  add({
    owner: "SCH_PIN",
    name: "Length",
    kind: "int",
    display: "size",
    get: (i) => nmOf(i.pin!.length_mm),
    validate: (v, _i, c) => positiveInt(v, "size", c.units),
    // `SCH_PIN::ChangeLength`: the body end stays, the connection point moves by the change
    set: (i, v, c) => {
      const next = changePinLength(i.pin!, mmOf(num(v)));
      return JSON.stringify(next) === JSON.stringify(i.pin) ? [] : [pinCmd(c, next)];
    },
  });
  add({ owner: "SCH_PIN", name: "Name Text Size", kind: "int", display: "size", get: (i) => nmOf(i.pin!.name_size_mm ?? 1.27), validate: (v, _i, c) => positiveInt(v, "size", c.units), set: (i, v, c) => pinWith(i, c, { name_size_mm: mmOf(num(v)) }) });
  add({ owner: "SCH_PIN", name: "Number Text Size", kind: "int", display: "size", get: (i) => nmOf(i.pin!.number_size_mm ?? 1.27), validate: (v, _i, c) => positiveInt(v, "size", c.units), set: (i, v, c) => pinWith(i, c, { number_size_mm: mmOf(num(v)) }) });
  add({ owner: "SCH_PIN", name: "Visible", kind: "bool", get: (i) => !i.pin!.hidden, set: (i, v, c) => pinWith(i, c, { hidden: !v }) });

  // ----------------------------------------------------------------------------------------------------------------- EDA_TEXT
  pm.registerType("EDA_TEXT");
  const textOf = (i: SymItem): { text: string; bold: boolean; italic: boolean; h: string; v: string; sizeMm: number } => {
    const g = i.graphic;
    if (g?.kind === "text") return { text: g.text, bold: !!g.bold, italic: !!g.italic, h: g.h_align ?? "center", v: g.v_align ?? "center", sizeMm: g.size_mm };
    if (g?.kind === "text_box") return { text: g.text, bold: !!g.bold, italic: !!g.italic, h: g.h_align, v: g.v_align, sizeMm: g.size_mm };
    const f = i.field!;
    return { text: f.text, bold: f.bold, italic: f.italic, h: f.h_align, v: f.v_align, sizeMm: f.size_mm };
  };
  /** Every text kind's setter: a field through the fields verb, a text or a text box through its graphic. */
  const textWith = (i: SymItem, c: SymCtx, patch: { text?: string; bold?: boolean; italic?: boolean; h_align?: string; v_align?: string; size_mm?: number }): SymCmd[] => (i.field ? fieldWith(i, patch as Partial<FieldRow>) : graphicWith(i, c, patch));
  add({ owner: "EDA_TEXT", name: "Orientation", kind: "double", display: "degree", get: () => 0 });
  add({ owner: "EDA_TEXT", name: "Text", kind: "string", get: (i) => textOf(i).text, set: (i, v, c) => textWith(i, c, { text: String(v) }) }, TEXT_PROPS);
  // "Font" (`EDA_TEXT::GetFontProp` / `SetFontProp`): eeschema's "Default Font" is none, any other name the face (`SetFont( FONT::GetFont( aFontName, IsBold(), IsItalic() ) )`);
  // a text's and a text box's is its `font` (LibTextLooks, the verb of Text Properties' Font, UI-LEFT Y4), a field keeps none here.
  const fontOf = (i: SymItem): string => (i.graphic?.kind === "text" || i.graphic?.kind === "text_box" ? i.graphic.font || "Default Font" : "Default Font");
  add(
    {
      owner: "EDA_TEXT",
      name: "Font",
      kind: "string",
      get: fontOf,
      writeable: (i) => i.graphic?.kind === "text" || i.graphic?.kind === "text_box",
      readOnlyReason: READ_ONLY.font,
      set: (i, v, c) => {
        const next = String(v).trim();
        if (!next || next === fontOf(i)) return [];
        const g = { ...i.graphic! } as LibrarySymbolGraphic & { font?: string | null };
        if (next === "Default Font") delete g.font;
        else g.font = next;
        return [graphicCmd(c, g)];
      },
    },
    TEXT_PROPS
  );
  add({ owner: "EDA_TEXT", name: "Auto Thickness", kind: "bool", get: (i) => !textOf(i).bold, readOnlyReason: READ_ONLY.pen }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Thickness", kind: "int", display: "size", get: () => 0 }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Italic", kind: "bool", get: (i) => textOf(i).italic, set: (i, v, c) => textWith(i, c, { italic: Boolean(v) }) }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Bold", kind: "bool", get: (i) => textOf(i).bold, set: (i, v, c) => textWith(i, c, { bold: Boolean(v) }) }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Mirrored", kind: "bool", get: () => false, readOnlyReason: "A library symbol's text box is not mirrored here." }, TEXT_PROPS);
  // `isField`: only a field has Visible
  add({ owner: "EDA_TEXT", name: "Visible", kind: "bool", available: (i) => i.type === "SCH_FIELD", get: (i) => !i.field?.hidden, set: (i, v) => fieldWith(i, { hidden: !v }) }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Width", kind: "int", display: "size", get: () => 0 }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Height", kind: "int", display: "size", get: () => 0 }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Horizontal Justification", kind: "enum", choices: () => H_JUSTIFY, get: (i) => textOf(i).h, set: (i, v, c) => textWith(i, c, { h_align: String(v) }) }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Vertical Justification", kind: "enum", choices: () => V_JUSTIFY, get: (i) => textOf(i).v, set: (i, v, c) => textWith(i, c, { v_align: String(v) }) }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Color", kind: "color", get: () => "", readOnlyReason: READ_ONLY.color }, TEXT_PROPS);
  add({ owner: "EDA_TEXT", name: "Hyperlink", kind: "string", get: () => "", readOnlyReason: READ_ONLY.link }, TEXT_PROPS);
  /** "Text Size" (`SetSchTextSize`) of `owner`: "Don't allow text to disappear" (0.01 to 1000 mm, `DIALOG_TEXT_PROPERTIES`'s range). */
  const textSize = (owner: string): void => {
    add(
      {
        owner,
        name: "Text Size",
        kind: "int",
        display: "size",
        get: (i) => nmOf(textOf(i).sizeMm),
        validate: (v, _i, c) => (num(v) < 10_000 ? tooSmall(10_000, "size", c.units) : num(v) > 1_000_000_000 ? "Value must be less than or equal to 1000 mm" : null),
        set: (i, v, c) => textWith(i, c, { size_mm: mmOf(num(v)) }),
      },
      TEXT_PROPS
    );
  };

  // ----------------------------------------------------------------------------------------------------------------- SCH_TEXT
  pm.inheritsAfter("SCH_TEXT", "SCH_ITEM");
  pm.inheritsAfter("SCH_TEXT", "EDA_TEXT");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Mirrored");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Width");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Height");
  pm.mask("SCH_TEXT", "EDA_TEXT", "Thickness");
  textSize("SCH_TEXT");
  // "Orientation is exposed differently in schematic; mask the base for now"
  pm.mask("SCH_TEXT", "EDA_TEXT", "Orientation");

  // ----------------------------------------------------------------------------------------------------------------- EDA_SHAPE
  pm.registerType("EDA_SHAPE");
  const g = (i: SymItem): LibrarySymbolGraphic => i.graphic!;
  const isKind = (i: SymItem, ...kinds: string[]) => !!i.graphic && kinds.includes(i.graphic.kind);
  // `isNotPolygonOrCircle`, `isCircle`, `isRectangle` (a text box is a rectangle)
  const notPolyOrCircle = (i: SymItem) => !!i.graphic && !isKind(i, "polyline", "circle");
  const isRect = (i: SymItem) => isKind(i, "rectangle", "text_box");
  add({ owner: "EDA_SHAPE", name: "Shape", kind: "enum", choices: () => SHAPE_CHOICES, get: (i) => (isKind(i, "text_box") ? "rectangle" : g(i).kind) }, SHAPE_PROPS);
  /** A start or end row: X or Y of the start or the end (internal Y), writable where the point is one of the shape's own (an arc's ends are not). */
  const pointRow = (name: string, which: "start" | "end", axis: "x" | "y"): void => {
    add(
      {
        owner: "EDA_SHAPE",
        name,
        kind: "int",
        display: "coord",
        available: notPolyOrCircle,
        get: (i) => {
          const p = which === "start" ? startOf(g(i)) : endOf(g(i));
          return p ? (axis === "x" ? nmOf(p.x) : nmOf(-p.y) || 0) : 0;
        },
        writeable: (i) => !isKind(i, "arc"),
        readOnlyReason: (i) => (isKind(i, "arc") ? READ_ONLY.arcEnds : null),
        set: (i, v, c) => {
          const p = which === "start" ? startOf(g(i)) : endOf(g(i));
          if (!p) return [];
          const at = axis === "x" ? { x: mmOf(num(v)), y: p.y } : { x: p.x, y: mmOf(-num(v)) || 0 };
          return graphicWith(i, c, { [which]: at });
        },
      },
      SHAPE_PROPS
    );
  };
  pointRow("Start X", "start", "x");
  pointRow("Start Y", "start", "y");
  const circle = (i: SymItem) => i.graphic as Extract<LibrarySymbolGraphic, { kind: "circle" }>;
  add({ owner: "EDA_SHAPE", name: "Center X", kind: "int", display: "coord", available: (i) => isKind(i, "circle"), get: (i) => nmOf(circle(i).center.x), set: (i, v, c) => graphicWith(i, c, { center: { x: mmOf(num(v)), y: circle(i).center.y } }) }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Center Y", kind: "int", display: "coord", available: (i) => isKind(i, "circle"), get: (i) => nmOf(-circle(i).center.y) || 0, set: (i, v, c) => graphicWith(i, c, { center: { x: circle(i).center.x, y: mmOf(-num(v)) || 0 } }) }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Radius", kind: "int", display: "size", available: (i) => isKind(i, "circle"), get: (i) => nmOf(circle(i).radius_mm), validate: (v, _i, c) => positiveInt(v, "size", c.units), set: (i, v, c) => graphicWith(i, c, { radius_mm: mmOf(num(v)) }) }, SHAPE_PROPS);
  pointRow("End X", "end", "x");
  pointRow("End Y", "end", "y");
  // `GetRectangleWidth` / `GetRectangleHeight`: the end less the start (internal Y); a new width or height moves the end
  add({ owner: "EDA_SHAPE", name: "Width", kind: "int", display: "size", available: isRect, get: (i) => nmOf(endOf(g(i))!.x - startOf(g(i))!.x), set: (i, v, c) => graphicWith(i, c, { end: { x: mmOf(nmOf(startOf(g(i))!.x) + num(v)), y: endOf(g(i))!.y } }) }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Height", kind: "int", display: "size", available: isRect, get: (i) => nmOf(startOf(g(i))!.y - endOf(g(i))!.y), set: (i, v, c) => graphicWith(i, c, { end: { x: endOf(g(i))!.x, y: mmOf(nmOf(startOf(g(i))!.y) - num(v)) } }) }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Corner Radius", kind: "int", display: "size", available: isRect, get: () => 0, readOnlyReason: READ_ONLY.corner }, SHAPE_PROPS);
  const strokeOf = (i: SymItem): number => ("stroke_mm" in g(i) ? (g(i) as { stroke_mm: number }).stroke_mm : 0);
  add({
    owner: "EDA_SHAPE",
    name: "Line Width",
    kind: "int",
    display: "size",
    get: (i) => nmOf(Math.max(0, strokeOf(i))),
    validate: (v, _i, c) => positiveInt(v, "size", c.units),
    set: (i, v, c) => graphicWith(i, c, { stroke_mm: mmOf(num(v)) }),
  }, SHAPE_PROPS);
  add({
    owner: "EDA_SHAPE",
    name: "Line Style",
    kind: "enum",
    choices: () => LINE_STYLES,
    get: (i, c) => (c.sym as { kicad?: { stroke_types?: Record<string, string> } }).kicad?.stroke_types?.[i.id] ?? "solid",
    readOnlyReason: READ_ONLY.lineStyle,
  }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Line Color", kind: "color", get: () => "", readOnlyReason: READ_ONLY.color }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Angle", kind: "double", display: "degree", available: (i) => isKind(i, "arc"), get: (i) => arcSweepDeg(g(i) as Extract<LibrarySymbolGraphic, { kind: "arc" }>) }, SHAPE_PROPS);
  const fillOf = (i: SymItem): LibraryFill => ("fill" in g(i) ? (g(i) as { fill: LibraryFill }).fill : "none");
  // `fillAvailable`: a polygon, rectangle, circle or bezier (a text box is a rectangle)
  add({
    owner: "EDA_SHAPE",
    name: "Fill",
    kind: "enum",
    available: (i) => isKind(i, "polyline", "rectangle", "circle", "bezier", "text_box"),
    choices: () => UI_FILL,
    // `GetFillModeProp`: none, or solid for any fill; `SetFillModeProp( SOLID )` is `SetFilled( true )`, a symbol's shape's `FILLED_SHAPE` (the body outline colour)
    get: (i) => (fillOf(i) === "none" ? "none" : "solid"),
    validate: (v) => (v === "none" || v === "solid" ? null : "A library symbol's shape has no hatched fill here."),
    set: (i, v, c) => (v === "none" ? graphicWith(i, c, { fill: "none" }) : fillOf(i) === "none" ? graphicWith(i, c, { fill: "outline" }) : []),
  }, SHAPE_PROPS);
  add({ owner: "EDA_SHAPE", name: "Fill Color", kind: "color", get: () => "", readOnlyReason: READ_ONLY.color }, SHAPE_PROPS);

  // ----------------------------------------------------------------------------------------------------------------- SCH_SHAPE
  pm.inheritsAfter("SCH_SHAPE", "SCH_ITEM");
  pm.inheritsAfter("SCH_SHAPE", "EDA_SHAPE");
  // `isFillColorEditable`: a symbol's shape filled with a colour of its own -- none is here
  pm.overrideWriteability("SCH_SHAPE", "EDA_SHAPE", "Fill Color", () => false, READ_ONLY.color);
  add({
    owner: "SCH_SHAPE",
    name: "Fill Mode",
    kind: "enum",
    // `isSymbolItem`: `GetLayer() == LAYER_DEVICE`
    choices: () => FILL_MODES,
    get: fillOf,
    validate: (v) => (v === "color" ? "A library symbol's shape has no fill colour of its own here: fill it with the body's outline or background colour." : null),
    set: (i, v, c) => graphicWith(i, c, { fill: String(v) as LibraryFill }),
  }, SHAPE_PROPS);

  // ----------------------------------------------------------------------------------------------------------------- SCH_TEXTBOX
  pm.inheritsAfter("SCH_TEXTBOX", "SCH_SHAPE");
  pm.inheritsAfter("SCH_TEXTBOX", "EDA_SHAPE");
  pm.inheritsAfter("SCH_TEXTBOX", "EDA_TEXT");
  pm.mask("SCH_TEXTBOX", "EDA_SHAPE", "Shape");
  pm.mask("SCH_TEXTBOX", "EDA_SHAPE", "Corner Radius");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Width");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Height");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Thickness");
  const box = (i: SymItem) => i.graphic as Extract<LibrarySymbolGraphic, { kind: "text_box" }>;
  for (const side of ["Left", "Top", "Right", "Bottom"]) {
    add(
      {
        owner: "SCH_TEXTBOX",
        name: `Margin ${side}`,
        kind: "int",
        display: "size",
        // the box keeps one margin for its four sides here: a margin of one side is the margin of all four
        get: (i) => nmOf(textBoxMarginMm(box(i))),
        validate: (v, _i, c) => positiveInt(v, "size", c.units),
        set: (i, v, c) => graphicWith(i, c, { margin_mm: mmOf(num(v)) }),
      },
      "Margins"
    );
  }
  textSize("SCH_TEXTBOX");
  pm.mask("SCH_TEXTBOX", "EDA_TEXT", "Orientation");

  // ----------------------------------------------------------------------------------------------------------------- SCH_FIELD
  pm.inheritsAfter("SCH_FIELD", "SCH_ITEM");
  pm.inheritsAfter("SCH_FIELD", "EDA_TEXT");
  pm.mask("SCH_FIELD", "SCH_ITEM", "Locked");
  // `SetEffectiveHorizJustify` / `SetEffectiveVertJustify` stand in for EDA_TEXT's: a library field has no transform, so they are the field's own
  pm.replaceProperty("EDA_TEXT", "Horizontal Justification", { owner: "SCH_FIELD", name: "Horizontal Justification", kind: "enum", choices: () => H_JUSTIFY, get: (i) => i.field!.h_align, set: (i, v) => fieldWith(i, { h_align: String(v) as FieldRow["h_align"] }) }, TEXT_PROPS);
  pm.replaceProperty("EDA_TEXT", "Vertical Justification", { owner: "SCH_FIELD", name: "Vertical Justification", kind: "enum", choices: () => V_JUSTIFY, get: (i) => i.field!.v_align, set: (i, v) => fieldWith(i, { v_align: String(v) as FieldRow["v_align"] }) }, TEXT_PROPS);
  add({ owner: "SCH_FIELD", name: "Show Field Name", kind: "bool", get: (i) => i.field!.show_name, set: (i, v) => fieldWith(i, { show_name: Boolean(v) }) });
  add({ owner: "SCH_FIELD", name: "Allow Autoplacement", kind: "bool", get: (i) => !i.field!.do_not_autoplace, set: (i, v) => fieldWith(i, { do_not_autoplace: !v }) });
  pm.mask("SCH_FIELD", "EDA_TEXT", "Hyperlink");
  pm.mask("SCH_FIELD", "EDA_TEXT", "Thickness");
  pm.mask("SCH_FIELD", "EDA_TEXT", "Mirrored");
  pm.mask("SCH_FIELD", "EDA_TEXT", "Width");
  pm.mask("SCH_FIELD", "EDA_TEXT", "Height");
  textSize("SCH_FIELD");
  pm.mask("SCH_FIELD", "EDA_TEXT", "Orientation");
  // `isNonMandatoryField`: only a user field can be private
  pm.overrideAvailability("SCH_FIELD", "SCH_ITEM", "Private", (i) => !!i.field && !isMandatoryField(i.field.name));

  pm.rebuild();
  return pm;
}

export const SYM_PROPERTIES: PropertyManager<SymItem, SymCtx, SymCmd> = build();

/** The Symbol Editor's grid of the selection (`SCH_PROPERTIES_PANEL::rebuildProperties` in the library editor frame). */
export function symGrid(ctx: SymCtx, ids: Iterable<string>): GridModel {
  return buildGrid(SYM_PROPERTIES, symItemsOf(ctx.sym, ids), ctx, symFriendlyName, { libraryEditor: true });
}

/**
 * `SCH_PROPERTIES_PANEL::valueChanged`: the commands that give every selected item the value, as one batch's worth. The fields a row changes (several fields of the selection,
 * or a symbol row that is a field) become ONE `edit_library_symbol_fields` with the symbol's own list, those fields put in (a field the symbol inherits becomes its own).
 */
export function symEdit(ctx: SymCtx, ids: Iterable<string>, name: string, value: PropValue): EditPlan<Cmd> {
  const plan = planEdit(SYM_PROPERTIES, symItemsOf(ctx.sym, ids), name, value, ctx);
  if (!plan.ok) return plan;
  const cmds: Cmd[] = [];
  const changed: FieldRow[] = [];
  for (const k of plan.cmds) {
    if (k.op === "__field") changed.push((k as FieldChange).field);
    else cmds.push(k as Cmd);
  }
  if (changed.length > 0) {
    const own = allFields(ctx.own);
    for (const f of changed) {
      const at = own.findIndex((o) => o.name === f.name);
      if (at < 0) own.push(f);
      else own[at] = f;
    }
    cmds.push({ op: "edit_library_symbol_fields", lib_id: ctx.libId, fields: own.map(wireField) });
  }
  return { ok: true, cmds };
}

/** The id a field row of the canvas has (`fld:lib:<name>`), for a caller that selects one. */
export const symFieldItemId = libFieldId;
