// `common/increment.cpp::IncrementString`, the exact function
// `SCH_LABEL_BASE::IncrementLabel` calls (sch_label.cpp) for both the
// "Repeat last item" (Insert) feature and, per `sch_drawing_tools.cpp`'s
// own `TwoClickPlace`/`createNewLabel` comments, a chained label
// placement's starting suggestion -- a label ending in a number (DATA0,
// D3, ...) increments that number on the next one; a label with no
// trailing number (RESET, CLK) is left completely unchanged, matching
// source exactly (`if (digits.IsEmpty()) return true;` -- "succeeded",
// with zero mutation). Ported line-for-line, not just in spirit: the
// zero-padding (`"%0<n>ld"`) and "don't go below zero" (silently
// unchanged, not clamped to 0) behaviors are easy to get subtly wrong by
// re-deriving them from a description instead of the source's own logic.

/**
 * `aIncrement` defaults to 1 (every real caller in source does too --
 * `IncrementLabel`'s own default argument, and `TwoClickPlace`'s chained
 * placement always advances by exactly one). Returns `name` itself,
 * unchanged, when there's no trailing digit run to increment, or when
 * doing so would go negative -- never throws, matching source's own
 * `bool` success flag folded into "did anything change" here instead
 * (this app has no separate caller that needs to distinguish "no digits"
 * from "digits, but would go negative"; both just mean "nothing to do").
 */
export function incrementLabelText(name: string, increment = 1): string {
  if (name.length === 0) return name;

  let i = name.length - 1;
  let suffix = "";
  while (i >= 0 && !isDigit(name[i]!)) {
    suffix = name[i] + suffix;
    i--;
  }

  let digits = "";
  while (i >= 0 && isDigit(name[i]!)) {
    digits = name[i] + digits;
    i--;
  }

  if (digits.length === 0) return name;

  const number = Number.parseInt(digits, 10) + increment;
  if (number < 0) return name;

  const prefix = name.slice(0, i + 1);
  const padded = String(number).padStart(digits.length, "0");
  return prefix + padded + suffix;
}

function isDigit(ch: string): boolean {
  return ch >= "0" && ch <= "9";
}
