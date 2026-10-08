// The Appearance panel (docked right, with the selection filter and
// Activity -- see RightDock.tsx). KiCad's has Layers/Objects/Nets tabs,
// opacity sliders, and saved presets/viewports
// (pcbnew/widgets/appearance_controls.cpp, which this session could not
// read). Layers/Nets are implemented against real board data; Objects is
// a smaller, reasonable set of this app's own toggles; presets/viewports
// are not implemented (see the report's gap list) -- KiCad's exact UI
// for those needs the source this session couldn't reach.
import { useState } from "react";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { layerColor, copperColorKey, STANDARD_LAYERS } from "../canvas/layers";

type SubTab = "layers" | "objects" | "nets";

function LayerRow({ layerKey, label, colorKey }: { layerKey: string; label: string; colorKey: string }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const visible = state.layerVisible[layerKey] ?? true;
  const opacity = state.layerOpacity[layerKey] ?? 1;
  return (
    <div className={`layer-row${state.activeLayer === layerKey ? " active" : ""}`}>
      <input type="checkbox" checked={visible} onChange={(e) => dispatch({ type: "SET_LAYER_VISIBLE", layer: layerKey, visible: e.target.checked })} />
      <span className="swatch" style={{ background: layerColor(colorKey) }} />
      <span className="name" onClick={() => dispatch({ type: "SET_ACTIVE_LAYER", layer: state.activeLayer === layerKey ? null : layerKey })} title="Set as active layer">
        {label}
      </span>
      <input type="range" min={0} max={1} step={0.05} value={opacity} onChange={(e) => dispatch({ type: "SET_LAYER_OPACITY", layer: layerKey, opacity: parseFloat(e.target.value) })} />
    </div>
  );
}

function LayersTab() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;
  if (!board) return null;
  return (
    <div>
      <h3>Copper Layers</h3>
      {board.layers.map((layer) => (
        <LayerRow key={layer} layerKey={layer} label={layer} colorKey={copperColorKey(layer)} />
      ))}
      <h3 style={{ marginTop: 10 }}>Technical Layers</h3>
      {STANDARD_LAYERS.map((l) => (
        <LayerRow key={l.key} layerKey={l.key} label={l.label} colorKey={l.key} />
      ))}
      <label className="filter-row" style={{ marginTop: 8 }}>
        <input type="checkbox" checked={state.highContrast} onChange={() => dispatch({ type: "TOGGLE_HIGH_CONTRAST" })} />
        High contrast (dim inactive layers)
      </label>
      {/* The Appearance panel's "Flip board view" (m_cbFlipBoard): the same switch as View > Flip Board View. */}
      <label className="filter-row">
        <input type="checkbox" checked={state.bcx.boardFlipped} onChange={() => dispatch({ type: "BCX", patch: { boardFlipped: !state.bcx.boardFlipped } })} />
        Flip board view
      </label>
    </div>
  );
}

function ObjectsTab() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  return (
    <div>
      <label className="filter-row">
        <input type="checkbox" checked={state.showRatsnest} onChange={() => dispatch({ type: "TOGGLE_RATSNEST" })} />
        Ratsnest
      </label>
      {/* `m_RatsnestMode`, the second state of pcbnew.Control.ratsnestModeCycle: lines only between items on a copper layer that is shown. */}
      <label className="filter-row">
        <input type="checkbox" checked={state.bcx.ratsnestMode === "visible"} onChange={() => dispatch({ type: "BCX", patch: { ratsnestMode: state.bcx.ratsnestMode === "visible" ? "all" : "visible" } })} />
        Ratsnest: visible layers only
      </label>
      <label className="filter-row">
        <input type="checkbox" checked={state.gridVisible} onChange={() => dispatch({ type: "TOGGLE_GRID_VISIBLE" })} />
        Grid
      </label>
      {/* Preferences > Display Options "Show pad numbers" (`m_DisplayPadNumbers`), the switch pcbnew.Control.showPadNumbers flips. */}
      <label className="filter-row">
        <input type="checkbox" checked={state.bcx.showPadNumbers} onChange={() => dispatch({ type: "BCX", patch: { showPadNumbers: !state.bcx.showPadNumbers } })} />
        Pad numbers
      </label>
    </div>
  );
}

function NetsTab() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;
  if (!board) return null;
  const nets = [...new Set(board.parts.flatMap((p) => (p.pads ?? []).map((q) => q.net).filter((n): n is string => !!n)))].sort();
  if (nets.length === 0) return <div className="panel-empty">No nets.</div>;
  return (
    <div>
      {nets.map((net) => (
        <div key={net} className="filter-row" style={{ cursor: "default" }} onClick={() => dispatch({ type: "SET_NET_HIGHLIGHT", net: state.netHighlight === net ? null : net })}>
          <input type="checkbox" readOnly checked={state.netHighlight === net} />
          <span>{net}</span>
        </div>
      ))}
    </div>
  );
}

export function AppearancePanel() {
  const [tab, setTab] = useState<SubTab>("layers");
  return (
    <div className="panel-section">
      <div className="dock-tabs" style={{ marginBottom: 8 }}>
        {(["layers", "objects", "nets"] as const).map((t) => (
          <div key={t} className={`dock-tab${tab === t ? " active" : ""}`} onClick={() => setTab(t)}>
            {t[0]!.toUpperCase() + t.slice(1)}
          </div>
        ))}
      </div>
      {tab === "layers" && <LayersTab />}
      {tab === "objects" && <ObjectsTab />}
      {tab === "nets" && <NetsTab />}
    </div>
  );
}
