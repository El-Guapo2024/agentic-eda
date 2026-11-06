// KiCad's `IsContentModified()` per editor, for the questions that ask it ("The current PCB has been modified.  Save changes?" before an import replaces the board,
// `PCB_EDIT_FRAME::OpenProjectFiles`; the schematic's in `SCH_EDIT_FRAME::AskToSaveChanges`). KiCad's flag is set by every change of the editor's content (`OnModify`) and
// cleared by a save. Here every edit is written to design.json as it is made, so "modified" is what this page has changed since it opened or since File > Save: a command
// the server says modified that editor's content (`/api/cmd`'s `domain`, the step's history tag: `both` is both editors), and an Undo or Redo in it. A reload starts
// clean, as KiCad's flag does on opening the file. Plain module state with no React, so the API client can set it.

export type ModifiedEditor = "pcb" | "schematic";

const modified: Record<ModifiedEditor, boolean> = { pcb: false, schematic: false };

/** A step of the history was made: `domain` is its tag (`pcb`, `schematic`, `both`, `file`, `footprint_editor`, `symbol_editor`). */
export function noteContentModified(domain: string | undefined): void {
  if (domain === "pcb" || domain === "both" || domain === "file") modified.pcb = true;
  if (domain === "schematic" || domain === "both" || domain === "file") modified.schematic = true;
}

export function isContentModified(editor: ModifiedEditor): boolean {
  return modified[editor];
}

/** `SavePcbFile` / `SaveProject` cleared the flag (`GetScreen()->SetContentModified( false )`). */
export function markContentSaved(editor: ModifiedEditor): void {
  modified[editor] = false;
}
