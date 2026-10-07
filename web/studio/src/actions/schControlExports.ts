// The control actions that hand the browser a file: Save Current Sheet Copy As... (the sheet being shown as a `.kicad_sch`), and -- added with their
// routes -- the legacy BOM, the sheet drawing on the clipboard and the symbols exported to a library. Plain async functions over the action context.
import type { SchControlContext } from "./schControlActions";
import { fetchHierarchy, fetchSchematicFiles } from "../api/schControlClient";
import { fileStem } from "../kicad-port/saveAs";
import { samePath } from "../kicad-port/sheetPages";
import { saveTextFile } from "../api/libraryClient";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * `SCH_EDITOR_CONTROL::SaveCurrSheetCopyAs`: `m_frame->saveSchematicFile( curr_sheet, newFilename )` writes the one screen being shown to a new
 * file -- the root sheet under the board's name, a sub-sheet under its own file name (the sheet blocks that place it name it).
 */
export async function saveSheetCopy(ctx: SchControlContext): Promise<void> {
  const toast = (text: string, kind: "info" | "error") => ctx.dispatch({ type: "TOAST", message: text, kind });
  try {
    const [files, hierarchy] = await Promise.all([fetchSchematicFiles(), fetchHierarchy()]);
    const path = ctx.state.currentSheetPath;
    const sheet = hierarchy.find((h) => samePath(h.path, path));
    const atRoot = path.length === 0;
    const source = atRoot ? files[0] : files.find((f) => f.name === sheet?.file);
    if (!source) return toast("This sheet has no file to copy.", "error");
    const name = atRoot ? `${fileStem(ctx.state.board?.name ?? "board")}.kicad_sch` : source.name;
    saveTextFile(source.text, name);
    toast(`Saved a copy of this sheet as ${name}`, "info");
  } catch (e) {
    toast(`Could not save the sheet: ${message(e)}`, "error");
  }
}
