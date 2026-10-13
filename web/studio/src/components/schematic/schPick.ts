// What a click, a right click, a double click or a hotkey under the pointer reaches on the sheet: `SCH_SELECTION_TOOL::SelectPoint` with `GuessSelectionCandidates`
// (eeschema/tools/sch_selection_tool.cpp at 8303b2ad), one function for all of them so they agree.
//
// The candidates are tried in the order the studio has always used -- a shown field, then the symbol under the point, then any other placed item (a label, text, power symbol,
// sheet, shape, junction ...), then a wire -- with the pins in front of them (`GuessSelectionCandidates`: an exact hit on a pin makes it "the closest": it wins over the symbol, the
// fields and everything else under the point) and, when nothing else is there, the nearest pin within the pick radius.
import type { Schematic } from "../../api/types";
import { hitField } from "../../kicad-port/schFieldEdit";
import { pickPin } from "../../kicad-port/schPins";
import { measureStrokeText } from "../text/strokeFont";
import { hitItems } from "./schItems";
import { hitSymbol, hitWire } from "./schHit";

/** The accuracy of an "exact" hit: about the width of a pen (6 mils), micrometres. */
const EXACT_UM = 150;

/** Which pins take part: `first` (a click: an exact pin hit wins, a near one is the last resort), `only` (`RequestSelection( { SCH_PIN_T } )`) or `none` (every other tool: a pin is no movable item). */
export type PinPick = "first" | "only" | "none";

/**
 * The id of the item at (`x`, `y`) (micrometres), or null. `scale` is the view's pixels per micrometre: the pick radius is 6 pixels, the same for an item and a wire (a field has 3).
 * `selectable` is the selection filter (`itemPassesFilter`): an item it keeps out is not there to pick.
 */
export function pickItem(sch: Schematic, x: number, y: number, scale: number, selectable: (id: string) => boolean, pins: PinPick = "none"): string | null {
  const tolerance = 6 / (scale || 1);
  const pickable = (id: string | null | undefined): string | null => (id && selectable(id) ? id : null);
  if (pins !== "none") {
    const exact = pickPin(sch, [x, y], tolerance, measureStrokeText, true, selectable);
    if (exact) return exact.id;
    if (pins === "only") return pickPin(sch, [x, y], tolerance, measureStrokeText, false, selectable)?.id ?? null;
  }
  // a shown field (the Reference, the Value, ...) is picked before the symbol whose box it may sit over: its own text box is what a click hits
  const field = pickable(hitField(sch, x, y, tolerance / 2, measureStrokeText));
  if (field) return field;
  const symbol = pickable(hitSymbol(sch, x, y));
  if (symbol) return symbol;
  // `GuessSelectionCandidates`: "Prefer exact hits to sloppy ones" -- an item the point is on, within the width of a pen, is picked before one that is only near it (a sheet's border
  // before the box of a pin of its sheet that the pick radius reaches)
  const isPlaced = (r: { kind: string; id: string }) => r.kind !== "symbol" && r.kind !== "wire" && pickable(r.id);
  const placed = hitItems(sch, x, y, EXACT_UM).find(isPlaced)?.id ?? hitItems(sch, x, y, tolerance).find(isPlaced)?.id ?? null;
  if (placed) return placed;
  const wire = pickable(hitWire(sch, x, y, tolerance));
  if (wire) return wire;
  return pins === "first" ? (pickPin(sch, [x, y], tolerance, measureStrokeText, false, selectable)?.id ?? null) : null;
}
