// `EDIT_TOOL::copyToClipboardAsText` (pcbnew/tools/edit_tool.cpp), Ctrl+Shift+C on the board: for each selected item of `canCopyAsText`'s types, the text it
// shows (`GetShownText( true )`: variables resolved, which the backend sends as `shown`):
//
//   case PCB_TEXT_T: case PCB_FIELD_T: case PCB_DIM_*:   text.GetShownText( true )
//   case PCB_TEXTBOX_T: case PCB_TABLECELL_T:             textBox.GetShownText( true )
//   case PCB_TABLE_T:  each cell's shown text, a tab between cells of a row, a newline between rows
//
// The items' texts are then joined as `selectionAsText` does (kicad-port/itemText.ts).
import type { BoardState } from "../api/types";
import { fieldById } from "./fpFields";
import { shownTextOf } from "./pcbTextBox";
import { cellAt, cellById, tableById } from "./tables";

/** The text Copy as Text takes from the item `id` of the board, `undefined` for an item it does not copy. */
export function boardItemAsText(board: BoardState, id: string): string | undefined {
  const dr = board.drawings;
  const text = dr?.texts.find((t) => t.id === id);
  if (text) return text.shown ?? text.content;
  const dim = dr?.dimensions.find((d) => d.id === id);
  if (dim) return dim.text;
  const box = dr?.text_boxes?.find((b) => b.id === id);
  if (box) return shownTextOf(box);
  const table = tableById(board, id);
  if (table) {
    const rows: string[] = [];
    for (let row = 0; row < table.rows; row++) {
      const cells: string[] = [];
      for (let col = 0; col < table.cols; col++) {
        const c = cellAt(table, row, col);
        cells.push(c ? (c.shown ?? c.text) : "");
      }
      rows.push(cells.join("\t"));
    }
    return rows.join("\n");
  }
  const cell = cellById(board, id);
  if (cell) return cell.cell.shown ?? cell.cell.text;
  const field = fieldById(board, id);
  if (field) return field.field.shown ?? field.field.text;
  return undefined;
}
