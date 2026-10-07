// The schematic's selection filter (eeschema/widgets/panel_sch_selection_filter.cpp): every switch gates what a click, a box
// selection, Select All and the cursor pick-up of an edit hotkey may take (kicad-port/schSelectionFilter.ts). "Locked items"
// stands apart from "All items" -- which only sweeps the categories -- and a category's right-click menu offers "Only <category>".
// KiCad's Pins and Images switches are left out: the studio selects neither.
import { useState } from "react";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { allCategoriesOn, onlyCategory, SCH_FILTER_CATEGORIES, setAllCategories, type SchFilterCategory, type SchSelectionFilter } from "../../kicad-port/schSelectionFilter";
import { ContextMenu } from "../canvas/ContextMenu";

export function SchSelectionFilterPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const f = state.schSelectionFilter;
  const set = (next: SchSelectionFilter) => dispatch({ type: "SET_SCH_SELECTION_FILTER", filter: next });
  const [only, setOnly] = useState<{ x: number; y: number; key: SchFilterCategory; label: string } | null>(null);
  return (
    <div className="panel-section">
      <h3>Selection Filter</h3>
      <label className="filter-row" title="Allow selection of locked items">
        <input type="checkbox" checked={f.lockedItems} onChange={(e) => set({ ...f, lockedItems: e.target.checked })} />
        Locked items
      </label>
      <label className="filter-row">
        <input type="checkbox" checked={allCategoriesOn(f)} onChange={(e) => set(setAllCategories(f, e.target.checked))} />
        All items
      </label>
      {SCH_FILTER_CATEGORIES.map((c) => (
        <label
          className="filter-row"
          key={c.key}
          onContextMenu={(e) => {
            e.preventDefault();
            setOnly({ x: e.clientX, y: e.clientY, key: c.key, label: c.label });
          }}
        >
          <input type="checkbox" checked={f[c.key]} onChange={(e) => set({ ...f, [c.key]: e.target.checked })} />
          {c.label}
        </label>
      ))}
      {only && <ContextMenu x={only.x} y={only.y} entries={[{ label: `Only ${only.label.toLowerCase()}`, onSelect: () => set(onlyCategory(f, only.key)) }]} onClose={() => setOnly(null)} />}
    </div>
  );
}
