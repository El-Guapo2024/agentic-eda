// A pin of a placed symbol as an item of the sheet: `SCH_PIN_T` of `SCH_SELECTION_TOOL` (eeschema/tools/sch_selection_tool.cpp, sch_pin.cpp at 8303b2ad).
//
// KiCad lets a pin be hovered, clicked, box-selected and acted on (Swap Pins, Swap Pin Labels, a net highlight that starts from it). A pin has no id of its own
// there -- it belongs to its symbol -- so the studio names it `pin:<field_key>:<number>` (`eda_model::ir::pin_item_id`): the symbol's reference, `#<unit>` after it for
// any unit but the first, then the pin's number. The server reads the same ids (`crates/ops/src/sch_pins.rs`).
//
// What a click on a pin hits is `SCH_PIN::HitTest`: the box of `PIN_LAYOUT_CACHE::GetPinBoundingBox` -- the pin line, its name and its number -- grown by the accuracy (at
// least a quarter of the pin symbol size, 25 mils: `Schematic()->Settings().m_PinSymbolSize / 4`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Schematic, SchematicSymbol } from "../api/types";
import { resolveLibSymbol } from "../components/schematic/libSymbol";
import { pinTextPlacements, PIN_TEXT_PEN_UM, PIN_TEXT_SIZE_UM, type PinTextPlacement } from "../components/schematic/pinText";
import { boxOfPoints, inflateBox, pointInBox, unionBox, type Box, type P } from "./schItemGeom";
import { ELECTRICAL_TYPE_NAMES } from "./symPinText";

export const PIN_ID_PREFIX = "pin:";

/** `field_key`: a placed symbol's reference, with the unit after a `#` for any unit but the first. */
export function symbolKey(reference: string, unit: number): string {
  return unit <= 1 ? reference : `${reference}#${unit}`;
}

/** The id of one pin of a placed symbol as an item of the sheet. */
export function pinItemId(key: string, number: string): string {
  return `${PIN_ID_PREFIX}${key}:${number}`;
}

/** The symbol key and the pin number of a pin id, or null for any other id. The key ends at the first `:` after the prefix (a reference holds none). */
export function parsePinItemId(id: string): { key: string; number: string } | null {
  if (!id.startsWith(PIN_ID_PREFIX)) return null;
  const rest = id.slice(PIN_ID_PREFIX.length);
  const colon = rest.indexOf(":");
  if (colon <= 0 || colon === rest.length - 1) return null;
  return { key: rest.slice(0, colon), number: rest.slice(colon + 1) };
}

export const isPinItemId = (id: string): boolean => parsePinItemId(id) !== null;

/** The reference of a symbol key (`U1#2` is unit 2 of `U1`). */
export function referenceOfKey(key: string): string {
  const hash = key.lastIndexOf("#");
  return hash > 0 && /^\d+$/.test(key.slice(hash + 1)) ? key.slice(0, hash) : key;
}

/** One pin of a placed symbol, where it is on the sheet. */
export interface PinItem {
  id: string;
  /** `symbolKey( reference, unit )`. */
  key: string;
  reference: string;
  unit: number;
  number: string;
  name: string | null;
  electricalType: string;
  /** The pin's connection end (where a wire meets it) and the end at the body, micrometres on the sheet. */
  tip: [number, number];
  root: [number, number];
  /** The net the pin is on, when the sheet says. */
  net: string | null;
  /** `(pin_names ...)` / `(pin_numbers ...)` of the symbol, for the texts the pin writes. */
  texts: { namesHidden: boolean; numbersHidden: boolean; nameOffsetUm: number };
}

const cache = new WeakMap<Schematic, PinItem[]>();

/** Every pin of every placed symbol a click can reach: not a hidden one (`!pin->IsVisible() && !m_frame->GetShowAllPins()`). Built once per sheet object. */
export function pinItems(sch: Schematic): PinItem[] {
  let items = cache.get(sch);
  if (!items) {
    items = [];
    for (const s of sch.symbols) items.push(...pinsOfSymbol(sch, s));
    cache.set(sch, items);
  }
  return items;
}

function pinsOfSymbol(sch: Schematic, s: SchematicSymbol): PinItem[] {
  const real = resolveLibSymbol(s, sch.lib_symbols);
  if (!real) return [];
  const key = symbolKey(s.id, s.unit || 1);
  const nets = new Map((s.pins ?? []).map((p) => [p.number, p.net ?? null]));
  const out: PinItem[] = [];
  for (const r of real.pins) {
    if (r.pin.hidden) continue;
    out.push({
      id: pinItemId(key, r.pin.number),
      key,
      reference: s.id,
      unit: s.unit || 1,
      number: r.pin.number,
      name: r.pin.name,
      electricalType: r.pin.electrical_type,
      tip: r.tip,
      root: r.root,
      net: nets.get(r.pin.number) ?? null,
      texts: real.texts,
    });
  }
  return out;
}

/** The pin with this id, or null. */
export function pinById(sch: Schematic, id: string): PinItem | null {
  const parsed = parsePinItemId(id);
  if (!parsed) return null;
  return pinItems(sch).find((p) => p.key === parsed.key && p.number === parsed.number) ?? null;
}

/** The pins of one placed symbol (every unit of a reference when `unit` is not given). */
export function pinsOf(sch: Schematic, reference: string, unit?: number): PinItem[] {
  return pinItems(sch).filter((p) => p.reference === reference && (unit === undefined || p.unit === unit));
}

/** How wide a text of the given size is, micrometres (`measureStrokeText`, passed in so this module stays free of the canvas font). */
export type Measure = (text: string, sizeUm: number) => number;

/** The box of a pin text: its anchor, justification and whether it runs up (a quarter turn counter-clockwise). */
function textBox(t: PinTextPlacement, sizeUm: number, measure: Measure): Box {
  const w = measure(t.text, sizeUm);
  const h = sizeUm;
  const x0 = t.h === "left" ? 0 : t.h === "center" ? -w / 2 : -w;
  const y0 = t.v === "top" ? 0 : t.v === "center" ? -h / 2 : -h;
  const corners: P[] = [
    [x0, y0],
    [x0 + w, y0],
    [x0 + w, y0 + h],
    [x0, y0 + h],
  ];
  // a quarter turn counter-clockwise on a screen whose y runs down: (x, y) -> (y, -x)
  const at = t.at;
  return boxOfPoints(corners.map(([x, y]): P => (t.vertical ? [at[0] + y, at[1] - x] : [at[0] + x, at[1] + y])))!;
}

/**
 * `SCH_PIN::GetBoundingBox( false, true, ... )` (`PIN_LAYOUT_CACHE::GetPinBoundingBox`): the pin line with the width of its pen, merged with the boxes of its name and its
 * number. (A pin shape's decoration -- the bubble of an inverted pin, the clock triangle -- is inside the box the line and the texts give, within the accuracy.)
 */
export function pinBox(pin: PinItem, measure: Measure): Box {
  const pen = PIN_TEXT_PEN_UM;
  let box: Box | null = inflateBox(boxOfPoints([pin.tip, pin.root])!, pen / 2);
  const texts = pinTextPlacements({ name: pin.name, number: pin.number }, pin.tip, pin.root, pin.texts);
  for (const t of [texts.name, texts.number]) if (t) box = unionBox(box, textBox(t, PIN_TEXT_SIZE_UM, measure));
  return box!;
}

/** `Schematic()->Settings().m_PinSymbolSize / 4`: the least accuracy a pin hit test is given, micrometres (25 mils / 4). */
export const PIN_HIT_FLOOR_UM = 158.75;

/** How far `p` is from the pin's box, micrometres: 0 inside it. */
export function pinDistance(pin: PinItem, p: P, measure: Measure): number {
  const b = pinBox(pin, measure);
  const dx = Math.max(b.minX - p[0], 0, p[0] - b.maxX);
  const dy = Math.max(b.minY - p[1], 0, p[1] - b.maxY);
  return Math.hypot(dx, dy);
}

/** `SCH_PIN::HitTest( aPosition, aAccuracy )`: `aAccuracy` is raised to the floor, so an "exact" hit (accuracy 0) still has some width. */
export function hitPin(pin: PinItem, p: P, accuracyUm: number, measure: Measure): boolean {
  return pointInBox(p, inflateBox(pinBox(pin, measure), Math.max(accuracyUm, PIN_HIT_FLOOR_UM)));
}

/**
 * The pin a click at `p` reaches (`GuessSelectionCandidates` for pins): with `exactOnly`, only a pin whose box holds the point as it is (accuracy 0 -- "exact hits win",
 * and a pin that is one wins over the symbol, fields and everything else under it); without it, the nearest pin within `toleranceUm` (what is left when nothing else is
 * under the pointer). `selectable` is the selection filter.
 */
export function pickPin(sch: Schematic, p: P, toleranceUm: number, measure: Measure, exactOnly: boolean, selectable: (id: string) => boolean = () => true): PinItem | null {
  let best: { pin: PinItem; d: number } | null = null;
  for (const pin of pinItems(sch)) {
    if (!selectable(pin.id)) continue;
    const exact = hitPin(pin, p, 0, measure);
    if (exactOnly ? !exact : !hitPin(pin, p, toleranceUm, measure)) continue;
    // "closest": the one the point is nearest the middle of its line (an exact hit on a name text and on a neighbour's line is ambiguous in KiCad too)
    const mid: P = [(pin.tip[0] + pin.root[0]) / 2, (pin.tip[1] + pin.root[1]) / 2];
    const d = Math.hypot(p[0] - mid[0], p[1] - mid[1]);
    if (!best || d < best.d) best = { pin, d };
  }
  return best?.pin ?? null;
}

/**
 * `SCH_SELECTION_TOOL::SelectMultiple` for pins: they are collected apart and used only when the box found nothing else ("collector = pinsCollector" when the main collector
 * is empty), so a box over a symbol selects the symbol and one over just its pins selects the pins. `pinBoxOverlaps` is the `HitTest( rect, contained )` of each pin.
 */
export function boxPins(sch: Schematic, box: Box, crossing: boolean, measure: Measure): string[] {
  const out: string[] = [];
  for (const pin of pinItems(sch)) {
    const b = pinBox(pin, measure);
    const inside = !(b.minX > box.maxX || b.maxX < box.minX || b.minY > box.maxY || b.maxY < box.minY);
    const contained = b.minX >= box.minX && b.maxX <= box.maxX && b.minY >= box.minY && b.maxY <= box.maxY;
    if (crossing ? inside : contained) out.push(pin.id);
  }
  return out;
}

/** `SCH_PIN::GetItemDescription` as the Properties panel and the status bar show it: `Pin 3 [VIN, Power input, Line]`, with the symbol in front (`Symbol U1 Pin 3 ...`). */
export function describePin(pin: PinItem): string {
  const type = ELECTRICAL_TYPE_NAMES[pin.electricalType as keyof typeof ELECTRICAL_TYPE_NAMES] ?? pin.electricalType;
  const name = pin.name && pin.name !== "~" ? pin.name : "";
  const inner = name ? `${name}, ${type}, Line` : `${type}, Line`;
  return `Symbol ${pin.reference} Pin ${pin.number} [${inner}]`;
}

/** The pins in `ids`, resolved, in the order given (selection order); an id that is no pin of the sheet is left out. */
export function selectedPins(sch: Schematic, ids: Iterable<string>): PinItem[] {
  const out: PinItem[] = [];
  for (const id of ids) {
    const pin = pinById(sch, id);
    if (pin) out.push(pin);
  }
  return out;
}

/**
 * `SCH_EDIT_TOOL::Properties` and `EditField`: "If a single pin is selected, promote to its parent symbol" -- Properties on one selected pin opens its symbol's, and so do the
 * Reference, Value and Footprint hotkeys. Any other selection is as it was.
 */
export function promoteLonePin(ids: readonly string[]): string[] {
  const pin = ids.length === 1 ? parsePinItemId(ids[0]!) : null;
  return pin ? [referenceOfKey(pin.key)] : [...ids];
}
