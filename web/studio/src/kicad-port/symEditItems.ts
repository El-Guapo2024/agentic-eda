// The Symbol Editor's item edits that work on a selection (eeschema/tools/symbol_editor_edit_tool.cpp, symbol_editor_pin_tool.cpp), as plans the store turns into one
// `batch` verb -- one commit, one undo step, as each C++ function pushes one `SCH_COMMIT`:
//
// - `DoDelete`: the selected pins and graphics, and in Synchronized Pins Mode the twin of each pin in every other unit ([`deletePlan`]).
// - `Rotate`: the selection a quarter turn about the pin's own position (one item) or the selection's centre snapped to the half grid ([`rotatePlan`]).
// - `RepeatPin` (Insert): the last placed pin again, one step further, with its name and number incremented ([`repeatPin`]).
// - `Duplicate` (Ctrl+D): copies of the selected items, a pin with a number taking `GetMaxPinNumber() + 1` ([`duplicateItems`]).
// - `Paste` (Ctrl+V): the copied items on the unit and body style being edited ([`pastedItems`]).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). Lengths are the wire's mm, +y up (the file's frame); KiCad's internal frame is +y down, which only
// matters for the order `Duplicate` numbers its pins in (`LexicographicalCompare` of the internal positions).
import type { LibrarySymbolGraphic, LibrarySymbolPin, MmPoint } from "../api/types";
import { incrementLabelText } from "./incrementLabelText";
import { strNumCmp } from "./sheetPages";

/** Schematic internal units per mm (`SCH_IU_PER_MM`): positions are compared as KiCad's integer `VECTOR2I`s. */
const IU_PER_MM = 1e4;
const iu = (mm: number) => Math.round(mm * IU_PER_MM);
const samePos = (a: MmPoint, b: MmPoint) => iu(a.x) === iu(b.x) && iu(a.y) === iu(b.y);

/**
 * `SYMBOL_EDITOR_EDIT_TOOL::DoDelete`: the ids to remove. Every selected pin goes, and "when pin editing is synchronized, pins in the same position, with the same name in
 * different units are also removed. But only one pin per unit (matching)": the same position, body style, electrical type and name, the first such pin of each other unit.
 * Graphics go as they are. (A selected field is hidden, not deleted, in KiCad; the selection here holds no fields.)
 */
export function deletePlan(selected: readonly string[], pins: readonly LibrarySymbolPin[], graphics: readonly LibrarySymbolGraphic[], sync: boolean, unitCount: number): { pins: string[]; graphics: string[] } {
  const pinIds = new Set<string>();
  for (const id of selected) {
    const cur = pins.find((p) => p.id === id);
    if (!cur) continue;
    pinIds.add(id);
    if (!sync) continue;
    const gotUnit = new Array<boolean>(Math.max(unitCount, cur.unit) + 1).fill(false);
    gotUnit[cur.unit] = true;
    for (const pin of pins) {
      if (!pin.id || gotUnit[pin.unit]) continue;
      if (!samePos(pin.at, cur.at) || pin.body_style !== cur.body_style || pin.electrical_type !== cur.electrical_type || pin.name !== cur.name) continue;
      pinIds.add(pin.id);
      gotUnit[pin.unit] = true;
    }
  }
  const graphicIds = selected.filter((id) => graphics.some((g) => g.id === id));
  return { pins: [...pinIds], graphics: graphicIds };
}

/** Where a pin's other end is (its length along `angle_deg`, the direction from the pin's position into the body). */
function pinEnd(p: LibrarySymbolPin): MmPoint {
  const a = (p.angle_deg * Math.PI) / 180;
  return { x: p.at.x + Math.round(Math.cos(a)) * p.length_mm, y: p.at.y + Math.round(Math.sin(a)) * p.length_mm };
}

/** `EDA_DRAW_FRAME::GetNearestHalfGridPosition`: the point on the grid of half the grid size nearest `p`. */
export function nearestHalfGrid(p: MmPoint, gridMm: number): MmPoint {
  const half = gridMm / 2;
  if (!(half > 0)) return p;
  const snap = (v: number) => Math.round(v / half) * half;
  return { x: snap(p.x), y: snap(p.y) };
}

/**
 * `SYMBOL_EDITOR_EDIT_TOOL::Rotate`: the selected pins turned a quarter, counter-clockwise on screen with `ccw`, about one point -- the pin's own position when the selection is
 * one item, else `GetNearestHalfGridPosition( selection.GetCenter() )`, the centre of the box of every selected item (`boxes`: the graphics' boxes the caller has; the pins'
 * are their two ends). Only the selected pins move: no twin in another unit is touched, the commit holds only the selection. `SCH_PIN::Rotate` turns the position about
 * the point and the orientation with it. Returns the pins as they are after the turn.
 */
export function rotatePlan(selectedPins: readonly LibrarySymbolPin[], selectionSize: number, ccw: boolean, gridMm: number, otherBoxes: readonly [MmPoint, MmPoint][] = []): LibrarySymbolPin[] {
  if (selectedPins.length === 0) return [];
  let pivot: MmPoint;
  if (selectionSize === 1) pivot = selectedPins[0]!.at;
  else {
    const pts: MmPoint[] = [...selectedPins.flatMap((p) => [p.at, pinEnd(p)]), ...otherBoxes.flat()];
    const xs = pts.map((p) => p.x);
    const ys = pts.map((p) => p.y);
    pivot = nearestHalfGrid({ x: (Math.min(...xs) + Math.max(...xs)) / 2, y: (Math.min(...ys) + Math.max(...ys)) / 2 }, gridMm);
  }
  const s = ccw ? 1 : -1;
  return selectedPins.map((p) => {
    const dx = p.at.x - pivot.x;
    const dy = p.at.y - pivot.y;
    // +y up: a counter-clockwise quarter turn takes (dx, dy) to (-dy, dx)
    const at = { x: round(pivot.x - s * dy), y: round(pivot.y + s * dx) };
    const angle = (((p.angle_deg + s * 90) % 360) + 360) % 360;
    return { ...p, at, angle_deg: angle };
  });
}

const round = (v: number) => Math.round(v * IU_PER_MM) / IU_PER_MM;

/** `SYMBOL_EDITOR_SETTINGS::m_Repeat`: `repeat.pin_step` (100 mil) and `repeat.label_delta` (1). */
export const REPEAT_PIN_STEP_MM = 2.54;
export const REPEAT_LABEL_DELTA = 1;

/** `g_lastPin = pin->m_Uuid` (`CreatePin`, `RepeatPin`): the pin as it was placed, given the id the server gave it -- the pin of `pins` (the symbol after the
 * add) with its number, unit, body style and place; the last such one when several match. Without a match it keeps the id it has (none). */
export function placedPinWithId(placed: LibrarySymbolPin, pins: readonly LibrarySymbolPin[]): LibrarySymbolPin {
  const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
  const matches = pins.filter((p) => p.id && p.number === placed.number && p.unit === placed.unit && p.body_style === placed.body_style && near(p.at.x, placed.at.x) && near(p.at.y, placed.at.y));
  const found = matches[matches.length - 1];
  return found ? { ...placed, id: found.id } : placed;
}

/**
 * `SYMBOL_EDITOR_PIN_TOOL::RepeatPin`: a duplicate of `source` moved by the pin step -- down the screen (`step.y`) for a pin pointing right or left, across (`step.x`) for one
 * pointing up or down -- with `IncrementString( name, label_delta )` and the same for the number. The step is in KiCad's +y-down frame, so "down" is -y here.
 */
export function repeatPin(source: LibrarySymbolPin, stepMm = REPEAT_PIN_STEP_MM, delta = REPEAT_LABEL_DELTA): LibrarySymbolPin {
  const horizontal = Math.round(source.angle_deg / 90) % 2 === 0;
  const at = horizontal ? { x: source.at.x, y: round(source.at.y - stepMm) } : { x: round(source.at.x + stepMm), y: source.at.y };
  return { ...source, id: undefined, at, name: incrementLabelText(source.name, delta), number: incrementLabelText(source.number, delta) };
}

/** `wxString::IsNumber`: digits only (with an optional sign). */
const isNumber = (s: string) => /^[+-]?\d+$/.test(s);

/** `LIB_SYMBOL::GetMaxPinNumber`: the largest pin number that reads as an integer (`ToLong`), 0 when there is none. */
export function maxPinNumber(pins: readonly Pick<LibrarySymbolPin, "number">[]): number {
  let max = 0;
  for (const p of pins) if (isNumber(p.number.trim())) max = Math.max(max, parseInt(p.number.trim(), 10));
  return max;
}

/**
 * `SYMBOL_EDITOR_EDIT_TOOL::Duplicate`: a copy of each selected item. The pins are copied in the order of their numbers (`StrNumCmp`, when both numbers are numeric and
 * differ) or else of their positions (`LexicographicalCompare`); each copied pin that has a number gets `GetMaxPinNumber() + 1`, counting the copies already added.
 * The copies are where the originals are (KiCad then starts a move with them).
 */
export function duplicateItems(selected: readonly string[], pins: readonly LibrarySymbolPin[], graphics: readonly LibrarySymbolGraphic[]): { pins: LibrarySymbolPin[]; graphics: LibrarySymbolGraphic[] } {
  const chosen = pins.filter((p) => p.id && selected.includes(p.id));
  chosen.sort((a, b) => {
    const cmp = strNumCmp(a.number, b.number);
    if (isNumber(a.number) && isNumber(b.number) && cmp !== 0) return cmp;
    // internal frame: x, then y down
    const ax = iu(a.at.x), bx = iu(b.at.x), ay = -iu(a.at.y), by = -iu(b.at.y);
    if (ax !== bx) return ax - bx;
    if (ay !== by) return ay - by;
    return (a.id ?? "").localeCompare(b.id ?? "");
  });
  const all: Pick<LibrarySymbolPin, "number">[] = [...pins];
  const outPins: LibrarySymbolPin[] = [];
  for (const p of chosen) {
    const copy: LibrarySymbolPin = { ...p, id: undefined };
    if (copy.number !== "") copy.number = String(maxPinNumber(all) + 1);
    all.push(copy);
    outPins.push(copy);
  }
  const outGraphics = graphics.filter((g) => g.id && selected.includes(g.id)).map((g) => ({ ...g, id: undefined }));
  return { pins: outPins, graphics: outGraphics };
}

/**
 * `SYMBOL_EDITOR_EDIT_TOOL::Paste`: each copied item on the unit and the body style being edited, unless it is common to all of them (`SetUnit( GetUnit() ? m_frame->GetUnit()
 * : 0 )`, the same for the body style).
 */
export function pastedItems(copied: { pins: readonly LibrarySymbolPin[]; graphics: readonly LibrarySymbolGraphic[] }, unit: number, bodyStyle: number): { pins: LibrarySymbolPin[]; graphics: LibrarySymbolGraphic[] } {
  const place = <T extends { unit: number; body_style: number; id?: string }>(it: T): T => ({ ...it, id: undefined, unit: it.unit ? unit : 0, body_style: it.body_style ? bodyStyle : 0 });
  return { pins: copied.pins.map(place), graphics: copied.graphics.map(place) };
}
