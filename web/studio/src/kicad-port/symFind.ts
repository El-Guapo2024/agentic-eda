// Find and Replace in the Symbol Editor: `SCH_FIND_REPLACE_TOOL` (eeschema/tools/sch_find_replace_tool.cpp at 8303b2ad) in a `SYMBOL_EDIT_FRAME`, over the
// symbol being edited, and the dialog's rules there (`DIALOG_SCH_FIND`'s constructor for `FRAME_SCH_SYMBOL_EDITOR`). Pure: the walk, the rules of which item
// matches on which of its texts, the next match and what a Replace changes; the matching of a text itself is the schematic's matcher on the server
// (`POST /api/find/text`, `eda_ops::search::matches_text` / `replace_text`), so a regular expression is wxRegEx's there too.
//
// - The items are `symbol->GetDrawItems()` -- every unit's and body style's fields, pins, texts and text boxes (shapes match nothing) -- or, with "Search the
//   current selection only", the selection (`nextMatch`'s `addItem`); sorted by position, x then y (`GetPosition()`, KiCad's Y down), then by id.
// - A field matches on its unescaped text (`SCH_FIELD::Matches`: `UnescapeString( GetText() )`), a hidden one only with "Include hidden fields"
//   (`searchAllFields`), and the Reference never: its parent is a `LIB_SYMBOL`, not an `SCH_SYMBOL` ("we don't have a sheet path to resolve the
//   reference"). A pin matches on its name or its number (`SCH_PIN::Matches`; the dialog turns `searchAllPins` on in the Symbol Editor and hides the box), a
//   text and a text box on their text (`SCH_ITEM::Matches( GetText() )`).
// - Find (`FindNext`): the first match after the last one found (`m_afterItem`), backwards for Find Previous; none is "Reached end of symbol. Find again to
//   wrap around to the start.", and a Find within 4 s of it starts over.
// - Replace (`ReplaceAndFindNext`): the current match (the selection's first item, or with "selected only" the last found) if it still matches, then Find
//   Next; a pin has its name and its number replaced (`SCH_PIN::Replace` of a `LIB_SYMBOL`'s pin), a field, a text and a text box their text
//   (`EDA_TEXT::Replace`). One undo step, "Find and Replace".
// - Replace All: every match (of the selection, with "selected only"), one undo step "Find and Replace All". **Differs**: in KiCad's Symbol Editor its
//   `doReplace` returns at `wxCHECK_RET( aSheet, .. )` (there is no sheet path), so Replace All replaces nothing; here it replaces.
import type { LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import { allFields, type FieldRow } from "./symFields";
import { libFieldId, libFieldName } from "./symFieldLayout";
import { unescapeString } from "./schNetName";

export type SymFindKind = "field" | "pin" | "text" | "text_box";

/** One searchable item of the symbol: the id the editor selects it by, the texts `Matches` tests, and where it is (`GetPosition()`, nm, Y down). */
export interface SymFindItem {
  id: string;
  kind: SymFindKind;
  texts: string[];
  at: [number, number];
}

/** The status `FindNext` shows when the walk runs out in the Symbol Editor (`FRAME_SCH_SYMBOL_EDITOR`). */
export const SYMBOL_END_MESSAGE = "Reached end of symbol. Find again to wrap around to the start.";

const nm = (mm: number) => Math.round(mm * 1e6);

/** `SCH_SHAPE::GetPosition` of a text box: its start corner. */
function textBoxAt(g: Extract<LibrarySymbolGraphic, { kind: "text_box" }>): [number, number] {
  return [nm(g.start.x), -nm(g.start.y)];
}

/**
 * `nextMatch`'s `sorted_items` for the Symbol Editor: the searchable items of `sym` (the fields as the editor holds them, `fields`), the hidden fields only when
 * `searchHiddenFields`, and of the selection only when `selection` is given; sorted by x, then y, then id.
 */
export function symFindItems(sym: LibrarySymbol, fields: readonly FieldRow[], searchHiddenFields: boolean, selection: ReadonlySet<string> | null): SymFindItem[] {
  const out: SymFindItem[] = [];
  for (const f of fields) {
    // `if( !IsVisible() && !searchHiddenFields ) return false`; the Reference of a library symbol never matches
    if (f.name === "Reference") continue;
    if (f.hidden && !searchHiddenFields) continue;
    out.push({ id: libFieldId(f.name), kind: "field", texts: [unescapeString(f.text)], at: [nm(f.at.x), -nm(f.at.y)] });
  }
  for (const p of sym.pins) {
    if (!p.id) continue;
    out.push({ id: p.id, kind: "pin", texts: [p.name, p.number], at: [nm(p.at.x), -nm(p.at.y)] });
  }
  for (const g of sym.graphics) {
    if (!g.id) continue;
    if (g.kind === "text") out.push({ id: g.id, kind: "text", texts: [g.text], at: [nm(g.at.x), -nm(g.at.y)] });
    else if (g.kind === "text_box") out.push({ id: g.id, kind: "text_box", texts: [g.text], at: textBoxAt(g) });
  }
  const chosen = selection ? out.filter((it) => selection.has(it.id)) : out;
  return chosen.sort((a, b) => (a.at[0] !== b.at[0] ? a.at[0] - b.at[0] : a.at[1] !== b.at[1] ? a.at[1] - b.at[1] : a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

/** The flat list of texts to send to the matcher, and for each item the range of its texts in it. */
export function textsOf(items: readonly SymFindItem[]): { texts: string[]; at: number[] } {
  const texts: string[] = [];
  const at: number[] = [];
  for (const it of items) {
    at.push(texts.length);
    texts.push(...it.texts);
  }
  return { texts, at };
}

/** Which items match, given the matcher's answer for each text: an item matches when one of its texts does. */
export function matchedItems(items: readonly SymFindItem[], matches: readonly boolean[]): boolean[] {
  const { at } = textsOf(items);
  return items.map((it, i) => it.texts.some((_, k) => matches[at[i]! + k] === true));
}

/**
 * `nextMatch( .., aAfter, .., reversed )`: the first matching item after `after` in the walk (reversed for Find Previous), or null. With `after` null the
 * walk starts at the beginning; an `after` the walk no longer has (`item == aAfter` never holds) finds nothing, as KiCad's `past_item` stays false.
 */
export function nextSymMatch(items: readonly SymFindItem[], matched: readonly boolean[], after: string | null, reversed: boolean): SymFindItem | null {
  const order = items.map((it, i) => ({ it, ok: matched[i] === true }));
  if (reversed) order.reverse();
  let past = after === null;
  for (const { it, ok } of order) {
    if (it.id === after) past = true;
    else if (past && ok) return it;
  }
  return null;
}

/** `getCurrentMatch`: what Replace acts on -- with "selected only" the last match found (`m_afterItem`), else the selection's first item. */
export function currentSymMatch(selectionOrder: readonly string[], cursor: string | null, selectedOnly: boolean): string | null {
  return selectedOnly ? cursor : (selectionOrder[0] ?? null);
}

/** The texts a Replace of `item` rewrites, in the order the matcher is asked: a pin's name and number, the stored (escaped) text of the others (`EDA_TEXT::Replace`). */
export function replaceTexts(item: SymFindItem, sym: LibrarySymbol, fields: readonly FieldRow[]): string[] {
  switch (item.kind) {
    case "pin": {
      const p = sym.pins.find((q) => q.id === item.id);
      return p ? [p.name, p.number] : [];
    }
    case "field": {
      const f = fields.find((q) => q.name === libFieldName(item.id));
      return f ? [f.text] : [];
    }
    default: {
      const g = sym.graphics.find((q) => q.id === item.id);
      return g && (g.kind === "text" || g.kind === "text_box") ? [g.text] : [];
    }
  }
}

/** One item's replaced texts, as the edit they make: a pin with its new name / number, a text or text box with its new text, a field with its new text. */
export type SymReplaceEdit = { kind: "pin"; pin: LibrarySymbolPin } | { kind: "graphic"; graphic: LibrarySymbolGraphic } | { kind: "field"; field: FieldRow };

/** `Replace`: the edit `replaced` (the matcher's new texts, null where nothing was replaced) makes of `item`, or null when it replaced nothing. */
export function replaceEdit(item: SymFindItem, sym: LibrarySymbol, fields: readonly FieldRow[], replaced: readonly (string | null)[]): SymReplaceEdit | null {
  if (replaced.every((r) => r === null)) return null;
  switch (item.kind) {
    case "pin": {
      const p = sym.pins.find((q) => q.id === item.id);
      if (!p) return null;
      // `isReplaced |= EDA_ITEM::Replace( aSearchData, m_name ); isReplaced |= EDA_ITEM::Replace( aSearchData, m_number );`
      return { kind: "pin", pin: { ...p, name: replaced[0] ?? p.name, number: replaced[1] ?? p.number } };
    }
    case "field": {
      const f = fields.find((q) => q.name === libFieldName(item.id));
      return f && replaced[0] != null ? { kind: "field", field: { ...f, text: replaced[0] } } : null;
    }
    default: {
      const g = sym.graphics.find((q) => q.id === item.id);
      return g && (g.kind === "text" || g.kind === "text_box") && replaced[0] != null ? { kind: "graphic", graphic: { ...g, text: replaced[0] } } : null;
    }
  }
}

/** `SCH_BASE_FRAME::ShowFindReplaceDialog`'s starting text in the Symbol Editor: the one selected field's unescaped text, or a selected text's first line. */
export function findStringFromSelection(selection: readonly string[], sym: LibrarySymbol | null, fields: readonly FieldRow[]): string {
  if (selection.length !== 1 || !sym) return "";
  const id = selection[0]!;
  const name = libFieldName(id);
  if (name !== null) {
    const f = fields.find((q) => q.name === name);
    return f ? unescapeString(f.text) : "";
  }
  const g = sym.graphics.find((q) => q.id === id);
  if (g && g.kind === "text") {
    const t = unescapeString(g.text);
    return t.includes("\n") ? t.slice(0, t.indexOf("\n")) : t;
  }
  return "";
}

/** The fields the editor holds for the symbol it shows (`allFields` of the flat symbol). */
export function editorFields(sym: LibrarySymbol): FieldRow[] {
  return allFields(sym);
}
