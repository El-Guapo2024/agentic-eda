// The table verbs of crates/ops/src/pcb_tables.rs (`TableCmd`, sent as `{ op: "table", verb: ..., ... }`) and the pieces of them the dialogs build: the fields of the Table
// Properties and Table Cell Properties dialogs (crates/model/src/table_edit.rs `TablePatch`, `CellEdit`).
import type { PointXY, Um } from "./types";

export type TableLineStyleName = "default" | "solid" | "dash" | "dot" | "dash_dot" | "dash_dot_dot";

export type TableHAlign = "left" | "center" | "right";
export type TableVAlign = "top" | "center" | "bottom";

/** `BorderEdit`: the border half of Table Properties. With neither switch on the stroke is not drawn (width -1), as the dialog sets it. */
export interface TableBorderEdit {
  external: boolean;
  header: boolean;
  width_um: Um;
  style?: TableLineStyleName;
}

/** `SeparatorsEdit`. */
export interface TableSeparatorsEdit {
  rows: boolean;
  cols: boolean;
  width_um: Um;
  style?: TableLineStyleName;
}

/** `TablePatch`: what the Table Properties dialog and the Properties panel change in the table itself. A field left out stays. */
export interface TablePatch {
  /** "Start X" / "Start Y". */
  at?: PointXY;
  border?: TableBorderEdit;
  separators?: TableSeparatorsEdit;
  /** The text of every cell, row by row. */
  texts?: string[][];
}

/** `CellEdit`: the fields of Table Cell Properties and the cell rows of the Properties panel. A field left out stays. */
export interface TableCellEdit {
  text?: string;
  bold?: boolean;
  italic?: boolean;
  /** The font's name; an empty one is the default font. */
  face?: string;
  width_um?: Um;
  height_um?: Um;
  /** 0 is KiCad's automatic stroke width. */
  thickness_um?: Um;
  h_align?: TableHAlign;
  v_align?: TableVAlign;
  /** left, top, right, bottom; null leaves one alone. */
  margins?: [Um | null, Um | null, Um | null, Um | null];
  col_width_um?: Um;
  row_height_um?: Um;
}

/** A cell address: `[row, col]`, both from 0. */
export type TableCellAddr = [number, number];

export type TableCmdWire =
  | { verb: "draw"; layer: string; at: PointXY; cols: number; rows: number; cell_w_um: Um; cell_h_um: Um; line_width_um: Um; locked?: boolean; patch?: TablePatch }
  | { verb: "delete"; id: string }
  | { verb: "set"; id: string; layer?: string; locked?: boolean; patch?: TablePatch }
  | { verb: "edit_cells"; id: string; cells: TableCellAddr[]; edit: TableCellEdit }
  | { verb: "add_row"; id: string; row: number; above?: boolean }
  | { verb: "add_column"; id: string; col: number; before?: boolean }
  | { verb: "delete_rows"; id: string; rows: number[] }
  | { verb: "delete_columns"; id: string; cols: number[] }
  | { verb: "merge_cells"; id: string; cells: TableCellAddr[] }
  | { verb: "unmerge_cells"; id: string; cells: TableCellAddr[] };
