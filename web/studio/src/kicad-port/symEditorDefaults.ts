// What the Symbol Editor's Defaults for New Objects decide (`SYMBOL_EDITOR_SETTINGS::m_Defaults`, Preferences > Symbol Editor > Editing Options,
// eeschema/dialogs/panel_sym_editing_options.cpp at KiCad 8303b2ad), and the global field name templates its Symbol Properties offers (UI-LEFT-3 C3-2):
//
//   * a new pin: `SYMBOL_EDITOR_PIN_TOOL`'s `GetLastPinLength` / `GetLastPinNameSize` / `GetLastPinNumSize` take `pin_length`, `pin_name_size` and `pin_num_size` the
//     first time a pin is made ("The -1 is a non-valid value to trigger delayed initialization"); from then on the last pin's (Pin Properties' OK);
//   * a new text (`SCH_TEXT_T` of `TwoClickPlace`), a new text box (`doDrawShape`, `isTextBox`) and a field Symbol Properties adds (`OnAddField`): `text_size`. A text
//     box keeps the margins its constructor gave it from the default 50 mil (`SCH_TEXTBOX::SCH_TEXTBOX`, before `SetTextSize`);
//   * a shape's border switched on in Shape Properties (`DIALOG_SHAPE_PROPERTIES::onBorderChecked`): `line_width` when the width was "no border". A new shape itself takes
//     the tool's last stroke (`item->SetStroke( m_lastStroke )` after the constructor's `lineWidth`), so `line_width` sets no new shape's width in 8303b2ad;
//   * Symbol Properties (`DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataToWindow`): a row for each global template (`cfg->m_Drawing.field_names`) the symbol lacks, shown as
//     the template says; OK leaves out such a row still empty (`m_addedTemplateFields`: "Skip empty fields that are not mandatory or template fields").
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { TemplateFieldName } from "../api/types";
import type { Preferences } from "./preferences";
import { DEFAULT_PIN_TEMPLATE, type PinTemplate } from "./symPinTemplate";
import { newUserField, type GridRow } from "./symFieldsGrid";

/** One mil, millimetres (`schIUScale.MilsToIU`). */
export const MIL_MM = 0.0254;

type Defaults = Pick<Preferences, "symDefaultLineWidthMils" | "symDefaultTextSizeMils" | "symDefaultPinLengthMils" | "symDefaultPinNumSizeMils" | "symDefaultPinNameSizeMils">;

const mm = (mils: number) => Math.round(mils * MIL_MM * 1e6) / 1e6;

/**
 * The template the next pin starts with: while no pin has been made or edited yet (`template` is still the store's starting `DEFAULT_PIN_TEMPLATE`, KiCad's -1), its length
 * and its name and number sizes come from the defaults (`GetLastPinLength` ...); after that the template is the last pin's.
 */
export function pinTemplateWithDefaults(template: PinTemplate, d: Defaults): PinTemplate {
  if (template !== DEFAULT_PIN_TEMPLATE) return template;
  return { ...template, length_mm: mm(d.symDefaultPinLengthMils), name_size_mm: mm(d.symDefaultPinNameSizeMils), number_size_mm: mm(d.symDefaultPinNumSizeMils) };
}

/** `text->SetTextSize( MilsToIU( cfg->m_Defaults.text_size ) )`: a new text's, text box's or field's size, mm. */
export function newTextSizeMm(d: Pick<Defaults, "symDefaultTextSizeMils">): number {
  return mm(d.symDefaultTextSizeMils);
}

/** `onBorderChecked`: the width the border takes when it is switched on from "no border" (`m_borderWidth.GetValue() < 0`), mm. */
export function borderOnWidthMm(d: Pick<Defaults, "symDefaultLineWidthMils">): number {
  return mm(d.symDefaultLineWidthMils);
}

/** The rows `TransferDataToWindow` adds for the global templates the symbol's rows lack (`defined.count( name ) <= 0`), and the names of those templates. */
export function globalTemplateRows(rows: readonly GridRow[], global: readonly TemplateFieldName[], nextKey: () => number): { rows: GridRow[]; added: Set<string> } {
  const defined = new Set(rows.map((r) => r.name));
  const out: GridRow[] = [];
  const added = new Set<string>();
  for (const t of global) {
    if (defined.has(t.name) || added.has(t.name)) continue;
    // `SCH_FIELD field( m_libEntry, FIELD_T::USER, name ); field.SetVisible( m_Visible )`: the default text size, not the editor's default
    out.push({ ...newUserField(0, nextKey()), name: t.name, hidden: !t.visible });
    added.add(t.name);
  }
  return { rows: out, added };
}

/** OK: a template row still empty is not kept (`if( fieldName.IsEmpty() || m_addedTemplateFields.contains( fieldName ) ) continue;`). */
export function withoutEmptyTemplateRows<T extends { name: string; text: string }>(rows: readonly T[], added: ReadonlySet<string>): T[] {
  return rows.filter((r) => !(r.text === "" && added.has(r.name)));
}
