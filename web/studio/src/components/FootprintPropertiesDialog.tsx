// pcbnew.InteractiveEdit.properties ("E") on a selected footprint. KiCad's
// real dialog (pcb_properties_panel.cpp's sibling) has several tabs
// (General/Fields/3D Models/...) and lets you edit most of what it
// shows -- this session read pcb_properties_panel.cpp for the docked
// panel, not the modal dialog's exact layout, so this is a single-page,
// read-only view of the same fields the docked Properties panel already
// has real data for. Rotate/Delete work through the same commands the
// panel's own buttons use; everything else has no command to write
// through yet, so it's display-only rather than a disabled input.
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatXY } from "../state/units";

export function FootprintPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  if (!state.footprintPropertiesOpen) return null;
  const close = () => dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: false });

  const ref = [...state.selection][0];
  const p = ref ? api.partByRef(ref) : undefined;
  if (!p) return null;

  const nets = [...new Set((p.pads ?? []).map((q) => q.net).filter((n): n is string => !!n))];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Footprint Properties</span>
          <span>{p.ref}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid">
            <span>Reference</span>
            <span>{p.ref}</span>
            <span>Value</span>
            <span>{p.value ?? "–"}</span>
            <span>Footprint</span>
            <span>{p.package ?? "–"}</span>
            {p.mpn && (
              <>
                <span>MPN</span>
                <span>{p.mpn}</span>
              </>
            )}
            {p.placed && p.at && (
              <>
                <span>Position</span>
                <span>{formatXY(p.at[0], p.at[1], state.units)}</span>
                <span>Orientation</span>
                <span>{`${p.rot ?? 0}°`}</span>
                <span>Side</span>
                <span>{p.side === "bottom" ? "Bottom" : "Top"}</span>
              </>
            )}
            <span>Pads</span>
            <span>{p.pads?.length ?? 0}</span>
            <span>Nets</span>
            <span>{nets.join(", ") || "–"}</span>
          </div>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>Read-only: no backend command edits these fields directly yet (beyond Rotate/Delete, available from the canvas and its right-click menu).</p>
        </div>
        <div className="dialog-footer">
          <button onClick={() => api.rotateSelection(1)}>Rotate</button>
          <button onClick={() => api.ripSelection()}>Delete</button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
