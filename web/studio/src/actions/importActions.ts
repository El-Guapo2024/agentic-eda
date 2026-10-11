// The file-in actions: Import Graphics (File > Import > Graphics... in the board editor, `DRAWING_TOOL::PlaceImportedGraphics`; the schematic's and the symbol editor's twin is
// `SCH_DRAWING_TOOLS::ImportGraphics`) and the drops that reach them (`DdImportGraphics`: a DXF or SVG dropped on the window). Append Board and its drop are `appendBoardActions`
// below. A browser drop gives the page a file's text and not its path, so the drop actions take the file as their parameter -- `{ name, text }` -- where KiCad's takes a file name.
import { openImportGraphics, type PickedFile } from "../state/importDialogs";
import { graphicsFormatOf } from "../kicad-port/importGraphics";
import type { Action } from "../state/store";
import type { Dispatch } from "react";
import type { ActionHandler } from "./commonActions";

/** A drop action's parameter: the file the drop host read. */
export function asPickedFile(param: unknown): PickedFile | null {
  if (!param || typeof param !== "object") return null;
  const f = param as Partial<PickedFile>;
  return typeof f.name === "string" && typeof f.text === "string" ? { name: f.name, text: f.text } : null;
}

export function registerImportActions(m: Map<string, ActionHandler>, ctx: { tab: string; dispatch: Dispatch<Action> }): void {
  if (ctx.tab === "pcb") {
    // PCB_ACTIONS::placeImportedGraphics ("Import Graphics..."): the dialog; PCB_ACTIONS::ddImportGraphics: the same dialog with the dropped file in its File: field.
    m.set("pcbnew.InteractiveDrawing.placeImportedGraphics", () => openImportGraphics("pcb"));
    m.set("pcbnew.InteractiveDrawing.ddImportGraphics", (param) => {
      const file = asPickedFile(param);
      // Only the extensions a plugin handles are accepted (`m_acceptedExts`).
      if (file && !graphicsFormatOf(file.name)) return ctx.dispatch({ type: "TOAST", message: "There is no plugin to handle this file type.", kind: "error" });
      openImportGraphics("pcb", file);
    });
  }
  if (ctx.tab === "schematic" || ctx.tab === "symbol") {
    // SCH_ACTIONS::importGraphics (the schematic's and the Symbol Editor's File > Import > Graphics...) and ddImportGraphics: DIALOG_IMPORT_GFX_SCH.
    const editor = ctx.tab === "symbol" ? "symbol" : "schematic";
    m.set("eeschema.EditorControl.importGraphics", () => openImportGraphics(editor));
    m.set("eeschema.EditorControl.ddImportGraphics", (param) => {
      const file = asPickedFile(param);
      if (file && !graphicsFormatOf(file.name)) return ctx.dispatch({ type: "TOAST", message: "There is no plugin to handle this file type.", kind: "error" });
      openImportGraphics(editor, file);
    });
  }
}
