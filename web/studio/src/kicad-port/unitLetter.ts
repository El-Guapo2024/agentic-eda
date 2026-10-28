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

/** The project's unit naming (`SCHEMATIC_SETTINGS::m_SubpartIdSeparator` / `m_SubpartFirstId`, the `.kicad_pro`'s `schematic.subpart_id_separator` and
 *  `schematic.subpart_first_id`): character codes. Absent: KiCad's defaults, no separator (0) and 'A' (65). */
export interface SubpartIds {
  separator?: number | null;
  firstId?: number | null;
}

/** `LIB_SYMBOL::LetterSubReference( aUnit, aInitialLetter )` (lib_symbol.cpp): base 26 from the initial letter, so 'a' gives lower case. */
export function letterSubReference(unit: number, initial = 65): string {
  let n = Math.trunc(unit);
  let out = "";
  do {
    const u = (n - 1) % 26;
    out = String.fromCharCode(initial + u) + out;
    n = Math.trunc((n - u) / 26);
  } while (n > 0);
  return out;
}

/**
 * `SCH_SYMBOL::SubReference( aUnit, aAddSeparator )` over `SCHEMATIC_SETTINGS::SubReference` (KiCad 25ec2335, sch_symbol.cpp and schematic_settings.cpp):
 * nothing for a unit below 1; the separator first (when the project has one and `addSeparator`); then the unit's display name when the library symbol gives it
 * one, else the unit's number when the first id is a digit ('0'..'9'), else its letters from the first id.
 */
export function subReference(unit: number, ids: SubpartIds = {}, displayName?: string | null, addSeparator = true): string {
  const sep = ids.separator ?? 0;
  const first = ids.firstId ?? 65;
  const prefix = sep !== 0 && addSeparator ? String.fromCharCode(sep) : "";
  if (displayName != null) return prefix + displayName;
  if (unit < 1) return "";
  if (first >= 48 && first <= 57) return prefix + String(unit);
  return prefix + letterSubReference(unit, first);
}
