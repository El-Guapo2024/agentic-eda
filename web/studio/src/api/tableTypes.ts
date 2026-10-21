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

/** `StrokeProps`: the rows of the Properties panel's "Table Properties" group, each of which changes one field of the border or of the separators. */
export interface TableStrokeProps {
  external?: boolean;
  header?: boolean;
  border_width_um?: Um;
  border_style?: TableLineStyleName;
  /** A colour with no alpha is `COLOR4D::UNSPECIFIED` and clears it. */
  border_color?: { r: number; g: number; b: number; a: number };
  rows?: boolean;
  cols?: boolean;
  separators_width_um?: Um;
  separators_style?: TableLineStyleName;
  separators_color?: { r: number; g: number; b: number; a: number };
}

/** `TablePatch`: what the Table Properties dialog and the Properties panel change in the table itself. A field left out stays. */
export interface TablePatch {
  /** "Start X" / "Start Y". */
  at?: PointXY;
  border?: TableBorderEdit;
  separators?: TableSeparatorsEdit;
  props?: TableStrokeProps;
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

/** `BoardNumbers` (crates/ops/src/pcb_table_builders.rs): what the characteristics table prints that kicad-cli measures (`pcb export stats`), in nanometres. */
export interface TableBoardNumbers {
  thickness_nm: number;
  width_nm: number;
  height_nm: number;
  min_track_nm: number;
  min_clearance_nm: number;
  min_drill_nm: number;
  castellated_pads?: number;
  press_fit_pads?: number;
}

export type TableCmdWire =
  | { verb: "draw"; layer: string; at: PointXY; cols: number; rows: number; cell_w_um: Um; cell_h_um: Um; line_width_um: Um; locked?: boolean; patch?: TablePatch }
  | { verb: "place_stackup"; layer: string; at: PointXY; units?: string }
  | { verb: "place_characteristics"; layer: string; at: PointXY; units?: string; numbers: TableBoardNumbers }
  | { verb: "delete"; id: string }
  | { verb: "set"; id: string; layer?: string; locked?: boolean; patch?: TablePatch }
  | { verb: "edit_cells"; id: string; cells: TableCellAddr[]; edit: TableCellEdit }
  | { verb: "add_row"; id: string; row: number; above?: boolean }
  | { verb: "add_column"; id: string; col: number; before?: boolean }
  | { verb: "delete_rows"; id: string; rows: number[] }
  | { verb: "delete_columns"; id: string; cols: number[] }
  | { verb: "merge_cells"; id: string; cells: TableCellAddr[] }
  | { verb: "unmerge_cells"; id: string; cells: TableCellAddr[] };
