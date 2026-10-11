// Paste Special on the PCB -- `DIALOG_PASTE_SPECIAL` (common/dialogs/dialog_paste_special.cpp at 8303b2ad): a "Reference Designators" box with KiCad's three choices
// (the box opens on "Keep existing reference designators, even if they are duplicated", what a plain Paste does), and the "Clear net assignments" box that
// `PCB_CONTROL::Paste` hides for a single footprint. OK pastes (actions/pcbEditDetailsActions.ts hands the answers to `api.pasteClipboard`); Cancel pastes nothing.
import { useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import type { PcbPasteMode } from "../api/types";
import { CLEAR_NETS_LABEL, CLEAR_NETS_TIP, DEFAULT_PASTE_MODE, PCB_PASTE_SPECIAL_OPTIONS } from "../kicad-port/pcbPasteSpecial";
import { DialogShell } from "./pcbDialogKit";

export function PcbPasteSpecialDialog({ showClearNets, onOk }: { /** The clipboard is a board fragment (the box is offered), not a single footprint. */ showClearNets: boolean; onOk: (mode: PcbPasteMode, clearNets: boolean) => void }) {
  const [mode, setMode] = useState<PcbPasteMode>(DEFAULT_PASTE_MODE);
  const [clearNets, setClearNets] = useState(false);
  const submit = () => {
    closeSweepDialog();
    onOk(mode, showClearNets && clearNets);
  };
  return (
    <DialogShell title="Paste Special" width={480} onCancel={closeSweepDialog} onOk={submit}>
      <fieldset style={{ border: "1px solid var(--border, #555)", padding: "8px 12px" }} data-testid="paste-special-options">
        <legend>Reference Designators</legend>
        {PCB_PASTE_SPECIAL_OPTIONS.map((o) => (
          <label key={o.mode} title={o.tip} style={{ display: "block", margin: "6px 0" }}>
            <input type="radio" name="pcb-paste-special-mode" data-testid={`paste-special-${o.mode}`} checked={mode === o.mode} onChange={() => setMode(o.mode)} /> {o.label}
          </label>
        ))}
      </fieldset>
      {showClearNets && (
        <label title={CLEAR_NETS_TIP} style={{ display: "block", margin: "10px 4px 0" }}>
          <input type="checkbox" data-testid="paste-special-clear-nets" checked={clearNets} onChange={(e) => setClearNets(e.target.checked)} /> {CLEAR_NETS_LABEL}
        </label>
      )}
    </DialogShell>
  );
}
