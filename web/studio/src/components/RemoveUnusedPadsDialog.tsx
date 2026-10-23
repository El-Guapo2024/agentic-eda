// Port of pcbnew/dialogs/dialog_unused_pad_layers{,_base}.cpp + GLOBAL_EDIT_TOOL::RemoveUnusedPads (pcbnew/tools/global_edit_tool.cpp) --
// "Remove Unused Pads..." (pcbnew.GlobalEdit.removeUnusedPads). The dialog's four check boxes -- Vias, Pads, Selected only, Keep outside layers (the
// only one that starts checked) -- and its three buttons: "Remove Unused Layers" (OK: `updatePadsAndVias( true )`), "Restore All Layers" (Apply:
// `updatePadsAndVias( false )`) and Cancel. Both of the first two close the dialog, as `EndModal` does. Each sends one `remove_unused_pads` Cmd (one undo
// step, `m_commit.Push( "Remove Unused Pads" )`); "Selected only" sends the ids of the selection, otherwise the whole board is taken. The picture of the
// dialog is a pad's cross section and follows "Keep outside layers" (`updateImage`).
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

/** A plated through-hole pad seen from the side on four copper layers: its barrel, and the rings on the layers that keep theirs. */
function PadSection({ keepOutside }: { keepOutside: boolean }) {
  const rows = [14, 34, 54, 74];
  return (
    <svg width="132" height="92" viewBox="0 0 132 92" aria-hidden="true" data-testid="rup-image" style={{ color: "var(--text-dim, #888)" }}>
      <rect x="58" y="6" width="16" height="80" fill="currentColor" opacity="0.28" />
      {rows.map((y, i) => {
        const outside = i === 0 || i === rows.length - 1;
        const ring = outside && keepOutside;
        return <rect key={y} x={ring ? 28 : 58} y={y - 3} width={ring ? 76 : 16} height={6} fill={ring ? "var(--accent, #c83434)" : "currentColor"} opacity={ring ? 0.9 : 0.5} />;
      })}
    </svg>
  );
}

export function RemoveUnusedPadsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.removeUnusedPadsDialogOpen;

  // `m_cbVias`, `m_cbPads` and `m_cbSelectedOnly` start unchecked, `m_cbPreserveExternalLayers` checked; the dialog is made anew each time.
  const [vias, setVias] = useState(false);
  const [pads, setPads] = useState(false);
  const [selectedOnly, setSelectedOnly] = useState(false);
  const [keepOutside, setKeepOutside] = useState(true);
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const reset = () => {
    setVias(false);
    setPads(false);
    setSelectedOnly(false);
    setKeepOutside(true);
  };
  const close = () => {
    reset();
    dispatch({ type: "SET_REMOVE_UNUSED_PADS_DIALOG_OPEN", open: false });
  };

  const send = async (remove: boolean) => {
    // Nothing is picked: the dialog would loop over nothing (and KiCad commits an empty change).
    if (!vias && !pads) {
      close();
      return;
    }
    setBusy(true);
    try {
      const ok = await api.cmd({ op: "remove_unused_pads", selected: selectedOnly ? [...state.selection] : null, vias, pads, keep_outside: keepOutside, remove });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const check = (id: string, label: string, value: boolean, set: (v: boolean) => void) => (
    <label style={{ display: "block", margin: "4px 0" }}>
      <input type="checkbox" data-testid={id} checked={value} onChange={(e) => set(e.target.checked)} /> {label}
    </label>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 400 }} data-testid="remove-unused-pads" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Remove Unused Pads</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", gap: 16, alignItems: "flex-start" }}>
          <div style={{ flex: 1 }}>
            {check("rup-vias", "Vias", vias, setVias)}
            {check("rup-pads", "Pads", pads, setPads)}
            <div style={{ height: 14 }} />
            {check("rup-selected", "Selected only", selectedOnly, setSelectedOnly)}
            {check("rup-keep-outside", "Keep outside layers", keepOutside, setKeepOutside)}
          </div>
          <PadSection keepOutside={keepOutside} />
        </div>
        <div className="dialog-footer">
          <button data-testid="rup-cancel" onClick={close}>
            Cancel
          </button>
          <button data-testid="rup-restore" onClick={() => void send(false)} disabled={busy}>
            Restore All Layers
          </button>
          <button className="primary" data-testid="rup-remove" onClick={() => void send(true)} disabled={busy}>
            Remove Unused Layers
          </button>
        </div>
      </div>
    </div>
  );
}
