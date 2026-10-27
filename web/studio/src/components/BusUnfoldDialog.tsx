// `C` (SCH_LINE_WIRE_BUS_TOOL::UnfoldBus): the member nets of the bus under the cursor, as `BUS_UNFOLD_MENU`
// lists them in a popup at the cursor -- a small modal list here (the studio has no popup-menu primitive).
// Picking one arms the wire tool from the far end of a new bus entry: the entry (rooted on the bus at the
// cursor, `doUnfoldBus`) leans toward the cursor until the first click, which places the member's label
// (`doDrawSegments`, kicad-port/schBusUnfold.ts); the entry, the wire and the label are committed together
// when the wire is finished (SchematicView.tsx's `commitDrawn`).
import { useStudioDispatch, useStudioState } from "../state/store";
import { BUS_ENTRY_SIZE_NM as ENTRY_SIZE_NM } from "../kicad-port/schBusUnfold";

export function BusUnfoldDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const picker = state.busUnfoldPicker;
  if (!picker) return null;
  const close = () => dispatch({ type: "SET_BUS_UNFOLD_PICKER", picker: null });

  const choose = (net: string) => {
    const size: [number, number] = [ENTRY_SIZE_NM, ENTRY_SIZE_NM];
    const start: [number, number] = [picker.entryAt[0] + size[0], picker.entryAt[1] + size[1]];
    dispatch({ type: "SET_BUS_UNFOLD_PICKER", picker: null });
    dispatch({ type: "SET_BUS_UNFOLD", unfold: { net, entryAt: picker.entryAt, size } });
    // `startSegments( aCommit, LAYER_WIRE, m_busUnfold.entry->GetEnd() )`: the wire starts at the entry's far end.
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "wire" });
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "wire", pts: [start] } });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 300 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Unfold from Bus {picker.bus}</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontSize: 11, opacity: 0.7, marginBottom: 6 }}>Choose the net to break out of the bus:</div>
          <div style={{ maxHeight: 240, overflowY: "auto", display: "flex", flexDirection: "column", gap: 2 }}>
            {picker.members.map((m) => (
              <button key={m} style={{ textAlign: "left" }} onClick={() => choose(m)}>
                {m}
              </button>
            ))}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
