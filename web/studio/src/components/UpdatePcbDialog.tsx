// Update PCB from Schematic (F8, `DIALOG_UPDATE_PCB`) and Update Schematic from PCB (`DIALOG_BOARD_UPDATE_SCHEMATIC`'s back-annotation), the footprint half:
// the dialog lists what the update would do -- the dry run of `BOARD_NETLIST_UPDATER` ("Change R1 footprint from 'A' to 'B'.") -- and its button applies it, as one
// undo step, and tells it again in the past. The schematic and the board are one design here, so the rest of KiCad's list (new footprints, nets, values) is already
// done as each schematic edit is made; the line at the top says so and counts what is left to place.
import { useState } from "react";
import type { UpdatePcbChange } from "../api/fpAssign";
import { fetchUpdatePcb } from "../api/fpAssign";
import { backAnnotateCommands, backAnnotateLines, changeLines, updateCommands, updatePcbMessage, type UpdatePart } from "../kicad-port/updatePcb";
import type { ReportLine } from "../kicad-port/pcbExchange";
import type { Cmd } from "../api/types";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { useEscape } from "./pcbDialogKit";
import { ReportWindow } from "./ReportWindow";

export type UpdateDirection = "pcb" | "schematic";

/** `apply` sends the commands as one undo step and answers why not (the server's reason) when it was refused, `null` when it was done. */
export function UpdatePcbDialog({ direction, initial, parts, apply }: { direction: UpdateDirection; initial: UpdatePcbChange[]; parts: readonly UpdatePart[]; apply: (cmds: Cmd[]) => Promise<string | null> }) {
  useEscape();
  const [changes, setChanges] = useState<UpdatePcbChange[]>(initial);
  const [told, setTold] = useState<ReportLine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const pcb = direction === "pcb";
  const cmds = pcb ? updateCommands(changes) : backAnnotateCommands(changes);
  const dry: ReportLine[] = pcb ? changeLines(changes, false) : backAnnotateLines(changes, false);
  const title = pcb ? "Update PCB from Schematic" : "Update Schematic from PCB";

  const run = async () => {
    setBusy(true);
    try {
      const lines: ReportLine[] = pcb ? changeLines(changes, true) : backAnnotateLines(changes, true);
      const refused = await apply(cmds);
      if (refused === null) {
        setTold(lines);
        const fresh = await fetchUpdatePcb();
        if (fresh.ok) setChanges(fresh.changes);
      } else {
        setTold([{ severity: "error", text: refused }]);
      }
    } finally {
      setBusy(false);
    }
  };

  const status = pcb ? updatePcbMessage(parts) : "The schematic names the footprints the board has, except for the symbols listed.";
  const shown: ReportLine[] = told ?? (dry.length > 0 ? dry : [{ severity: "info", text: status }]);
  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()} data-testid="update-pcb-dialog">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginBottom: 6 }}>Changes to be applied:</div>
          <ReportWindow lines={shown} testId="update-pcb-messages" height={170} />
          {told && dry.length === 0 && <div style={{ fontSize: 11, marginTop: 6, color: "var(--chrome-text-dim)" }}>{status}</div>}
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog}>Close</button>
          <button className="primary" disabled={busy || cmds.length === 0} onClick={() => void run()} data-testid="update-pcb-apply">
            {pcb ? "Update PCB" : "Update Schematic"}
          </button>
        </div>
      </div>
    </div>
  );
}
