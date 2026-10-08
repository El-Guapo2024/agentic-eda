// Paste Special on the Schematic tab -- `DIALOG_PASTE_SPECIAL` (common/dialogs/dialog_paste_special.cpp at 8303b2ad): a "Reference Designators" box with
// KiCad's three choices, then the paste itself (actions/schClipboardActions.ts `startPaste`) with the one picked. The dialog's "Clear net assignments"
// box belongs to the board editor; the schematic's Paste does not read it, so it is not offered.
import { useEffect, useState } from "react";
import { startPaste } from "../actions/schClipboardActions";
import { PASTE_SPECIAL_OPTIONS, type SchPasteMode } from "../kicad-port/schClipboard";
import { closePasteSpecial, usePasteSpecialOpen } from "../state/schPasteStore";
import { useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";

export function SchPasteSpecialDialog() {
  const open = usePasteSpecialOpen();
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  // `PASTE_MODE pasteMode = annotateAutomatic ? UNIQUE_ANNOTATIONS : REMOVE_ANNOTATIONS`: the box opens on the mode a plain Paste uses.
  const [mode, setMode] = useState<SchPasteMode>("unique");

  useEffect(() => {
    if (open) setMode("unique");
  }, [open]);

  if (!open) return null;

  const ok = () => {
    closePasteSpecial();
    void startPaste({ state, dispatch }, { mode, duplicate: false });
  };

  return (
    <SchDialogShell title="Paste Special" width={460} onCancel={closePasteSpecial} onOk={ok}>
      <fieldset style={{ border: "1px solid var(--border, #555)", padding: "8px 12px" }}>
        <legend>Reference Designators</legend>
        {PASTE_SPECIAL_OPTIONS.map((o) => (
          <label key={o.mode} title={o.tip} style={{ display: "block", margin: "6px 0" }}>
            <input type="radio" name="paste-special-mode" checked={mode === o.mode} onChange={() => setMode(o.mode)} /> {o.label}
          </label>
        ))}
      </fieldset>
    </SchDialogShell>
  );
}
