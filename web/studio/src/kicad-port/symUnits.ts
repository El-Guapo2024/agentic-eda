// Library Symbol Properties' questions before it changes the units and body styles (`DIALOG_LIB_SYMBOL_PROPERTIES::Validate`, "Confirm destructive
// actions."): fewer units than the symbol has deletes the extra units' items, and a single body style on a De Morgan symbol deletes body style 2
// (`LIB_SYMBOL::SetUnitCount( n, true )`, `SetBodyStyleCount`, done by `edit_symbol_properties`, crates/ops/src/library_editors.rs).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

import { bodyStyleCount, type BodyStyled } from "./symBodyStyles";

/** The questions, word for word and in `Validate`'s order, each an `IsOK` (Yes / No; No leaves the dialog open). A derived symbol's units and body
 * styles are its parent's (`GetUnitCount` of the root), and its dialog does not show them: no question. The body styles are counted as
 * `GetBodyStyleCount` counts them (De Morgan 2, else one per custom name), the dialog's as `Validate` does (`bodyStyleCount` of what OK will set). */
export function symbolPropertiesQuestions(before: { unit_count: number } & BodyStyled, after: { unit_count: number } & BodyStyled, derived: boolean): string[] {
  if (derived) return [];
  const out: string[] = [];
  if (after.unit_count < before.unit_count) out.push("Delete extra units from symbol?");
  if (bodyStyleCount(after) < bodyStyleCount(before)) out.push("Delete extra body styles from symbol?");
  return out;
}
