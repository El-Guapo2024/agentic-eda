// Port of `SYMBOL_EDITOR_PIN_TOOL`'s own pin auto-increment (the "Repeat"
// behavior while placing pins in sequence: a chained pin's number advances
// by `cfg->m_Repeat.label_delta`, default 1): split a pin number into its
// non-numeric prefix and trailing integer, then increment the integer
// until `prefix+integer` collides with no existing pin number on the
// symbol -- not just "+1", so a manually-renumbered or out-of-order
// symbol still gets a sane next number.
//
// Mirrors `crates/model/src/ir.rs`'s `LibrarySymbol::next_pin_number_after`/
// `next_pin_number` exactly (same algorithm, same "ASCII digits only"
// simplification for a non-numeric trailing run), and
// `kicad-port/padNumbering.ts`'s own identical shape for the Footprint
// Editor's pads -- so this client-side "what number will the next pin
// get" preview (shown while the Pin tool is armed, before the
// `add_symbol_pin` Cmd round-trip ever reaches the backend) can never
// disagree with the backend's own authoritative answer.
import type { LibrarySymbolPin } from "../api/types";

/** `s`'s non-numeric prefix and trailing integer (0 if there is none). */
function splitTrailingNumber(s: string): { prefix: string; num: number } {
  const m = /^(.*?)(\d*)$/.exec(s);
  const prefix = m?.[1] ?? s;
  const num = m?.[2] ? parseInt(m[2], 10) : 0;
  return { prefix, num };
}

/** The first `prefix+integer` after `last` not already used by `pins`. */
export function nextPinNumberAfter(pins: ReadonlyArray<Pick<LibrarySymbolPin, "number">>, last: string): string {
  const { prefix, num: startNum } = splitTrailingNumber(last);
  const used = new Set(pins.map((p) => p.number));
  let num = startNum;
  let candidate: string;
  do {
    num += 1;
    candidate = `${prefix}${num}`;
  } while (used.has(candidate));
  return candidate;
}

/**
 * `nextPinNumberAfter`, seeded from `pins`' own highest-numbered entry
 * (this editor has no standing "last placed" session state to carry
 * between placements the way KiCad's interactive tool does -- see the
 * Rust port's own doc). `"1"` for a symbol with no pins yet.
 */
export function nextPinNumber(pins: ReadonlyArray<Pick<LibrarySymbolPin, "number">>): string {
  let bestLast = "0";
  let bestNum = -1;
  for (const p of pins) {
    const { num } = splitTrailingNumber(p.number);
    if (num > bestNum) {
      bestNum = num;
      bestLast = p.number;
    }
  }
  return nextPinNumberAfter(pins, bestLast);
}
