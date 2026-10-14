// A library symbol's fields and the derivation between library symbols, as the editor shows them: the five mandatory fields and the user fields (`SCH_FIELD`s of a
// `LIB_SYMBOL`, `MANDATORY_FIELDS`), `(extends "Parent")`, the parent choice of Symbol Properties (`DIALOG_LIB_SYMBOL_PROPERTIES`: not itself, not a symbol derived from it)
// and the related symbols of the fields table (`setScope( SCOPE_RELATED_SYMBOLS )`: the root and everything derived from it).
//
// `allFields` is `LibrarySymbol::all_fields` (crates/model/src/sym_fields.rs): the same rules, so what the dialogs show is what the server holds -- the text of `Reference`,
// `Datasheet` and `Description` is the symbol's own member and wins over a copy in the list; `Value` with no stored text is the symbol's name; a mandatory field that is not
// stored is at its default place. The server leaves out every field value that is its default; `fieldRow` puts them back.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { FieldHAlign, FieldVAlign, LibrarySymbol, LibrarySymbolField, MmPoint } from "../api/types";

/** `MANDATORY_FIELDS`, by the names a `.kicad_sym` gives them. */
export const MANDATORY_FIELDS = ["Reference", "Value", "Footprint", "Datasheet", "Description"] as const;

export const isMandatoryField = (name: string): boolean => (MANDATORY_FIELDS as readonly string[]).includes(name);

/** The properties of a symbol that are not its fields: `parseProperty` reads them as the symbol's keywords and footprint filters. */
export const RESERVED_FIELD_NAMES: readonly string[] = ["ki_keywords", "ki_description", "ki_fp_filters", "ki_locked"];

/** A field with every value present (the wire shape leaves out what has its default). */
export interface FieldRow {
  name: string;
  text: string;
  at: MmPoint;
  angle_deg: number;
  size_mm: number;
  hidden: boolean;
  show_name: boolean;
  do_not_autoplace: boolean;
  bold: boolean;
  italic: boolean;
  h_align: FieldHAlign;
  v_align: FieldVAlign;
}

/** The part of a `lib_id` after the library nickname. */
export const bareName = (libId: string): string => (libId.includes(":") ? libId.slice(libId.indexOf(":") + 1) : libId);

/** The library nickname of a `lib_id` (`""` for a bare name). */
export const libraryOf = (libId: string): string => (libId.includes(":") ? libId.slice(0, libId.indexOf(":")) : "");

/** A wire field with its defaults put back. */
export function fieldRow(f: LibrarySymbolField): FieldRow {
  return {
    name: f.name,
    text: f.text ?? "",
    at: f.at ?? { x: 0, y: 0 },
    angle_deg: f.angle_deg ?? 0,
    size_mm: f.size_mm ?? 1.27,
    hidden: f.hidden ?? false,
    show_name: f.show_name ?? false,
    do_not_autoplace: f.do_not_autoplace ?? false,
    bold: f.bold ?? false,
    italic: f.italic ?? false,
    h_align: f.h_align ?? "center",
    v_align: f.v_align ?? "center",
  };
}

/** `LibrarySymbol::default_field`: the reference and the value are shown at their old places, the rest hidden. */
export function defaultField(name: string): FieldRow {
  const f = fieldRow({ name });
  if (name === "Reference") f.at = { x: 0, y: 5.08 };
  else if (name === "Value") f.at = { x: 0, y: 2.54 };
  else f.hidden = true;
  return f;
}

/** `LibrarySymbol::all_fields`: the five mandatory fields in order, then the user fields in the order they were added. */
export function allFields(sym: Pick<LibrarySymbol, "lib_id" | "reference_prefix" | "datasheet" | "description" | "fields">): FieldRow[] {
  const stored = sym.fields ?? [];
  const out: FieldRow[] = [];
  for (const name of MANDATORY_FIELDS) {
    const own = stored.find((f) => f.name === name);
    const f = own ? fieldRow(own) : defaultField(name);
    f.name = name;
    switch (name) {
      case "Reference":
        f.text = sym.reference_prefix ?? "";
        break;
      case "Datasheet":
        f.text = sym.datasheet ?? "";
        break;
      case "Description":
        f.text = sym.description ?? "";
        break;
      case "Value":
        if (!own) f.text = bareName(sym.lib_id);
        break;
      default:
        break;
    }
    out.push(f);
  }
  for (const f of stored) if (!isMandatoryField(f.name)) out.push(fieldRow(f));
  return out;
}

/** What the grid sends back for a field: every value, so nothing is left to the server's defaults. */
export function wireField(f: FieldRow): LibrarySymbolField {
  return { ...f };
}

/** The symbols derived from `libId`, directly or through others, parents first (`GetDerivedSymbolNames`). `derived` maps each derived symbol to its parent. */
export function descendantsOf(derived: Readonly<Record<string, string>>, libId: string): string[] {
  const out: string[] = [];
  const walk = (parent: string) => {
    for (const [child, p] of Object.entries(derived)) {
      if (p !== parent || child === libId || out.includes(child)) continue;
      out.push(child);
      walk(child);
    }
  };
  walk(libId);
  return out;
}

/** The root of the chain `libId` stands at the end of (itself when it is not derived); a cycle ends where it closes. */
export function rootOf(derived: Readonly<Record<string, string>>, libId: string): string {
  const seen = new Set<string>([libId]);
  let at = libId;
  for (;;) {
    const parent = derived[at];
    if (!parent || seen.has(parent)) return at;
    seen.add(parent);
    at = parent;
  }
}

/** `setScope( SCOPE_RELATED_SYMBOLS )`: the root of `libId`, then everything derived from the root. */
export function relatedSymbols(derived: Readonly<Record<string, string>>, libId: string): string[] {
  const root = rootOf(derived, libId);
  return [root, ...descendantsOf(derived, root)];
}

/**
 * The parents Symbol Properties' "Derive from" choice offers: the symbols of the symbol's own library, but not itself and not one derived from it ("Don't allow a symbol to be
 * derived from itself ... or from any of its descendants (would create circular inheritance)"), sorted by name as numbers sort inside names (`StrNumCmp`).
 */
export function parentCandidates(libId: string, names: readonly string[], derived: Readonly<Record<string, string>>): string[] {
  const lib = libraryOf(libId);
  const banned = new Set([libId, ...descendantsOf(derived, libId)]);
  return names.filter((n) => libraryOf(n) === lib && !banned.has(n)).sort((a, b) => bareName(a).localeCompare(bareName(b), undefined, { numeric: true, sensitivity: "base" }));
}

/** `symbolSelectedCondition` / `derivedSymbolSelectedCondition` / `relatedSymbolSelectedCondition` of the library tree's menu. */
export function treeConditions(derived: Readonly<Record<string, string>>, libId: string | undefined): { isDerived: boolean; hasChildren: boolean; related: boolean } {
  if (!libId) return { isDerived: false, hasChildren: false, related: false };
  const isDerived = libId in derived;
  const hasChildren = Object.values(derived).includes(libId);
  return { isDerived, hasChildren, related: isDerived || hasChildren };
}

/** The label a field is drawn with: `Name: text` when its name is shown (`SCH_FIELD::GetShownText`). */
export function shownFieldText(f: Pick<FieldRow, "name" | "text" | "show_name">): string {
  return f.show_name ? `${f.name}: ${f.text}` : f.text;
}

/** Whether a name can be given to a user field: not empty, not a mandatory field's, not one of the symbol's own `ki_` properties, not taken. Null when it can; else the reason. */
export function fieldNameProblem(name: string, others: readonly string[]): string | null {
  if (name.trim() === "") return "Fields must have a name.";
  if (RESERVED_FIELD_NAMES.includes(name)) return `${name} is the symbol's keywords or footprint filters, not a field.`;
  if (isMandatoryField(name) || others.includes(name)) return `Field name '${name}' already in use.`;
  return null;
}
