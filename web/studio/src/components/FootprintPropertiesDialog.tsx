// pcbnew.InteractiveEdit.properties ("E") on a selected footprint. KiCad's
// real dialog (pcb_properties_panel.cpp's sibling) has several tabs
// (General/Fields/3D Models/...) and lets you edit most of what it
// shows -- this session read pcb_properties_panel.cpp for the docked
// panel, not the modal dialog's exact layout, so this is a single-page
// view of the same fields the docked Properties panel already has real
// data for. Reference/Value/Footprint/MPN live on the *intent*-derived
// model (crates/model/src/lib.rs `Part`), not the editable `design.json`
// IR this app's `Cmd`s mutate, so they stay read-only (same reasoning
// BoardSetupDialog's Net Classes panel documents for the same model
// split) -- editable here: the refdes "Text Placement" side (new this
// session, `set_label_side`), plus Rotate/Flip/Move Exactly/Delete
// through the same commands the docked panel's own buttons use.
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatXY } from "../state/units";
import type { LabelSide } from "../api/types";

const LABEL_SIDE_OPTIONS: { value: LabelSide; label: string }[] = [
  { value: "above", label: "Above" },
  { value: "below", label: "Below" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

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
                <span>Text placement</span>
                <span>
                  <select value={p.label ?? "above"} onChange={(e) => api.cmd({ op: "set_label_side", part: p.ref, side: e.target.value as LabelSide })}>
                    {LABEL_SIDE_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </span>
              </>
            )}
            <span>Pads</span>
            <span>{p.pads?.length ?? 0}</span>
            <span>Nets</span>
            <span>{nets.join(", ") || "–"}</span>
          </div>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            Reference/Value/Footprint/MPN have no edit command yet (they come from the BOM, not the board) -- Position/Orientation/Side/Text placement do.
          </p>
        </div>
        <div className="dialog-footer">
          <button onClick={() => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: true })}>Move Exactly...</button>
          <button onClick={() => api.rotateSelection(1)}>Rotate</button>
          <button onClick={() => api.flipSelection()}>Flip</button>
          <button onClick={() => api.ripSelection()}>Delete</button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
