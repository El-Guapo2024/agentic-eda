// The Properties panel's grid (`PROPERTIES_PANEL`, common/widgets/properties_panel.cpp at KiCad 8303b2ad), as a model the React grid draws:
//
//   `buildGrid`  <- `PROPERTIES_PANEL::rebuildProperties` + `extractValueAndWritability`: the properties every selected item has (a name found on the
//                  class of each, available and not hidden, with the same choices), each with the value they share or "unspecified" (`<...>`) when they
//                  differ, writeable only when it is for every item, in the groups and the order the first item's class gives.
//   `planEdit`   <- `valueChanging` (validation) and `PCB_PROPERTIES_PANEL::valueChanged` / `SCH_PROPERTIES_PANEL::valueChanged` (set the property on every
//                  item of the selection): the commands that make every item have the new value. The caller sends them as ONE `Batch`, which is
//                  KiCad's one `COMMIT::Push( "Edit Properties" )`.
//   `formatValue` / `parseValue` <- `PGPROPERTY_DISTANCE`, `_AREA`, `_ANGLE`, `_RATIO` and the plain int / float / bool / string properties
//                  (common/properties/pg_properties.cpp): what a cell shows and what typing into it means.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import { umFrom, umTo, type LengthUnit } from "../state/units";
import type { Choice, PropDisplay, PropItem, PropKind, PropValue, PropertyManager } from "./propertyManager";

/** One row. `value` is null when the items of the selection differ (`SetUnspecifiedValueAppearance( "<...>" )`). */
export interface GridRow {
  name: string;
  kind: PropKind;
  display: PropDisplay;
  value: PropValue | null;
  /** False when the property is read-only for any of the items. */
  writable: boolean;
  /** For an enum or net row. */
  choices: readonly Choice[] | null;
}

export interface GridGroup {
  /** The group as registered ("" is the first, `unspecifiedGroupCaption`). */
  name: string;
  caption: string;
  rows: GridRow[];
}

export interface GridModel {
  /** `m_caption`: "No objects selected", the friendly name of the item, or "N objects selected". */
  caption: string;
  count: number;
  groups: GridGroup[];
}

/** `unspecifiedGroupCaption`. */
export const BASIC_PROPERTIES = "Basic Properties";

function sameChoices(a: readonly Choice[] | null, b: readonly Choice[] | null): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  return a.every((c, i) => c.label === b[i]!.label && c.value === b[i]!.value);
}

interface Extracted {
  value: PropValue | null;
  writable: boolean;
  choices: readonly Choice[] | null;
}

/**
 * `extractValueAndWritability`: null when the property is not available for the whole selection (a class lacks it, the item does not have it, it is hidden, or
 * the items offer different choices); else the value they share (null when they differ) and whether every item can write it.
 */
export function extractValueAndWritability<I extends PropItem, C, K>(pm: PropertyManager<I, C, K>, items: readonly I[], name: string, ctx: C): Extracted | null {
  let different = false;
  let have = false;
  let value: PropValue | null = null;
  let writable = true;
  let choices: readonly Choice[] | null = null;
  let first = true;
  for (const item of items) {
    const property = pm.getProperty(item.type, name, item, ctx);
    if (!property) return null;
    if (!pm.isAvailableFor(item.type, property, item, ctx)) return null;
    if (property.hidden) return null;
    const c = property.choices ? property.choices(item, ctx) : null;
    if (first) {
      choices = c;
      first = false;
    } else if (!sameChoices(c, choices)) {
      return null;
    }
    if (!pm.isWriteableFor(item.type, property, item, ctx)) writable = false;
    const v = property.get(item, ctx);
    if (!different && have && v !== value) {
      different = true;
      value = null;
    } else if (!different) {
      value = v;
      have = true;
    }
  }
  return { value: different ? null : value, writable, choices };
}

/** `PROPERTIES_PANEL::rebuildProperties` for a selection, in selection order. */
export function buildGrid<I extends PropItem, C, K>(pm: PropertyManager<I, C, K>, items: readonly I[], ctx: C, friendlyName: (item: I) => string): GridModel {
  const front = items[0];
  if (!front) return { caption: "No objects selected", count: 0, groups: [] };
  const caption = items.length === 1 ? friendlyName(front) : `${items.length} objects selected`;

  // The classes in the selection, in the order they first appear; the first decides the order and the groups.
  const types: string[] = [];
  for (const item of items) if (!types.includes(item.type)) types.push(item.type);
  const firstType = types[0]!;
  const firstItem = front;

  // `commonProps`: the properties of the first class by name, then thinned to those every other class has too.
  const names: string[] = [];
  for (const p of pm.getProperties(firstType)) if (!names.includes(p.name)) names.push(p.name);
  const groupOrder = [...pm.getGroupDisplayOrder(firstType)];
  const known = new Set(groupOrder);
  let common = names;
  for (const type of types.slice(1)) {
    for (const g of pm.getGroupDisplayOrder(type)) {
      if (!known.has(g)) {
        groupOrder.push(g);
        known.add(g);
      }
    }
    common = common.filter((n) => pm.getProperty(type, n) !== undefined);
  }

  const order = pm.getDisplayOrder(firstType);
  const byGroup = new Map<string, Array<{ row: GridRow; at: number }>>();
  for (const name of common) {
    const property = pm.getProperty(firstType, name, firstItem, ctx);
    if (!property || property.hidden || property.hiddenFromDesignEditors) continue;
    const got = extractValueAndWritability(pm, items, name, ctx);
    if (!got) continue;
    const row: GridRow = { name: property.name, kind: property.kind, display: property.display ?? "default", value: got.value, writable: got.writable, choices: got.choices };
    const group = property.group ?? "";
    const list = byGroup.get(group) ?? [];
    list.push({ row, at: order.get(property) ?? 0 });
    byGroup.set(group, list);
  }

  const groups: GridGroup[] = [];
  for (const name of groupOrder) {
    const list = byGroup.get(name);
    if (!list || list.length === 0) continue;
    list.sort((a, b) => a.at - b.at);
    groups.push({ name, caption: name === "" ? BASIC_PROPERTIES : name, rows: list.map((e) => e.row) });
  }
  return { caption, count: items.length, groups };
}

/** What `planEdit` answers. */
export type EditPlan<K> = { ok: true; cmds: K[] } | { ok: false; error: string };

/**
 * The commands that give property `name` the value `value` on every item that can take it. A validator that refuses stops it, with the message the info bar shows
 * ("Name: message"). KiCad validates against the first item only; here every item is asked, because a command the backend refuses refuses the whole batch.
 */
export function planEdit<I extends PropItem, C, K>(pm: PropertyManager<I, C, K>, items: readonly I[], name: string, value: PropValue, ctx: C): EditPlan<K> {
  if (items.length === 0) return { ok: false, error: "Nothing is selected." };
  const cmds: K[] = [];
  for (const item of items) {
    const property = pm.getProperty(item.type, name, item, ctx);
    if (!property || !pm.isWriteableFor(item.type, property, item, ctx) || !property.set) continue;
    const refusal = property.validate?.(value, item, ctx);
    if (refusal) return { ok: false, error: `${property.name}: ${refusal}` };
    cmds.push(...property.set(item, value, ctx));
  }
  return { ok: true, cmds };
}

// ------------------------------------------------------------------------------------------------------------------------------- cells

const DECIMALS: Record<LengthUnit, number> = { mm: 4, mil: 2, in: 5 };

/** A number without trailing noise (`10.1600` -> `10.16`). */
export function trimmed(v: number, decimals: number): string {
  return String(Number(v.toFixed(decimals)));
}

/** A length in the display units, no suffix (`lengthText` of the edit dialogs). */
export function lengthNumber(um: number, units: LengthUnit): string {
  return trimmed(umTo(um, units), DECIMALS[units]);
}

/** The text of a cell for a value: lengths and areas with their unit (`StringFromValue( v, true )`), angles with the degree sign, a choice by its label. */
export function formatValue(row: Pick<GridRow, "kind" | "display" | "choices">, value: PropValue, units: LengthUnit): string {
  if (typeof value === "boolean") return value ? "true" : "false";
  if (row.choices && (row.kind === "enum" || row.kind === "net")) {
    const hit = row.choices.find((c) => c.value === value);
    if (hit) return hit.label;
  }
  if (typeof value === "string") return value;
  switch (row.display) {
    case "size":
    case "coord":
      return `${lengthNumber(value, units)} ${units}`;
    case "area": {
      // um^2 as mm^2 (`EDA_UNIT_UTILS::UI::StringFromValue` with `EDA_DATA_TYPE::AREA`), whatever the display unit.
      return `${trimmed(value / 1e6, 4)} mm²`;
    }
    case "degree":
      return `${trimmed(value, 4)}°`;
    case "ratio":
      return trimmed(value, 4);
    default:
      return row.kind === "int" ? String(Math.round(value)) : trimmed(value, 6);
  }
}

/** A cell as the text a user starts editing: the same, without the unit (typing "1.5" means the display unit). */
export function editText(row: Pick<GridRow, "kind" | "display" | "choices">, value: PropValue, units: LengthUnit): string {
  if (typeof value === "string" || typeof value === "boolean") return String(value);
  switch (row.display) {
    case "size":
    case "coord":
      return lengthNumber(value, units);
    case "area":
      return trimmed(value / 1e6, 4);
    case "degree":
      return trimmed(value, 4);
    default:
      return formatValue(row, value, units);
  }
}

export type Parsed = { ok: true; value: PropValue } | { ok: false; error: string };

const NUMBER = "[-+]?(?:\\d+\\.?\\d*|\\.\\d+)(?:[eE][-+]?\\d+)?";

/**
 * What the text typed into a cell means. A length takes a unit suffix (`mm`, `mil`, `in`, `"`) and means the display unit without one; an angle may end with a degree sign;
 * an integer must be whole. A string is taken as typed.
 */
export function parseValue(row: Pick<GridRow, "kind" | "display">, text: string, units: LengthUnit): Parsed {
  if (row.kind === "string") return { ok: true, value: text };
  const t = text.trim().replace(",", ".");
  if (row.kind === "bool") return { ok: false, error: "Not a check box value." };
  if (row.kind === "enum" || row.kind === "net" || row.kind === "color") return { ok: true, value: text };
  const bad = (what: string): Parsed => ({ ok: false, error: `"${text.trim()}" is not ${what}.` });
  switch (row.display) {
    case "size":
    case "coord": {
      const m = new RegExp(`^(${NUMBER})\\s*(mm|mil|in|")?$`, "i").exec(t);
      if (!m) return bad("a length");
      const unit: LengthUnit = m[2] ? (m[2] === '"' ? "in" : (m[2].toLowerCase() as LengthUnit)) : units;
      return { ok: true, value: Math.round(umFrom(parseFloat(m[1]!), unit)) };
    }
    case "area": {
      const m = new RegExp(`^(${NUMBER})\\s*(?:mm²|mm2)?$`, "i").exec(t);
      return m ? { ok: true, value: Math.round(parseFloat(m[1]!) * 1e6) } : bad("an area");
    }
    case "degree": {
      const m = new RegExp(`^(${NUMBER})\\s*(?:°|deg)?$`, "i").exec(t);
      return m ? { ok: true, value: parseFloat(m[1]!) } : bad("an angle");
    }
    default: {
      const m = new RegExp(`^(${NUMBER})$`).exec(t);
      if (!m) return bad("a number");
      const v = parseFloat(m[1]!);
      if (row.kind === "int") return Number.isInteger(v) ? { ok: true, value: v } : bad("a whole number");
      return { ok: true, value: v };
    }
  }
}

// ------------------------------------------------------------------------------------------------------------------------- validators

/** `VALIDATION_ERROR_TOO_SMALL::Format`. */
export function tooSmall(min: number, display: PropDisplay, units: LengthUnit): string {
  return `Value must be greater than or equal to ${display === "size" || display === "coord" ? `${lengthNumber(min, units)} ${units}` : String(min)}`;
}

/** `VALIDATION_ERROR_TOO_LARGE::Format`. */
export function tooLarge(max: number, display: PropDisplay, units: LengthUnit): string {
  return `Value must be less than or equal to ${display === "size" || display === "coord" ? `${lengthNumber(max, units)} ${units}` : String(max)}`;
}

/** `PROPERTY_VALIDATORS::PositiveIntValidator`: not below zero (the units are named by the caller's display). */
export function positiveInt(value: PropValue, display: PropDisplay = "size", units: LengthUnit = "mm"): string | null {
  return typeof value === "number" && value < 0 ? tooSmall(0, display, units) : null;
}

/** `PROPERTY_VALIDATORS::RangeIntValidator<Min, Max>`. */
export function rangeInt(min: number, max: number, display: PropDisplay = "size", units: LengthUnit = "mm"): (value: PropValue) => string | null {
  return (value) => {
    if (typeof value !== "number") return null;
    if (value > max) return tooLarge(max, display, units);
    if (value < min) return tooSmall(min, display, units);
    return null;
  };
}

/** `PROPERTY_VALIDATORS::PositiveRatioValidator`: 0 to 1, unitless. */
export function positiveRatio(value: PropValue): string | null {
  if (typeof value !== "number") return null;
  if (value > 1) return tooLarge(1, "ratio", "mm");
  if (value < 0) return tooSmall(0, "ratio", "mm");
  return null;
}
