// Port of `FOOTPRINT::GetNextPadNumber` (pcbnew/footprint.cpp:3357) and of how
// pad_tool.cpp's `PAD_PLACER` and edit_tool.cpp's Duplicate and Increment use it:
// the prefix (`GetRefDesPrefix`) and trailing integer (`GetTrailingInt`) of the
// tool's last pad number, then the integer counted up from there until
// `prefix + integer` is a number no pad has. The last number is "1" each time a
// footprint is loaded (`PAD_TOOL::Reset( MODEL_RELOAD )`) and then the number
// of each pad placed or duplicated with increment.
//
// Mirrors `crates/model/src/ir.rs`'s `LibraryFootprint::next_pad_number_after`.
import type { LibraryPad } from "../api/types";

/** `UTIL::GetRefDesPrefix` (common/refdes_utils.cpp): what is left of `s` once its trailing digits and `?` are taken off. */
function refDesPrefix(s: string): string {
  let i = s.length;
  while (i > 0 && (s[i - 1] === "?" || (s[i - 1]! >= "0" && s[i - 1]! <= "9"))) i--;
  return s.slice(0, i);
}

/** `GetTrailingInt` (common/string_utils.cpp): the number the trailing digits of `s` make, 0 when there are none. */
function trailingInt(s: string): number {
  let i = s.length;
  while (i > 0 && s[i - 1]! >= "0" && s[i - 1]! <= "9") i--;
  return i < s.length ? parseInt(s.slice(i), 10) : 0;
}

/** `FOOTPRINT::GetNextPadNumber( aLastPadNumber )`: from the prefix and the trailing number of `last`, the first `prefix + n` (`%s%d`) that no pad of `pads` has -- `last`
 * itself when it is free (the pad tool's last number starts as "1", so a footprint whose pad 1 is gone gets it back), else the next free one. */
export function nextPadNumberAfter(pads: ReadonlyArray<Pick<LibraryPad, "number">>, last: string): string {
  const prefix = refDesPrefix(last);
  const used = new Set(pads.map((p) => p.number));
  let num = trailingInt(last);
  while (used.has(`${prefix}${num}`)) num++;
  return `${prefix}${num}`;
}

/** `PAD_TOOL::m_lastPadNumber` when the editor has none yet: `PAD_TOOL::Reset( MODEL_RELOAD )` sets it to "1" each time a footprint is loaded. */
export const FIRST_LAST_PAD_NUMBER = "1";

/** The number the pad tool gives the next pad (`PAD_PLACER::CreateItem`): `GetNextPadNumber( m_lastPadNumber )`, the last number being "1" until a pad is placed. */
export function nextPadNumber(pads: ReadonlyArray<Pick<LibraryPad, "number">>, lastPadNumber: string | null = null): string {
  return nextPadNumberAfter(pads, lastPadNumber ?? FIRST_LAST_PAD_NUMBER);
}
