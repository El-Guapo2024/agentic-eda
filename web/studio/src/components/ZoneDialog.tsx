// pcbnew.InteractiveDrawing.zone ("Draw Filled Zones", Ctrl+Alt+Shift+Z).
// KiCad asks for a zone's net/layer/clearance/etc. in a full "Zone
// Properties" dialog *before* you draw the outline; this app's task spec
// asks for the outline first, then a small dialog for "the net and layer"
// specifically -- modelled on that same real dialog's net/layer fields,
// not a full port of it (no clearance/priority/hatch settings exist in
// this data model). Canvas.tsx's finishDraw() moves the drawn outline
// into state.zonePending once it has 3+ points; this dialog is what
// turns that pending outline into a real add_zone.
import { useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

export function ZoneDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const outline = state.zonePending;

  const nets = useMemo(() => {
    const set = new Set<string>();
    for (const p of state.board?.parts ?? []) for (const pad of p.pads ?? []) if (pad.net) set.add(pad.net);
    return [...set].sort();
  }, [state.board]);

  const [net, setNet] = useState<string>("");
  const [layer, setLayer] = useState<string>("");
  const effectiveNet = net || nets[0] || "";
  const effectiveLayer = layer || state.activeLayer || state.board?.layers[0] || "F.Cu";

  if (!outline) return null;
  const close = () => dispatch({ type: "SET_ZONE_PENDING", outline: null });

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Zone Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr" }}>
            <span>Net</span>
            <select value={effectiveNet} onChange={(e) => setNet(e.target.value)}>
              {nets.length === 0 && <option value="">(no nets on this board)</option>}
              {nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            <span>Layer</span>
            <select value={effectiveLayer} onChange={(e) => setLayer(e.target.value)}>
              {(state.board?.layers ?? []).map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </div>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            {outline.length} point outline. Shown as an outline, not a fill -- zone fill isn't computed yet (KiCad's own "outline display mode" is the same idea).
          </p>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button
            className="primary"
            disabled={!effectiveNet}
            onClick={() => {
              api.cmd({ op: "add_zone", net: effectiveNet, layer: effectiveLayer, outline: outline.map(([x, y]) => ({ x, y })) });
              close();
            }}
          >
            Add Zone
          </button>
        </div>
      </div>
    </div>
  );
}
