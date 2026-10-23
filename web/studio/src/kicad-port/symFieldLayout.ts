// A library symbol's fields on the Symbol Editor's canvas: where each one is drawn, which are drawn and what a click on one picks (`SCH_FIELD` items of a `LIB_SYMBOL`,
// `SCH_PAINTER::draw( SCH_FIELD )`, `SYMBOL_EDITOR_CONTROL::ToggleHiddenFields`).
//
// The fields are the symbol's own list (kicad-port/symFields.ts `allFields`: the five mandatory ones and the user fields, each with its place, size, justification and Show flag).
// A field is drawn where `(property "Name" "text" (at x y angle))` puts it -- the symbol's mm, +y up, turned into the editor's micrometres, +y down -- at its size, justified
// against that anchor in its own axes, a text at 90 degrees running up the sheet; `Name: text` when its name is shown. A field that is not shown (`(hide yes)`) is drawn, in the
// hidden-items colour, only while Show Hidden Fields is on -- which it starts on in the Symbol Editor (`m_ShowHiddenFields` of `SYMBOL_EDITOR_SETTINGS` defaults to true) -- and a field
// with no text is not drawn at all (`SCH_FIELD::HitTest`: "if( GetShownText( true ).IsEmpty() ) return false"). The Reference of a symbol with several units is drawn with the unit's letter
// (`SCH_FIELD::GetFullText( unit )`, `LIB_SYMBOL::SubReference`).
//
// A field is named for the canvas, the selection and the move by the id `fld:lib:<name>` (the sheet's `fld:<owner>:<name>`, kicad-port/schFieldEdit.ts).
// Pure: no React, no DOM (compiled by `npm run test:unit`); the text measure is the caller's.
import type { SchField } from "../api/types";
import { fieldBox, fieldId, parseFieldId, shownText, type FieldBox, type Measure } from "./schFieldEdit";
import type { FieldRow } from "./symFields";

/** The owner part of a library field's id. */
export const LIB_FIELD_OWNER = "lib";

export const libFieldId = (name: string): string => fieldId(LIB_FIELD_OWNER, name);

/** The field name a library field id stands for, or null for any other id. */
export function libFieldName(id: string): string | null {
  const p = parseFieldId(id);
  return p && p.owner === LIB_FIELD_OWNER ? p.name : null;
}

/** `LIB_SYMBOL::SubReference`: the letter of a unit -- A for the first, ... Z, then AA. */
export function unitLetters(unit: number): string {
  let n = Math.max(1, Math.round(unit));
  let out = "";
  while (n > 0) {
    out = String.fromCharCode(65 + ((n - 1) % 26)) + out;
    n = Math.floor((n - 1) / 26);
  }
  return out;
}

/** The field as the sheet's field type (the painter and the boxes of schFieldEdit read it): micrometres, +y down. `unitCount` > 1 puts the unit's letter after the Reference. */
export function schFieldOf(f: FieldRow, unit = 1, unitCount = 1): SchField {
  const text = f.name === "Reference" && unitCount > 1 ? f.text + unitLetters(unit) : f.text;
  return {
    name: f.name,
    text,
    at: [f.at.x * 1e6, -f.at.y * 1e6],
    vertical: Math.abs(f.angle_deg % 180) === 90,
    h: f.h_align,
    v: f.v_align,
    visible: !f.hidden,
    id: libFieldId(f.name),
    size_nm: f.size_mm * 1e6,
    bold: f.bold,
    italic: f.italic,
    name_shown: f.show_name,
  };
}

/** The fields to draw and to pick: the ones with a text, and the hidden ones only while Show Hidden Fields is on. */
export function fieldsOnShow(rows: readonly FieldRow[], showHidden: boolean, unit = 1, unitCount = 1): SchField[] {
  return rows.filter((f) => f.text.length > 0 && (!f.hidden || showHidden)).map((f) => schFieldOf(f, unit, unitCount));
}

/** The text a field draws. */
export const fieldShownText = (f: SchField): string => shownText(f);

/** The field under the point, within `tolNm` of its box; of several the one with the smallest box. Null when none. */
export function hitLibField(fields: readonly SchField[], x: number, y: number, tolNm: number, measure: Measure): string | null {
  let best: { id: string; area: number } | null = null;
  for (const f of fields) {
    const b = fieldBox(f, measure);
    if (x < b.minX - tolNm || x > b.maxX + tolNm || y < b.minY - tolNm || y > b.maxY + tolNm) continue;
    const area = (b.maxX - b.minX) * (b.maxY - b.minY);
    if (!best || area < best.area) best = { id: f.id!, area };
  }
  return best?.id ?? null;
}

/** The box a field takes on the canvas. */
export const libFieldBox = (f: SchField, measure: Measure): FieldBox => fieldBox(f, measure);

/** The rows with the fields named in `names` moved by (`dxMm`, `dyMm`), in the symbol's own frame (+y up): what a drag of fields commits. */
export function movedRows(rows: readonly FieldRow[], names: ReadonlySet<string>, dxMm: number, dyMm: number): FieldRow[] {
  return rows.map((f) => (names.has(f.name) ? { ...f, at: { x: round(f.at.x + dxMm), y: round(f.at.y + dyMm) } } : f));
}

/** Millimetres to the 0.1 um the file keeps. */
const round = (mm: number): number => Math.round(mm * 10_000) / 10_000;
