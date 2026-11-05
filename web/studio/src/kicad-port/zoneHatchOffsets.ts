// Zone Properties' "Hatch offset overrides:" grid (the Hatched Fill tab of the copper zone panel): for some copper layers of the zone, how far the pattern of
// its hatched fill is moved from where Board Setup > Zone Hatch Offsets (or the board's origin) puts it. Pure helpers; the grid is components/ZoneDialog.tsx.
//
// Ported from (8303b2ad):
//   pcbnew/dialogs/panel_zone_properties.cpp   TransferZoneSettingsToWindow (the rows, in `LSET::AllCuMask().UIOrder()`), AcceptOptions (the rows replace the
//                                              zone's: `props.hatching_offset = std::nullopt` for every layer, then each row), OnAddLayerItem ("All zone layers
//                                              already overridden."), OnDeleteLayerItem
//   pcbnew/zone_settings.cpp                   LAYER_PROPERTIES_GRID_TABLE::AppendRows (the next layer of the zone without a row, at an offset of 0, 0)
//   pcbnew/zone_settings.h                     ZONE_LAYER_PROPERTIES::hatching_offset

import { uiOrderRank } from "./padEditMode";
import { isCopper } from "./layerPresets";

export interface HatchOffsetRow {
  layer: string;
  x_nm: number;
  y_nm: number;
}

/** `OnAddLayerItem`'s infobar message when every layer of the zone already has a row. */
export const ALL_ZONE_LAYERS_OVERRIDDEN = "All zone layers already overridden.";

/** The grid's rows for a zone's offsets (`TransferZoneSettingsToWindow`): the copper layers that have one, in the layer list's order. */
export function hatchOffsetRows(offsets: Readonly<Record<string, { x_nm: number; y_nm: number }>> | undefined): HatchOffsetRow[] {
  return Object.entries(offsets ?? {})
    .filter(([layer]) => isCopper(layer))
    .sort(([a], [b]) => uiOrderRank(a) - uiOrderRank(b))
    .map(([layer, o]) => ({ layer, x_nm: o.x_nm, y_nm: o.y_nm }));
}

/** `LAYER_PROPERTIES_GRID_TABLE::AppendRows`: the first copper layer of the zone (in the layer list's order) that has no row yet, or null (`AppendRows` returns false). */
export function nextOffsetLayer(rows: readonly HatchOffsetRow[], zoneLayers: readonly string[]): string | null {
  const taken = new Set(rows.map((r) => r.layer));
  const next = [...zoneLayers].filter(isCopper).sort((a, b) => uiOrderRank(a) - uiOrderRank(b)).find((l) => !taken.has(l));
  return next ?? null;
}

/**
 * What `AcceptOptions` gives the zone: one offset per row. A row whose layer the zone is no longer on is left out, as `ZONE::SetLayerSet` erases the properties of the
 * layers that left the set; a second row of a layer replaces the first (`m_LayerProperties[layer] = props`).
 */
export function hatchOffsetsOf(rows: readonly HatchOffsetRow[], zoneLayers: readonly string[]): Record<string, { x_nm: number; y_nm: number }> {
  const out: Record<string, { x_nm: number; y_nm: number }> = {};
  for (const r of rows) if (zoneLayers.includes(r.layer) && isCopper(r.layer)) out[r.layer] = { x_nm: r.x_nm, y_nm: r.y_nm };
  return out;
}

/** The two grids hold the same offsets, row for row. */
export function sameHatchOffsets(a: readonly HatchOffsetRow[], b: readonly HatchOffsetRow[]): boolean {
  return a.length === b.length && a.every((r, i) => r.layer === b[i]!.layer && r.x_nm === b[i]!.x_nm && r.y_nm === b[i]!.y_nm);
}
