// "Synchronized Pins Mode" (`eeschema.SymbolLibraryControl.toggleSyncedPinsMode`, `SYMBOL_EDIT_FRAME::m_SyncPinEdit`): in
// a multi-unit symbol whose units are interchangeable, the pins that sit on top of each other in different units are one
// pin as far as editing goes -- placing a pin places its image in every other unit, moving or editing one carries the
// others along. Ported from `SYMBOL_EDITOR_PIN_TOOL::{CreateImagePins, PlacePin, EditPinProperties}` and
// `SYMBOL_EDITOR_MOVE_TOOL::Main` (eeschema/tools/), `SCH_PIN::ChangeLength` (eeschema/sch_pin.cpp) and
// `SYMBOL_EDIT_FRAME::SynchronizePins`. Pure planners: the callers turn the plans into verbs.
import type { LibrarySymbolPin, MmPoint } from "../api/types";

const EPS = 1e-6;

const samePos = (a: MmPoint, b: MmPoint) => Math.abs(a.x - b.x) < EPS && Math.abs(a.y - b.y) < EPS;
const norm360 = (deg: number) => ((Math.round(deg) % 360) + 360) % 360;
const sameAngle = (a: number, b: number) => norm360(a) === norm360(b);

/** `SYMBOL_EDIT_FRAME::SynchronizePins`: the mode is on and the symbol has more than one unit (units are never "locked" here: the symbol has no such flag). */
export function synchronizePins(syncMode: boolean, unitCount: number): boolean {
  return syncMode && unitCount > 1;
}

/** `m_SyncPinEdit = IsMultiUnit() && !UnitsLocked()`: the mode a symbol starts in when it is opened. */
export function defaultSyncMode(unitCount: number): boolean {
  return unitCount > 1;
}

/**
 * `SCH_PIN::ChangeLength`: the pin's inner end stays where it is and the connection point (`at`) moves along the pin, i.e. by
 * `old - new` along `(cos a, sin a)` (`angle_deg` points from the connection point into the body). Same rule as
 * `crates/ops/src/library_editors.rs::change_pin_length`.
 */
export function changePinLength(pin: LibrarySymbolPin, newLengthMm: number): LibrarySymbolPin {
  const change = pin.length_mm - newLengthMm;
  const a = norm360(pin.angle_deg);
  const [dx, dy] = a === 0 ? [1, 0] : a === 90 ? [0, 1] : a === 180 ? [-1, 0] : a === 270 ? [0, -1] : [Math.cos((pin.angle_deg * Math.PI) / 180), Math.sin((pin.angle_deg * Math.PI) / 180)];
  const snap = (v: number) => Math.round(v * 1e6) / 1e6;
  return { ...pin, length_mm: newLengthMm, at: { x: snap(pin.at.x + dx * change), y: snap(pin.at.y + dy * change) } };
}

/**
 * `SYMBOL_EDITOR_PIN_TOOL::CreateImagePins`: with the mode on, a pin placed on one unit (not on `0`, "common to all units")
 * gets an image in every other unit, at the same position, with a temporary number `"<number>-U<letter>"` -- the letter is
 * `'A' + unit - 1` -- ("we do not know the actual number").
 */
export function imagePinsFor(pin: LibrarySymbolPin, unitCount: number): LibrarySymbolPin[] {
  if (pin.unit === 0) return [];
  const out: LibrarySymbolPin[] = [];
  for (let unit = 1; unit <= unitCount; unit++) {
    if (unit === pin.unit) continue;
    out.push({ ...pin, id: undefined, unit, number: `${pin.number}-U${String.fromCharCode("A".charCodeAt(0) + unit - 1)}` });
  }
  return out;
}

/**
 * `SYMBOL_EDITOR_PIN_TOOL::PlacePin`'s "This position is already occupied by another pin, in unit %d" test: the first pin (not the new
 * one) at the same position on the same body style -- a pin of body style `0` counts for every style.
 */
export function pinOccupying(pin: LibrarySymbolPin, pins: readonly LibrarySymbolPin[]): LibrarySymbolPin | undefined {
  return pins.find((t) => t.id !== pin.id && samePos(t.at, pin.at) && !(t.body_style !== 0 && t.body_style !== pin.body_style));
}

/**
 * `SYMBOL_EDITOR_MOVE_TOOL::Main`: "Pick up any synchronized pins" -- for a moved pin, one pin per other unit that sits at the same
 * position with the same orientation, body style, electrical type and name travels with it.
 */
export function linkedPinsToMove(cur: LibrarySymbolPin, pins: readonly LibrarySymbolPin[], unitCount: number): LibrarySymbolPin[] {
  const gotUnit = new Array<boolean>(unitCount + 1).fill(false);
  gotUnit[cur.unit] = true;
  const out: LibrarySymbolPin[] = [];
  for (const p of pins) {
    if (gotUnit[p.unit]) continue;
    if (p.id !== cur.id && samePos(p.at, cur.at) && sameAngle(p.angle_deg, cur.angle_deg) && p.body_style === cur.body_style && p.electrical_type === cur.electrical_type && p.name === cur.name) {
      out.push(p);
      gotUnit[p.unit] = true;
    }
  }
  return out;
}

export interface SyncedEditPlan {
  /** The other pins, with the edited pin's values copied in. */
  updates: LibrarySymbolPin[];
  /** Pins made redundant by an edit that turned the pin into a shared (unit 0 / body style 0) one. */
  removeIds: string[];
}

/**
 * `SYMBOL_EDITOR_PIN_TOOL::EditPinProperties`' synchronized branch, run after the Pin Properties dialog changed `original` into
 * `edited`: one pin per other unit that matched the pin *before* the edit (same position, orientation, electrical type, visibility
 * and name) follows it -- length (and with it the position) and shape when it is on the same body style, then orientation,
 * type, visibility, name and the two text sizes. A pin that became common to all units (or styles) makes the matching pins of
 * the other units redundant, and they are removed instead. The pin numbers are never synchronized.
 */
export function planSyncedEdit(original: LibrarySymbolPin, edited: LibrarySymbolPin, pins: readonly LibrarySymbolPin[], unitCount: number): SyncedEditPlan {
  const plan: SyncedEditPlan = { updates: [], removeIds: [] };
  // "a pin can have a unit id = 0 (common to all units) to unit count, so we need a buffer size = GetUnitCount()+1"
  const gotUnit = new Array<boolean>(unitCount + 1).fill(false);
  gotUnit[edited.unit] = true;

  for (const other of pins) {
    if (other.id === edited.id) continue;
    // "Only change one pin per unit to allow stacking pins"
    if (gotUnit[other.unit]) continue;
    if (!(samePos(other.at, original.at) && sameAngle(other.angle_deg, original.angle_deg) && other.electrical_type === original.electrical_type && other.hidden === original.hidden && other.name === original.name)) continue;

    let removed = false;
    if (edited.body_style === 0 && (edited.unit === 0 || other.unit === edited.unit)) removed = true;
    let next = other;
    if (other.body_style === edited.body_style) next = { ...changePinLength(next, edited.length_mm), at: edited.at, shape: edited.shape };
    if (edited.unit === 0 && (edited.body_style === 0 || other.body_style === edited.body_style)) removed = true;
    next = { ...next, angle_deg: edited.angle_deg, electrical_type: edited.electrical_type, hidden: edited.hidden, name: edited.name, name_size_mm: edited.name_size_mm, number_size_mm: edited.number_size_mm };

    if (removed) plan.removeIds.push(other.id ?? "");
    else plan.updates.push(next);
    gotUnit[other.unit] = true;
  }
  return plan;
}
