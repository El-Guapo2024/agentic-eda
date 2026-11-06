// A library symbol's body styles and unit names as `LIB_SYMBOL` tells them (eeschema/lib_symbol.h, lib_symbol.cpp; UI-LEFT Y4): De Morgan ("Standard" and
// "Alternate", `HasDeMorganBodyStyles`), the custom body styles of Symbol Properties' "Custom body styles:" grid (`GetBodyStyleNames`), or one body style; and a
// unit's display name (`GetUnitDisplayNames`, the "Unit display names (optional):" grid). The backend's twin is `eda_model::ir::LibrarySymbol::body_style_count`.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

import { letterSubReference } from "./unitLetter";

/** What these need of a symbol. */
export interface BodyStyled {
  has_alternate_body_style?: boolean;
  body_style_names?: string[];
}

/** `LIB_SYMBOL::GetBodyStyleCount`: 2 for De Morgan, else one per custom name (at least 1). */
export function bodyStyleCount(sym: BodyStyled | null | undefined): number {
  if (!sym) return 1;
  if (sym.has_alternate_body_style) return 2;
  return Math.max(1, sym.body_style_names?.length ?? 0);
}

/** `LIB_SYMBOL::IsMultiBodyStyle`. */
export function isMultiBodyStyle(sym: BodyStyled | null | undefined): boolean {
  return bodyStyleCount(sym) > 1;
}

/** `LIB_SYMBOL::GetBodyStyleDescription( aBodyStyle, true )`: "Standard" / "Alternate" of a De Morgan symbol, a custom style's name, else "?". */
export function bodyStyleDescription(sym: BodyStyled | null | undefined, style: number): string {
  if (sym?.has_alternate_body_style) {
    if (style === 2) return "Alternate";
    if (style === 1) return "Standard";
  } else if (isMultiBodyStyle(sym) && style >= 1) {
    const name = sym?.body_style_names?.[style - 1];
    if (name !== undefined) return name;
  }
  return "?";
}

/**
 * `SYMBOL_EDIT_FRAME::RebuildSymbolUnitAndBodyStyleLists`' body style box: one empty entry for a symbol of one body style, `DEMORGAN_STD` and `DEMORGAN_ALT`
 * ("Standard", "Alternate") for a De Morgan one, else the custom names; body style `i + 1` is entry `i`.
 */
export function bodyStyleSelectorEntries(sym: BodyStyled | null | undefined): string[] {
  if (!isMultiBodyStyle(sym)) return [""];
  if (sym?.has_alternate_body_style) return ["Standard", "Alternate"];
  return [...(sym?.body_style_names ?? [])];
}

/** The unit names a symbol keeps (`LibrarySymbolKeep::unit_names`, by unit number as a JSON key). */
export function unitNamesOf(sym: { kicad?: Record<string, unknown> } | null | undefined): Record<string, string> {
  const names = sym?.kicad?.unit_names;
  return names && typeof names === "object" ? (names as Record<string, string>) : {};
}

/** `LIB_SYMBOL::GetUnitDisplayName( aUnit, aLabel )`: the unit's display name, else "Unit A" (with `label`) or the bare letters. */
export function unitDisplayName(sym: { kicad?: Record<string, unknown> } | null | undefined, unit: number, label = true): string {
  const name = unitNamesOf(sym)[String(unit)];
  if (name !== undefined) return name;
  return label ? `Unit ${letterSubReference(unit)}` : letterSubReference(unit);
}

/** The Symbol Properties dialog's body style choice (the three radio buttons and the grid of names). */
export type BodyStyleMode = "single" | "demorgan" | "custom";

/** `TransferDataToWindow`: De Morgan, else custom when the symbol has several body styles, else single; the grid holds the custom names. */
export function bodyStyleModeOf(sym: BodyStyled): { mode: BodyStyleMode; names: string[] } {
  if (sym.has_alternate_body_style) return { mode: "demorgan", names: [] };
  if (isMultiBodyStyle(sym)) return { mode: "custom", names: [...(sym.body_style_names ?? [])] };
  return { mode: "single", names: [] };
}

/** `Validate`'s body style count: 1, 2, or the grid's non-empty names. */
export function bodyStyleCountOf(mode: BodyStyleMode, names: readonly string[]): number {
  if (mode === "single") return 1;
  if (mode === "demorgan") return 2;
  return names.filter((n) => n !== "").length;
}

/**
 * KiCad 25ec2335 `DIALOG_LIB_SYMBOL_PROPERTIES::Validate`'s checks of the Units & Body Styles tab, in its order: a unit's name -- its display name, or its letters
 * when the row is empty -- may be one other row's only once, case aside ("The unit name '%s' is already in use."); custom body styles need two names ("Custom body
 * styles must have at least 2 entries"). `null` when the tab is fine.
 */
export function unitsAndBodyStylesProblem(unitNameRows: readonly string[], mode: BodyStyleMode, names: readonly string[]): string | null {
  const seen = new Set<string>();
  for (let row = 0; row < unitNameRows.length; row++) {
    const subRef = unitNameRows[row] || letterSubReference(row + 1);
    const key = subRef.toUpperCase();
    if (seen.has(key)) return `The unit name '${subRef}' is already in use.`;
    seen.add(key);
  }
  if (mode === "custom" && bodyStyleCountOf(mode, names) < 2) return "Custom body styles must have at least 2 entries";
  return null;
}

/** `updateUnitCount`: the unit names grid has one row per unit; rows past the count go, new rows start empty. */
export function resizeUnitNameRows(rows: readonly string[], count: number): string[] {
  const out = rows.slice(0, Math.max(1, count));
  while (out.length < count) out.push("");
  return out;
}

/** The grid's rows of a symbol (`TransferDataToWindow`: the display name of each unit up to the count). */
export function unitNameRowsOf(sym: { unit_count: number; kicad?: Record<string, unknown> }): string[] {
  const names = unitNamesOf(sym);
  return Array.from({ length: Math.max(1, sym.unit_count) }, (_, i) => names[String(i + 1)] ?? "");
}

/** The jumper groups grid's rows (`TransferDataToWindow`: each group's numbers joined with ", "). */
export function jumperRowsOf(sym: { kicad?: Record<string, unknown> }): string[] {
  const groups = sym.kicad?.jumper_pin_groups;
  return Array.isArray(groups) ? (groups as string[][]).map((g) => g.join(", ")) : [];
}

/** `NO_BODY_STYLES_WITH_ALTERNATE_PIN_FUNCTIONS` (KiCad 25ec2335 eeschema/tools/sch_actions.h). */
export const NO_BODY_STYLES_WITH_ALTERNATE_PIN_FUNCTIONS = "Multiple body styles are not supported on symbols with alternate pin functions.";

/**
 * KiCad 25ec2335 `DIALOG_LIB_SYMBOL_PROPERTIES::OnBodyStyle`: choosing De Morgan or custom body styles on a symbol whose pins have alternate functions is refused
 * with `NO_BODY_STYLES_WITH_ALTERNATE_PIN_FUNCTIONS` and "(Pins %s.)" of those pins' numbers (`PIN_NUMBERS::GetSummary`), and the choice goes back to a single body
 * style. `null` when the choice stands.
 */
export function bodyStyleChoiceRefusal(mode: BodyStyleMode, pins: ReadonlyArray<{ number: string; alternates?: readonly unknown[] }>, summary: (numbers: string[]) => string): string | null {
  if (mode === "single") return null;
  const numbers = pins.filter((p) => (p.alternates?.length ?? 0) > 0).map((p) => p.number);
  if (numbers.length === 0) return null;
  return `${NO_BODY_STYLES_WITH_ALTERNATE_PIN_FUNCTIONS}\n(Pins ${summary(numbers)}.)`;
}
