// The board's Selection Filter panel (`PANEL_SELECTION_FILTER`, pcbnew/widgets/panel_selection_filter.cpp at 8303b2ad): its rows, the right-click "Only ..."
// menu of a row (`onRightClick`, `onPopupSelection`), and the filter as the project keeps it (`PROJECT_LOCAL_SETTINGS::m_PcbSelectionFilter`, the
// `.kicad_prl`'s `board.selection_filter`, common/project/project_local_settings.cpp; `PCB_EDIT_FRAME::LoadProjectSettings` / `SaveProjectLocalSettings`).
// UI-LEFT P22. Pure; components/panels/SelectionFilterPanel.tsx shows it, the Appearance panel's file (`appearance.json`'s `local`) keeps it.

/** The filter's switches, as `PCB_SELECTION_FILTER_OPTIONS` names them (the studio's `SelectionFilter`; `keepouts` and `gridItems` absent mean on). */
export interface FilterOptions {
  lockedItems: boolean;
  footprints: boolean;
  text: boolean;
  tracks: boolean;
  vias: boolean;
  pads: boolean;
  graphics: boolean;
  zones: boolean;
  keepouts?: boolean;
  dimensions: boolean;
  points: boolean;
  otherItems: boolean;
  gridItems?: boolean;
}

export type FilterRowKey = Exclude<keyof FilterOptions, "lockedItems" | "gridItems">;

/**
 * The rows "All items" sweeps and that take the right-click menu (`m_cbFootprints->Bind( wxEVT_RIGHT_DOWN, .. )` ... `m_cbOtherItems`), in the panel's order and
 * words (`OnLanguageChanged`). "All items" and "Locked items" have no menu.
 */
export const FILTER_ROWS: readonly { key: FilterRowKey; label: string }[] = [
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

/** `label.Printf( _( "Only %s" ), cb->GetLabel().Lower() )`: "Only footprints", "Only rule areas", "Only other items". */
export function onlyLabel(label: string): string {
  return `Only ${label.toLowerCase()}`;
}

/**
 * `onPopupSelection`: every row off ("All items" with them, and KiCad 10's Grid Items, `m_cbGridItems->SetValue( false )` at 25ec2335), then the row the menu
 * was opened on; "Locked items" is left as it is.
 */
export function onlyFilter<F extends FilterOptions>(filter: F, key: FilterRowKey | "gridItems"): F {
  const next = { ...filter };
  for (const r of FILTER_ROWS) (next as FilterOptions)[r.key] = false;
  next.gridItems = false;
  (next as FilterOptions)[key] = true;
  return next;
}

/** The keys `board.selection_filter` holds, with KiCad's defaults (`lockedItems` off, the rest on) -- the 8303b2ad project keeps no `points`; the studio does. */
const SAVED_KEYS: readonly (keyof FilterOptions)[] = ["lockedItems", "footprints", "text", "tracks", "vias", "pads", "graphics", "zones", "keepouts", "dimensions", "points", "otherItems", "gridItems"];

/** `SaveProjectLocalSettings`: the filter as the project keeps it (every key written, a row that is absent being on). */
export function selectionFilterJson(filter: FilterOptions): Record<string, boolean> {
  const out: Record<string, boolean> = {};
  for (const k of SAVED_KEYS) out[k] = k === "lockedItems" ? filter.lockedItems : filter[k] !== false;
  return out;
}

/** `LoadProjectSettings` with `SetIfPresent`: each key that is a boolean is taken, anything else leaves the switch as it is; not an object leaves the filter. */
export function selectionFilterFromJson<F extends FilterOptions>(filter: F, json: unknown): F {
  if (typeof json !== "object" || json === null || Array.isArray(json)) return filter;
  const next = { ...filter };
  for (const k of SAVED_KEYS) {
    const v = (json as Record<string, unknown>)[k];
    if (typeof v === "boolean") (next as FilterOptions)[k] = v as never;
  }
  return next;
}
