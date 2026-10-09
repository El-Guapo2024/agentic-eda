// The board editor's snapping, as the canvas uses it: one object that keeps the grid helper of the tool in force (kicad-port/pcbGridHelper.ts) fed with the view, the
// grid, the grid overrides, the magnetic settings and the board's items, and answers "where does a click here go?".
//
// KiCad's tools each make a PCB_GRID_HELPER when they start and call `BestSnapAnchor` on every mouse event; the same happens here: the canvas calls `point( tool, ... )` from
// the pointer move (so the snap marker, the snap lines and the construction geometry follow the cursor) and again for the press. A different `tool` key starts a new helper,
// as a different tool does; `reset()` ends the session.
//
// The route tool does not use BestSnapAnchor in KiCad: `TOOL_BASE::updateStartItem` / `updateEndItem` pick the pad, via or track under the cursor and `snapToItem` it
// (pad or via centre, a track's end or `AlignToSegment` / `AlignToArc`), else `Align` to the wire grid (the vias' for a via). `routePoint` is that.
import type { BoardState } from "../../api/types";
import { PcbGridHelper, type GridEnv, type LayerSel, type SnapOverlay, type Visibility } from "../../kicad-port/pcbGridHelper";
import { sceneOf, type PadItem, type SnapScene, type TrackItem, type ViaItem } from "../../kicad-port/snapScene";
import { computeVisibleGridSize } from "../../kicad-port/grid";
import { gridSizeFor, pcbItemGrid, selectionGrid, type GridCategory, type GridOverrides, type PcbItemKind } from "../../kicad-port/gridOverrides";
import { itemKind } from "../../kicad-port/pcbItems";
import { reportSnap } from "../../kicad-port/snapReport";
import type { DragFilter, PcbMagnetic } from "../../kicad-port/snapAnchors";
import { distanceTo, dist, type Pt } from "../../kicad-port/snapGeom";
import { getSnapOrigin } from "./gridHelper";

/** Everything about the editor the snapping depends on; the canvas hands it over on every render. */
export interface PcbSnapView {
  board: BoardState | null;
  /** Screen pixels per um. */
  scale: number;
  /** The grid on screen, um. */
  gridUm: number;
  /** The editor's grid list and the overrides over it. */
  grids: readonly number[];
  overrides: GridOverrides | null;
  magnetic: PcbMagnetic;
  /** `state.layerVisible`: a layer is hidden only when it is `false`. */
  layerVisible: Record<string, boolean>;
  highContrast: boolean;
  activeLayer: string | null;
}

/** The modifiers of the event, with Ctrl meaning Cmd on a Mac: Ctrl turns the grid off (`DisableGridSnapping`), Shift the anchors (`MD_SHIFT`). */
export interface SnapMods {
  ctrl: boolean;
  shift: boolean;
}

export interface PointOpts {
  /** The layers of the item being drawn or moved; the active layer when left out, every layer when `"all"`. */
  layers?: LayerSel;
  /** Which grid override applies (`GRID_GRAPHICS` for shapes ...). */
  category?: GridCategory;
  /** Ids of items to ignore (the ones being moved). */
  skip?: ReadonlySet<string>;
  /** `SetSkipPoint` / `ClearSkipPoint`: the point a line started from, which it must not snap back to. `null` clears it; left out keeps what was set. */
  skipPoint?: Pt | null;
}

const NO_SKIP: ReadonlySet<string> = new Set();

/** Whole micrometres, as KiCad's integer coordinates are: with the grid off, or on a grid that is not a whole number of um (1 mil is 25.4 um), a point is wherever it falls, and the verbs take integers. */
const whole = (p: Pt): Pt => [Math.round(p[0]), Math.round(p[1])];

export class PcbSnap {
  private view: PcbSnapView | null = null;
  private helper: PcbGridHelper | null = null;
  private tool: string | null = null;
  private cachedOverlay: SnapOverlay | null = null;
  private last: Pt | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** Called when the overlay changed without a pointer event (a construction proposal came due). */
  onChange: (() => void) | null = null;

  update(view: PcbSnapView): void {
    this.view = view;
    // A new board object: what the helper remembers refers to items that are gone.
    if (this.helper && view.board) this.helper.setScene(sceneOf(view.board));
  }

  private env(): GridEnv {
    const v = this.view!;
    return {
      scale: v.scale > 0 ? v.scale : 1,
      gridUm: v.gridUm,
      visibleGridUm: computeVisibleGridSize(v.gridUm, v.scale > 0 ? v.scale : 1),
      origin: [getSnapOrigin().x, getSnapOrigin().y],
      gridSizeOf: (c) => gridSizeFor(c, v.gridUm, v.grids, v.overrides),
    };
  }

  /** What is drawn: a layer unless the Appearance panel turned it off, and in high-contrast mode only the active layer's items at full strength. */
  private visibility(): Visibility {
    const v = this.view!;
    return { layerVisible: (layer) => v.layerVisible[layer] !== false, highContrastLayers: v.highContrast && v.activeLayer ? new Set([v.activeLayer]) : null };
  }

  private helperFor(tool: string): PcbGridHelper {
    const v = this.view!;
    if (!this.helper || this.tool !== tool) {
      this.cancelTimer();
      const scene: SnapScene = v.board ? sceneOf(v.board) : { items: [] };
      this.helper = new PcbGridHelper(this.env(), scene, v.magnetic, this.visibility());
      this.helper.setClock(() => performance.now());
      this.tool = tool;
      this.cachedOverlay = null;
    }
    this.helper.visibility = this.visibility();
    this.helper.magnetic = v.magnetic;
    this.helper.setEnv(this.env());
    return this.helper;
  }

  private cancelTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  /** The construction manager accepts a proposal only after it has stood for its timeout: arrange to look again then. */
  private armTimer(h: PcbGridHelper): void {
    this.cancelTimer();
    const due = h.pendingDueAt();
    if (due === null) return;
    this.timer = setTimeout(
      () => {
        this.timer = null;
        if (this.helper === h && h.tick()) {
          this.cachedOverlay = h.overlay();
          this.onChange?.();
        }
        if (this.helper === h) this.armTimer(h);
      },
      Math.max(0, due - performance.now()) + 5
    );
  }

  /** `BestSnapAnchor` for the tool `tool`: where a click or a move at `world` goes. */
  point(tool: string, world: Pt, mods: SnapMods, opts: PointOpts = {}): Pt {
    if (!this.view) return world;
    const h = this.helperFor(tool);
    h.setUseGrid(!mods.ctrl);
    h.setSnap(!mods.shift);
    if (opts.skipPoint === null) h.clearSkipPoint();
    else if (opts.skipPoint) h.setSkipPoint(opts.skipPoint);
    const layers: LayerSel = opts.layers ?? (this.view.activeLayer ? new Set([this.view.activeLayer]) : "all");
    const p = whole(h.bestSnapAnchor(world, layers, opts.category ?? "current", opts.skip ?? NO_SKIP));
    this.cachedOverlay = h.overlay();
    this.armTimer(h);
    this.last = p;
    reportSnap({ tool, input: world, output: p, types: this.cachedOverlay.snapPoint?.types ?? 0, anchored: this.cachedOverlay.snapPoint !== null });
    return p;
  }

  /** The point the last call put the cursor at (what a rubber band is drawn to); null before a call and after `reset`. */
  lastPoint(): Pt | null {
    return this.last;
  }

  /**
   * `GRID_HELPER::GetSelectionGrid` for ids: the coarsest grid of the items' categories (a footprint and a track together take the larger of the connected and the wire
   * grid), the current grid when overrides are off.
   */
  private selectionCategory(ids: readonly string[]): GridCategory {
    const v = this.view;
    if (!v?.board) return "current";
    const cats = ids.map((id): GridCategory => {
      const kind = itemKind(v.board!, id);
      const mapped: PcbItemKind | null = kind === "part" ? "footprint" : kind;
      return pcbItemGrid(mapped);
    });
    return selectionGrid(cats, (c) => gridSizeFor(c, v.gridUm, v.grids, v.overrides));
  }

  /** The cursor of a move of `ids`: `BestSnapAnchor( mouse, { active layer }, selectionGrid, ids )`. */
  moveCursor(mouse: Pt, mods: SnapMods, ids: readonly string[]): Pt {
    return this.point("move", mouse, mods, { category: this.selectionCategory(ids), skip: new Set(ids) });
  }

  /** `Align( point, category )` on the helper of `tool`: the grid, with no anchors (the grid is off with Ctrl). */
  align(tool: string, world: Pt, mods: SnapMods, category: GridCategory = "current"): Pt {
    if (!this.view) return world;
    const h = this.helperFor(tool);
    h.setUseGrid(!mods.ctrl);
    const p = whole(h.align(world, category));
    this.last = p;
    return p;
  }

  /** `BestDragOrigin`: the point a move of `items` starts from. */
  dragOrigin(tool: string, mouse: Pt, ids: readonly string[], filter?: DragFilter): Pt {
    if (!this.view?.board) return mouse;
    const h = this.helperFor(tool);
    const wanted = new Set(ids);
    const items = h.scene.items.filter((i) => wanted.has(i.owner) || wanted.has(i.id));
    return h.bestDragOrigin(mouse, items, filter);
  }

  /** The anchor the last `point` snapped to, if the cursor is on one. */
  snappedItemId(): string | null {
    return this.helper?.getSnapped()?.owner ?? null;
  }

  overlay(): SnapOverlay | null {
    return this.cachedOverlay;
  }

  /** The tool ended: forget what it snapped to and take the marker down. */
  reset(): void {
    this.cancelTimer();
    this.helper = null;
    this.tool = null;
    this.cachedOverlay = null;
    this.last = null;
    reportSnap(null);
  }

  // ---------------------------------------------------------------------------------------------------------------------------------- the route tool

  /**
   * `TOOL_BASE::updateStartItem` / `updateEndItem` + `snapToItem`: the point the router is given for a cursor at `world`.
   *   * the pad, via or track under the cursor (`pickSingleItem`: exact hit first, then within a grid of it; pads and vias before tracks; the nearest centre / end) --
   *     for the end, only one of `net` when it is given, and only when Shift is not held and the magnetic setting allows (`checkSnap`);
   *   * a pad or via snaps to its centre; a track to the nearer end if the cursor is within half its width of one, else along the segment (`AlignToSegment`);
   *   * nothing under the cursor: `Align` to the wire grid (the via grid for a via being placed). Ctrl turns the grid off.
   */
  routePoint(world: Pt, mods: SnapMods, opts: { net?: string | null; isEnd: boolean; placingVia?: boolean }): Pt {
    if (!this.view?.board) return world;
    const h = this.helperFor("route");
    h.setUseGrid(!mods.ctrl);
    h.setSnap(!mods.shift);
    const category: GridCategory = opts.placingVia ? "vias" : "wires";
    const align = (): Pt => h.align(world, category);
    const gridSlop = Math.max(h.gridSize("current")[0], 1);
    const item = this.pickRouteItem(h.scene, world, gridSlop, opts.net ?? null);
    const { pads, tracks } = this.view.magnetic;
    // `checkSnap`: an end only snaps to an item the magnetic settings allow; a start always does.
    const allowed = !opts.isEnd || (h.getSnap() && (item?.type === "pad" ? pads !== "never" : item ? tracks !== "never" : false));
    let out: Pt;
    if (!item || !allowed) out = align();
    else if (item.type === "pad") out = item.at;
    else if (item.type === "via") out = item.at;
    else {
      const half = item.width / 2;
      if (dist(world, item.start) < half || dist(world, item.end) < half) out = dist(world, item.start) < dist(world, item.end) ? item.start : item.end;
      else if (item.arc) out = h.alignToArc(world, item.arc);
      else out = h.alignToSegment(world, item.start, item.end);
    }
    out = whole(out);
    this.last = out;
    reportSnap({ tool: "route", input: world, output: out, types: 0, anchored: item !== null && allowed });
    return out;
  }

  /** `pickSingleItem`: pads and vias first (the nearest centre), then tracks (the nearest end); each tried at slop 0 and then within a grid. */
  private pickRouteItem(scene: SnapScene, p: Pt, gridSlop: number, net: string | null): PadItem | ViaItem | TrackItem | null {
    const board = this.view!.board!;
    const visible = (layers: readonly string[]) => layers.some((l) => this.view!.layerVisible[l] !== false);
    for (const slop of [0, gridSlop]) {
      let best: PadItem | ViaItem | null = null;
      let bestD = Infinity;
      let bestSeg: TrackItem | null = null;
      let bestSegD = Infinity;
      for (const item of scene.items) {
        if (item.type !== "pad" && item.type !== "via" && item.type !== "track") continue;
        if (!visible(item.layers)) continue;
        if (item.type === "track") {
          if (net !== null && trackNet(board, item) !== net) continue;
          if (distanceTo(item.geom!, p) - item.width / 2 > slop) continue;
          const d = Math.min(dist(item.start, p), dist(item.end, p));
          if (d < bestSegD) {
            bestSegD = d;
            bestSeg = item;
          }
        } else {
          if (item.type === "pad" && net !== null && padNet(board, item) !== net) continue;
          const reach = item.type === "pad" ? Math.max(item.w, item.h) / 2 : item.d / 2;
          if (slop === 0 ? !item.hit(p) : dist(item.at, p) - reach > slop) continue;
          const d = dist(item.at, p);
          if (d < bestD) {
            bestD = d;
            best = item;
          }
        }
      }
      if (best) return best;
      if (bestSeg) return bestSeg;
    }
    return null;
  }
}

/** The net of the pad item (its footprint's pad by number). */
function padNet(board: BoardState, pad: PadItem): string | null {
  const part = board.parts.find((p) => p.ref === pad.footprint);
  const num = pad.id.slice(pad.footprint.length + 1).split("#")[0];
  return part?.pads?.find((q) => q.num === num)?.net ?? null;
}

function trackNet(board: BoardState, item: TrackItem): string | null {
  return board.routing?.tracks.find((t) => t.id === item.owner)?.net ?? null;
}
