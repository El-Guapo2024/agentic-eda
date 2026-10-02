// Port of pcbnew/dialogs/dialog_swap_layers{,_base}.cpp + GLOBAL_EDIT_TOOL::
// SwapLayers (pcbnew/tools/global_edit_tool.cpp) -- "Swap Layers..."
// (pcbnew.GlobalEdit.swapLayers). One row per enabled copper layer in UI
// order ("Move items on:"), each defaulting to itself; the "To layer:"
// editor offers copper layers only (`GRID_CELL_LAYER_SELECTOR( m_parent,
// LSET::AllNonCuMask() )` -- the mask is the *hidden* set). OK sends one
// `swap_layers` Cmd (one undo step, like source's single `m_commit->Push(
// _( "Swap Layers" ) )`); the map is applied simultaneously, so F.Cu->B.Cu
// plus B.Cu->F.Cu really swaps the two. Rows left on themselves are not
// sent, and when every row is identity nothing is sent at all -- source
// only pushes a commit `if( hasChanges )`.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

export function SwapLayersDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.swapLayersDialogOpen;
  const copper = state.board?.layers ?? ["F.Cu", "B.Cu"];

  // Keyed by source layer; reset to identity every time the dialog opens
  // (`TransferDataToWindow` builds a fresh table each time).
  const [dest, setDest] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const close = () => {
    setDest({});
    dispatch({ type: "SET_SWAP_LAYERS_DIALOG_OPEN", open: false });
  };

  const ok = async () => {
    const mapping: [string, string][] = copper.filter((l) => (dest[l] ?? l) !== l).map((l) => [l, dest[l]!]);
    if (mapping.length === 0) {
      close();
      return;
    }
    setBusy(true);
    try {
      if (await api.cmd({ op: "swap_layers", mapping })) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Swap Layers</span>
        </div>
        <div className="dialog-body">
          <table className="setup-table" style={{ width: "100%" }}>
            <thead>
              <tr>
                <th style={{ width: "50%" }}>Move items on:</th>
                <th>To layer:</th>
              </tr>
            </thead>
            <tbody>
              {copper.map((layer) => (
                <tr key={layer}>
                  <td>{layer}</td>
                  <td>
                    <select value={dest[layer] ?? layer} onChange={(e) => setDest((d) => ({ ...d, [layer]: e.target.value }))} style={{ width: "100%" }}>
                      {copper.map((l) => (
                        <option key={l} value={l}>
                          {l}
                        </option>
                      ))}
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={ok} disabled={busy}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
