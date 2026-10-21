// A table as the server sends it (`crates/cli/src/table_json.rs`), for the tests of the modules that read one.
import type { BoardTable, BoardTableCell } from "../api/types";
import { cellItemId } from "./tables";

/** A `rows` x `cols` table of 10 x 5 mm cells at the origin, as the server sends it. */
export function fixtureTable(rows: number, cols: number, layer = "F.SilkS"): BoardTable {
  const cells: BoardTableCell[] = [];
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const x = col * 10_000;
      const y = row * 5_000;
      cells.push({
        id: cellItemId("tbl_a", row, col),
        row,
        col,
        text: `r${row}c${col}`,
        col_span: 1,
        row_span: 1,
        corners: [
          [x, y],
          [x + 10_000, y],
          [x + 10_000, y + 5_000],
          [x, y + 5_000],
        ],
        margins: [1_000, 1_000, 1_000, 1_000],
        h_align: "left",
        v_align: "center",
        size_w: 1_270,
        size_h: 1_270,
        thickness: 0,
        pen: 159,
        bold: false,
        italic: false,
        mirror: false,
        face: null,
        line_spacing: 0,
        color: null,
        lines: [],
      });
    }
  }
  return {
    id: "tbl_a",
    layer,
    x: 0,
    y: 0,
    angle: 0,
    rows,
    cols,
    col_widths: Array(cols).fill(10_000),
    row_heights: Array(rows).fill(5_000),
    border: { external: true, header: true, stroke: { width: 100, style: "solid", color: null } },
    separators: { rows: true, cols: true, stroke: { width: 100, style: "solid", color: null } },
    bbox: [0, 0, cols * 10_000, rows * 5_000],
    borders: [],
    cells,
  };
}
