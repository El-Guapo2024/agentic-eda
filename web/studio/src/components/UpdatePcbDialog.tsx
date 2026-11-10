// Update PCB from Schematic (F8, `DIALOG_UPDATE_PCB`) and Update Schematic from PCB (`DIALOG_UPDATE_FROM_PCB`'s back-annotation), the footprint half:
// the dialog lists what the update would do -- the dry run of `BOARD_NETLIST_UPDATER` ("Change R1 footprint from 'A' to 'B'.") -- and its button applies it, as one
// undo step, and tells it again in the past. The schematic and the board are one design here, so the rest of KiCad's list (new footprints, nets, values) is already
// done as each schematic edit is made; the line at the top says so and counts what is left to place.
//
// Update Schematic from PCB also carries the board's nets back (UI-LEFT-2 PP17; KiCad 25ec2335 `BACK_ANNOTATE::applyChangelist`): after Swap Pad Nets or Swap Gate
// Nets the server's dry run (`POST /api/back_annotate`) lists the unit swaps, pin swaps and labels and answers the verbs, which go in the same batch. Its three
// options are the dialog's "Net names", "Prefer symbol unit swaps over label swaps" and "Prefer symbol pin swaps over label swaps" (on by default; a change runs the
// dry run again, `OnOptionChanged`); a pin swap also needs Preferences > Editing Options > "Allow unconstrained pin swaps". KiCad's back-annotation lines read the
// same before and after the update.
import { useEffect, useState } from "react";
import type { BackAnnotateOptions, BackAnnotateReport, UpdatePcbChange } from "../api/fpAssign";
import { fetchUpdatePcb, postBackAnnotate } from "../api/fpAssign";
import { backAnnotateCommands, backAnnotateLines, changeLines, updateCommands, updatePcbMessage, type UpdatePart } from "../kicad-port/updatePcb";
import type { ReportLine } from "../kicad-port/pcbExchange";
import type { Cmd } from "../api/types";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { useEscape } from "./pcbDialogKit";
import { ReportWindow } from "./ReportWindow";

export type UpdateDirection = "pcb" | "schematic";

/** `apply` sends the commands as one undo step and answers why not (the server's reason) when it was refused, `null` when it was done. */
/** The defaults of `DIALOG_UPDATE_FROM_PCB_BASE`'s three net options. */
const NET_OPTIONS = { net_names: true, prefer_unit_swaps: true, prefer_pin_swaps: true } as const;
type NetOptions = Omit<BackAnnotateOptions, "allow_pin_swaps">;

/** `allowPinSwaps` is the schematic preference "Allow unconstrained pin swaps", which Update Schematic from PCB's pin swaps read. */
export function UpdatePcbDialog({ direction, initial, parts, apply, allowPinSwaps = false }: { direction: UpdateDirection; initial: UpdatePcbChange[]; parts: readonly UpdatePart[]; apply: (cmds: Cmd[]) => Promise<string | null>; allowPinSwaps?: boolean }) {
  useEscape();
  const [changes, setChanges] = useState<UpdatePcbChange[]>(initial);
  const [told, setTold] = useState<ReportLine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const pcb = direction === "pcb";
  const [netOpts, setNetOpts] = useState<NetOptions>(NET_OPTIONS);
  const [nets, setNets] = useState<BackAnnotateReport | null>(null);
  const refreshNets = async (opts: NetOptions) => {
    if (pcb) return;
    const r = await postBackAnnotate({ ...opts, allow_pin_swaps: allowPinSwaps });
    setNets(r);
  };
  useEffect(() => {
    void refreshNets(netOpts);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [netOpts]);
  const netLines: ReportLine[] = pcb || !nets ? [] : nets.ok ? nets.lines : [{ severity: "error", text: nets.message ?? "The nets could not be compared." }];
  const netCmds: Cmd[] = pcb || !nets?.ok ? [] : nets.cmds;
  const cmds = pcb ? updateCommands(changes) : [...backAnnotateCommands(changes), ...netCmds];
  const dry: ReportLine[] = pcb ? changeLines(changes, false) : [...backAnnotateLines(changes, false), ...netLines];
  const title = pcb ? "Update PCB from Schematic" : "Update Schematic from PCB";

  const run = async () => {
    setBusy(true);
    try {
      const lines: ReportLine[] = pcb ? changeLines(changes, true) : [...backAnnotateLines(changes, true), ...netLines];
      const refused = await apply(cmds);
      if (refused === null) {
        setTold(lines);
        const fresh = await fetchUpdatePcb();
        if (fresh.ok) setChanges(fresh.changes);
        await refreshNets(netOpts);
      } else {
        setTold([{ severity: "error", text: refused }]);
      }
    } finally {
      setBusy(false);
    }
  };

  const status = pcb ? updatePcbMessage(parts) : "The schematic names the footprints the board has and draws the nets its pads are on, except for what is listed.";
  const shown: ReportLine[] = told ?? (dry.length > 0 ? dry : [{ severity: "info", text: status }]);
  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()} data-testid="update-pcb-dialog">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          {!pcb && (
            <div style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 12, marginBottom: 8 }} data-testid="update-sch-options">
              {(
                [
                  ["net_names", "Net names", "Update schematic net labels to match net names from the PCB editor.", "update-sch-net-names"],
                  ["prefer_unit_swaps", "Prefer symbol unit swaps over label swaps", "When possible, detect footprint gate swaps and apply corresponding symbol unit swaps instead of changing net labels.", "update-sch-unit-swaps"],
                  ["prefer_pin_swaps", "Prefer symbol pin swaps over label swaps", "When possible, swap symbol pins to match footprint pad net swaps instead of changing net labels.", "update-sch-pin-swaps"],
                ] as const
              ).map(([key, label, tip, testId]) => (
                <label key={key} title={tip}>
                  <input type="checkbox" checked={netOpts[key]} disabled={busy} onChange={(e) => { setTold(null); setNetOpts({ ...netOpts, [key]: e.target.checked }); }} data-testid={testId} /> {label}
                </label>
              ))}
            </div>
          )}
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
