// Fields as items of the schematic: the symbol's Reference, Value, Footprint and Datasheet, a power symbol's Value, a sheet's name and file. Each is selected, moved,
// turned, mirrored and edited on its own (`SCH_FIELD`, eeschema/sch_field.cpp; `SCH_MOVE_TOOL::moveItem`, `SCH_EDIT_TOOL::Rotate` / `Mirror` /
// `AutoplaceFields`, `DIALOG_FIELD_PROPERTIES`), through the same backend verbs as every other item (crates/ops/src/sch_fields.rs).
//
// A field is named by the id `fld:<owner>:<name>` (`eda_engine::fields_edit::field_id`): `owner` is the item's key (a symbol's reference, `#<unit>` after it for a unit
// but the first, a power symbol's or a sheet's id). This module is the studio's half: the ids, the box a field takes on the sheet (what a click, a marquee and the
// selection halo are measured by), and the commands the Field Properties dialog and Autoplace Fields send.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`); the text measure is the caller's.
import type { Cmd, Schematic, SchField } from "../api/types";
import { defaultPenUm, FIELD_SIZE_UM } from "./schText";

export const FIELD_ID_PREFIX = "fld:";

/** The id of the field `name` of the item `owner`. */
export function fieldId(owner: string, name: string): string {
  return `${FIELD_ID_PREFIX}${owner}:${name}`;
}

/** `{ owner, name }` of a field id; null for any other id. */
export function parseFieldId(id: string): { owner: string; name: string } | null {
  if (!id.startsWith(FIELD_ID_PREFIX)) return null;
  const rest = id.slice(FIELD_ID_PREFIX.length);
  const i = rest.indexOf(":");
  return i < 0 ? null : { owner: rest.slice(0, i), name: rest.slice(i + 1) };
}

export const isFieldId = (id: string): boolean => parseFieldId(id) !== null;

/** The key of a placed symbol's fields: its reference, `#<unit>` after it for any unit but the first (`eda_model::ir::field_key`). */
export function ownerKey(sym: { id: string; unit: number }): string {
  return sym.unit <= 1 ? sym.id : `${sym.id}#${sym.unit}`;
}

export type FieldOwnerKind = "symbol" | "power" | "sheet";

/** One field of the sheet with the item that owns it. `field.id` is always set. */
export interface FieldItem {
  id: string;
  owner: string;
  ownerKind: FieldOwnerKind;
  field: SchField & { id: string };
}

/** Every field of the sheet, in drawing order: the sheets', the power symbols', the symbols'. */
export function fieldItems(sch: Pick<Schematic, "symbols" | "power_symbols" | "sheets">): FieldItem[] {
  const out: FieldItem[] = [];
  const push = (owner: string, ownerKind: FieldOwnerKind, fields: SchField[] | undefined) => {
    for (const f of fields ?? []) {
      const id = f.id ?? fieldId(owner, f.name);
      out.push({ id, owner, ownerKind, field: { ...f, id } });
    }
  };
  for (const s of sch.sheets) push(s.id, "sheet", s.fields);
  for (const p of sch.power_symbols) push(p.id, "power", p.fields);
  for (const s of sch.symbols) push(ownerKey(s), "symbol", s.fields);
  return out;
}

/** The field `id` names, or null. */
export function findField(sch: Pick<Schematic, "symbols" | "power_symbols" | "sheets">, id: string): FieldItem | null {
  return fieldItems(sch).find((f) => f.id === id) ?? null;
}

/** The text the field draws (`SCH_FIELD::GetShownText`): `Name: value` when the name is shown. */
export function shownText(f: Pick<SchField, "name" | "text" | "name_shown">): string {
  return f.name_shown ? `${f.name}: ${f.text}` : f.text;
}

export interface FieldBox {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

/** The width of a text set in `sizeUm`: the sum of the glyphs' advances. */
export type Measure = (text: string, sizeUm: number) => number;

/**
 * The box a field's text takes on the sheet (`SCH_FIELD::GetBoundingBox`, `EDA_TEXT::GetTextBox`: the glyphs and the pen's reach, a little taller than the ink, justified
 * against the anchor in the text's own axes, a text that runs up the sheet turned a quarter counter-clockwise about its anchor).
 */
export function fieldBox(f: SchField, measure: Measure): FieldBox {
  const size = f.size_um && f.size_um > 0 ? f.size_um : FIELD_SIZE_UM;
  const pen = f.bold ? Math.round(size / 5) : defaultPenUm(size);
  const k = Math.round(pen * 1.5);
  const w = measure(shownText(f), size) + 2 * k;
  const ext = size + 2 * k;
  const fudge = Math.round(ext * 0.17);
  const h = ext + fudge;
  const x0 = f.h === "left" ? 0 : f.h === "center" ? -Math.trunc(w / 2) : -w;
  const y0 = f.v === "top" ? -fudge : f.v === "center" ? -Math.trunc(h / 2) : -h + fudge;
  const corners: Array<[number, number]> = [
    [x0, y0],
    [x0 + w, y0 + h],
  ];
  const place = ([x, y]: [number, number]): [number, number] => (f.vertical ? [f.at[0] + y, f.at[1] - x] : [f.at[0] + x, f.at[1] + y]);
  const [a, b] = [place(corners[0]!), place(corners[1]!)];
  return { minX: Math.min(a[0], b[0]), minY: Math.min(a[1], b[1]), maxX: Math.max(a[0], b[0]), maxY: Math.max(a[1], b[1]) };
}

/** A field is there to select when it is shown and has something to show (`SCH_FIELD::HitTest`: "if( GetShownText( true ).IsEmpty() ) return false"). */
export const isShownField = (f: SchField): boolean => f.visible && f.text.length > 0;

/**
 * The field under the point, within `tolUm` of its box; of several the one with the smallest box (the one least likely to be what a click on a crowded spot did not
 * mean). Null when none.
 */
export function hitField(sch: Pick<Schematic, "symbols" | "power_symbols" | "sheets">, x: number, y: number, tolUm: number, measure: Measure): string | null {
  let best: { id: string; area: number } | null = null;
  for (const it of fieldItems(sch)) {
    if (!isShownField(it.field)) continue;
    const b = fieldBox(it.field, measure);
    if (x < b.minX - tolUm || x > b.maxX + tolUm || y < b.minY - tolUm || y > b.maxY + tolUm) continue;
    const area = (b.maxX - b.minX) * (b.maxY - b.minY);
    if (!best || area < best.area) best = { id: it.id, area };
  }
  return best?.id ?? null;
}

/** The ids of the shown fields a marquee takes: those the box overlaps (`crossing`) or encloses. */
export function boxFields(sch: Pick<Schematic, "symbols" | "power_symbols" | "sheets">, box: readonly [number, number, number, number], crossing: boolean, measure: Measure): string[] {
  const [x0, y0, x1, y1] = box;
  const out: string[] = [];
  for (const it of fieldItems(sch)) {
    if (!isShownField(it.field)) continue;
    const b = fieldBox(it.field, measure);
    const overlaps = b.minX <= x1 && b.maxX >= x0 && b.minY <= y1 && b.maxY >= y0;
    const inside = b.minX >= x0 && b.maxX <= x1 && b.minY >= y0 && b.maxY <= y1;
    if (crossing ? overlaps : inside) out.push(it.id);
  }
  return out;
}

/** What Field Properties can change of one field; a member left out stays. `at`, `vertical`, `h` and `v` are as the field reads on the sheet. */
export interface FieldChanges {
  text?: string;
  at?: [number, number];
  vertical?: boolean;
  h?: SchField["h"];
  v?: SchField["v"];
  sizeUm?: number;
  bold?: boolean;
  italic?: boolean;
  visible?: boolean;
  nameShown?: boolean;
  allowAutoplace?: boolean;
}

/** `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM`: "Don't allow text to disappear; it can be difficult to correct if you can't select it". */
export const FIELD_SIZE_MIN_UM = 10;
export const FIELD_SIZE_MAX_UM = 1_000_000;

/** The size of a field, as the dialog shows it (the default when the server sent none). */
export const fieldSizeUm = (f: Pick<SchField, "size_um">): number => (f.size_um && f.size_um > 0 ? f.size_um : FIELD_SIZE_UM);

/**
 * The `edit_field` command for what the dialog changed of `field`: only the members that differ from what the field has, or null when nothing does (OK without a change
 * sends nothing, so it is no undo step).
 */
export function fieldEditCmd(field: SchField & { id: string }, c: FieldChanges): Cmd | null {
  const cmd: Record<string, unknown> = { op: "sch_edit", verb: "edit_field", id: field.id };
  let changed = false;
  const set = (key: string, value: unknown, differs: boolean) => {
    if (differs) {
      cmd[key] = value;
      changed = true;
    }
  };
  set("text", c.text, c.text !== undefined && c.text !== field.text);
  set("at", c.at ? { x: c.at[0], y: c.at[1] } : undefined, c.at !== undefined && (Math.round(c.at[0]) !== Math.round(field.at[0]) || Math.round(c.at[1]) !== Math.round(field.at[1])));
  set("vertical", c.vertical, c.vertical !== undefined && c.vertical !== field.vertical);
  set("h", c.h, c.h !== undefined && c.h !== field.h);
  set("v", c.v, c.v !== undefined && c.v !== field.v);
  set("size_um", c.sizeUm, c.sizeUm !== undefined && Math.round(c.sizeUm) !== fieldSizeUm(field));
  set("bold", c.bold, c.bold !== undefined && c.bold !== !!field.bold);
  set("italic", c.italic, c.italic !== undefined && c.italic !== !!field.italic);
  set("visible", c.visible, c.visible !== undefined && c.visible !== field.visible);
  set("name_shown", c.nameShown, c.nameShown !== undefined && c.nameShown !== !!field.name_shown);
  set("allow_autoplace", c.allowAutoplace, c.allowAutoplace !== undefined && c.allowAutoplace !== (field.allow_autoplace ?? true));
  return changed ? (cmd as unknown as Cmd) : null;
}

/** Autoplace Fields (`SCH_EDIT_TOOL::AutoplaceFields`) on the selection: the items that have fields, and the items of the fields selected. Null when it holds neither. */
export function autoplaceCmd(sch: Pick<Schematic, "symbols" | "power_symbols" | "sheets">, selection: readonly string[]): Cmd | null {
  const owners = new Set<string>();
  const known = new Set<string>([...sch.symbols.map((s) => s.id), ...sch.power_symbols.map((p) => p.id), ...sch.sheets.map((s) => s.id)]);
  for (const id of selection) {
    const f = parseFieldId(id);
    if (f) owners.add(f.owner);
    else if (known.has(id)) owners.add(id);
  }
  return owners.size === 0 ? null : ({ op: "sch_edit", verb: "autoplace_fields", ids: [...owners] } as unknown as Cmd);
}

/** The ids of a selection that are fields. */
export const selectedFields = (selection: Iterable<string>): string[] => [...selection].filter(isFieldId);
