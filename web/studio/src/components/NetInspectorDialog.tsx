// pcbnew.Control.showNetInspector ("Net Inspector"). KiCad's real one is
// a dockable, sortable grid (name/class/length/vias/...); this app has
// no track-length-per-net computation and no net classes, so "a basic
// net list is fine" (per the task): net name, pad count, connected
// refs, click to highlight -- computed client-side from state.board's
// existing per-pad net field, no new backend data needed.
import { useMemo, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";

export function NetInspectorDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [filter, setFilter] = useState("");

  const nets = useMemo(() => {
    const byNet = new Map<string, Set<string>>();
    for (const p of state.board?.parts ?? []) {
      for (const pad of p.pads ?? []) {
        if (!pad.net) continue;
        if (!byNet.has(pad.net)) byNet.set(pad.net, new Set());
        byNet.get(pad.net)!.add(p.ref);
      }
    }
    return [...byNet.entries()]
      .map(([net, refs]) => ({ net, refs: [...refs].sort() }))
      .filter((n) => !filter || n.net.toLowerCase().includes(filter.toLowerCase()))
      .sort((a, b) => a.net.localeCompare(b.net));
  }, [state.board, filter]);

  if (!state.netInspectorOpen) return null;
  const close = () => dispatch({ type: "SET_NET_INSPECTOR_OPEN", open: false });

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460, maxHeight: "80vh" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Net Inspector</span>
          <span>{nets.length} net(s)</span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 8 }}>
          <input autoFocus placeholder="Filter…" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ width: "100%", padding: "5px 8px", marginBottom: 8 }} />
          <div className="kv-grid" style={{ gridTemplateColumns: "1fr auto auto", rowGap: 4 }}>
            {nets.map((n) => (
              <span
                key={n.net}
                style={{ display: "contents", cursor: "default" }}
                onClick={() => dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight === n.net ? null : n.net })}
              >
                <span style={{ fontWeight: state.netHighlight === n.net ? 600 : 400, color: state.netHighlight === n.net ? "var(--chrome-accent)" : undefined }}>{n.net}</span>
                <span style={{ color: "var(--chrome-text-dim)", textAlign: "right" }}>{n.refs.length} pad(s)</span>
                <span style={{ color: "var(--chrome-text-dim)", fontSize: 10 }}>{n.refs.join(", ")}</span>
              </span>
            ))}
            {nets.length === 0 && <span className="panel-empty">No nets.</span>}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
