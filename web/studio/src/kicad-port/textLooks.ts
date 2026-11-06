// The looks Text, Label and Text Box Properties set beyond the text itself (UI-LEFT S6): the Font choice (`FONT_CHOICE`), the Color swatch and the Link of
// `DIALOG_TEXT_PROPERTIES` / `DIALOG_LABEL_PROPERTIES` (eeschema/dialogs/dialog_text_properties.cpp, dialog_label_properties.cpp, common/widgets/font_choice.cpp,
// common/eda_text.cpp), kept by the backend in `SchExtras::text_attrs` (`set_text_attrs`, crates/ops/src/sch_props.rs).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { SchColor } from "../api/schEditTypes";
import type { Cmd } from "../api/types";

/** `DEFAULT_FONT_NAME` and `KICAD_FONT_NAME` (include/font/kicad_font_name.h): the Font choice's first two entries. */
export const DEFAULT_FONT = "Default Font";
export const KICAD_FONT = "KiCad Font";
/** `FONT_CHOICE::m_notFound`: what follows a font the machine does not have. */
export const NOT_FOUND = " <not found>";

/**
 * The Font choice of a text (`FONT_CHOICE`: "Default Font", "KiCad Font", then the machine's families, sorted) with the text's own font selected
 * (`SetFontSelection`: no font is the first entry; a font the list does not have is appended with " <not found>").
 */
export function fontChoices(families: readonly string[], current: string | null | undefined): { choices: string[]; selected: string } {
  const choices = [DEFAULT_FONT, KICAD_FONT, ...[...new Set(families)].filter((f) => f !== DEFAULT_FONT && f !== KICAD_FONT).sort()];
  if (!current) return { choices, selected: DEFAULT_FONT };
  if (choices.includes(current)) return { choices, selected: current };
  const missing = current + NOT_FOUND;
  return { choices: [...choices, missing], selected: missing };
}

/**
 * `FONT_CHOICE::HaveFontSelection` and `GetFontSelection`: the font the choice gives the text -- `undefined` when it gives none (a font that was not found keeps
 * the text's own: "SetFont" is not called), "" for the default font (`nullptr`), else the font's name.
 */
export function chosenFont(selected: string): string | undefined {
  if (selected.endsWith(NOT_FOUND)) return undefined;
  return selected === DEFAULT_FONT ? "" : selected;
}

/** `EDA_TEXT::IsGotoPageHref` / `ValidateHyperlink`: none, "#" and a page of this schematic, or a URL with a scheme (`wxURI::HasScheme`). */
export function validHyperlink(url: string): boolean {
  if (url === "" || url.startsWith("#")) return true;
  const i = url.indexOf(":");
  if (i <= 0) return false;
  return /^[A-Za-z][A-Za-z0-9+.-]*$/.test(url.slice(0, i));
}

/** `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`'s refusal of a link that `ValidateHyperlink` refuses, word for word. */
export const HYPERLINK_ERROR = 'Invalid hyperlink destination. Please enter either a valid URL (e.g. file:// or http(s)://) or "#<page number>" to create a hyperlink to a page in this schematic.';

/** One entry of the Link combo: what it puts in the box and how the list shows it. */
export interface LinkChoice {
  value: string;
  label: string;
}

/**
 * The Link combo of Text Properties (`DIALOG_TEXT_PROPERTIES::DIALOG_TEXT_PROPERTIES`): "#<page>" for each sheet of the hierarchy, as "Page %s (%s)" with the sheet's
 * name ("<root sheet>" for the root), then "---", "file://...", "http://..." and "https://...". `sheets` are the hierarchy's sheet paths in order (the root's `depth`
 * is 0).
 */
export function linkChoices(sheets: ReadonlyArray<{ page: string; name: string; depth: number }>): LinkChoice[] {
  const out: LinkChoice[] = sheets.map((s) => ({ value: `#${s.page}`, label: `Page ${s.page} (${s.depth === 0 ? "<root sheet>" : s.name})` }));
  out.push({ value: "---", label: "---" });
  out.push({ value: "file://", label: "file://..." }, { value: "http://", label: "http://..." }, { value: "https://", label: "https://..." });
  return out;
}

/** What a text item's looks are now, as the scene reports them (`SchExtras::text_attrs`). */
export interface TextLooks {
  font?: string | null;
  color?: SchColor | null;
  href?: string | null;
  bold?: boolean;
  italic?: boolean;
  excludeFromSim?: boolean;
}

const sameColor = (a: SchColor | null | undefined, b: SchColor | null | undefined): boolean => {
  const ua = !a || a.a === 0;
  const ub = !b || b.a === 0;
  if (ua || ub) return ua === ub;
  return a!.r === b!.r && a!.g === b!.g && a!.b === b!.b && a!.a === b!.a;
};

/**
 * The `set_text_attrs` command for what a dialog changed of an item's looks, or null when nothing changed. `next.font` is `chosenFont`'s answer (undefined:
 * unchanged); a colour of all zeroes is the layer's (`COLOR4D::UNSPECIFIED`); `withStyle` says whether the item's bold and italic are kept here (a free text's: a
 * label keeps its own in Label Properties' verb, a text box in its shape).
 */
export function textAttrsCmd(id: string, now: TextLooks, next: TextLooks & { font?: string }, withStyle: boolean): Cmd | null {
  const cmd: Record<string, unknown> = { op: "sch_edit", verb: "set_text_attrs", ids: [id] };
  let changed = false;
  const set = (key: string, value: unknown) => {
    cmd[key] = value;
    changed = true;
  };
  if (next.font !== undefined && next.font !== (now.font ?? "")) set("font", next.font);
  if (next.color !== undefined && !sameColor(next.color, now.color)) set("color", next.color ?? { r: 0, g: 0, b: 0, a: 0 });
  if (next.href !== undefined && (next.href ?? "").trim() !== (now.href ?? "")) set("href", (next.href ?? "").trim());
  if (next.excludeFromSim !== undefined && next.excludeFromSim !== !!now.excludeFromSim) set("exclude_from_sim", next.excludeFromSim);
  if (withStyle && next.bold !== undefined && next.bold !== !!now.bold) set("bold", next.bold);
  if (withStyle && next.italic !== undefined && next.italic !== !!now.italic) set("italic", next.italic);
  return changed ? (cmd as unknown as Cmd) : null;
}

/** One undo step for a dialog's OK: the commands it built (nulls dropped), one alone, several as a batch, none as null. */
export function oneStep(cmds: ReadonlyArray<Cmd | null>): Cmd | null {
  const list = cmds.filter((c): c is Cmd => c !== null);
  if (list.length === 0) return null;
  return list.length === 1 ? list[0]! : ({ op: "batch", cmds: list } as unknown as Cmd);
}
