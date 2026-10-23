// A pin of a hierarchical sheet as an item of the sheet it is drawn on: `SCH_SHEET_PIN_T` of `SCH_SELECTION_TOOL` (eeschema/sch_sheet_pin.cpp, sch_label.cpp at 8303b2ad).
//
// A sheet pin has an id of its own (`shpin_...`, the id the verbs `edit_sheet_pin`, `delete_sheet_pin` and `sch_move` take), so it is selected, moved along the border of its sheet,
// edited and deleted one at a time. What a click hits is `SCH_SHEET_PIN::HitTest`: `GetBoundingBox` -- for a hierarchical label the body box, `SCH_HIERLABEL::GetBodyBoundingBox`:
// the pin's flag and its name, grown by the accuracy. `SCH_SHEET_PIN::SetSide` gives the text its spin from the side the pin is on (a pin on the left border reads to the right).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LabelShape, Schematic, Sheet } from "../api/types";
import type { Box, P } from "./schItemGeom";
import { defaultPenNm } from "./schText";

export type SheetSide = "left" | "right" | "top" | "bottom";

/** `m_DefaultTextSize`: the size a sheet pin's text is set in (50 mils), micrometres. */
export const SHEET_PIN_TEXT_SIZE_NM = 1_270_000;
/** `DANGLING_SYMBOL_SIZE`: 12 mils. */
const DANGLING_NM = 304_800;
/** `m_textOffsetRatio` (0.15) of the text size: `EDA_TEXT`'s margin around a label's text. */
const TEXT_OFFSET_RATIO = 0.15;

/** The border of its sheet a pin at `at` is on (the order the writer and the painter use: left, right, top, then bottom). */
export function sheetPinSide(sheet: { at: readonly [number, number]; size: readonly [number, number] }, at: readonly [number, number]): SheetSide {
  const [x, y] = sheet.at;
  const [w] = sheet.size;
  return at[0] <= x ? "left" : at[0] >= x + w ? "right" : at[1] <= y ? "top" : "bottom";
}

/** `SCH_HIERLABEL::GetBodyBoundingBox` of a sheet pin on `side`: the flag and the name, from the pin's own point (`widthNm` is the name's width in the font). */
export function sheetPinBodyBox(at: readonly [number, number], side: SheetSide, widthNm: number, sizeNm = SHEET_PIN_TEXT_SIZE_NM): Box {
  const pen = defaultPenNm(sizeNm);
  const margin = Math.round(sizeNm * TEXT_OFFSET_RATIO);
  const height = sizeNm + pen + margin;
  const length = widthNm + height; // "add height for triangular shapes"
  let x = at[0];
  let y = at[1];
  let dx: number;
  let dy: number;
  switch (side) {
    case "right": // the pin is on the right border: its text runs left into the sheet (`SPIN_STYLE::LEFT`)
      dx = -length;
      dy = height;
      x += DANGLING_NM;
      y -= height / 2;
      break;
    case "bottom": // `SPIN_STYLE::UP`
      dx = height;
      dy = -length;
      x -= height / 2;
      y += DANGLING_NM;
      break;
    case "left": // `SPIN_STYLE::RIGHT`
      dx = length;
      dy = height;
      x -= DANGLING_NM;
      y -= height / 2;
      break;
    case "top": // `SPIN_STYLE::BOTTOM`
      dx = height;
      dy = length;
      x -= height / 2;
      y -= DANGLING_NM;
      break;
  }
  return { minX: Math.min(x, x + dx), minY: Math.min(y, y + dy), maxX: Math.max(x, x + dx), maxY: Math.max(y, y + dy) };
}

/** One pin of a placed sheet. */
export interface SheetPinItem {
  id: string;
  sheetId: string;
  name: string;
  shape: LabelShape;
  at: [number, number];
  side: SheetSide;
  /** The size the pin's text is set in (`EDA_TEXT::GetTextWidth`), micrometres: Sheet Pin Properties' own, else 50 mil. */
  sizeNm: number;
}

const cache = new WeakMap<Schematic, SheetPinItem[]>();

/** Every pin of every sheet placed on the sheet in view, built once per sheet object. */
export function sheetPinItems(sch: Schematic): SheetPinItem[] {
  let items = cache.get(sch);
  if (!items) {
    items = [];
    for (const s of sch.sheets ?? []) for (const p of s.pins ?? []) if (p.id) items.push({ id: p.id, sheetId: s.id, name: p.name, shape: p.shape, at: p.at, side: sheetPinSide(s, p.at), sizeNm: p.size_nm && p.size_nm > 0 ? p.size_nm : SHEET_PIN_TEXT_SIZE_NM });
    cache.set(sch, items);
  }
  return items;
}

export function sheetPinById(sch: Schematic, id: string): SheetPinItem | null {
  return sheetPinItems(sch).find((p) => p.id === id) ?? null;
}

/** The sheet a pin is on. */
export function sheetOfPin(sch: Schematic, pin: SheetPinItem): Sheet | undefined {
  return sch.sheets.find((s) => s.id === pin.sheetId);
}

/** How wide a text of the given size is, micrometres. */
export type Measure = (text: string, sizeNm: number) => number;

/** The box of a sheet pin (`SCH_SHEET_PIN::GetBoundingBox`). */
export function sheetPinBox(pin: SheetPinItem, measure: Measure): Box {
  return sheetPinBodyBox(pin.at, pin.side, measure(pin.name, pin.sizeNm), pin.sizeNm);
}

/** `SCH_SHEET_PIN::HitTest( aPoint, aAccuracy )`: the box, grown by the accuracy. */
export function hitSheetPin(pin: SheetPinItem, p: P, accuracyNm: number, measure: Measure): boolean {
  const b = sheetPinBox(pin, measure);
  return p[0] >= b.minX - accuracyNm && p[0] <= b.maxX + accuracyNm && p[1] >= b.minY - accuracyNm && p[1] <= b.maxY + accuracyNm;
}

/**
 * `SCH_SELECTION_TOOL::SelectMultiple` for the pins of a sheet: a pin is taken by a box that touches (crossing) or holds (window) its box, as the pins of symbols are --
 * but unlike those they are collected with everything else, not as a fallback.
 */
export function boxSheetPins(sch: Schematic, box: Box, crossing: boolean, measure: Measure): string[] {
  const out: string[] = [];
  for (const pin of sheetPinItems(sch)) {
    const b = sheetPinBox(pin, measure);
    const touches = !(b.minX > box.maxX || b.maxX < box.minX || b.minY > box.maxY || b.maxY < box.minY);
    const held = b.minX >= box.minX && b.maxX <= box.maxX && b.minY >= box.minY && b.maxY <= box.maxY;
    if (crossing ? touches : held) out.push(pin.id);
  }
  return out;
}
