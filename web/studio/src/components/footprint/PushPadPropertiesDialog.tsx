// Port of `DIALOG_PUSH_PAD_PROPERTIES` (pcbnew/dialogs/dialog_push_pad_properties.cpp), opened by `pcbnew.PadTool.PushPadSettings`
// (`PAD_TOOL::pushPadSettings`): the one selected pad's settings go to the other pads of the footprint, each filter keeping a pad
// whose shape / layers / orientation / type differs from the source out of it. All four filters start on. In the footprint editor the
// dialog has the single button "Change Pads on Current Footprint" (the "identical footprints" one is a board-editor thing).
import { useEffect, useState } from "react";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";

export function PushPadPropertiesDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const open = state.pushPadOpen;
  const [filters, setFilters] = useState({ shape: true, layers: true, orientation: true, type: true });

  useEffect(() => {
    if (open) setFilters({ shape: true, layers: true, orientation: true, type: true });
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_PUSH_PAD_OPEN", open: false });
  const source = [...state.selection].find((id) => state.footprint?.pads.some((p) => p.id === id));
  const row = (key: keyof typeof filters, label: string) => (
    <label key={key} style={{ display: "block", margin: "4px 0" }}>
      <input type="checkbox" checked={filters[key]} onChange={(e) => setFilters((f) => ({ ...f, [key]: e.target.checked }))} /> {label}
    </label>
  );
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Push Pad Properties">
        <div className="dialog-header">
          <span>Push Pad Properties to Other Pads</span>
        </div>
        <div className="dialog-body">
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "0 0 8px" }}>Options</p>
          {row("shape", "Do not modify pads having a different shape")}
          {row("layers", "Do not modify pads having different layers")}
          {row("orientation", "Do not modify pads having a different orientation")}
          {row("type", "Do not modify pads having a different type")}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button
            className="primary"
            disabled={!source}
            onClick={async () => {
              if (source) await api.pushPadProperties(source, { shape: filters.shape, orientation: filters.orientation, layers: filters.layers, type: filters.type });
              close();
            }}
          >
            Change Pads on Current Footprint
          </button>
        </div>
      </div>
    </div>
  );
}
