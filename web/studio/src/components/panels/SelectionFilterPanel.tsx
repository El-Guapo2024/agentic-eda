// The selection filter (pcbnew/widgets/panel_selection_filter.cpp): every
// toggle here actually gates what a click/box-select/select-all can pick,
// via components/canvas/selectionCandidates.ts's `SelectionFilter` (the
// subset of KiCad's real categories this app's model has a selectable
// equivalent for -- see that file's own doc comment for what's
// deliberately left out).
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { SelectionFilter } from "../canvas/selectionCandidates";

// `PANEL_SELECTION_FILTER`'s rows in its order (panel_selection_filter_base.cpp): Footprints, Text, Tracks, Vias, Pads, Graphics, Zones, Rule Areas,
// Dimensions, Points, Other items.
const ROWS: Array<{ key: keyof SelectionFilter; label: string }> = [
  { key: "footprints", label: "Footprints" },
  { key: "text", label: "Text" },
  { key: "tracks", label: "Tracks" },
  { key: "vias", label: "Vias" },
  { key: "pads", label: "Pads" },
  { key: "graphics", label: "Graphics" },
  { key: "zones", label: "Zones" },
  { key: "keepouts", label: "Rule Areas" },
  { key: "dimensions", label: "Dimensions" },
  { key: "points", label: "Points" },
  { key: "otherItems", label: "Other items" },
];

export function SelectionFilterPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const f = state.selectionFilter;
  const allOn = ROWS.every((r) => f[r.key] !== false);
  return (
    <div className="panel-section">
      <h3>Selection Filter</h3>
      {/* panel_selection_filter.cpp: "All items" first, then "Locked items", which is not one of the rows "All items" sweeps. */}
      <label className="filter-row">
        <input
          type="checkbox"
          checked={allOn}
          onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: Object.fromEntries(ROWS.map((r) => [r.key, e.target.checked])) as Partial<SelectionFilter> })}
        />
        All items
      </label>
      <label className="filter-row" title="Allow selection of locked items">
        <input type="checkbox" checked={f.lockedItems} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { lockedItems: e.target.checked } })} />
        Locked items
      </label>
      {ROWS.map((r) => (
        <label className="filter-row" key={r.key}>
          <input type="checkbox" checked={f[r.key] !== false} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { [r.key]: e.target.checked } })} />
          {r.label}
        </label>
      ))}
    </div>
  );
}
