// The selection filter (pcbnew/widgets/panel_selection_filter*, not read
// this session -- see PropertiesPanel.tsx's header comment for why).
// Footprints is wired into canvas/Canvas.tsx's hit-testing; Tracks/Vias
// are shown for shape parity with KiCad's panel but have no effect yet,
// since track/via selection isn't implemented at all (see state/store.tsx
// `selectionFilter`'s comment).
import React from "react";
import { useStudioDispatch, useStudioState } from "../../state/store";

export function SelectionFilterPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const f = state.selectionFilter;
  return (
    <div className="panel-section">
      <h3>Selection Filter</h3>
      <label className="filter-row">
        <input type="checkbox" checked={f.footprints} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { footprints: e.target.checked } })} />
        Footprints
      </label>
      <label className="filter-row" title="Not implemented yet -- tracks aren't individually selectable">
        <input type="checkbox" checked={f.tracks} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { tracks: e.target.checked } })} disabled />
        Tracks
      </label>
      <label className="filter-row" title="Not implemented yet -- vias aren't individually selectable">
        <input type="checkbox" checked={f.vias} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { vias: e.target.checked } })} disabled />
        Vias
      </label>
    </div>
  );
}
