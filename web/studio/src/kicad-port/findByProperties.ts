// Find by Properties (`PCB_ACTIONS::findByProperties`, pcbnew/dialogs/dialog_find_by_properties.cpp at 8303b2ad), the part that decides what the dialog shows and selects. Its two
// tabs: "From Selection" lists the properties the selected items share, each with a Match of Ignored, Matching or Different, and Select Matching selects every item of the
// board whose class is one of the selection's and whose properties match; "Query" takes a PCBEXPR expression and selects the items it is true for. The expression
// runs on the backend (`POST /api/pcb/query`, `eda_drc::query`, the workspace's one copy of the language); `pcbQuery.ts` builds the request.
//
//   rebuildPropertyGrid                 the rows: the properties every selected class has by name, visible, available for each item, in the alphabetical order of their names,
//                                       then the fields every selected footprint has; a value that differs among the items is `<...>` and its Match cannot be set
//   itemMatchesPropertyCriteria         what an item must be: of a class of the selection, and equal (Matching) or not equal (Different) to the row's value for each row
//   generateExpressionFromProperties    "Create from Selection Tab": `A.Name == value && ...`, a length as `0.25mm`, a choice or a text as `'text'`, a flag as `true`
//   normalizeQueryFieldAliases          `A.Reference` (and `A.Datasheet`, `A.Description`, a user field's name) becomes `A.getField('Reference')`
//   queryUsesUnsupportedPairwiseSyntax  `B.` is refused: a query is about one item
//   collectAllBoardItems                the tracks, the footprints with their pads and fields, the drawings, the zones
//
// What the studio's board has instead of KiCad's: footprint graphics and a footprint's other texts are not items of the selection here (they are drawn with their footprint), so
// a query cannot select them; the Type property is the class KiCad calls it (Footprint, Pad, Track, Via, Zone, Text, Graphic, Dimension, Leader, Group).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, Cmd } from "../api/types";
import type { LengthUnit } from "../state/units";
import { pcbQueryRequest, type PcbQueryRequest, type PcbQueryResponse } from "./pcbQuery";
import { padIds } from "./pcbItems";
import { PCB_PROPERTIES, pcbContext, pcbFriendlyName, pcbItemsOf, type PcbCtx, type PcbItem } from "./pcbProperties";
import { formatValue } from "./propertyGrid";
import type { PropValue, PropertyDef } from "./propertyManager";

/** `PROPERTY_MATCH_MODE`. */
export type MatchMode = "ignored" | "matching" | "different";

type Prop = PropertyDef<PcbItem, PcbCtx, Cmd>;

/** `PROPERTY_ROW_DATA`: one row of the grid. A row with no `prop` is a footprint field (named by `name`). */
export interface FindRow {
  name: string;
  display: string;
  /** `rawValue`; null when the items of the selection differ. */
  raw: PropValue | null;
  prop: Prop | null;
  mode: MatchMode;
  mixed: boolean;
}

/** The labels of the Match column's choice (`wxGridCellChoiceEditor`). */
export const MATCH_LABELS: Record<MatchMode, string> = { ignored: "Ignored", matching: "Matching", different: "Different" };

// ------------------------------------------------------------------------------------------------------------------------------------------ values

/** `isVisibleInFindByProperties`: hidden from the Properties panel, except Type, and hidden from the design editors, are not offered. */
function isVisible(name: string, prop: Prop): boolean {
  if (prop.hidden && name !== "Type") return false;
  if (prop.hiddenFromDesignEditors) return false;
  return true;
}

/** The user fields of a footprint (`PCB_FIELD`s other than the Reference and Value, which are properties) by name, with their text. */
function fieldsOf(item: PcbItem): Map<string, string> {
  const out = new Map<string, string>();
  if (item.type !== "FOOTPRINT") return out;
  for (const f of item.part!.fields ?? []) out.set(f.name, f.text);
  return out;
}

/** `formatFindByPropertiesDisplayValue`: what the Properties panel would show for the value (a length with its unit, a choice by its label). */
function displayOf(prop: Prop | null, raw: PropValue, item: PcbItem | null, ctx: PcbCtx, units: LengthUnit): string {
  if (!prop) return String(raw);
  const choices = prop.choices && item ? prop.choices(item, ctx) : null;
  return formatValue({ kind: prop.kind, display: prop.display ?? "default", choices }, raw, units);
}

/**
 * `DIALOG_FIND_BY_PROPERTIES::rebuildPropertyGrid` for the selection `ids`: the rows, and the caption above them ("No items selected", the friendly name of the one item, "N objects
 * selected"). `saved` are the Match modes the rows of the last build had (a mode other than Ignored is kept for a row of the same name that is not mixed).
 */
export function buildRows(board: BoardState, ids: Iterable<string>, units: LengthUnit, saved: ReadonlyMap<string, MatchMode> = new Map()): { caption: string; rows: FindRow[]; types: string[] } {
  const ctx = pcbContext(board, units);
  const items = pcbItemsOf(board, ids);
  if (items.length === 0) return { caption: "No items selected", rows: [], types: [] };
  const caption = items.length === 1 ? pcbFriendlyName(items[0] as PcbItem) : `${items.length} objects selected`;
  const types: string[] = [];
  for (const item of items) if (!types.includes(item.type)) types.push(item.type);

  // `commonProps`: the properties of the first class by name, thinned to those every other class has too.
  const first = types[0] as string;
  const common = new Map<string, Prop>();
  for (const p of PCB_PROPERTIES.getProperties(first)) if (!common.has(p.name)) common.set(p.name, p);
  for (const type of types.slice(1)) for (const name of [...common.keys()]) if (!PCB_PROPERTIES.getProperty(type, name)) common.delete(name);

  const rows: FindRow[] = [];
  for (const name of [...common.keys()].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))) {
    const property = common.get(name) as Prop;
    if (!isVisible(name, property)) continue;
    if (!items.every((item) => PCB_PROPERTIES.isAvailableFor(item.type, PCB_PROPERTIES.getProperty(item.type, name, item, ctx) ?? property, item, ctx))) continue;
    let raw: PropValue | null = null;
    let mixed = false;
    let ok = true;
    let firstItem = true;
    for (const item of items) {
      const own = PCB_PROPERTIES.getProperty(item.type, name, item, ctx);
      if (!own) {
        ok = false;
        break;
      }
      const v = own.get(item, ctx);
      if (firstItem) {
        raw = v;
        firstItem = false;
      } else if (!mixed && raw !== null && v !== raw) {
        mixed = true;
        raw = null;
      }
    }
    if (!ok) continue;
    const mode = !mixed && saved.get(name) ? (saved.get(name) as MatchMode) : "ignored";
    rows.push({ name, display: mixed ? "<...>" : displayOf(property, raw as PropValue, items[0] as PcbItem, ctx, units), raw, prop: property, mode, mixed });
  }

  // `getCommonFootprintFieldNames`: when every selected item is a footprint, the fields they all have and the properties do not already show.
  if (items.every((i) => i.type === "FOOTPRINT")) {
    let names: string[] | null = null;
    for (const item of items) {
      const own = [...fieldsOf(item).keys()];
      names = names === null ? own : names.filter((n) => own.includes(n));
    }
    for (const name of (names ?? []).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))) {
      if (common.has(name)) continue;
      let raw: PropValue | null = null;
      let mixed = false;
      let firstItem = true;
      for (const item of items) {
        const v = fieldsOf(item).get(name) as string;
        if (firstItem) {
          raw = v;
          firstItem = false;
        } else if (!mixed && raw !== null && v !== raw) {
          mixed = true;
          raw = null;
        }
      }
      const mode = !mixed && saved.get(name) ? (saved.get(name) as MatchMode) : "ignored";
      rows.push({ name, display: mixed ? "<...>" : String(raw), raw, prop: null, mode, mixed });
    }
  }
  return { caption, rows, types };
}

/** Is there a row that selects (the "Create from Selection Tab" button is enabled)? */
export const anyActive = (rows: readonly FindRow[]): boolean => rows.some((r) => r.mode !== "ignored");

/** The modes of the rows that are not Ignored, by name: what a rebuild keeps. */
export function savedModes(rows: readonly FindRow[]): Map<string, MatchMode> {
  const out = new Map<string, MatchMode>();
  for (const r of rows) if (r.mode !== "ignored") out.set(r.name, r.mode);
  return out;
}

// ----------------------------------------------------------------------------------------------------------------------------- the whole board

/** `collectAllBoardItems`: the ids of the tracks and vias, the footprints with their pads and fields, the drawings and the zones (groups are not drawings). */
export function collectAllBoardItems(board: BoardState): PcbItem[] {
  const ids: string[] = [];
  for (const t of board.routing?.tracks ?? []) ids.push(t.id);
  for (const v of board.routing?.vias ?? []) ids.push(v.id);
  for (const part of board.parts) {
    if (!part.placed) continue;
    ids.push(part.ref);
    ids.push(...padIds(part));
    for (const f of part.fields ?? []) ids.push(f.id);
  }
  for (const s of board.drawings?.shapes ?? []) ids.push(s.id);
  for (const t of board.drawings?.texts ?? []) ids.push(t.id);
  for (const d of board.drawings?.dimensions ?? []) ids.push(d.id);
  for (const z of board.routing?.zones ?? []) ids.push(z.id);
  return pcbItemsOf(board, ids);
}

/** `itemMatchesPropertyCriteria`: the item's class is one of the selection's and every row that is not Ignored (and not mixed) agrees. */
export function itemMatchesRows(item: PcbItem, rows: readonly FindRow[], types: readonly string[], ctx: PcbCtx): boolean {
  if (!types.includes(item.type)) return false;
  for (const row of rows) {
    if (row.mode === "ignored" || row.mixed) continue;
    let value: PropValue | null;
    if (row.prop === null) {
      if (item.type !== "FOOTPRINT") return false;
      const text = fieldsOf(item).get(row.name);
      value = text === undefined ? null : text;
    } else {
      const own = PCB_PROPERTIES.getProperty(item.type, row.name, item, ctx);
      if (!own) return false;
      if (!PCB_PROPERTIES.isAvailableFor(item.type, own, item, ctx)) return false;
      value = own.get(item, ctx);
    }
    if (value === null) return false;
    const equal = value === row.raw;
    if (row.mode === "matching" && !equal) return false;
    if (row.mode === "different" && equal) return false;
  }
  return true;
}

/** `selectMatchingFromProperties`: the ids of the items of the board that match the rows. */
export function selectMatchingFromRows(board: BoardState, rows: readonly FindRow[], types: readonly string[], units: LengthUnit): string[] {
  const ctx = pcbContext(board, units);
  return collectAllBoardItems(board)
    .filter((item) => itemMatchesRows(item, rows, types, ctx))
    .map((item) => item.id);
}

/** `applyMatchResults`' status line. */
export const matchedText = (n: number): string => `${n} items matched`;

// ------------------------------------------------------------------------------------------------------------------------------------ the query

const isIdentChar = (c: string): boolean => /[A-Za-z0-9_]/.test(c);

/** `propNameToExprField`. */
export const exprFieldName = (name: string): string => name.replace(/ /g, "_");

/** `normalizeFormattedValueForExpression`: "0.25 mm" is `0.25mm`, "10 mils" `10mil`, "90°" `90deg`. */
function normalizeFormatted(prop: Prop, text: string): string {
  switch (prop.display) {
    case "coord":
    case "size":
    case "time":
      return text.replace(/ /g, "").replace(/mils/g, "mil");
    case "degree":
      return text.replace(/°/g, "deg").replace(/ /g, "");
    default:
      return text;
  }
}

/** `formatValueForExpression`: a flag as `true`, a choice or text quoted, a length or an angle with its unit, a number as it is. */
export function formatValueForExpression(prop: Prop | null, raw: PropValue, item: PcbItem | null, ctx: PcbCtx, units: LengthUnit): string {
  if (typeof raw === "boolean") return raw ? "true" : "false";
  const quote = (s: string): string => `'${s.replace(/'/g, "\\'")}'`;
  if (prop && (prop.kind === "enum" || prop.kind === "net") && prop.choices) return quote(displayOf(prop, raw, item, ctx, units));
  if (prop) {
    switch (prop.display) {
      case "coord":
      case "size":
      case "degree":
      case "time":
        return normalizeFormatted(prop, displayOf(prop, raw, item, ctx, units));
      default:
        break;
    }
  }
  // An area is a number of nm^2 to the language (it has no unit of its own).
  if (typeof raw === "number" && prop?.display === "area") return String(raw * 1_000_000);
  if (typeof raw === "number") return displayOf(prop, raw, item, ctx, units);
  return quote(prop ? displayOf(prop, raw, item, ctx, units) : String(raw));
}

/** `generateExpressionFromProperties`: the conditions of the rows that are not Ignored (and not mixed), joined by `&&`. */
export function generateExpression(rows: readonly FindRow[], board: BoardState, ids: Iterable<string>, units: LengthUnit): string {
  const ctx = pcbContext(board, units);
  const first = pcbItemsOf(board, ids)[0] ?? null;
  const conditions: string[] = [];
  for (const row of rows) {
    if (row.mode === "ignored" || row.mixed || row.raw === null) continue;
    const lhs = row.prop === null ? `A.getField(${formatValueForExpression(null, row.name, null, ctx, units)})` : `A.${exprFieldName(row.name)}`;
    const value = formatValueForExpression(row.prop, row.raw, first, ctx, units);
    conditions.push(`${lhs} ${row.mode === "matching" ? "==" : "!="} ${value}`);
  }
  return conditions.join(" && ");
}

/** `getQueryableFootprintFieldNames`: Reference, Value, Datasheet and Description, and the fields of the rows. */
export function queryableFieldNames(rows: readonly FindRow[]): string[] {
  const names = new Set<string>(["Reference", "Value", "Datasheet", "Description"]);
  for (const r of rows) if (r.prop === null) names.add(r.name);
  return [...names].sort();
}

/** `normalizeQueryFieldAliases`: `A.Name` for a field's name (blanks as `_`) becomes `A.getField('Name')`, outside quotes, whole words, the longest name first. */
export function normalizeQueryFieldAliases(expression: string, rows: readonly FindRow[]): string {
  const aliases = queryableFieldNames(rows).map((field) => ({ from: `A.${field.replace(/ /g, "_")}`, to: `A.getField('${field.replace(/'/g, "\\'")}')` }));
  aliases.sort((a, b) => b.from.length - a.from.length);
  const matchAt = (pos: number, alias: string): boolean => {
    if (expression.slice(pos, pos + alias.length) !== alias) return false;
    if (pos > 0 && isIdentChar(expression[pos - 1] as string)) return false;
    const end = pos + alias.length;
    return !(end < expression.length && isIdentChar(expression[end] as string));
  };
  let out = "";
  let inString = false;
  for (let i = 0; i < expression.length; ) {
    const ch = expression[i] as string;
    if (ch === "'" && (i === 0 || expression[i - 1] !== "\\")) {
      inString = !inString;
      out += ch;
      i++;
      continue;
    }
    if (!inString) {
      const alias = aliases.find((a) => matchAt(i, a.from));
      if (alias) {
        out += alias.to;
        i += alias.from.length;
        continue;
      }
    }
    out += ch;
    i++;
  }
  return out;
}

/** `queryUsesUnsupportedPairwiseSyntax`: a `B.` outside quotes. */
export function queryUsesPairwiseSyntax(expression: string): boolean {
  let inString = false;
  for (let i = 0; i < expression.length; i++) {
    const ch = expression[i] as string;
    if (ch === "'" && (i === 0 || expression[i - 1] !== "\\")) {
      inString = !inString;
      continue;
    }
    if (!inString && expression.slice(i, i + 2) === "B." && (i === 0 || !isIdentChar(expression[i - 1] as string))) return true;
  }
  return false;
}

/** The text of `Check Syntax` and of Select Matching's failure. */
export interface QueryCheck {
  ok: boolean;
  /** The status line: "Syntax OK", "Syntax error: ...", "B. expressions are not supported.", "Expression is empty". */
  status: string;
  /** The messages the compiler gave (the box of `Expression Error`). */
  errors: string[];
}

/** Who answers a query: `POST /api/pcb/query` in the studio (`api/client.ts`); the tests hand in another. */
export type QueryRunner = (request: PcbQueryRequest) => Promise<PcbQueryResponse>;

/** What the dialog does before it compiles: an empty text and a `B.` are answered here; anything else is normalized (field aliases) and sent. */
export function prepareQuery(expression: string, rows: readonly FindRow[]): { check: QueryCheck } | { normalized: string } {
  const text = expression.trim();
  if (text === "") return { check: { ok: false, status: "Expression is empty", errors: [] } };
  const normalized = normalizeQueryFieldAliases(text, rows);
  if (queryUsesPairwiseSyntax(normalized)) return { check: { ok: false, status: "B. expressions are not supported.", errors: ["B. expressions are not supported."] } };
  return { normalized };
}

/** The status line of a compile: "Syntax OK" when the text parses, else "Syntax error: " and what the compiler told. */
export function checkOf(response: PcbQueryResponse): QueryCheck {
  return response.ok ? { ok: true, status: "Syntax OK", errors: response.errors } : { ok: false, status: `Syntax error: ${response.errors.join("\n")}`, errors: response.errors };
}

/** `onCheckSyntaxClick` (and the compile `selectMatchingFromQuery` does first) over an expression, as the dialog would after normalizing it. */
export async function checkQuery(expression: string, rows: readonly FindRow[], board: BoardState, units: LengthUnit, run: QueryRunner): Promise<QueryCheck> {
  const prepared = prepareQuery(expression, rows);
  if ("check" in prepared) return prepared.check;
  return checkOf(await run(pcbQueryRequest(prepared.normalized, board, [], units)));
}

/** `selectMatchingFromQuery`: the ids of the items the expression is true for, on any layer they are on; null when it does not compile. */
export async function selectMatchingFromQuery(expression: string, rows: readonly FindRow[], board: BoardState, units: LengthUnit, run: QueryRunner): Promise<{ check: QueryCheck; ids: string[] | null }> {
  const prepared = prepareQuery(expression, rows);
  if ("check" in prepared) return { check: prepared.check, ids: null };
  const response = await run(pcbQueryRequest(prepared.normalized, board, collectAllBoardItems(board), units));
  const check = checkOf(response);
  return { check, ids: check.ok ? (response.ids ?? []) : null };
}

/**
 * `onScintillaCharAdded` after "A.": the property names the editor offers -- those of the track, via, arc, pad, footprint, zone, shape and text classes that Find by Properties
 * shows, and the field names -- with blanks as `_`, sorted.
 */
export function completionNames(rows: readonly FindRow[]): string[] {
  const seen = new Set<string>();
  const tokens: string[] = [];
  for (const cls of ["PCB_TRACK", "PCB_VIA", "PCB_ARC", "PAD", "FOOTPRINT", "ZONE", "PCB_SHAPE", "PCB_TEXT"]) {
    for (const prop of PCB_PROPERTIES.getProperties(cls)) {
      if (!isVisible(prop.name, prop)) continue;
      const name = exprFieldName(prop.name);
      if (!seen.has(name)) {
        seen.add(name);
        tokens.push(name);
      }
    }
  }
  for (const field of queryableFieldNames(rows)) {
    const name = exprFieldName(field);
    if (!seen.has(name)) {
      seen.add(name);
      tokens.push(name);
    }
  }
  return tokens.sort();
}

/** `saveRecentQuery`: the query goes to the front, once, and the list keeps ten. */
export function withRecentQuery(recent: readonly string[], query: string): string[] {
  if (query === "") return [...recent];
  return [query, ...recent.filter((q) => q !== query)].slice(0, 10);
}
