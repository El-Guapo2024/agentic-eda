// The Footprint Editor's Paste (`PCB_CONTROL::Paste` with `isFootprintEditor`, pcbnew/tools/pcb_control.cpp at 8303b2ad; UI-LEFT-2 FF1): which clipboard texts are
// KiCad's (`CLIPBOARD_IO::Parse` reads a footprint's `(footprint ..)` -- a `(module ..)` of KiCad 6 too -- or a board's `(kicad_pcb ..)`; their items go in through the
// `fp_item paste` verb, crates/ops/src/fp_copy.rs), and the text item any other text becomes ("If it wasn't content, then paste as a text object": the text as it is on
// the active layer, with `EDA_TEXT`'s own defaults, as the board's paste makes it -- kicad-port/plainTextPaste.ts).
//
// Pure: no React, no DOM.
import type { CmdText } from "../api/types";
import { plainTextPaste } from "./plainTextPaste";
import { fpFieldById, fpItemKind, type Fp, type FpItemKind } from "./fpItems";
import { shownTextOf } from "./pcbTextBox";
import { cellAt } from "./tables";
import { selectionAsText } from "./itemText";

/** `CLIPBOARD_IO::Parse` succeeds (`"kicad"`) or the paste makes a text of the clipboard (`"text"`); an empty clipboard pastes nothing (`"none"`). */
export function fpClipboardKind(text: string): "kicad" | "text" | "none" {
  if (text === "") return "none";
  return /^\s*\(\s*(footprint|module|kicad_pcb)\b/.test(text) ? "kicad" : "text";
}

/** The text item a plain-text paste adds to the footprint (`item->SetText( clipText ); item->SetLayer( GetActiveLayer() )`), and whether KiCad asks first (`longText`). */
export function fpPlainTextPaste(text: string, at: { x: number; y: number }, layer: string): { text: CmdText; longText: boolean } | null {
  const p = plainTextPaste(text, at, layer);
  if (!p || p.cmd.op !== "add_text") return null;
  return { text: { ...p.cmd.text }, longText: p.longText };
}

// ------------------------------------------------------------------------------------------------------------------------------- Copy as Text

/** `EDIT_TOOL::Init`'s `canCopyAsText`: a selection of only fields, texts, text boxes, dimensions and tables (`OnlyTypes( { PCB_FIELD_T, PCB_TEXT_T, PCB_TEXTBOX_T, PCB_DIM_*, PCB_TABLE_T, PCB_TABLECELL_T } )`), not empty. */
export function fpCanCopyAsText(fp: Fp, ids: readonly string[]): boolean {
  return ids.length > 0 && ids.every((id) => COPY_AS_TEXT_KINDS.has(fpItemKind(fp, id) ?? "pad"));
}
const COPY_AS_TEXT_KINDS: ReadonlySet<FpItemKind> = new Set(["field", "text", "textbox", "dimension", "table"]);

/**
 * The text Copy as Text takes from the item `id` of the footprint (`EDIT_TOOL::copyToClipboardAsText`'s `getItemText`, UI-LEFT-3 F3-10), `undefined` for one it does not copy:
 * a text's, a field's, a text box's, a dimension's `GetShownText( true )`, a table's cells a tab between the cells of a row and a newline between rows. In the Footprint
 * Editor the shown text is the text as it is: its board is a `BOARD_USE::FPHOLDER`, whose footprint resolves no variable (`FOOTPRINT::ResolveTextVar`'s first line).
 * A dimension's is the text it draws (the server's `text`).
 */
export function fpItemAsText(fp: Fp, id: string): string | undefined {
  switch (fpItemKind(fp, id)) {
    case "text":
      return fp.texts.find((t) => t.id === id)?.content;
    case "field":
      return fpFieldById(fp, id)?.text;
    case "textbox": {
      const b = fp.display?.text_boxes.find((x) => x.id === id);
      return b ? shownTextOf(b) : undefined;
    }
    case "dimension":
      return fp.display?.dimensions.find((x) => x.id === id)?.text;
    case "table": {
      const t = fp.display?.tables.find((x) => x.id === id);
      if (!t) return undefined;
      const rows: string[] = [];
      for (let row = 0; row < t.rows; row++) {
        const cells: string[] = [];
        for (let col = 0; col < t.cols; col++) {
          const c = cellAt(t, row, col);
          cells.push(c ? (c.shown ?? c.text) : "");
        }
        rows.push(cells.join("\t"));
      }
      return rows.join("\n");
    }
    default:
      return undefined;
  }
}

/** Copy as Text of `ids`: each item's text, trimmed, the empty ones left out, a newline between them (`wxJoin( itemTexts, '\n', '\0' )`); "" for nothing to copy. */
export function fpSelectionAsText(fp: Fp, ids: readonly string[]): string {
  return selectionAsText(ids.map((id) => fpItemAsText(fp, id)));
}
