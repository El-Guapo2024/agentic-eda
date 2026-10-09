// Which marker on a canvas a right click is on -- `PCB_SELECTION_TOOL` / `SCH_SELECTION_TOOL` pick a `PCB_MARKER` / `SCH_MARKER` like any other item, by the
// marker's own shape (`MARKER_BASE` draws a small icon). Here a marker is a circle (components/canvas/painter.ts `drawDrcMarkers`, components/schematic/painter.ts
// `drawErcMarkers`), so the pick is the nearest marker whose circle holds the point.
//
// The radii are the circles' radii as drawn when selected (the painters' radius times 1.4): a right click on a marker's glyph always lands inside, and
// a click that misses by a hair still finds it. The painters' own constants are `DRC_MARKER_RADIUS_UM = 300` and `ERC_MARKER_RADIUS_UM = 450`.

/** µm. */
export const DRC_MARKER_HIT_UM = 420;
export const ERC_MARKER_HIT_UM = 630;

export type MarkerPoint = readonly [number, number];

/**
 * The index of the marker nearest to (x, y) among those within `radius` of it, or -1. `points[i]` is where marker i is drawn, null for one that has
 * no place on this canvas (an ERC finding whose item the schematic no longer has) or is not drawn (its severity is switched off).
 */
export function nearestMarker(points: ReadonlyArray<MarkerPoint | null>, x: number, y: number, radius: number): number {
  let best = -1;
  let bestD2 = radius * radius;
  points.forEach((p, i) => {
    if (!p) return;
    const d2 = (p[0] - x) ** 2 + (p[1] - y) ** 2;
    // Equal distance: the earlier marker, the one listed first.
    if (d2 < bestD2 || (d2 === bestD2 && best === -1)) {
      best = i;
      bestD2 = d2;
    }
  });
  return best;
}
