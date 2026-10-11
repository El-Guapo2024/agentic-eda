// The file-in actions: Import Graphics (File > Import > Graphics... in the board editor, `DRAWING_TOOL::PlaceImportedGraphics`; the schematic's and the symbol editor's twin is
// `SCH_DRAWING_TOOLS::ImportGraphics`) and the drops that reach them (`DdImportGraphics`: a DXF or SVG dropped on the window). Append Board and its drop are `appendBoardActions`
// below. A browser drop gives the page a file's text and not its path, so the drop actions take the file as their parameter -- `{ name, text }` -- where KiCad's takes a file name.
import { openImportGraphics, type PickedFile } from "../state/importDialogs";
import { graphicsFormatOf } from "../kicad-port/importGraphics";
import { appendBoardCmd, describeAppended, isKicadBoardName } from "../kicad-port/appendBoard";
import { readAppendBoard } from "../api/importClient";
import { pickTextFile } from "../api/libraryClient";
import type { Action, StudioApi } from "../state/store";
import type { Dispatch } from "react";
import type { ActionHandler } from "./commonActions";

/** A drop action's parameter: the file the drop host read. */
export function asPickedFile(param: unknown): PickedFile | null {
  if (!param || typeof param !== "object") return null;
  const f = param as Partial<PickedFile>;
  return typeof f.name === "string" && typeof f.text === "string" ? { name: f.name, text: f.text } : null;
}

export interface ImportActionContext {
  tab: string;
  dispatch: Dispatch<Action>;
  api: StudioApi;
}

/**
 * `PCB_CONTROL::AppendBoard`: the file's items are put on the board (the paste of its text, `Cmd::PasteClipboard`, one undo step) with the top left item at the cursor and carried by the
 * Move tool until a click drops them; Escape takes them back. What the paste leaves out or changes on this board is said afterwards.
 */
async function appendBoardFile(ctx: ImportActionContext, file: PickedFile): Promise<void> {
  const toast = (message: string, kind: "error" | "info") => ctx.dispatch({ type: "TOAST", message, kind });
  if (!isKicadBoardName(file.name)) return toast("Only KiCad boards (.kicad_pcb files) can be appended.", "error");
  let info;
  try {
    info = await readAppendBoard(file);
  } catch (e) {
    return toast(e instanceof Error ? e.message : String(e), "error");
  }
  if (!info.ok || !info.reference) return toast(info.message ?? "Error loading board.", "error");
  const reference = info.reference;
  const placed = await ctx.api.placeNewItems((at) => appendBoardCmd(file.text, reference, at), true);
  if (!placed) return; // the refusal is on screen as a toast
  const notes = info.notes ?? [];
  toast(`Appended ${describeAppended(info.counts)}.${notes.length > 0 ? ` ${notes.join(" ")}` : ""}`, "info");
}

export function registerImportActions(m: Map<string, ActionHandler>, ctx: ImportActionContext): void {
  if (ctx.tab === "pcb") {
    // PCB_ACTIONS::appendBoard ("Append Board..."): a file chooser; PCB_ACTIONS::ddAppendBoard: the board that was dropped on the window.
    m.set("pcbnew.EditorControl.appendBoard", () => {
      void (async () => {
        try {
          const file = await pickTextFile(".kicad_pcb");
          if (file) await appendBoardFile(ctx, file);
        } catch (e) {
          ctx.dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
        }
      })();
    });
    m.set("pcbnew.Control.DdAppendBoard", (param) => {
      const file = asPickedFile(param);
      if (file) void appendBoardFile(ctx, file);
    });
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
