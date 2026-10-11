// The clicks of the Import Sheet tool (`SCH_DRAWING_TOOLS::ImportSheet` once a file is chosen, then `DrawSheet` with `drawSheetFromFile`, and `placeSheetContents`):
//   as a sheet   the first click is the sheet's top-left corner, the second sizes it (`sizeSheet`) and opens SheetDialog with the file (name and file from the file's name);
//   as contents  one click: the file's items come in on the sheet in view, carried to the click, numbered anew unless "Keep annotations".
import type { Dispatch } from "react";
import { fetchSchematic } from "../../api/client";
import type { Cmd } from "../../api/types";
import type { Action, StudioApi, StudioState } from "../../state/store";
import { rememberedAnnotateSettings } from "../../kicad-port/schAnnotate";
import { newIdsAfter } from "../../kicad-port/schClipboard";
import { importContentsCmd, importNumbering } from "../../kicad-port/schImportSheet";
import { sheetSize } from "../../kicad-port/schSheet";

/**
 * Send `cmd` (one undo step) and, when it worked, select what it put on the sheet in view (`AddItemToSel( sheet )` / the new items, which KiCad leaves selected). Resolves to whether it worked.
 */
export async function sendAndSelectNew(api: Pick<StudioApi, "cmd" | "getState">, dispatch: Dispatch<Action>, cmd: Cmd): Promise<boolean> {
  const before = api.getState().schematic;
  const path = [...api.getState().currentSheetPath];
  const ok = await api.cmd(cmd);
  if (!ok || !before) return ok;
  try {
    const ids = newIdsAfter(before, await fetchSchematic(path));
    if (ids.length > 0) dispatch({ type: "SET_SELECTION", refs: ids });
  } catch {
    // The poll shows the new items; only the selection of them is lost.
  }
  return ok;
}

export function importSheetClick(state: Pick<StudioState, "schSheetImport" | "drawState">, at: [number, number], grid: number, api: Pick<StudioApi, "cmd" | "getState">, dispatch: Dispatch<Action>): void {
  const plan = state.schSheetImport;
  if (!plan) return;
  if (!plan.asSheet) {
    // `placeSheetContents`: the items follow the cursor until the click puts them down; one command, so one undo step. Without repeated copies the tool ends.
    void sendAndSelectNew(api, dispatch, importContentsCmd(plan, at, importNumbering(rememberedAnnotateSettings()))).then((ok) => {
      if (ok && !plan.repeated) dispatch({ type: "SET_SCH_SHEET_IMPORT", plan: null });
    });
    return;
  }
  const draw = state.drawState;
  if (draw?.kind !== "sheet") {
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sheet", start: at } });
    return;
  }
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  dispatch({ type: "SET_SCH_SHEET_PENDING", pending: { at: draw.start, size: sheetSize(draw.start, at, grid), source: plan } });
}
