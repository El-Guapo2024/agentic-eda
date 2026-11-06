// The selection filter (pcbnew/widgets/panel_selection_filter.cpp): every
// toggle here actually gates what a click/box-select/select-all can pick,
// via components/canvas/selectionCandidates.ts's `SelectionFilter` (the
// subset of KiCad's real categories this app's model has a selectable
// equivalent for -- see that file's own doc comment for what's
// deliberately left out).
// A row's right-click menu offers "Only <row>" (`onRightClick` / `onPopupSelection`, kicad-port/selectionFilterRows.ts); the filter is the project's, kept with the
// Appearance panel's settings (`appearance.json`, the derived `.kicad_prl`'s `board.selection_filter`). A row that refused every item a click found flashes
// (`OnFlashEvent`, kicad-port/filterFlash.ts: ten steps of 50 ms, the highlight fading out over the row), UI-LEFT-2 PP19.
import { useState, useSyncExternalStore } from "react";
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { SelectionFilter } from "../canvas/selectionCandidates";
import { ContextMenu } from "../canvas/ContextMenu";
import { onlyFilter, onlyLabel } from "../../kicad-port/selectionFilterRows";
import { flashOverlay, getFilterFlash, subscribeFilterFlash } from "../../kicad-port/filterFlash";

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
  /** The row a right-click opened the "Only ..." menu on (`m_onlyCheckbox`), and where. */
  const [menu, setMenu] = useState<{ key: (typeof ROWS)[number]["key"]; label: string; x: number; y: number } | null>(null);
  const flash = useSyncExternalStore(subscribeFilterFlash, getFilterFlash, getFilterFlash);
  /** `onPanelPaint`'s overlay over a flashing row (none for a row at rest). */
  const flashStyle = (key: string) => {
    const step = flash.get(key);
    return step ? { boxShadow: `inset 0 0 0 999px ${flashOverlay(step)}` } : undefined;
  };
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
      <label className="filter-row" title="Allow selection of locked items" data-filter-row="lockedItems" data-flash={flash.get("lockedItems") ?? undefined} style={flashStyle("lockedItems")}>
        <input type="checkbox" checked={f.lockedItems} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { lockedItems: e.target.checked } })} />
        Locked items
      </label>
      {ROWS.map((r) => (
        <label
          className="filter-row"
          key={r.key}
          data-filter-row={r.key}
          data-flash={flash.get(r.key) ?? undefined}
          style={flashStyle(r.key)}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ key: r.key, label: r.label, x: e.clientX, y: e.clientY });
          }}
        >
          <input type="checkbox" checked={f[r.key] !== false} onChange={(e) => dispatch({ type: "SET_SELECTION_FILTER", filter: { [r.key]: e.target.checked } })} />
          {r.label}
        </label>
      ))}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          entries={[{ label: onlyLabel(menu.label), onSelect: () => dispatch({ type: "SET_SELECTION_FILTER", filter: onlyFilter(f, menu.key as Parameters<typeof onlyFilter>[1]) }) }]}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}
