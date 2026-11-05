// Schematic Setup's pages, the rules they apply (eeschema/dialogs/dialog_schematic_setup.cpp and its panels at 8303b2ad), kept apart from the components so
// node --test checks them:
//
//   General > Formatting        panel_setup_formatting.cpp      the rows, their units and KiCad's defaults; the one check (the connection grid's
//                                                               UNIT_BINDER::Validate) with its words
//   General > Annotation        panel_eeschema_annotation_options.cpp   the symbol unit notation's seven choices (loadEEschemaSettings / TransferDataFromWindow)
//   General > Field Name Templates  panel_template_fieldnames.cpp  "Untitled Field", the white-space question, the rows kept (TEMPLATES::AddTemplateFieldName)
//   Project > Net Classes       common/dialogs/panel_setup_netclasses.cpp  the Eeschema columns (wire and bus thickness, colour, line style)
//   Project > Bus Alias Definitions  panel_setup_buses.cpp       the grid's vetoes with their words, members split at spaces, the list sorted
//
// The verbs that store each page are crates/ops/src/sch_setup.rs (and Board Setup's `set_net_classes`).
import type { AnnotationSettings, BusAliasDef, NetClassSchematic, SchFormatting, TemplateFieldName } from "../api/types";

/** Nanometres in a mil. */
export const NM_PER_MIL = 25_400;

// ----------------------------------------------------------------------------------------------------------------------------- Formatting

/** `SCHEMATIC_SETTINGS::SCHEMATIC_SETTINGS` with no user configuration (`eeschema/default_values.h`, `FONT_METRICS::m_OverbarHeight`). */
export const DEFAULT_SCH_FORMATTING: SchFormatting = {
  default_text_size_nm: 50 * NM_PER_MIL,
  overbar_offset_ratio: 1.23,
  text_offset_ratio: 0.15,
  label_size_ratio: 0.375,
  default_line_width_nm: 6 * NM_PER_MIL,
  pin_symbol_size_nm: 25 * NM_PER_MIL,
  junction_size_choice: 3,
  hop_over_size_choice: 0,
  dash_ratio: 12,
  gap_ratio: 3,
  opo_v_precision: 3,
  opo_v_range: "~V",
  opo_i_precision: 3,
  opo_i_range: "~A",
};

/** Whether two pages of settings say the same, whatever order their keys came in (the backend's JSON sorts them): the page sends only the parts that changed,
 *  as a verb that changes nothing is refused (and would refuse the whole `batch`). */
export function samePageSettings<T extends object>(a: T | undefined | null, b: T | undefined | null): boolean {
  if (!a || !b) return a === b;
  const ka = Object.keys(a).sort();
  const kb = Object.keys(b).sort();
  return ka.length === kb.length && ka.every((k, i) => k === kb[i] && Object.is((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/** `m_choiceJunctionDotSizeChoices`. */
export const JUNCTION_SIZE_CHOICES = ["None", "Smallest", "Small", "Default", "Large", "Largest"] as const;
/** `m_choiceHopOverSizeChoices`. */
export const HOP_OVER_SIZE_CHOICES = ["None", "Smallest", "Small", "Medium", "Large", "Largest"] as const;
/** `m_vRangeCtrlChoices`: the first, "Auto", is stored as "~V" (`TransferDataFromWindow`). */
export const V_RANGES = ["Auto", "fV", "pV", "nV", "uV", "mV", "V", "KV", "MV", "GV", "TV", "PV"] as const;
/** `m_iRangeCtrlChoices`: "Auto" is "~A". */
export const I_RANGES = ["Auto", "fA", "pA", "nA", "uA", "mA", "A", "KA", "MA", "GA", "TA", "PA"] as const;
/** `junction_size_mult_list` (`schematic_settings.cpp`). */
export const JUNCTION_SIZE_MULT = [0.0, 1.7, 4.0, 6.0, 9.0, 12.0] as const;

/** A range's stored value as the list shows it: "~V" / "~A" is "Auto" (`m_vRangeCtrl->SetStringSelection( settings.m_OPO_VRange )` finds no "~V", which is the first row). */
export function rangeLabel(stored: string): string {
  return stored.startsWith("~") ? "Auto" : stored;
}

/** `TransferDataFromWindow`: the range a list's row stores -- "~V" (or "~A") for "Auto", else the row's text. */
export function rangeStored(label: string, unit: "V" | "A"): string {
  return label === "Auto" ? `~${unit}` : label;
}

/**
 * `EDA_UNIT_UTILS::UI::DoubleValueFromString`: the text's leading number (digits, a decimal point -- "." or ",", the locale's -- and signs), 0 when there is none.
 * The panel's percentage, ratio and size fields read their text so: KiCad says nothing of a text that is not a number, it reads it as 0.
 */
export function doubleValueFromString(text: string): number {
  const buf = text.trim().replace(/,/g, ".");
  let end = 0;
  while (end < buf.length && /[0-9.+-]/.test(buf[end]!)) end++;
  const v = Number(buf.slice(0, end));
  return end > 0 && Number.isFinite(v) ? v : 0;
}

/** `StringFromValue( unityScale, .., value )` of the percentage and ratio fields: up to ten places, trailing zeros removed (`removeTrailingZeros`). */
export function ratioText(v: number): string {
  if (!Number.isFinite(v)) return "0";
  const t = v.toFixed(10).replace(/\.?0+$/, "");
  return t === "-0" ? "0" : t;
}

/** A size field's text in mils, as `UNIT_BINDER` shows a schematic length (`%.3f`, trailing zeros removed). */
export function milsText(nm: number): string {
  return (nm / NM_PER_MIL).toFixed(3).replace(/\.?0+$/, "");
}

/** A size field's text read in mils, as nanometres on the schematic's 100 nm unit (`schIUScale.MilsToIU`). */
export function milsToNm(text: string): number {
  return Math.round(doubleValueFromString(text) * 254) * 100;
}

/**
 * `m_connectionGridSize.Validate( MIN_CONNECTION_GRID_MILS, 10000, EDA_UNITS::MILS )`: `UNIT_BINDER::Validate`'s "%s must be at least %s." and "%s must be less
 * than %s." with the label less its colon ("Connection grid") and the limit as `StringFromValue( .., MILS, .., true )` writes it ("25 mils"). Null when it is in range.
 */
export function connectionGridError(mils: number, min = 25, max = 10_000): string | null {
  if (mils < min) return `Connection grid must be at least ${milsText(min * NM_PER_MIL)} mils.`;
  if (mils > max) return `Connection grid must be less than ${milsText(max * NM_PER_MIL)} mils.`;
  return null;
}

/** `SCHEMATIC_SETTINGS::GetJunctionSize`: the dot's diameter for the Default class's wire width (`KiROUND` on the 100 nm unit, at least one unit -- no dot). */
export function junctionDiameterNm(choice: number, defaultWireWidthNm: number): number {
  const mult = JUNCTION_SIZE_MULT[choice] ?? 6.0;
  return Math.max(Math.round((defaultWireWidthNm / 100) * mult), 1) * 100;
}

/**
 * `SCH_JUNCTION::getEffectiveShape`: the radius a junction dot is drawn at -- the dot's diameter, but at least 1.7 times the width of the wire it joins (the
 * net class's), and none at all when the diameter is one unit (100 nm: Junction dot size "None"). `SCH_PAINTER::draw( SCH_JUNCTION )` draws it only when the
 * radius is more than one unit.
 */
export function junctionRadiusNm(diameterNm: number, wireWidthNm: number): number {
  // in KiCad's schematic unit, 100 nm, as `getEffectiveShape` works it: `std::max<int>( diameter, wire width * 1.7 )`, then `std::max( diameter / 2, 1 )`
  const iu = Math.round(diameterNm / 100);
  const d = iu === 1 ? iu : Math.max(iu, Math.trunc((wireWidthNm / 100) * 1.7));
  return Math.max(Math.trunc(d / 2), 1) * 100;
}

/** `SCH_PAINTER::draw( SCH_JUNCTION )`: a dot whose radius is one unit or less is not drawn. */
export function junctionDrawn(radiusNm: number): boolean {
  return radiusNm > 100;
}

// ----------------------------------------------------------------------------------------------------------------------------- Annotation

/** `m_choiceSeparatorRefIdChoices` with what `TransferDataFromWindow` stores for each: the first unit's id and the separator, as character codes. */
export const UNIT_NOTATIONS: ReadonlyArray<{ label: string; firstId: number; separator: number }> = [
  { label: "A", firstId: 65, separator: 0 },
  { label: ".A", firstId: 65, separator: 46 },
  { label: "-A", firstId: 65, separator: 45 },
  { label: "_A", firstId: 65, separator: 95 },
  { label: ".1", firstId: 49, separator: 46 },
  { label: "-1", firstId: 49, separator: 45 },
  { label: "_1", firstId: 49, separator: 95 },
];

/** `loadEEschemaSettings`: the choice a project's first id and separator show as (with 'A' the separator's, anything else "A"; with any other first id the numbered three, '.' by default). */
export function unitNotationChoice(firstId: number, separator: number): number {
  if (firstId === 65) return separator === 46 ? 1 : separator === 45 ? 2 : separator === 95 ? 3 : 0;
  return separator === 45 ? 5 : separator === 95 ? 6 : 4;
}

/** `SCHEMATIC_SETTINGS`' annotation defaults. */
export const DEFAULT_ANNOTATION: AnnotationSettings = { sort_order: 0, method: 0, start_num: 0, reuse_designators: true };

// ---------------------------------------------------------------------------------------------------------------- Field Name Templates

/** `OnAddButtonClick`: a new row's name, its Visible box off. */
export const NEW_TEMPLATE_FIELD: TemplateFieldName = { name: "Untitled Field", visible: false, url: false };

/** `MANDATORY_FIELDS` by `GetCanonicalFieldName`: no template of these names is kept (`TEMPLATES::AddTemplateFieldName`). */
export const MANDATORY_FIELD_NAMES = ["Reference", "Value", "Footprint", "Datasheet", "Description"] as const;

/** The question `TransferDataFromWindow` asks of a name with white space at either end, word for word: the message, its extended message and its two answers. */
export const WHITESPACE_EXTENDED =
  "This may result in what appears to be duplicate field names but are actually unique names differing only by white space characters.  Removing the white space characters will have no effect on existing symbol field names.";
export const WHITESPACE_REMOVE = "Remove White Space";
export const WHITESPACE_KEEP = "Keep White Space";

export function whitespaceWarning(name: string): string | null {
  return name !== "" && name.trim() !== name ? `The field name '${name}' contains trailing and/or leading white space.` : null;
}

/** The first row whose name has white space at an end, with the question: the page asks about each in turn before it applies. */
export function firstWhitespaceRow(rows: readonly TemplateFieldName[]): { row: number; message: string } | null {
  for (let i = 0; i < rows.length; i++) {
    const m = whitespaceWarning(rows[i]!.name);
    if (m) return { row: i, message: m };
  }
  return null;
}

/** `TransferDataFromWindow` over `AddTemplateFieldName`: every row with a name, in order, but one named like a mandatory field; a second row of a name replaces the first in its place. */
export function keptTemplateFields(rows: readonly TemplateFieldName[]): TemplateFieldName[] {
  const out: TemplateFieldName[] = [];
  for (const f of rows) {
    if (f.name === "" || (MANDATORY_FIELD_NAMES as readonly string[]).includes(f.name)) continue;
    const at = out.findIndex((k) => k.name === f.name);
    if (at >= 0) out[at] = { ...f };
    else out.push({ ...f });
  }
  return out;
}

/** `WX_GRID::OnMoveRowUp` / `OnMoveRowDown`: the rows with `row` swapped with its neighbour (`by` -1 up, +1 down); out of range is no move. */
export function moveRow<T>(rows: readonly T[], row: number, by: -1 | 1): T[] {
  const j = row + by;
  if (row < 0 || row >= rows.length || j < 0 || j >= rows.length) return [...rows];
  const out = [...rows];
  [out[row], out[j]] = [out[j]!, out[row]!];
  return out;
}

// ---------------------------------------------------------------------------------------------------------------- Bus Alias Definitions

/** `OnAliasesGridCellChanging`'s veto: another row has the name, compared as typed. */
export function aliasNameError(aliases: readonly BusAliasDef[], row: number, name: string): string | null {
  return aliases.some((a, i) => i !== row && a.name === name) ? `Alias name '${name}' already in use.` : null;
}

/** `OnMemberGridCellChanging`'s veto. */
export const EMPTY_MEMBER_ERROR = "Member net/alias name cannot be empty.";

/** `m_membersLabelTemplate` ("Members of '%s'") with the alias's name. */
export function membersLabel(name: string): string {
  return `Members of '${name}'`;
}

/** `OnMemberGridCellChanging`: a member typed as a space-separated list is one member per word (`wxStringTokenizer( name, " " )`). */
export function splitMembers(text: string): string[] {
  return text.split(" ").filter((t) => t !== "");
}

/** `loadAliases`: the project's aliases as the page lists them, by name without regard to case (`CmpNoCase`). */
export function sortAliases(aliases: readonly BusAliasDef[]): BusAliasDef[] {
  return [...aliases].sort((a, b) => {
    const x = a.name.toLowerCase();
    const y = b.name.toLowerCase();
    return x < y ? -1 : x > y ? 1 : 0;
  });
}

/** The page's vetoes over the whole list, the first one found (the grid refuses the cell; Apply refuses the page): a name another row has, an empty member. An
 *  alias left with no name is dropped when the project is written (`PROJECT_FILE`'s `schematic.bus_aliases` reader), so it is no mistake. */
export function busAliasIssue(aliases: readonly BusAliasDef[]): { row: number; member: number | null; message: string } | null {
  for (let i = 0; i < aliases.length; i++) {
    const a = aliases[i]!;
    // (a row never named is never vetoed: the grid checks a name as it is typed in, and the project drops an alias with no name)
    const named = a.name === "" ? null : aliasNameError(aliases.slice(0, i), -1, a.name);
    if (named) return { row: i, member: null, message: named };
    const m = a.members.findIndex((x) => x === "");
    if (m >= 0) return { row: i, member: m, message: EMPTY_MEMBER_ERROR };
  }
  return null;
}

/** The page's OK: each member split at spaces, as the grid splits it on entry. */
export function busAliasesToSend(aliases: readonly BusAliasDef[]): BusAliasDef[] {
  return aliases.map((a) => ({ name: a.name, members: a.members.flatMap((m) => (m === "" ? [m] : splitMembers(m))) }));
}

// ------------------------------------------------------------------------------------------------------------------------- Net Classes

/** `g_lineStyleNames`: row 0 "<Not defined>" (no line style), then `LINE_STYLE` 0 to 4. */
export const LINE_STYLE_NAMES = ["<Not defined>", "Solid", "Dashed", "Dotted", "Dash-Dot", "Dash-Dot-Dot"] as const;

/** The Eeschema columns (`GRID_FIRST_EESCHEMA` .. `GRID_LINESTYLE`) with their labels and tooltips (`OnNetclassGridMouseEvent`). */
export const SCH_NETCLASS_COLUMNS = [
  { key: "wire_width", label: "Wire Thickness", title: "Schematic wire thickness" },
  { key: "bus_width", label: "Bus Thickness", title: "Bus wire thickness" },
  { key: "color", label: "Color", title: "Schematic wire color" },
  { key: "line_style", label: "Line Style", title: "Schematic wire line style" },
] as const;

/** `m_colorDefaultHelpText` as Eeschema leaves it. */
export const SCH_COLOR_HELP = "Set color to transparent to use KiCad default color.";

/** `loadNetclasses`: the line style row a class shows (`<Not defined>` when it has none). */
export function lineStyleRow(style: number | null | undefined): number {
  if (style === null || style === undefined) return 0;
  const i = Math.max(0, style);
  return i + 1 >= LINE_STYLE_NAMES.length ? 0 : i + 1;
}

/** `TransferDataFromWindow`: a row's line style back (`lineIdx == 0` is none). */
export function lineStyleOfRow(row: number): number | null {
  return row <= 0 ? null : row - 1;
}

/** `#rrggbb` (an `<input type="color">`) as `COLOR4D::ToCSSString` writes an opaque colour. */
export function cssFromHex(hex: string): string | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  return m ? `rgb(${parseInt(m[1]!, 16)}, ${parseInt(m[2]!, 16)}, ${parseInt(m[3]!, 16)})` : null;
}

/** A project colour (`rgb(..)`, `rgba(..)`) as `#rrggbb` for a colour swatch; null for none or an unreadable one. */
export function hexFromCss(css: string | null | undefined): string | null {
  if (!css) return null;
  const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/.exec(css.trim());
  if (!m) return null;
  const h = (v: string) => Math.min(255, Number(v)).toString(16).padStart(2, "0");
  return `#${h(m[1]!)}${h(m[2]!)}${h(m[3]!)}`;
}

/** The schematic columns of one row of the page: the class's own entry, none when it has none. */
export function schematicOf(table: Readonly<Record<string, NetClassSchematic>> | undefined, name: string): NetClassSchematic {
  return table?.[name] ?? {};
}

/** The columns' checks: a width that is set must be larger than zero (`GRID_CELL_MARK_AS_NULLABLE`: the Default row may not be blank). Null when fine. */
export function schematicColumnsIssue(look: NetClassSchematic, isDefault: boolean): string | null {
  for (const [what, v] of [
    ["Wire thickness", look.wire_width],
    ["Bus thickness", look.bus_width],
  ] as const) {
    if (v === undefined || v === null) {
      if (isDefault) return `${what} of the Default class is required.`;
      continue;
    }
    if (!Number.isFinite(v)) return `${what} is not a number.`;
    if (v <= 0) return `${what} must be larger than zero.`;
  }
  return null;
}

/** The table without its empty entries and with each renamed class's entry under its new name: what the page sends. */
export function schematicToSend(table: Readonly<Record<string, NetClassSchematic>>, names: readonly string[]): Record<string, NetClassSchematic> {
  const out: Record<string, NetClassSchematic> = {};
  for (const name of names) {
    const look = table[name];
    if (!look) continue;
    const kept: NetClassSchematic = {};
    if (look.wire_width !== undefined && look.wire_width !== null) kept.wire_width = look.wire_width;
    if (look.bus_width !== undefined && look.bus_width !== null) kept.bus_width = look.bus_width;
    if (look.color) kept.color = look.color;
    if (look.line_style !== undefined && look.line_style !== null) kept.line_style = look.line_style;
    if (Object.keys(kept).length > 0) out[name] = kept;
  }
  return out;
}

/** The Default class's columns with KiCad's defaults for what it leaves unset (6 and 12 mils, solid): what its row shows. */
export function defaultClassSchematic(own: NetClassSchematic | undefined): NetClassSchematic {
  return { wire_width: own?.wire_width ?? 6 * NM_PER_MIL, bus_width: own?.bus_width ?? 12 * NM_PER_MIL, color: own?.color ?? null, line_style: own?.line_style ?? 0 };
}
