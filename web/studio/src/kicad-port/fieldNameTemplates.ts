// The field name templates a symbol is offered: `TEMPLATES` (common/template_fieldnames.cpp at KiCad 8303b2ad), the project's (Schematic Setup > General > Field
// Name Templates, `SCHEMATIC_SETTINGS::m_TemplateFieldNames`) and the person's own, the global ones (Preferences > Schematic Editor > Field Name Templates,
// `EESCHEMA_SETTINGS::m_Drawing.field_names`, which `SCH_EDIT_FRAME` adds to the project's manager: `AddTemplateFieldNames( cfg->m_Drawing.field_names )`).
//
//   * `resolveTemplates`: "Flatten project and global templates into a single list. (Project templates take precedence.)": the project's in order, then each
//     global one whose name no project template has. Symbol Properties offers each of them a symbol lacks (`DIALOG_SYMBOL_PROPERTIES::TransferDataToWindow`), and
//     the Symbol Fields Table has a column for each (`DIALOG_SYMBOL_FIELDS_TABLE::LoadFieldNames`).
//   * The Symbol Editor's Symbol Properties offers the global ones alone (`DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataToWindow`: a `TEMPLATES` of
//     `cfg->m_Drawing.field_names` only).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { TemplateFieldName } from "../api/types";
import { keptTemplateFields, whitespaceWarning } from "./schSetup";

/** `TEMPLATES::resolveTemplates` (via `GetTemplateFieldNames()`): the project's templates, then the global ones the project does not override by name. */
export function resolveTemplates(project: readonly TemplateFieldName[], global: readonly TemplateFieldName[]): TemplateFieldName[] {
  return [...project, ...global.filter((g) => !project.some((p) => p.name === g.name))];
}

/**
 * `PANEL_TEMPLATE_FIELDNAMES::TransferDataFromWindow` for the global list: each row with a name that has white space at an end is asked about (`ask` gets the
 * message and answers true for "Remove White Space"), then the rows are kept as `AddTemplateFieldName` keeps them (`keptTemplateFields`).
 */
export async function transferGlobalTemplates(rows: readonly TemplateFieldName[], ask: (message: string) => Promise<boolean>): Promise<TemplateFieldName[]> {
  const out: TemplateFieldName[] = [];
  for (const f of rows) {
    const m = f.name === "" ? null : whitespaceWarning(f.name);
    out.push(m && (await ask(m)) ? { ...f, name: f.name.trim() } : { ...f });
  }
  return keptTemplateFields(out);
}
