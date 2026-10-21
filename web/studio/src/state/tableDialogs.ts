// Which of the table dialogs is open and what it is about: Table Properties (`DIALOG_TABLE_PROPERTIES`, for a table that exists or for the one Draw Table has just sized) and
// Table Cell Properties (`DIALOG_TABLECELL_PROPERTIES`, the cells selected). Plain module state with subscribers (like `state/commonDialogs.ts`), so an action registered in the
// action runner can open a dialog without a place in the big reducer. `components/TableDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { TableCellAddr } from "../api/tableTypes";

/** What Draw Table has made when its second click lands: the layer, where the first corner is, how many cells and how big each is. Nothing is on the board until the dialog's OK. */
export interface DrawnTable {
  layer: string;
  at: [number, number];
  cols: number;
  rows: number;
  cellW: number;
  cellH: number;
  /** `bds.GetLineThickness( layer )`: the width of the border and of the separators the new table starts with. */
  lineWidth: number;
}

/** Whose table the dialog is about: the board's or the sheet's (the same dialogs, the sheet's without a layer and a lock). */
export type TableScope = "pcb" | "sch";

export type TableDialog = { mode: "edit"; tableId: string; scope: TableScope } | { mode: "draw"; drawn: DrawnTable; scope: TableScope };

export interface TableCellDialog {
  tableId: string;
  cells: TableCellAddr[];
  scope: TableScope;
}

export interface TableDialogsState {
  table: TableDialog | null;
  cells: TableCellDialog | null;
}

const CLOSED: TableDialogsState = { table: null, cells: null };

let current: TableDialogsState = CLOSED;
const listeners = new Set<() => void>();

function set(next: TableDialogsState): void {
  current = next;
  for (const l of listeners) l();
}

export function getTableDialogs(): TableDialogsState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useTableDialogs(): TableDialogsState {
  return useSyncExternalStore(subscribe, getTableDialogs, getTableDialogs);
}

/** `DIALOG_TABLE_PROPERTIES` on a table of the board. */
export function openTableDialog(tableId: string, scope: TableScope = "pcb"): void {
  set({ ...current, table: { mode: "edit", tableId, scope } });
}

/** Draw Table's second click: the dialog on the table that is not on the board yet. */
export function openDrawnTableDialog(drawn: DrawnTable, scope: TableScope = "pcb"): void {
  set({ ...current, table: { mode: "draw", drawn, scope } });
}

export function closeTableDialog(): void {
  set({ ...current, table: null });
}

/** `DIALOG_TABLECELL_PROPERTIES` on `cells` of the table. */
export function openTableCellDialog(tableId: string, cells: TableCellAddr[], scope: TableScope = "pcb"): void {
  set({ ...current, cells: { tableId, cells, scope } });
}

export function closeTableCellDialog(): void {
  set({ ...current, cells: null });
}

/** Is one of the table dialogs open (the canvas leaves its keys alone then)? */
export function tableDialogOpen(): boolean {
  return current.table != null || current.cells != null;
}
