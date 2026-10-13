// Which of the file-in dialogs is open: Import Graphics (`DIALOG_IMPORT_GRAPHICS`, `DIALOG_IMPORT_GFX_SCH`), with the file it holds. Plain module state with subscribers, like
// `state/commonDialogs.ts` and `state/libraryTableDialogs.ts`, so an action registered in the action runner (or the drop host) can open one without a place in the big reducer;
// `components/ImportGraphicsDialog.tsx` renders it.
import { useSyncExternalStore } from "react";

/** The editor the dialog imports into: the board, the Footprint Editor, the schematic or the Symbol Editor. */
export type ImportEditor = "pcb" | "footprint" | "schematic" | "symbol";

/** A file the page has read: its name and its text. */
export interface PickedFile {
  name: string;
  text: string;
}

export interface ImportGraphicsDialog {
  editor: ImportEditor;
  /** The file in the dialog's "File:" field; `null` until one is chosen or dropped. */
  file: PickedFile | null;
}

let current: ImportGraphicsDialog | null = null;
const listeners = new Set<() => void>();

function set(next: ImportGraphicsDialog | null): void {
  current = next;
  for (const l of listeners) l();
}

export function getImportGraphicsDialog(): ImportGraphicsDialog | null {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useImportGraphicsDialog(): ImportGraphicsDialog | null {
  return useSyncExternalStore(subscribe, getImportGraphicsDialog, getImportGraphicsDialog);
}

/** Import Graphics... (`placeImportedGraphics` / `importGraphics`), or the same dialog with the file that was dropped (`SetFilenameOverride`). A dialog that is open takes the new file. */
export function openImportGraphics(editor: ImportEditor, file: PickedFile | null = null): void {
  set({ editor, file: file ?? (current && current.editor === editor ? current.file : null) });
}

export function closeImportGraphics(): void {
  set(null);
}
