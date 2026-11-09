// What a click inside a zone hits (UI-LEFT-2 PP10): `PCB_COLLECTOR::Inspect` (pcbnew/collectors.cpp at KiCad 8303b2ad) takes a zone on its outline
// (`HitTestForCorner( m_refPos, accuracy * 2 ) || HitTestForEdge( m_refPos, accuracy )`) or, unless the guide ignores zone fills, where it is filled on a visible
// layer (`HitTestFilledArea( layer, m_refPos )`: the fill polygons, `m_FilledPolysList`); a rule area, which has no fill, is hit anywhere inside its outline
// (`m_Poly->Contains( aRefPos, -1, aAccuracy )`). The guide ignores the fills unless they are shown filled (`PCB_SELECTION_TOOL::getCollectorsGuide`:
// `guide.SetIgnoreZoneFills( displayOpts.m_ZoneDisplayMode != ZONE_DISPLAY_MODE::SHOW_FILLED )`). So an unfilled pour is picked by its outline only.
//
// The fills are the studio's `GET /api/fill` (state.zoneFill), which the store hands here as they change (`setZonePickFills`), with the display mode; until it
// has (a test, a page that has not read them yet) a zone's inside stands in for its fill, as before.
import type { FillReport, Zone } from "../api/types";
import { layerIsVisible } from "./layerPresets";
import { zoneLayers } from "./zoneLayers";

interface PickFills {
  /** zone id -> its fill on each layer: the fractured rings (`FillZone::fragments`), even-odd. */
  byZone: Map<string, { layer: string; rings: [number, number][][] }[]>;
  /** `m_ZoneDisplayMode == SHOW_FILLED`: the fills can be clicked. */
  showFilled: boolean;
}

let current: PickFills | null = null;

/** The fills a click can hit and whether they are shown filled (the store's `zoneFill` and `zoneDisplayMode`); `null` forgets them. */
export function setZonePickFills(fill: FillReport | null | undefined, displayMode: string): void {
  if (!fill) {
    current = null;
    return;
  }
  const byZone: PickFills["byZone"] = new Map();
  for (const z of fill.zones) {
    if (z.fragments.length === 0) continue;
    const list = byZone.get(z.id) ?? [];
    list.push({ layer: z.layer, rings: z.fragments });
    byZone.set(z.id, list);
  }
  current = { byZone, showFilled: displayMode === "filled" };
}

/** Even-odd: inside an odd number of the rings (a fractured ring's slit has no width). */
function insideRings(rings: readonly (readonly [number, number])[][], x: number, y: number): boolean {
  let inside = false;
  for (const r of rings) {
    for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
      const [xi, yi] = r[i]!;
      const [xj, yj] = r[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
  }
  return inside;
}

/**
 * `HitTestFilledArea` over the zone's visible layers: true when (x, y) is in the zone's fill on one of them and fills are shown filled; false when it is not
 * (an unfilled zone included); `null` when no fills were handed over (the caller then takes the zone's inside).
 */
export function zoneFillHit(z: Pick<Zone, "id" | "layer" | "layers">, x: number, y: number, layerVisible: Readonly<Record<string, boolean>>): boolean | null {
  if (!current) return null;
  if (!current.showFilled) return false;
  const fills = current.byZone.get(z.id);
  if (!fills) return false;
  const visible = new Set(zoneLayers(z).filter((l) => layerIsVisible(layerVisible, l)));
  return fills.some((f) => visible.has(f.layer) && insideRings(f.rings, x, y));
}
