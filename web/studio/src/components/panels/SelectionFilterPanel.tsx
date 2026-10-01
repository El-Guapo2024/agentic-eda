// The selection filter (pcbnew/widgets/panel_selection_filter.cpp): every
// toggle here actually gates what a click/box-select/select-all can pick,
// via components/canvas/selectionCandidates.ts's `SelectionFilter` (the
// subset of KiCad's real categories this app's model has a selectable
// equivalent for -- see that file's own doc comment for what's
// deliberately left out, e.g. standalone pads).
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { SelectionFilter } from "../canvas/selectionCandidates";

const ROWS: Array<{ key: keyof SelectionFilter; label: string }> = [
  { key: "footprints", label: "Footprints" },
  { key: "tracks", label: "Tracks" },
  { key: "vias", label: "Vias" },
  { key: "zones", label: "Zones" },
  { key: "graphics", label: "Graphics" },
  { key: "text", label: "Text" },
  { key: "dimensions", label: "Dimensions" },
];

export function SelectionFilterPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const f = state.selectionFilter;
  const allOn = ROWS.every((r) => f[r.key]);
  return (
    <div className="panel-section">
      <h3>Selection Filter</h3>
      <label className="filter-row">
        <input
          type="checkbox"
          checked={allOn}
          onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: Object.fromEntries(ROWS.map((r) => [r.key, e.target.checked])) as Partial<SelectionFilter> })}
        />
        All Items
      </label>
      {ROWS.map((r) => (
        <label className="filter-row" key={r.key}>
          <input type="checkbox" checked={f[r.key]} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { [r.key]: e.target.checked } })} />
          {r.label}
        </label>
      ))}
    </div>
  );
}
