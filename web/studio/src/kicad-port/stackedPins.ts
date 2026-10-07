// `ExpandStackedPinNotation` (common/string_utils.cpp): a pin whose number is `[1,2,5-7]` is several pins stacked on one spot. The
// symbol checker (eeschema/symbol_checker.cpp) judges each of them separately, so it expands first.

/** `ParseAlphaNumericPin`: the letters in front of the trailing number, and that number (-1 when there is none). */
function parseAlphaNumericPin(pin: string): { prefix: string; value: number } {
  let numStart = pin.length;
  for (let i = pin.length - 1; i >= 0; i--) {
    if (!/[0-9]/.test(pin[i]!)) {
      numStart = i + 1;
      break;
    }
    if (i === 0) numStart = 0; // all digits
  }
  if (numStart < pin.length) {
    const prefix = pin.slice(0, numStart);
    const n = Number.parseInt(pin.slice(numStart), 10);
    return { prefix, value: Number.isNaN(n) ? -1 : n };
  }
  return { prefix: "", value: -1 };
}

/**
 * The pin numbers a (possibly stacked) pin number stands for, and whether the notation is valid. A number without brackets is one pin; a
 * bracket on only one end, or a range that does not make sense (`[1-a]`, `[5-3]`, mixed prefixes), is invalid and comes back as the text itself.
 */
export function expandStackedPinNotation(pinName: string): { numbers: string[]; valid: boolean } {
  const hasOpen = pinName.includes("[");
  const hasClose = pinName.includes("]");
  if ((hasOpen || hasClose) && (!pinName.startsWith("[") || !pinName.endsWith("]"))) return { numbers: [pinName], valid: false };
  if (!pinName.startsWith("[") || !pinName.endsWith("]")) return { numbers: [pinName], valid: true };

  const inner = pinName.slice(1, -1);
  const expanded: string[] = [];
  for (const raw of inner.split(",")) {
    const part = raw.trim();
    if (part === "") continue;
    const dash = part.indexOf("-");
    if (dash >= 0) {
      const a = parseAlphaNumericPin(part.slice(0, dash).trim());
      const b = parseAlphaNumericPin(part.slice(dash + 1).trim());
      if (a.prefix !== b.prefix || a.value === -1 || b.value === -1 || a.value > b.value) return { numbers: [pinName], valid: false };
      for (let n = a.value; n <= b.value; n++) expanded.push(`${a.prefix}${n}`);
    } else {
      expanded.push(part);
    }
  }
  if (expanded.length === 0) return { numbers: [pinName], valid: false };
  return { numbers: expanded, valid: true };
}
