// Design variants (a KiCad 10 feature, ported from KiCad 25ec2335): the variant names as the UI lists them, the checks a new name passes, the toolbar
// selector's entries, and what a placed symbol shows in a variant -- its Do not Populate and exclusion flags and its field texts. Pure, so it is tested
// (designVariants.test.ts); the dialogs are components/DesignVariantDialogs.tsx, the selector components/toolbar/VariantSelector.tsx.
//
// KiCad keeps the variants as per-instance records on each symbol (`SCH_SYMBOL_INSTANCE::m_Variants`, a `SCH_SYMBOL_VARIANT` per variant name:
// `eeschema/sch_sheet_path.h`) and the list of variants with their descriptions in the project file (`SCHEMATIC_SETTINGS::m_VariantDescriptions`,
// `schematic.variants`). The default variant has no name of its own: "< Default >" is only a placeholder for the UI controls
// (`common/string_utils.cpp` `defaultVariantName`, not translated), and an empty name means the default everywhere else.
import type { DesignVariantInfo, Schematic, SchematicSymbol, Sheet, SymbolVariantInfo } from "../api/types";
import { strNumCmp } from "./sheetPages";

/** `GetDefaultVariantName()`: the placeholder the variant controls list first (`wxT( "< Default >" )`, never translated). */
export const DEFAULT_VARIANT_NAME = "< Default >";

/** `UpdateVariantSelectionCtrl`: the separator and the entry that adds a variant, after the names. */
export const VARIANT_SEPARATOR = "---";
export const ADD_VARIANT_ENTRY = "Add New Design Variant...";

/** The tooltip of the variant selector of both editors (`m_currentVariantCtrl->SetToolTip`, toolbars_sch_editor.cpp; `m_CurrentVariantCtrl`, toolbars_pcb_editor.cpp). */
export const VARIANT_SELECTOR_TIP = "Select the current variant to display and edit.";

/** `SortVariantNames` (common/string_utils.cpp): the default placeholder first, the rest by `StrNumCmp`. */
export function sortVariantNames(a: string, b: string): number {
  if (a === DEFAULT_VARIANT_NAME && b !== DEFAULT_VARIANT_NAME) return -1;
  if (a !== DEFAULT_VARIANT_NAME && b === DEFAULT_VARIANT_NAME) return 1;
  return strNumCmp(a, b);
}

/** The names of `variants` (a list of names or of `{ name }` records). */
function namesOf(variants: ReadonlyArray<string | { name: string }> | null | undefined): string[] {
  return (variants ?? []).map((v) => (typeof v === "string" ? v : v.name));
}

/** `SCHEMATIC::GetVariantNamesForUI` (and `BOARD::GetVariantNamesForUI`): the default placeholder, then every variant, sorted by `SortVariantNames`. */
export function variantNamesForUI(variants: ReadonlyArray<string | { name: string }> | null | undefined): string[] {
  return [DEFAULT_VARIANT_NAME, ...namesOf(variants)].sort(sortVariantNames);
}

/**
 * `SCHEMATIC::SetCurrentVariant`: the variant asked for when the schematic has it (exactly that name) and it is not the default placeholder, else the default
 * (an empty name). Used whenever the list of variants changes, so a variant that was removed, renamed or undone falls back to the default.
 */
export function resolveCurrentVariant(variants: ReadonlyArray<string | { name: string }> | null | undefined, name: string): string {
  if (name === "" || name === DEFAULT_VARIANT_NAME) return "";
  return namesOf(variants).includes(name) ? name : "";
}

/** `wxString::CmpNoCase( .. ) == 0`. */
const sameNoCase = (a: string, b: string): boolean => a.toLowerCase() === b.toLowerCase();

/**
 * `SCH_EDIT_FRAME::ValidateNewVariantName( aName, aExcludeName )`: the message the info bar shows when `name` (already trimmed) cannot be a new variant, else null.
 * A name is compared with the others without regard to case; `exclude` is the variant being renamed, which may keep its own name in another case.
 */
export function validateNewVariantName(existing: ReadonlyArray<string | { name: string }> | null | undefined, name: string, exclude = ""): string | null {
  if (name === "") return "Variant name cannot be empty.";
  if (sameNoCase(name, DEFAULT_VARIANT_NAME)) return `'${DEFAULT_VARIANT_NAME}' is a reserved variant name.`;
  for (const other of namesOf(existing)) {
    if (sameNoCase(other, name) && !sameNoCase(other, exclude)) return `Variant '${other}' already exists.`;
  }
  return null;
}

/**
 * `DIALOG_SYMBOL_FIELDS_TABLE::onCopyVariant`'s own checks of the copy's name (the table's, not `ValidateNewVariantName`): an empty name, and a name the variant list
 * box has already (`wxListBox::FindString`, which ignores case), the default placeholder among them.
 */
export function validateCopyNameInTable(listed: readonly string[], name: string): string | null {
  if (name === "") return "Variant name cannot be empty.";
  if (listed.some((n) => sameNoCase(n, name))) return `Variant '${name}' already exists.`;
  return null;
}

/** `wxString::Trim().Trim( false )`: the white space at both ends goes. */
export function trimName(text: string): string {
  return text.replace(/^[ \t\r\n\f\v]+|[ \t\r\n\f\v]+$/g, "");
}

/** `UpdateVariantSelectionCtrl`: the schematic toolbar's choice lists the names for the UI, a separator and "Add New Design Variant..." last. */
export function variantSelectorEntries(variants: ReadonlyArray<string | { name: string }> | null | undefined): string[] {
  return [...variantNamesForUI(variants), VARIANT_SEPARATOR, ADD_VARIANT_ENTRY];
}

/** What picking an entry of the schematic's variant choice does (`onVariantSelected`). */
export type VariantPick = { kind: "add" } | { kind: "none" } | { kind: "select"; variant: string };

/**
 * `SCH_EDIT_FRAME::onVariantSelected`: the last entry restores the previous selection and opens the New Design Variant dialog, the separator just restores the
 * previous selection, any other entry becomes the current variant (the default placeholder is the empty name).
 */
export function pickVariantEntry(entries: readonly string[], index: number): VariantPick {
  if (index < 0 || index >= entries.length) return { kind: "none" };
  if (index === entries.length - 1) return { kind: "add" };
  if (index === entries.length - 2) return { kind: "none" };
  const name = entries[index]!;
  return { kind: "select", variant: name === DEFAULT_VARIANT_NAME ? "" : name };
}

/** The entry the choice shows for the current variant (`SetStringSelection( name )`, else entry 0, the default). */
export function selectorValue(variants: ReadonlyArray<string | { name: string }> | null | undefined, current: string): string {
  const name = resolveCurrentVariant(variants, current);
  return name === "" ? DEFAULT_VARIANT_NAME : name;
}

/** `SCH_EDIT_FRAME::RemoveVariant` / `EditVariantDescription` / `RenameVariant` / `CopyVariant`: the choices of their single-choice dialogs (the names for the UI, the default left out). */
export function variantChoices(variants: ReadonlyArray<string | { name: string }> | null | undefined): string[] {
  return variantNamesForUI(variants).slice(1);
}

/** `SCHEMATIC::GetVariantDescription`: a variant's description, "" for one with none (or none of that name). */
export function variantDescription(variants: readonly DesignVariantInfo[] | null | undefined, name: string): string {
  return (variants ?? []).find((v) => v.name === name)?.description ?? "";
}

// ----------------------------------------------------------------------------------------------------------------------------- what a symbol shows

/** The attributes a variant may override (`VARIANT`: `m_DNP`, `m_ExcludedFromBOM`, `m_ExcludedFromBoard`, `m_ExcludedFromSim`, `m_ExcludedFromPosFiles`). */
export type VariantFlagKey = "dnp" | "exclude_from_bom" | "exclude_from_board" | "exclude_from_sim" | "exclude_from_pos_files";

export const VARIANT_FLAG_KEYS: readonly VariantFlagKey[] = ["dnp", "exclude_from_bom", "exclude_from_board", "exclude_from_sim", "exclude_from_pos_files"];

export type VariantFlags = Record<VariantFlagKey, boolean>;

type SymbolLike = Pick<SchematicSymbol, "dnp" | "exclude_from_bom" | "exclude_from_board" | "exclude_from_sim" | "exclude_from_pos_files" | "variants">;

/** The record a symbol keeps of `variant` (`SCH_SYMBOL::GetVariant( sheet, variant )`), or null for the default variant or a variant it has no record of. */
export function variantRecord(sym: Pick<SchematicSymbol, "variants">, variant: string): SymbolVariantInfo | null {
  if (variant === "") return null;
  return sym.variants?.[variant] ?? null;
}

/**
 * `SCH_SYMBOL::GetDNP( sheet, variant )`, `GetExcludedFromBOM`, `GetExcludedFromBoard`, `GetExcludedFromSim`, `GetExcludedFromPosFiles`: the variant's value when the
 * symbol has a record of the variant, else the symbol's own (which the default variant always shows).
 */
export function variantFlags(sym: SymbolLike, variant: string): VariantFlags {
  const rec = variantRecord(sym, variant);
  const out = {} as VariantFlags;
  for (const k of VARIANT_FLAG_KEYS) out[k] = rec?.[k] ?? !!sym[k];
  return out;
}

/** A symbol's own text of field `name` (`SCH_FIELD::GetText`): its field's, else the Value / Footprint / Datasheet the state carries. */
export function ownFieldText(sym: Pick<SchematicSymbol, "fields" | "value" | "footprint" | "datasheet" | "id">, name: string): string {
  const f = sym.fields?.find((x) => x.name === name);
  if (f) return f.text;
  if (name === "Reference") return sym.id;
  if (name === "Value") return sym.value ?? "";
  if (name === "Footprint") return sym.footprint ?? "";
  if (name === "Datasheet") return sym.datasheet ?? "";
  return "";
}

/**
 * `SCH_SYMBOL::GetFieldText( field, sheet, variant )`: the Reference is never a variant's; any other field shows the variant's text where it has one, else the text its
 * variant symbol gives it (`GetVariantLibSymbol`: one the design draws, `symbol_fields`), else its own.
 */
export function variantFieldText(sym: Pick<SchematicSymbol, "fields" | "value" | "footprint" | "datasheet" | "id" | "variants">, name: string, variant: string): string {
  if (name !== "Reference") {
    const rec = variantRecord(sym, variant);
    const over = rec?.fields?.[name] ?? (rec?.symbol_resolved ? rec.symbol_fields?.[name] : undefined);
    if (over !== undefined) return over;
  }
  return ownFieldText(sym, name);
}

/** The texts a variant gives the symbol's fields: its record's, and its variant symbol's where the record has none (`GetFieldText`'s order). */
function variantTexts(rec: SymbolVariantInfo): Record<string, string> {
  return { ...(rec.symbol_resolved ? (rec.symbol_fields ?? {}) : {}), ...(rec.fields ?? {}) };
}

/** `SCH_SYMBOL::GetEffectiveLibSymbol`: the library id the symbol is drawn from in `variant` -- its variant symbol's, when the design draws one (KiCad 25ec2335). */
export function variantLibId(sym: Pick<SchematicSymbol, "lib_id" | "variants">, variant: string): string | null {
  const rec = variantRecord(sym, variant);
  return rec?.symbol_override && rec.symbol_resolved ? rec.symbol_override : sym.lib_id;
}

/** The symbol as it shows in `variant`: its flags, its Value / Footprint / Datasheet and its fields' texts (a field the variant overrides loses its own `shown`). */
export function symbolInVariant(sym: SchematicSymbol, variant: string, serverResolved = false): SchematicSymbol {
  const rec = variantRecord(sym, variant);
  if (!rec) return sym;
  const flags = variantFlags(sym, variant);
  const over = variantTexts(rec);
  const has = (name: string): boolean => Object.prototype.hasOwnProperty.call(over, name);
  // `serverResolved`: the server resolved the texts in this variant (`shown_variant`), so a field's `shown` is the variant text's, expanded; otherwise it is
  // the symbol's own text's and goes
  const fields = sym.fields?.map((f) => {
    if (f.name === "Reference" || !has(f.name)) return f;
    const out = { ...f, text: over[f.name]! };
    if (!serverResolved) delete out.shown;
    return out;
  });
  let shown = sym.shown;
  if (shown && !serverResolved && (has("Value") || has("Footprint"))) {
    shown = { ...shown };
    if (has("Value")) delete shown.value;
    if (has("Footprint")) delete shown.footprint;
  }
  return {
    ...sym,
    ...flags,
    lib_id: variantLibId(sym, variant),
    value: has("Value") ? over.Value! : sym.value,
    footprint: has("Footprint") ? over.Footprint! : sym.footprint,
    datasheet: has("Datasheet") ? over.Datasheet! : sym.datasheet,
    ...(fields ? { fields } : {}),
    ...(shown !== undefined ? { shown } : {}),
  };
}

/**
 * The sheet as the current variant shows it (`SCHEMATIC::SetCurrentVariant` then `HardRedraw`: the painter, the Properties panel and the dialogs read every symbol through
 * the variant). The same object for the default variant, so a memo keyed on it does not change.
 */
export function withVariant(sch: Schematic, variant: string): Schematic {
  const symbols = variant !== "" && sch.symbols.some((s) => s.variants?.[variant]);
  const sheets = variant !== "" && (sch.sheets ?? []).some((s) => s.variants?.[variant]);
  if (!symbols && !sheets) return sch;
  const serverResolved = (sch.shown_variant ?? "") === variant;
  return { ...sch, ...(symbols ? { symbols: sch.symbols.map((s) => symbolInVariant(s, variant, serverResolved)) } : {}), ...(sheets ? { sheets: sch.sheets.map((s) => sheetInVariant(s, variant)) } : {}) };
}

/** A sheet's four attributes in `variant` (`SCH_SHEET::GetDNP( &instance, variant )` ...): its record of the variant, else its own (UI-LEFT-2 SS15). */
export function sheetFlags(sheet: Pick<Sheet, "props" | "variants">, variant: string): { dnp: boolean; exclude_from_sim: boolean; exclude_from_bom: boolean; exclude_from_board: boolean } {
  const rec = variant !== "" ? sheet.variants?.[variant] : undefined;
  const own = sheet.props ?? {};
  const src = rec ?? own;
  return { dnp: !!src.dnp, exclude_from_sim: !!src.exclude_from_sim, exclude_from_bom: !!src.exclude_from_bom, exclude_from_board: !!src.exclude_from_board };
}

/** The sheet as `variant` shows it: `variant_flags`, its attributes in the variant (the DNP cross the painter draws, `aSheet->GetDNP( sheetPath, variant )`); its style its own. */
export function sheetInVariant(sheet: Sheet, variant: string): Sheet {
  const rec = variant !== "" ? sheet.variants?.[variant] : undefined;
  if (!rec) return sheet;
  // and its user fields' texts in the variant (`SCH_SHEET::GetFieldText( .., aPath, aVariantName )`, UI-LEFT-2 SS16)
  const texts = rec.fields ?? {};
  const has = (name: string): boolean => Object.prototype.hasOwnProperty.call(texts, name);
  const out: Sheet = { ...sheet, variant_flags: sheetFlags(sheet, variant) };
  if (Object.keys(texts).length > 0) {
    if (sheet.user_fields) out.user_fields = sheet.user_fields.map((f) => (has(f.name) ? { ...f, text: texts[f.name]! } : f));
    if (sheet.user_fields_drawn)
      out.user_fields_drawn = sheet.user_fields_drawn.map((f) => {
        if (!has(f.name)) return f;
        const g = { ...f, text: texts[f.name]! };
        delete g.shown;
        return g;
      });
  }
  return out;
}

// ----------------------------------------------------------------------------------------------------------------------------- the edits

/** What a variant edit of a set of symbols changes: some flags and some field texts. */
export interface VariantChange {
  flags?: Partial<VariantFlags>;
  fields?: Record<string, string>;
}

/** The `set_symbol_variant` verb for `change`, or null when it changes nothing. */
export function setSymbolVariantCmd(variant: string, ids: readonly string[], change: VariantChange) {
  const flags = change.flags ?? {};
  const fields = change.fields ?? {};
  if (variant === "" || ids.length === 0 || (Object.keys(flags).length === 0 && Object.keys(fields).length === 0)) return null;
  return { op: "set_symbol_variant" as const, variant, ids: [...ids], ...flags, ...(Object.keys(fields).length > 0 ? { fields: { ...fields } } : {}) };
}

/** `DIALOG_SYMBOL_PROPERTIES`'s title: the variant being edited is named in it (" - <variant> Design Variant"), to remind the user. */
export function symbolPropertiesTitle(variant: string): string {
  return variant === "" ? "Symbol Properties" : `Symbol Properties - ${variant} Design Variant`;
}
