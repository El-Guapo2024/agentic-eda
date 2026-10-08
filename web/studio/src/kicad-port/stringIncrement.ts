// `STRING_INCREMENTER` (common/increment.cpp at 8303b2ad): what `eeschema.Interactive.increment*` (SCH_TOOL_BASE::Increment) does to the text
// of a label, a text or a pin. The string is cut from the right into numbers, runs of one-case letters and skippable punctuation; the n-th
// number-or-letters part from the right (`rightIndex`: 0 is the primary part, 1 the secondary) goes up or down by `delta`.
//
//   "DATA7" +1 -> "DATA8"      "A9" +1 -> "A10"      "R007" +1 -> "R008"      "R007" -1 -> "R006"
//   "ROW_B3" index 1 +1 -> "ROW_C3"      "BA1" index 1 +1 -> "BB1"      "D0" -1 -> unchanged (a number never goes below 0)
//
// `incrementLabelText.ts` is the older, simpler `IncrementString` the Repeat tools use; this is the fuller one the Increment actions use.

const FULL_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/** Letters a BGA-style row name skips (`alphaNoIOSQXZ`) -- the symbol editor's pin numbers. */
const NO_IOSQXZ_ALPHABET = "ABCDEFGHJKLMNPRTUVWY";

export interface IncrementOptions {
  /** `SetSkipIOSQXZ`: skip I, O, S, Q, X, Z in alphabetic parts (the symbol editor does, the schematic does not). */
  skipIOSQXZ?: boolean;
  /** `SetAlphabeticMaxIndex`: a letter part above this index ("TX", "CAN") is left alone; a negative value means no limit. Default 50. */
  alphabeticMaxIndex?: number;
}

/** `IndexFromAlphabetic`: "A" is 0, "Z" 25, "AA" 26 ... -1 when a letter is not in the alphabet. */
export function indexFromAlphabetic(str: string, alphabet: string): number {
  let index = 0;
  const radix = alphabet.length;
  for (let i = 0; i < str.length; i++) {
    let alphaIndex = alphabet.indexOf(str[i]!);
    if (alphaIndex < 0) return -1;
    if (i !== str.length - 1) alphaIndex++;
    index += alphaIndex * Math.pow(radix, str.length - 1 - i);
  }
  return index;
}

/** `AlphabeticFromIndex( n, alphabet, true )`: the inverse of `indexFromAlphabetic`. */
export function alphabeticFromIndex(n: number, alphabet: string, zeroBasedNonUnitCols = true): string {
  const radix = alphabet.length;
  let out = "";
  let first = true;
  let rest = n;
  do {
    let modN = rest % radix;
    if (zeroBasedNonUnitCols && !first) modN--; // start the "tens/hundreds/etc column" at "Ax", not "Bx"
    if (modN < 0) return out; // the C++ would index before the alphabet here; no valid string gets this far
    out = alphabet[modN]! + out;
    rest = Math.floor(rest / radix);
    first = false;
  } while (rest > 0);
  return out;
}

type PartType = "integer" | "alphabetic" | "skip";

function incrementPart(part: string, type: PartType, delta: number, opts: Required<IncrementOptions>): string | null {
  if (type === "integer") {
    const zeroPadded = part.startsWith("0");
    const oldLen = part.length;
    const value = Number.parseInt(part, 10);
    if (!Number.isSafeInteger(value)) return null; // `ToLong` failed
    const next = value + delta;
    if (next < 0) return null; // going below zero makes things awkward and is not usually that useful
    let out = String(next);
    if (zeroPadded) out = "0".repeat(Math.max(0, oldLen - out.length)) + out;
    return out;
  }
  if (type === "alphabetic") {
    const upper = part.toUpperCase();
    const wasUpper = part === upper;
    // `containsIOSQXZ` looks for the capitals only, so a lowercase "x" still uses the short alphabet (and is then not found in it)
    const alphabet = opts.skipIOSQXZ && ![...part].some((c) => "IOSQXZ".includes(c)) ? NO_IOSQXZ_ALPHABET : FULL_ALPHABET;
    let index = indexFromAlphabetic(upper, alphabet);
    if (index === -1) return null;
    if (index > opts.alphabeticMaxIndex && opts.alphabeticMaxIndex >= 0) return null; // too big to be worth incrementing
    index += delta;
    if (index < 0) return null;
    const next = alphabeticFromIndex(index, alphabet, true);
    return wasUpper ? next : next.toLowerCase();
  }
  return null;
}

/**
 * `STRING_INCREMENTER::Increment`: the string with its `rightIndex`-th incrementable part (counting from the right, 0 first) moved by
 * `delta`, or `null` when there is no such part or it cannot move (an empty string, a number that would go below 0, a letter run that is
 * not in the alphabet or is beyond `alphabeticMaxIndex`).
 */
export function incrementString(str: string, delta: number, rightIndex: number, options: IncrementOptions = {}): string | null {
  if (str === "") return null;
  const opts: Required<IncrementOptions> = { skipIOSQXZ: options.skipIOSQXZ ?? false, alphabeticMaxIndex: options.alphabeticMaxIndex ?? 50 };

  let remaining = str;
  const parts: { text: string; type: PartType }[] = [];
  let goodParts = 0;
  // Keep popping chunks off the string until we have what we need
  while (goodParts < rightIndex + 1 && remaining !== "") {
    let m = /[0-9]+$/.exec(remaining);
    if (m) {
      parts.push({ text: m[0], type: "integer" });
      remaining = remaining.slice(0, remaining.length - m[0].length);
      goodParts++;
      continue;
    }
    // ABC or abc but not Abc
    m = /([a-z]+|[A-Z]+)$/.exec(remaining);
    if (m) {
      parts.push({ text: m[0], type: "alphabetic" });
      remaining = remaining.slice(0, remaining.length - m[0].length);
      goodParts++;
      continue;
    }
    // Skippables - for now anything that isn't a letter or number
    m = /[^a-zA-Z0-9]+$/.exec(remaining);
    if (m) {
      parts.push({ text: m[0], type: "skip" });
      remaining = remaining.slice(0, remaining.length - m[0].length);
      continue;
    }
    break; // out of ideas
  }

  if (goodParts < rightIndex + 1) return null; // couldn't find the part we wanted
  const target = parts[parts.length - 1]!;
  const next = incrementPart(target.text, target.type, delta, opts);
  if (next === null) return null;
  target.text = next;
  // Reassemble the string - the left-over part, then the parts in reverse
  return remaining + parts.reverse().map((p) => p.text).join("");
}
