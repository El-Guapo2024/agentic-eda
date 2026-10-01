// `LIB_SYMBOL::LetterSubReference` (lib_symbol.cpp), ported directly: a
// multi-unit part's unit index -> the letter suffix shown next to its
// reference designator ("U1" unit 1 -> "U1A", unit 2 -> "U1B", ...).
// Base-26, spreadsheet-column style: 1 -> "A", 26 -> "Z", 27 -> "AA". KiCad
// supports a configurable separator/first-letter (`SCHEMATIC_SETTINGS::
// SubReference`); this app has no settings surface for that, so it always
// uses the real default (no separator, 'A').
export function unitLetter(unit: number): string {
  let n = unit;
  let out = "";
  while (n > 0) {
    const rem = (n - 1) % 26;
    out = String.fromCharCode(65 + rem) + out;
    n = Math.floor((n - 1) / 26);
  }
  return out || "A";
}
