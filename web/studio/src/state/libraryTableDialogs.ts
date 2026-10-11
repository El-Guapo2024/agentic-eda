// Which of the library table dialogs is open: Manage Symbol Libraries / Manage Footprint Libraries (`DIALOG_EDIT_LIBRARY_TABLES` with `PANEL_SYM_LIB_TABLE` /
// `PANEL_FP_LIB_TABLE`), Configure Paths (`DIALOG_CONFIGURE_PATHS`) and New / Add Library (the file dialog of `SYMBOL_EDIT_FRAME::AddLibraryFile`,
// `PCB_BASE_EDIT_FRAME::CreateNewLibrary` and `AddLibrary`). Plain module state with subscribers, like `state/commonDialogs.ts`, so an action registered in the action
// runner can open one without a place in the big reducer; `components/libraryTables/LibraryTableDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { TableKind } from "../kicad-port/libraryTable";

export interface LibraryTableDialogs {
  /** Manage Symbol Libraries / Manage Footprint Libraries: which of the two is open. */
  tables: TableKind | null;
  /** Configure Paths. */
  paths: boolean;
  /** New Library (`create`) or Add Library (an existing one) for this kind. */
  library: { kind: TableKind; create: boolean } | null;
}

const CLOSED: LibraryTableDialogs = { tables: null, paths: false, library: null };

let current: LibraryTableDialogs = CLOSED;
const listeners = new Set<() => void>();

function set(next: LibraryTableDialogs): void {
  current = next;
  for (const l of listeners) l();
}

export function getLibraryTableDialogs(): LibraryTableDialogs {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useLibraryTableDialogs(): LibraryTableDialogs {
  return useSyncExternalStore(subscribe, getLibraryTableDialogs, getLibraryTableDialogs);
}

/** `common.SuiteControl.showSymbolLibTable` / `showFootprintLibTable`. */
export function openLibraryTables(kind: TableKind): void {
  set({ ...current, tables: kind });
}

export function closeLibraryTables(): void {
  set({ ...current, tables: null });
}

/** `common.SuiteControl.configurePaths`. */
export function openConfigurePaths(): void {
  set({ ...current, paths: true });
}

export function closeConfigurePaths(): void {
  set({ ...current, paths: false });
}

/** `common.Control.newLibrary` (`create`) and `common.Control.addLibrary`. */
export function openLibraryFile(kind: TableKind, create: boolean): void {
  set({ ...current, library: { kind, create } });
}

export function closeLibraryFile(): void {
  set({ ...current, library: null });
}
