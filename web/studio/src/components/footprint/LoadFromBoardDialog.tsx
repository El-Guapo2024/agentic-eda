// `FOOTPRINT_EDIT_FRAME::SelectFootprintFromBoard` (pcbnew/load_select_footprint.cpp), the list `LoadFootprintFromBoard` shows
// (`pcbnew.ModuleEditor.loadFootprintFromBoard`, "Load footprint from current PCB"): "Footprints [%u items]", one row per footprint
// of the board, named by its reference. The chosen one is opened in the editor as `LoadFootprintFromBoard` opens it: a copy of the board's footprint linked to it
// (`fp_editor open_from_board`, UI-LEFT F3), which Save and "Insert footprint into PCB" put back on that footprint only. The dialog is `EDA_LIST_DIALOG`:
// "Items:" over the one column "Footprint", sorted (`sortList`, `StrNumCmp( .., true )`: numbers by value, case ignored), and the "Filter" box under it, which
// keeps the rows matching `*<text>*` (wildcards `*` and `?`, case ignored, `textChangeInFilterBox`).
import { useState } from "react";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { listFilter } from "../../kicad-port/fpListFilter";

export function LoadFromBoardDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const board = useStudioState().board;
  const [picked, setPicked] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  if (!state.loadFromBoardOpen) return null;
  const all = (board?.parts ?? []).filter((p) => p.footprint);
  const matches = listFilter(filter);
  const parts = all.filter((p) => matches(p.ref)).sort((a, b) => a.ref.localeCompare(b.ref, undefined, { numeric: true, sensitivity: "base" }));
  const close = () => dispatch({ type: "SET_LOAD_FROM_BOARD_OPEN", open: false });
  const load = async (ref: string | null) => {
    const part = parts.find((p) => p.ref === ref);
    if (!part?.footprint) return;
    close();
    await api.openFromBoard(part.ref);
  };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 400 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Load footprint from current PCB">
        <div className="dialog-header">
          <span>Footprints [{all.length} items]</span>
        </div>
        <div className="dialog-body">
          <div style={{ marginBottom: 4 }}>Items:</div>
          <table className="setup-table" style={{ margin: 0 }}>
            <thead>
              <tr>
                <th>Footprint</th>
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
                </tr>
              ))}
            </tbody>
          </table>
          <input
            type="text"
            placeholder="Filter"
            value={filter}
            onChange={(e) => {
              setFilter(e.target.value);
              setPicked(null);
            }}
            style={{ width: "100%", marginTop: 6, boxSizing: "border-box" }}
          />
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
