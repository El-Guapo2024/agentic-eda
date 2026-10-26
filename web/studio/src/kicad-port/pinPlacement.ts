// `SYMBOL_EDITOR_PIN_TOOL::PlacePin`'s Synchronized Pins Mode confirmation: with the mode on, a pin dropped on top of another one
// (a body style that matches) asks first -- "This position is already occupied by another pin, in unit %d." -- and only a "Place Pin
// Anyway" places it. In KiCad this runs after `CreatePin` has taken the new pin's properties in Pin Properties; the studio's pin
// dialog calls the same test when its OK is pressed, so the pin is only added after both. Pure: `pinOccupying` has the matching rule.
import type { LibrarySymbolPin } from "../api/types";
import { pinOccupying } from "./symPinSync";

/** The pin whose click the dialog's OK would stack on, or null when nothing is asked: the mode must be on (see
 * `synchronizePins`) and another pin must already sit at that position on a matching body style. */
export function pinPlacementOccupant(pin: LibrarySymbolPin, pins: readonly LibrarySymbolPin[], syncMode: boolean): LibrarySymbolPin | null {
  if (!syncMode) return null;
  return pinOccupying(pin, pins) ?? null;
}

/** `KIDIALOG`'s message and extended message, as one text (the studio's confirm dialogs have a single message line). */
export function pinOccupiedMessage(unit: number): string {
  return `This position is already occupied by another pin, in unit ${unit}.\n\nDisable the 'Synchronized Pins Mode' option to avoid this message.`;
}
