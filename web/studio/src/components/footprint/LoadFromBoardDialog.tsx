// `FOOTPRINT_EDIT_FRAME::SelectFootprintFromBoard` (pcbnew/load_select_footprint.cpp), the list `LoadFootprintFromBoard` shows
// (`pcbnew.ModuleEditor.loadFootprintFromBoard`, "Load footprint from current PCB"): "Footprints [%u items]", one row per footprint
// of the board, named by its reference. The chosen one is opened in the editor, linked to its board part so "Insert footprint into
// PCB" knows what it updates.
import { useState } from "react";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";

export function LoadFromBoardDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const board = useStudioState().board;
  const [picked, setPicked] = useState<string | null>(null);
  if (!state.loadFromBoardOpen) return null;
  const parts = (board?.parts ?? []).filter((p) => p.footprint).sort((a, b) => a.ref.localeCompare(b.ref, undefined, { numeric: true }));
  const close = () => dispatch({ type: "SET_LOAD_FROM_BOARD_OPEN", open: false });
  const load = async (ref: string | null) => {
    const part = parts.find((p) => p.ref === ref);
    if (!part?.footprint) return;
    close();
    await api.openFootprint(part.footprint);
    dispatch({ type: "SET_LINKED_PART", ref: part.ref });
  };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 400 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Load footprint from current PCB">
        <div className="dialog-header">
          <span>Footprints [{parts.length} items]</span>
        </div>
        <div className="dialog-body" style={{ padding: 0 }}>
          <table className="setup-table" style={{ margin: 0 }}>
            <thead>
              <tr>
                <th>Footprint</th>
                <th>Library name</th>
              </tr>
            </thead>
            <tbody>
              {parts.map((p) => (
                <tr
                  key={p.ref}
                  aria-selected={picked === p.ref}
                  onClick={() => setPicked(p.ref)}
                  onDoubleClick={() => void load(p.ref)}
                  style={{ cursor: "pointer", background: picked === p.ref ? "var(--chrome-selected-bg)" : undefined, color: picked === p.ref ? "var(--chrome-selected-text)" : undefined }}
                >
                  <td>{p.ref}</td>
                  <td>{p.footprint}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!picked} onClick={() => void load(picked)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
