// Port of pcbnew/dialogs/dialog_global_edit_tracks_and_vias{,_base}.cpp --
// "Edit Track & Via Properties..." (pcbnew.GlobalEdit.editTracksAndVias,
// task item 2). Scope/filter checkboxes default unchecked, same literal
// fidelity CleanupTracksDialog.tsx already established for this port
// (nothing in the read dialog base ever calls SetValue(true) on them).
//
// Scoped to this model's one via "type" -- no through/micro/blind/buried
// distinction, no padstack/annular-ring/IPC4761 protection-feature
// concept -- and to the filters this app's board data can actually
// support: net, layer (tracks only -- a via has no single GetLayer() the
// way this model represents it), and "selected items only". Net-class
// filter and the per-item track-width/via-size filters are not ported
// (see PARITY-pcb.md section 13); the *action* side's "net class values"
// still is, since BoardRules::width_of/via_diameter_of/via_drill_of
// already exist for exactly this.
import { useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, type LengthUnit } from "../state/units";

type Action = "specified" | "net_class";

export function GlobalEditTracksAndViasDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.editTracksAndViasDialogOpen;

  const [scopeTracks, setScopeTracks] = useState(false);
  const [scopeVias, setScopeVias] = useState(false);
  const [netFilterOn, setNetFilterOn] = useState(false);
  const [netFilter, setNetFilter] = useState("");
  const [layerFilterOn, setLayerFilterOn] = useState(false);
  const [layerFilter, setLayerFilter] = useState("");
  const [selectedOnly, setSelectedOnly] = useState(false);

  const [action, setAction] = useState<Action>("specified");
  const [changeLayer, setChangeLayer] = useState(false);
  const [newLayer, setNewLayer] = useState("");
  const [changeTrackWidth, setChangeTrackWidth] = useState(false);
  const [trackWidth, setTrackWidth] = useState(0); // display units
  const [changeViaSize, setChangeViaSize] = useState(false);
  const [viaDiameter, setViaDiameter] = useState(0); // display units
  const [viaDrill, setViaDrill] = useState(0); // display units
  const [busy, setBusy] = useState(false);

  const layers = state.board?.layers ?? [];
  const nets = useMemo(() => {
    const set = new Set<string>();
    for (const p of state.board?.parts ?? []) for (const pad of p.pads ?? []) if (pad.net) set.add(pad.net);
    return [...set].sort();
  }, [state.board]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_EDIT_TRACKS_AND_VIAS_DIALOG_OPEN", open: false });

  const matchingIds = (): string[] => {
    const tracks = state.board?.routing?.tracks ?? [];
    const vias = state.board?.routing?.vias ?? [];
    const ids: string[] = [];
    const passCommon = (net: string, id: string) => {
      if (netFilterOn && netFilter && net !== netFilter) return false;
      if (selectedOnly && !state.selection.has(id)) return false;
      return true;
    };
    if (scopeTracks) {
      for (const t of tracks) {
        if (!passCommon(t.net, t.id)) continue;
        if (layerFilterOn && layerFilter && t.layer !== layerFilter) continue;
        ids.push(t.id);
      }
    }
    if (scopeVias) {
      // A via has no single "active layer" the way a track does in this
      // model (it spans from/to) -- the layer filter simply doesn't apply
      // to vias, same as it wouldn't narrow anything meaningful.
      for (const v of vias) if (passCommon(v.net, v.id)) ids.push(v.id);
    }
    return ids;
  };

  const ids = matchingIds();
  const unit: LengthUnit = state.units;

  const apply = async () => {
    if (ids.length === 0) return;
    setBusy(true);
    try {
      const ok = await api.cmd({
        op: "edit_tracks_and_vias",
        ids,
        track_width: action === "net_class" ? { kind: "net_class" } : changeTrackWidth ? { kind: "value", um: Math.round(umFrom(trackWidth, unit)) } : null,
        via_size: action === "net_class" ? { kind: "net_class" } : changeViaSize ? { kind: "value", diameter: Math.round(umFrom(viaDiameter, unit)), drill: Math.round(umFrom(viaDrill, unit)) } : null,
        layer: action === "specified" && changeLayer && newLayer ? newLayer : null,
      });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Edit Track & Via Properties</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Scope</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="checkbox" checked={scopeTracks} onChange={(e) => setScopeTracks(e.target.checked)} /> Tracks
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }}>
            <input type="checkbox" checked={scopeVias} onChange={(e) => setScopeVias(e.target.checked)} /> Vias
          </label>

          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Filter Items</div>
          <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <input type="checkbox" checked={netFilterOn} onChange={(e) => setNetFilterOn(e.target.checked)} /> Net:
            <select value={netFilter} onChange={(e) => setNetFilter(e.target.value)} disabled={!netFilterOn} style={{ flex: 1 }}>
              <option value="">(any)</option>
              {nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4 }}>
            <input type="checkbox" checked={layerFilterOn} onChange={(e) => setLayerFilterOn(e.target.checked)} /> Layer (tracks only):
            <select value={layerFilter} onChange={(e) => setLayerFilter(e.target.value)} disabled={!layerFilterOn} style={{ flex: 1 }}>
              <option value="">(any)</option>
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={{ display: "block", marginTop: 4, marginBottom: 8 }}>
            <input type="checkbox" checked={selectedOnly} onChange={(e) => setSelectedOnly(e.target.checked)} /> Selected items only
          </label>

          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Action</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="radio" checked={action === "specified"} onChange={() => setAction("specified")} /> Set to specified values
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 6 }}>
            <input type="radio" checked={action === "net_class"} onChange={() => setAction("net_class")} /> Set to net class values
          </label>

          {action === "specified" && (
            <div className="kv-grid" style={{ gridTemplateColumns: "20px 110px 1fr", rowGap: 4 }}>
              <input type="checkbox" checked={changeLayer} onChange={(e) => setChangeLayer(e.target.checked)} />
              <span>Layer:</span>
              <select value={newLayer} onChange={(e) => setNewLayer(e.target.value)} disabled={!changeLayer}>
                <option value="">-- leave unchanged --</option>
                {layers.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>

              <input type="checkbox" checked={changeTrackWidth} onChange={(e) => setChangeTrackWidth(e.target.checked)} />
              <span>Track width:</span>
              <input type="number" value={trackWidth} disabled={!changeTrackWidth} onChange={(e) => setTrackWidth(Number(e.target.value))} />

              <input type="checkbox" checked={changeViaSize} onChange={(e) => setChangeViaSize(e.target.checked)} />
              <span>Via diameter:</span>
              <input type="number" value={viaDiameter} disabled={!changeViaSize} onChange={(e) => setViaDiameter(Number(e.target.value))} />

              <span />
              <span>Via drill:</span>
              <input type="number" value={viaDrill} disabled={!changeViaSize} onChange={(e) => setViaDrill(Number(e.target.value))} />
            </div>
          )}

          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 10 }}>{ids.length} item(s) match the current scope and filters.</div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
          <button className="primary" onClick={apply} disabled={busy || ids.length === 0 || (action === "specified" && !changeLayer && !changeTrackWidth && !changeViaSize)}>
            Apply and Close
          </button>
        </div>
      </div>
    </div>
  );
}
