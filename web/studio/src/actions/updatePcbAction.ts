// F8 (`common.Control.updatePcbFromSchematic`) and Update Schematic from PCB (`common.Control.updateSchematicFromPCB`): ask the server which symbols name another
// footprint than the board has (crates/cli/src/fp_assign_api.rs) and open the dialog that lists and applies the change (components/UpdatePcbDialog.tsx).
import { createElement } from "react";
import type { Dispatch } from "react";
import { fetchUpdatePcb } from "../api/fpAssign";
import type { Action, StudioApi } from "../state/store";
import { UpdatePcbDialog, type UpdateDirection } from "../components/UpdatePcbDialog";
import { openSweepDialog } from "./pcbSweepDialogs";

export async function openUpdateDialog(api: StudioApi, dispatch: Dispatch<Action>, direction: UpdateDirection): Promise<void> {
  await api.refresh();
  const report = await fetchUpdatePcb();
  if (!report.ok) {
    dispatch({ type: "TOAST", message: report.message ?? "The update could not be worked out.", kind: "error" });
    return;
  }
  const parts = (api.getState().board?.parts ?? []).map((p) => ({ ref: p.ref, placed: p.placed }));
  openSweepDialog({
    kind: "element",
    element: createElement(UpdatePcbDialog, { direction, initial: report.changes, parts, apply: (cmds) => api.cmdBatch(cmds) }),
  });
}
