// The Footprint Editor's Footprint Properties beyond the General tab's fields grid (UI-LEFT F4): the footprint's name with `checkFootprintName`, the Layers tab's Private Layers,
// the Clearance Overrides tab (with `MARGIN_OFFSET_BINDER`'s one "Solder paste clearance:" field for the paste margin and ratio) and the Pad Connections tab (the zone
// connection, the Net Ties and the jumpers) -- ported from `DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR` (pcbnew/dialogs/dialog_footprint_properties_fp_editor.cpp at KiCad 8303b2ad:
// `TransferDataToWindow`, `checkFootprintName`, `Validate`, `TransferDataFromWindow`) and common/widgets/margin_offset_binder.cpp (`parseInput`, `formatValue`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). Lengths are nm.
import type { FootprintSettings } from "../api/fpEditorTypes";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";

// ------------------------------------------------------------------------------------------------------------------------------- the name

/** `FOOTPRINT::StringLibNameInvalidChars( false )` and `( true )`: what a footprint name may not hold, and how the message names it. */
export const FOOTPRINT_NAME_INVALID = "%$<>\t\n\r\"\\/:";
export const FOOTPRINT_NAME_INVALID_READABLE = "% $ < > 'tab' 'return' 'line feed' \\ \" / :";

/** `DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR::checkFootprintName`'s refusals: an empty name, a name with a character of `StringLibNameInvalidChars`; `null` when it is legal. */
export function checkFootprintName(name: string): string | null {
  if (name === "") return "Footprint must have a name.";
  for (const c of name) if (FOOTPRINT_NAME_INVALID.includes(c)) return `Footprint name may not contain '${FOOTPRINT_NAME_INVALID_READABLE}'.`;
  return null;
}

/** "Footprint '%s' already exists in library '%s'." -- the question `checkFootprintName` asks when the new name is taken (its OK button says "Overwrite"). */
export const footprintExistsMessage = (name: string, library: string): string => `Footprint '${name}' already exists in library '${library}'.`;

/** The `Lib:Name` of a footprint (`GetFPID()`) split: the library nickname and the item name the dialog's "Footprint name:" shows. */
export function fpidParts(fpid: string): { lib: string; item: string } {
  const i = fpid.indexOf(":");
  return i < 0 ? { lib: "", item: fpid } : { lib: fpid.slice(0, i), item: fpid.slice(i + 1) };
}

// ------------------------------------------------------------------------------------------------------------------------------- the paste clearance field

/** One length the user typed, as `EDA_UNIT_UTILS::UI::ValueFromString` reads it: a number with a unit (`mm`, `um`, `mil`, `in`, `"`), else in the user's units; `null` when it is no number. */
export function lengthFromText(text: string, units: LengthUnit): number | null {
  const m = /^([+-]?(?:\d+(?:[.,]\d*)?|[.,]\d+)(?:e[+-]?\d+)?)\s*(mm|um|µm|mil|mils|thou|in|")?$/i.exec(text.trim());
  if (!m) return null;
  const v = Number(m[1]!.replace(",", "."));
  if (!Number.isFinite(v)) return null;
  switch ((m[2] ?? "").toLowerCase()) {
    case "mm":
      return Math.round(v * 1e6);
    case "um":
    case "µm":
      return Math.round(v * 1e3);
    case "mil":
    case "mils":
    case "thou":
      return Math.round(v * 25_400);
    case "in":
    case '"':
      return Math.round(v * 25_400_000);
    default:
      return Math.round(nmFrom(v, units));
  }
}

/**
 * `MARGIN_OFFSET_BINDER::parseInput`: "-0.1mm", "-5%" or both ("-0.1mm - 5%"): an operator after a term splits the text into terms, a term ending in `%` is a ratio (a percent),
 * any other an offset; the signs before them add up. `{ offset, ratio }` with `null` for what the text does not hold; `null` for a text with neither (that is not empty).
 */
export function parseMarginOffset(input: string, units: LengthUnit): { offset: number | null; ratio: number | null } | null {
  const text = input.trim();
  if (text === "") return { offset: null, ratio: null };
  // an operator that follows a term is a term of its own
  let normalized = "";
  let lastWasOperator = true;
  for (const ch of text) {
    if ((ch === "+" || ch === "-") && !lastWasOperator) {
      normalized += ` ${ch} `;
      lastWasOperator = true;
    } else {
      normalized += ch;
      lastWasOperator = ch === "+" || ch === "-";
    }
  }
  const terms = normalized.split(" ").map((t) => t.trim()).filter((t) => t !== "");
  let totalOffset = 0;
  let totalRatio = 0;
  let hasOffset = false;
  let hasRatio = false;
  let sign = 1;
  for (const term of terms) {
    if (term === "+") {
      sign = 1;
      continue;
    }
    if (term === "-") {
      sign = -1;
      continue;
    }
    if (term.endsWith("%")) {
      const v = Number(term.slice(0, -1).trim());
      if (term.slice(0, -1).trim() !== "" && Number.isFinite(v)) {
        totalRatio += (sign * v) / 100;
        hasRatio = true;
      }
    } else {
      const v = lengthFromText(term, units);
      if (v !== null) {
        totalOffset += sign * v;
        hasOffset = true;
      }
    }
    sign = 1;
  }
  if (!hasOffset && !hasRatio) return null;
  return { offset: hasOffset ? Math.trunc(totalOffset) : null, ratio: hasRatio ? totalRatio : null };
}

/** `%.4g`. */
function g4(v: number): string {
  return Number(v.toPrecision(4)).toString();
}

/** `StringFromValue` without the unit label: the length in the user's units, its zeros trimmed. */
function lengthText(nm: number, units: LengthUnit): string {
  const decimals = units === "mm" ? 6 : units === "mil" ? 3 : 7;
  return Number(nmTo(nm, units).toFixed(decimals)).toString();
}

/** `MARGIN_OFFSET_BINDER::formatValue`: the offset (when it is not 0) and the ratio (when it is not 0) as one text, "-0.1 - 5%". */
export function formatMarginOffset(offset: number | null | undefined, ratio: number | null | undefined, units: LengthUnit): string {
  let result = offset != null && offset !== 0 ? lengthText(offset, units) : "";
  if (ratio != null && Math.abs(ratio) > 1e-9) {
    const percent = ratio * 100;
    if (result !== "") result += percent >= 0 ? ` + ${g4(percent)}%` : ` - ${g4(-percent)}%`;
    else result = `${g4(percent)}%`;
  }
  return result;
}

// ------------------------------------------------------------------------------------------------------------------------------- the tabs

/** The Layers, Clearance Overrides and Pad Connections tabs as the dialog edits them: the lengths as text (blank inherits), the grids as rows. */
export interface SettingsForm {
  clearance: string;
  solderMask: string;
  /** The "Solder paste clearance:" text (`MARGIN_OFFSET_BINDER`). */
  solderPaste: string;
  /** Whether the paste text was edited: an untouched field keeps the values it was given (`m_needsParsing` is set when the field takes the focus). */
  solderPasteTouched: boolean;
  privateLayers: string[];
  netTieGroups: string[];
  duplicatePadsAreJumpers: boolean;
  jumperGroups: string[];
}

/** `TransferDataToWindow` of the three tabs. */
export function settingsForm(s: FootprintSettings | undefined, units: LengthUnit): SettingsForm {
  const len = (v: number | null | undefined) => (v == null ? "" : lengthText(v, units));
  return {
    clearance: len(s?.clearance),
    solderMask: len(s?.solder_mask_margin),
    solderPaste: formatMarginOffset(s?.solder_paste_margin, s?.solder_paste_margin_ratio, units),
    solderPasteTouched: false,
    privateLayers: [...(s?.private_layers ?? [])],
    // `if( !group.IsEmpty() )`: a grid row per group
    netTieGroups: (s?.net_tie_pad_groups ?? []).filter((g) => g !== ""),
    duplicatePadsAreJumpers: !!s?.duplicate_pad_numbers_are_jumpers,
    jumperGroups: (s?.jumper_pad_groups ?? []).map((g) => g.join(", ")),
  };
}

/** `std::set<wxString>`: sorted by code point, no repeats. */
function sortedSet(v: string[]): string[] {
  return [...new Set(v)].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

/**
 * `Validate` and `TransferDataFromWindow` of the three tabs: the settings, or the message the dialog stops on -- a clearance below 0 ("Pad clearance must be at least %s.",
 * `m_netClearance.Validate( 0, INT_MAX )`), a length that is no number, a jumper group naming a pad the footprint has not ("Pad '%s' in jumper pad group %d does not exist in this
 * footprint."). `old` is what the footprint had: an untouched paste field keeps its offset and ratio.
 */
export function settingsOfForm(form: SettingsForm, padNumbers: ReadonlySet<string>, units: LengthUnit, old: FootprintSettings | undefined): { settings: FootprintSettings } | { error: string } {
  const length = (text: string, what: string): number | null | { error: string } => {
    if (text.trim() === "") return null;
    const v = lengthFromText(text, units);
    return v === null ? { error: `${what} is not a number.` } : v;
  };
  const clearance = length(form.clearance, "Pad clearance");
  if (clearance !== null && typeof clearance === "object") return clearance;
  if (clearance !== null && clearance < 0) return { error: `Pad clearance must be at least 0 ${units}.` };
  const mask = length(form.solderMask, "Solder mask expansion");
  if (mask !== null && typeof mask === "object") return mask;
  let pasteOffset = old?.solder_paste_margin ?? null;
  let pasteRatio = old?.solder_paste_margin_ratio ?? null;
  if (form.solderPasteTouched) {
    const parsed = parseMarginOffset(form.solderPaste, units);
    // a text that does not parse leaves the values as they were (`parseInput` returns false and the cache stays)
    if (parsed) {
      pasteOffset = parsed.offset;
      pasteRatio = parsed.ratio;
    }
  }
  const jumper_pad_groups: string[][] = [];
  for (let ii = 0; ii < form.jumperGroups.length; ii++) {
    const group: string[] = [];
    for (const token of form.jumperGroups[ii]!.split(/[, \t\r\n]/)) {
      if (token === "") continue;
      if (!padNumbers.has(token)) return { error: `Pad '${token}' in jumper pad group ${ii + 1} does not exist in this footprint.` };
      group.push(token);
    }
    jumper_pad_groups.push(sortedSet(group));
  }
  const settings: FootprintSettings = {};
  if (clearance !== null) settings.clearance = clearance;
  if (mask !== null) settings.solder_mask_margin = mask;
  if (pasteOffset !== null) settings.solder_paste_margin = pasteOffset;
  if (pasteRatio !== null) settings.solder_paste_margin_ratio = pasteRatio;
  if (form.privateLayers.length > 0) settings.private_layers = [...form.privateLayers];
  const ties = form.netTieGroups.filter((g) => g !== "");
  if (ties.length > 0) settings.net_tie_pad_groups = ties;
  if (form.duplicatePadsAreJumpers) settings.duplicate_pad_numbers_are_jumpers = true;
  if (jumper_pad_groups.length > 0) settings.jumper_pad_groups = jumper_pad_groups;
  return { settings };
}

/**
 * The layers the Private Layers grid offers (`LAYERS_GRID_TABLE( m_frame, forbiddenLayers )`: every layer but copper, the technical layers, `Edge.Cuts` and `Margin` -- the user layers),
 * of the board's layers, in the board's order; `onLayerGridRowAddUserLayer` adds the first that is not in the grid yet.
 */
export function privateLayerChoices(boardLayers: readonly string[]): string[] {
  const user = (l: string) => ["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User"].includes(l) || /^User\.\d+$/.test(l);
  const out = boardLayers.filter(user);
  // KiCad's board always has the four documentation layers and User.1..4 (`LSET::UserDefinedLayersMask( 4 )`) in the editor's board
  for (const l of ["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "User.1", "User.2", "User.3", "User.4"]) if (!out.includes(l)) out.push(l);
  return out;
}

/** `onLayerGridRowAddUserLayer`: the first user layer that is not in the grid yet, or `null` when every one is. */
export function nextPrivateLayer(rows: readonly string[], choices: readonly string[]): string | null {
  return choices.find((l) => !rows.includes(l)) ?? null;
}
