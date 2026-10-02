// Port of common/tool/grid_helper.cpp (GRID_HELPER, the base class) and
// the non-"construction geometry" core of pcbnew/tools/pcb_grid_helper.cpp
// (PCB_GRID_HELPER::BestSnapAnchor): round-to-grid with an arbitrary
// origin, the real Ctrl/Shift modifier semantics, and "prefer the nearest
// anchor (pad/track end/via/footprint origin) over the plain grid point
// when one is close enough."
//
// NOT ported: KiCad's newer snap-line/construction-geometry system
// (SNAP_MANAGER, SNAP_LINE_MANAGER, intersections, reference-only points
// -- most of BestSnapAnchor's ~250 lines) and per-call snap hysteresis
// (ADVANCED_CFG::m_SnapHysteresis, 5px -- needs state carried across
// calls to avoid flicker at the snap-radius boundary; this is a pure,
// stateless function). Both are documented gaps in PARITY-pcb.md.
//
// A real, source-verified correction to how this behavior is often
// described: it is NOT "Shift disables the grid." Per
// pcbnew/tools/edit_tool_move_fct.cpp:
//   grid.SetSnap( !evt->Modifier( MD_SHIFT ) );                    // Shift -> no ANCHOR snap
//   grid.SetUseGrid( ... && !evt->DisableGridSnapping() );          // Ctrl  -> no GRID round
// and tool_event.h: `DisableGridSnapping() { return Modifier(MD_CTRL); }`.
// So: Ctrl disables the plain grid round-off (move at full precision);
// Shift disables snapping to nearby item anchors (grid round-off still
// applies). This module implements exactly that split.

export interface Point {
  x: number;
  y: number;
}

export type AnchorKind = "footprint-origin" | "pad" | "track-end" | "track-mid" | "via";

export interface SnapAnchor extends Point {
  kind: AnchorKind;
  /** Whatever this app identifies the owning item by (a part ref, track/via id, ...) -- not read by this module, just carried through for a caller that wants to know what it snapped to. */
  ownerId: string;
}

/** grid_helper.cpp GRID_HELPER::computeNearest, byte-for-byte (KiROUND(x) == Math.round(x) for the magnitudes a board ever has). */
export function computeNearest(point: Point, grid: Point, origin: Point): Point {
  return {
    x: grid.x > 0 ? Math.round((point.x - origin.x) / grid.x) * grid.x + origin.x : point.x,
    y: grid.y > 0 ? Math.round((point.y - origin.y) / grid.y) * grid.y + origin.y : point.y,
  };
}

export interface GridSnapModifiers {
  /** tool_event.h TOOL_EVENT::DisableGridSnapping(): Ctrl (Cmd on macOS, same substitution as everywhere else in this app) disables the grid round-off entirely. */
  ctrlOrCmd: boolean;
  /** edit_tool_move_fct.cpp: Shift disables snapping to nearby item anchors. Does NOT affect the grid round-off. */
  shiftKey: boolean;
}

/** grid_helper.cpp GRID_HELPER::Align minus the aux-axis special case (this app has no "auxiliary axis" origin marker) -- the grid half of the decision, before any anchor is considered. */
export function alignToGrid(point: Point, gridUm: number, origin: Point, modifiers: Pick<GridSnapModifiers, "ctrlOrCmd">): Point {
  if (modifiers.ctrlOrCmd || gridUm <= 0) return point;
  return computeNearest(point, { x: gridUm, y: gridUm }, origin);
}

/** pcb_grid_helper.cpp BestSnapAnchor: "Tuning constant: snap radius in screen space." */
export const SNAP_RANGE_PX = 25;

/**
 * pcb_grid_helper.cpp BestSnapAnchor's non-snap-line path: find the
 * nearest anchor to `point`, convert the screen-space snap radius to
 * world units via the current scale, clamp it to the visible grid pitch
 * when grid snapping is on (source's own `min(snapScale, GetVisibleGrid().x)`,
 * cited there against GitLab issues #5638/#7125/#12303 -- without the
 * clamp, a coarse visible grid can make anchors "steal" clicks from grid
 * points the user can actually see), and prefer the anchor when it's
 * within range -- otherwise fall back to the grid point.
 *
 * `visibleGridUm` is `kicad-port/grid.ts`'s `computeVisibleGridSize`
 * result for the same `gridUm`; pass it through rather than
 * recomputing here so this module doesn't need to know about grid
 * *drawing* styles.
 */
export function bestSnapPoint(
  point: Point,
  gridUm: number,
  gridOriginUm: Point,
  scalePxPerUm: number,
  visibleGridUm: number,
  anchors: readonly SnapAnchor[],
  modifiers: GridSnapModifiers,
  snapRangePx = SNAP_RANGE_PX
): { point: Point; snappedTo: SnapAnchor | null } {
  const gridPoint = alignToGrid(point, gridUm, gridOriginUm, modifiers);

  if (modifiers.shiftKey || anchors.length === 0 || scalePxPerUm <= 0) {
    return { point: gridPoint, snappedTo: null };
  }

  const enableGrid = !modifiers.ctrlOrCmd && gridUm > 0;
  const snapScaleUm = snapRangePx / scalePxPerUm;
  const snapRangeUm = enableGrid ? Math.min(snapScaleUm, visibleGridUm) : snapScaleUm;

  let nearest: SnapAnchor | null = null;
  let nearestDist = Infinity;
  for (const a of anchors) {
    const d = Math.hypot(a.x - point.x, a.y - point.y);
    if (d < nearestDist) {
      nearestDist = d;
      nearest = a;
    }
  }

  if (nearest && nearestDist <= snapRangeUm) return { point: { x: nearest.x, y: nearest.y }, snappedTo: nearest };
  return { point: gridPoint, snappedTo: null };
}

/** Board-shaped input this module collects anchors from -- a narrow structural slice of api/types.ts's BoardState (duplicated here rather than imported, keeping this module dependency-free/independently testable; any real BoardState satisfies this). */
export interface AnchorSourceBoard {
  parts: ReadonlyArray<{
    ref: string;
    placed: boolean;
    at?: readonly [number, number];
    /** Which copper face the footprint is on -- only read when a layer filter is given. */
    side?: "top" | "bottom";
    pads?: ReadonlyArray<{ x: number; y: number; th?: boolean }>;
  }>;
  routing?: {
    tracks: ReadonlyArray<{ id: string; layer?: string; pts: ReadonlyArray<readonly [number, number]> }>;
    vias: ReadonlyArray<{ id: string; x: number; y: number }>;
  } | null;
}

export interface MagneticSettings {
  /** pcbnew_settings.cpp default: pads/tracks are only magnetic while the track (route) tool is active (MAGNETIC_OPTIONS::CAPTURE_CURSOR_IN_TRACK_TOOL) -- simplified here to a plain on/off this app's Move/Place tools can opt into, rather than modeling KiCad's 3-state (never/route-tool-only/always) setting per item type. */
  pads: boolean;
  tracks: boolean;
  /** Not in KiCad's MAGNETIC_ITEMS at all (footprint "origin" is really just its placement anchor) -- offered as its own flag since this app's model makes it trivial to include or exclude independently. */
  footprintOrigins: boolean;
}

/**
 * MAGNETIC_SETTINGS::allLayers (pcbnew_settings.cpp default false; flipped by
 * `common.Control.magneticSnapToggle`, PCB_CONTROL::SnapMode). pcb_grid_helper.cpp
 * keeps an item as a snap candidate only `if( allLayers || ( aLayers &
 * boardItem->GetLayerSet() ).any() )`, where `aLayers` is the active layer.
 * `activeLayer: null` (nothing active) leaves nothing filtered, as no layer
 * set exists to intersect with.
 */
export interface SnapLayerFilter {
  allLayers: boolean;
  activeLayer: string | null;
}

/** `allCopper`: the item's layer set is every copper layer (a via, a through-hole pad). */
export function matchesActiveLayer(filter: SnapLayerFilter | undefined, itemLayer: string | null | undefined, allCopper = false): boolean {
  if (!filter || filter.allLayers || filter.activeLayer == null) return true;
  if (allCopper) return /\.Cu$/.test(filter.activeLayer);
  // Items with no layer information (older callers) stay candidates.
  if (itemLayer == null) return true;
  return itemLayer === filter.activeLayer;
}

export const DEFAULT_MAGNETIC_SETTINGS: MagneticSettings = { pads: true, tracks: true, footprintOrigins: true };

/**
 * pcb_grid_helper.cpp's `computeAnchors` family, narrowed to the anchor
 * kinds the task calls out (pad centres, track ends/midpoints, footprint
 * origins) plus via centres (not explicitly named, but the same kind of
 * connection anchor `routing.ts:findRouteAnchor` already treats as one
 * for starting a route -- omitting them here while that file includes
 * them would be a strange, arbitrary inconsistency). `excludeOwnerId`
 * drops a single item's own anchors (e.g. don't let a part snap to its
 * own pads while it's the thing being dragged).
 */
export function collectAnchors(board: AnchorSourceBoard, magnetic: MagneticSettings = DEFAULT_MAGNETIC_SETTINGS, excludeOwnerId?: string, layerFilter?: SnapLayerFilter): SnapAnchor[] {
  const anchors: SnapAnchor[] = [];
  const onLayer = (layer: string | null | undefined, allCopper = false) => matchesActiveLayer(layerFilter, layer, allCopper);

  for (const part of board.parts) {
    if (!part.placed || !part.at || part.ref === excludeOwnerId) continue;
    const partLayer = part.side === "bottom" ? "B.Cu" : "F.Cu";
    if (magnetic.footprintOrigins && onLayer(partLayer)) anchors.push({ x: part.at[0], y: part.at[1], kind: "footprint-origin", ownerId: part.ref });
    if (magnetic.pads) {
      for (const pad of part.pads ?? []) {
        // A through-hole pad is on every copper layer; an SMD pad only on its footprint's face.
        if (onLayer(partLayer, pad.th === true)) anchors.push({ x: pad.x, y: pad.y, kind: "pad", ownerId: part.ref });
      }
    }
  }

  if (magnetic.tracks) {
    for (const track of board.routing?.tracks ?? []) {
      if (track.id === excludeOwnerId || track.pts.length === 0) continue;
      if (!onLayer(track.layer)) continue;
      const first = track.pts[0]!;
      const last = track.pts[track.pts.length - 1]!;
      anchors.push({ x: first[0], y: first[1], kind: "track-end", ownerId: track.id });
      if (track.pts.length > 1) anchors.push({ x: last[0], y: last[1], kind: "track-end", ownerId: track.id });
      for (let i = 0; i + 1 < track.pts.length; i++) {
        const a = track.pts[i]!;
        const b = track.pts[i + 1]!;
        anchors.push({ x: (a[0] + b[0]) / 2, y: (a[1] + b[1]) / 2, kind: "track-mid", ownerId: track.id });
      }
    }
    for (const via of board.routing?.vias ?? []) {
      if (via.id === excludeOwnerId) continue;
      if (!onLayer(null, true)) continue; // a via's layer set is every copper layer
      anchors.push({ x: via.x, y: via.y, kind: "via", ownerId: via.id });
    }
  }

  return anchors;
}
