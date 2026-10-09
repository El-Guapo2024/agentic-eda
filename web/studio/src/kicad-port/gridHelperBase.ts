// The base of the board's and the schematic's grid helpers: a port of common/tool/grid_helper.cpp and include/tool/grid_helper.h (GRID_HELPER), commit 8303b2ad -- the
// switches the modifier keys flip (grid, anchors, snap lines), the grid and its origin, Align, the auxiliary axes, the snap line / construction manager, and
// SnapToConstructionLines. kicad-port/pcbGridHelper.ts (PCB_GRID_HELPER) and kicad-port/schGridHelper.ts (EE_GRID_HELPER) extend it with their own anchors.
import { AnchorList, type Anchor } from "./snapAnchors";
import { SnapManager, nearestGridPointOnLine, type ConstructionManager, type Drawable } from "./constructionManager";
import { dist, PT, type Pt } from "./snapGeom";
import type { GridCategory } from "./gridOverrides";

/** `KiROUND`: half away from zero. */
export const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** `ADVANCED_CFG::m_SnapHysteresis`: pixels, 5 by default. */
export const SNAP_HYSTERESIS_PX = 5;
/** BestSnapAnchor's "Tuning constant: snap radius in screen space". */
export const SNAP_RANGE_PX = 25;

const SAME_POINT_TOL = 1e-6;
export const samePos = (a: Pt, b: Pt): boolean => Math.abs(a[0] - b[0]) <= SAME_POINT_TOL && Math.abs(a[1] - b[1]) <= SAME_POINT_TOL;

/** What the helper needs to know about the view and the grid; the editor refreshes it before each call. */
export interface GridEnv {
  /** Screen pixels per um (`ToWorld( px )` is `px / scale`). */
  scale: number;
  /** `GAL::GetGridSize()`: the current grid, um. */
  gridUm: number;
  /** `GAL::GetVisibleGridSize()`: the grid as it is drawn (a fine grid is drawn coarser when zoomed out), um. */
  visibleGridUm: number;
  /** `GAL::GetGridOrigin()`. */
  origin: Pt;
  /** `GetGridSize( category )`: the category's own grid when overrides are on (kicad-port/gridOverrides.ts `gridSizeFor`), else the current. */
  gridSizeOf(category: GridCategory): number;
}

export interface Visibility {
  layerVisible(layer: string): boolean;
  /** The layers that are drawn at full strength in high-contrast mode; null when the display is normal contrast. */
  highContrastLayers: ReadonlySet<string> | null;
}

/** A set of layers an item must be on to be snapped to, or every layer. */
export type LayerSel = ReadonlySet<string> | "all";

export const layersOverlap = (sel: LayerSel, layers: readonly string[]): boolean => sel === "all" || layers.some((l) => sel.has(l));

/** What the canvas draws for the helper's state. */
export interface SnapOverlay {
  /** The snap marker (`m_viewSnapPoint`): a circle and cross at the point, and an icon from the point types beside it. */
  snapPoint: { pos: Pt; types: number } | null;
  /** The auxiliary axes (`m_viewAxis`), a big cross at the point a move or a route started from. */
  auxAxis: Pt | null;
  /** The dashed guides through the snap line origin, one per direction, the one in use highlighted. */
  guides: { a: Pt; b: Pt; active: boolean }[];
  /** The snap line from its origin to where the cursor snapped on it. */
  snapLine: { a: Pt; b: Pt } | null;
  /** The construction geometry on show. */
  construction: { drawable: Drawable; persistent: boolean; lineWidth: number }[];
}

// ------------------------------------------------------------------------------------------------------------------------------------- GRID_HELPER

export class GridHelper {
  protected enableSnap = true;
  protected enableGrid = true;
  protected enableSnapLine = true;
  protected skipPoint: Pt | null = null;
  protected auxAxis: Pt | null = null;
  protected snapItem: Anchor | null = null;
  protected anchors = new AnchorList();
  readonly snapManager = new SnapManager();
  protected viewSnapPoint: { pos: Pt; types: number; visible: boolean } = { pos: [0, 0], types: PT.NONE, visible: false };
  protected constructionVisible = false;
  protected clock: () => number = () => Date.now();

  constructor(public env: GridEnv) {}

  setClock(clock: () => number): void {
    this.clock = clock;
  }

  setEnv(env: GridEnv): void {
    this.env = env;
  }

  // ---- the modifiers' switches

  /** `SetSnap`: snapping to anchors (Shift turns it off). */
  setSnap(on: boolean): void {
    this.enableSnap = on;
  }
  getSnap(): boolean {
    return this.enableSnap;
  }
  /** `SetUseGrid`: rounding to the grid (Ctrl turns it off). */
  setUseGrid(on: boolean): void {
    this.enableGrid = on;
  }
  getUseGrid(): boolean {
    return this.enableGrid;
  }
  setSnapLine(on: boolean): void {
    this.enableSnapLine = on;
  }
  setSkipPoint(p: Pt): void {
    this.skipPoint = p;
  }
  clearSkipPoint(): void {
    this.skipPoint = null;
  }
  /** `SetAuxAxes`. */
  setAuxAxes(enable: boolean, origin: Pt = [0, 0]): void {
    this.auxAxis = enable ? origin : null;
  }
  setSnapLineDirections(directions: readonly Pt[]): void {
    this.snapManager.snapLines.setDirections(directions);
  }
  setSnapLineOrigin(origin: Pt): void {
    this.snapManager.snapLines.setSnapLineOrigin(origin);
  }
  setSnapLineEnd(end: Pt | null): void {
    this.snapManager.snapLines.setSnapLineEnd(end);
  }
  clearSnapLine(): void {
    this.snapManager.snapLines.clearSnapLine();
  }

  /** `FullReset`: forget the anchor snapped to, the snap line and the construction geometry. */
  fullReset(): void {
    this.snapManager.clear();
    this.snapItem = null;
    this.anchors.clear();
    this.viewSnapPoint.visible = false;
  }

  /** `canUseGrid`. */
  protected canUseGrid(): boolean {
    return this.enableGrid;
  }

  // ---- the grid

  /** `GetGrid`. */
  getGrid(): Pt {
    return [this.env.gridUm, this.env.gridUm];
  }
  getVisibleGrid(): number {
    return this.env.visibleGridUm;
  }
  getOrigin(): Pt {
    return this.env.origin;
  }
  /** `GetGridSize( category )`: a square grid, so one number twice. */
  gridSize(category: GridCategory): Pt {
    const g = this.env.gridSizeOf(category);
    return [g, g];
  }

  /** `computeNearest`: the nearest point of the grid anchored at `offset`. */
  protected computeNearest(p: Pt, grid: Pt, offset: Pt): Pt {
    return [grid[0] > 0 ? kiRound((p[0] - offset[0]) / grid[0]) * grid[0] + offset[0] : p[0], grid[1] > 0 ? kiRound((p[1] - offset[1]) / grid[1]) * grid[1] + offset[1] : p[1]];
  }

  /** `AlignGrid( point, category )`. */
  alignGrid(p: Pt, category: GridCategory = "current"): Pt {
    return this.computeNearest(p, this.gridSize(category), this.getOrigin());
  }

  /** `Align( point, category )`: the nearest grid point, unless the grid is off; and a coordinate of the auxiliary axes when that is nearer than the grid's. */
  align(p: Pt, category: GridCategory = "current"): Pt {
    if (!this.canUseGrid()) return p;
    const nearest = this.alignGrid(p, category);
    const axis = this.auxAxis;
    if (!axis) return nearest;
    const out: [number, number] = [nearest[0], nearest[1]];
    if (Math.abs(axis[0] - p[0]) < Math.abs(nearest[0] - p[0])) out[0] = axis[0];
    if (Math.abs(axis[1] - p[1]) < Math.abs(nearest[1] - p[1])) out[1] = axis[1];
    return out;
  }

  /** `GetSnappedPoint`. */
  getSnappedPoint(): Pt | null {
    return this.snapItem ? this.snapItem.pos : null;
  }

  /**
   * `SnapToConstructionLines`: with a snap line origin and directions, the point on the nearest snap line within `snapRange` (the active direction's reach is 1.5 times
   * longer), put on the grid if the grid is on -- unless it is the skip point. Same-distance candidates go to the one nearer the cursor.
   */
  snapToConstructionLines(point: Pt, nearestGrid: Pt, grid: Pt, snapRange: number): Pt | null {
    const lines = this.snapManager.snapLines;
    const origin = lines.snapLineOrigin;
    const directions = lines.directions;
    if (!origin || directions.length === 0) return null;
    const active = lines.activeDirection;
    const delta: Pt = [point[0] - origin[0], point[1] - origin[1]];
    let best: Pt | null = null;
    let bestPerp = Infinity;
    let bestDistance = Infinity;
    directions.forEach((dir, ii) => {
      const len = Math.hypot(dir[0], dir[1]);
      if (len === 0) return;
      const unit: Pt = [dir[0] / len, dir[1] / len];
      const along = delta[0] * unit[0] + delta[1] * unit[1];
      const projection: Pt = [origin[0] + unit[0] * along, origin[1] + unit[1] * along];
      const perp = Math.hypot(delta[0] - unit[0] * along, delta[1] - unit[1] * along);
      let threshold = snapRange;
      if (active !== null && active === ii) threshold *= 1.5;
      if (perp > threshold) return;
      let candidate: Pt = projection;
      if (this.canUseGrid()) {
        if (dir[0] === 0 && dir[1] !== 0) candidate = [origin[0], nearestGrid[1]];
        else if (dir[1] === 0 && dir[0] !== 0) candidate = [nearestGrid[0], origin[1]];
        else candidate = nearestGridPointOnLine(projection, origin, unit, point, grid, this.getOrigin());
      }
      if (this.skipPoint && samePos(candidate, this.skipPoint)) return;
      const candidateDistance = dist(candidate, point);
      if (perp < bestPerp || (Math.abs(perp - bestPerp) < 1e-9 && candidateDistance < bestDistance)) {
        bestPerp = perp;
        bestDistance = candidateDistance;
        best = candidate;
      }
    });
    return best;
  }

  // ---- what the canvas draws

  /** The state to draw: see `SnapOverlay`. */
  overlay(): SnapOverlay {
    const lines = this.snapManager.snapLines;
    const origin = lines.snapLineOrigin;
    const reach = 5_000_000;
    const guides = origin ? lines.directions.map((d, i) => ({ a: [origin[0] - d[0] * reach, origin[1] - d[1] * reach] as Pt, b: [origin[0] + d[0] * reach, origin[1] + d[1] * reach] as Pt, active: lines.activeDirection === i })) : [];
    const end = lines.snapLineEnd;
    return {
      snapPoint: this.viewSnapPoint.visible ? { pos: this.viewSnapPoint.pos, types: this.viewSnapPoint.types } : null,
      auxAxis: this.auxAxis,
      guides,
      snapLine: origin && end && dist(origin, end) >= 0.01 ? { a: origin, b: end } : null,
      construction: this.constructionVisible ? this.snapManager.construction.drawables() : [],
    };
  }

  /** The construction manager's timer: call it when the time `pendingDueAt` returns has come. True if what is on show changed. */
  tick(): boolean {
    return this.snapManager.construction.tick(this.clock());
  }

  /** When a pending construction proposal comes due, on the helper's clock; null if none is waiting. */
  pendingDueAt(): number | null {
    return this.snapManager.construction.dueAt;
  }

  protected updateSnapPoint(pos: Pt, types: number): void {
    this.viewSnapPoint = { pos, types, visible: true };
  }

  protected hideSnapPoint(): void {
    this.viewSnapPoint = { ...this.viewSnapPoint, types: PT.NONE, visible: false };
  }

  protected get construction(): ConstructionManager {
    return this.snapManager.construction;
  }
}

