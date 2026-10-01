// Port of `FOOTPRINT::GetNextPadNumber` (pcbnew/footprint.cpp:3357,
// pad_tool.cpp's `PAD_PLACER`): split a pad number into its non-numeric
// prefix and trailing integer, then increment the integer until
// `prefix+integer` collides with no existing pad number on the footprint
// -- not just "+1", so a manually-renumbered or out-of-order footprint
// still gets a sane next number, same as source.
//
// Mirrors `crates/model/src/ir.rs`'s `LibraryFootprint::
// next_pad_number_after`/`next_pad_number` exactly (same algorithm, same
// "ASCII digits only" simplification for a non-numeric trailing run) so
// this client-side "what number will the next pad get" preview (shown
// while the Pad tool is armed, before the `add_pad` Cmd round-trip ever
// reaches the backend) can never disagree with the backend's own
// authoritative answer.
import type { LibraryPad } from "../api/types";

/** `s`'s non-numeric prefix and trailing integer (0 if there is none). */
function splitTrailingNumber(s: string): { prefix: string; num: number } {
  const m = /^(.*?)(\d*)$/.exec(s);
  const prefix = m?.[1] ?? s;
  const num = m?.[2] ? parseInt(m[2], 10) : 0;
  return { prefix, num };
}

/** `FOOTPRINT::GetNextPadNumber(last)` -- the first `prefix+integer` after `last` not already used by `pads`. */
export function nextPadNumberAfter(pads: ReadonlyArray<Pick<LibraryPad, "number">>, last: string): string {
  const { prefix, num: startNum } = splitTrailingNumber(last);
  const used = new Set(pads.map((p) => p.number));
  let num = startNum;
  let candidate: string;
  do {
    num += 1;
    candidate = `${prefix}${num}`;
  } while (used.has(candidate));
  return candidate;
}

/**
 * `nextPadNumberAfter`, seeded from `pads`' own highest-numbered entry
 * (this editor has no standing "last placed" session state to carry
 * between placements the way KiCad's interactive tool does -- see the
 * Rust port's own doc). `"1"` for an empty footprint.
 */
export function nextPadNumber(pads: ReadonlyArray<Pick<LibraryPad, "number">>): string {
  let bestLast = "0";
  let bestNum = -1;
  for (const p of pads) {
    const { num } = splitTrailingNumber(p.number);
    if (num > bestNum) {
      bestNum = num;
      bestLast = p.number;
    }
  }
  return nextPadNumberAfter(pads, bestLast);
}
