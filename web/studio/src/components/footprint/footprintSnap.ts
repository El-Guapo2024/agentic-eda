// The Footprint Editor's snapping, as its canvas uses it: the grid helper of the tool in force (kicad-port/pcbGridHelper.ts, a port of PCB_GRID_HELPER) kept fed with the view, the
// grid, the grid overrides and the open footprint's items (kicad-port/fpSnapScene.ts), answering "where does a click here go?". The same object the board editor's canvas uses
// (components/canvas/pcbSnap.ts), with the footprint as the board.
//
// KiCad's tools each make a PCB_GRID_HELPER when they start and call `BestSnapAnchor` on every mouse event, with the layers the tool works on: the shape and text tools the active
// layer (`{ m_frame->GetActiveLayer() }`), the pad tool and the anchor tool every layer (`LSET::AllLayersMask()`). A different `tool` key starts a new helper, as a different tool
// does; `reset()` ends the session. With a pad "always" magnetic (the Footprint Editor's own default) the centre and the outline of the pad under the cursor are anchors, and a
// shape's end points, middle and centre are; Ctrl turns the grid off and Shift the anchors.
import type { LibraryFootprint } from "../../api/types";
import { PcbGridHelper, type GridEnv, type LayerSel, type SnapOverlay, type Visibility } from "../../kicad-port/pcbGridHelper";
import { FOOTPRINT_EDITOR_MAGNETIC, footprintScene } from "../../kicad-port/fpSnapScene";
import type { SnapScene } from "../../kicad-port/snapScene";
import { computeVisibleGridSize } from "../../kicad-port/grid";
import { gridSizeFor, type GridCategory, type GridOverrides } from "../../kicad-port/gridOverrides";
import { reportSnap } from "../../kicad-port/snapReport";
import type { PcbMagnetic } from "../../kicad-port/snapAnchors";
import type { Pt } from "../../kicad-port/snapGeom";

/** Everything about the editor the snapping depends on; the canvas hands it over on every render. */
export interface FootprintSnapView {
  footprint: LibraryFootprint | null;
  /** Screen pixels per um. */
  scale: number;
  gridNm: number;
  grids: readonly number[];
  overrides: GridOverrides | null;
  /** The grid origin of the Footprint Editor (`useFootprintGridOrigin`). */
  origin: Pt;
  highContrast: boolean;
  activeLayer: string | null;
  magnetic?: PcbMagnetic;
}

/** The modifiers of the event, with Ctrl meaning Cmd on a Mac: Ctrl turns the grid off (`DisableGridSnapping`), Shift the anchors (`MD_SHIFT`). */
export interface FootprintSnapMods {
  ctrl: boolean;
  shift: boolean;
}

export interface FootprintPointOpts {
  /** The layers of the item being drawn; the active layer when left out, every layer when `"all"`. */
  layers?: LayerSel;
  category?: GridCategory;
  /** Ids of items to ignore (the ones being moved). */
  skip?: ReadonlySet<string>;
}

const NO_SKIP: ReadonlySet<string> = new Set();

/** Whole micrometres, as KiCad's integer coordinates are: with the grid off, or on a grid that is not a whole number of um, a point is wherever it falls, and the verbs take integers. */
const whole = (p: Pt): Pt => [Math.round(p[0]), Math.round(p[1])];

export class FootprintSnap {
  private view: FootprintSnapView | null = null;
  private helper: PcbGridHelper | null = null;
  private tool: string | null = null;
  private cachedOverlay: SnapOverlay | null = null;
  private last: Pt | null = null;
  private scene: SnapScene = { items: [] };
  private signature = "";
  private seen: LibraryFootprint | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** Called when the overlay changed without a pointer event (a construction proposal came due). */
  onChange: (() => void) | null = null;

  update(view: FootprintSnapView): void {
    this.view = view;
    if (view.footprint === this.seen) return;
    this.seen = view.footprint;
    // The editor re-reads the footprint every moment and hands over a new object each time: the scene is made again only when what is snapped to changed.
    const sig = view.footprint ? JSON.stringify([view.footprint.pads, view.footprint.graphics, view.footprint.texts]) : "";
    if (sig !== this.signature) {
      this.signature = sig;
      this.scene = footprintScene(view.footprint);
      this.helper?.setScene(this.scene);
    }
  }

  private env(): GridEnv {
    const v = this.view!;
    const scale = v.scale > 0 ? v.scale : 1;
    return { scale, gridNm: v.gridNm, visibleGridNm: computeVisibleGridSize(v.gridNm, scale), origin: v.origin, gridSizeOf: (c) => gridSizeFor(c, v.gridNm, v.grids, v.overrides) };
  }

  private visibility(): Visibility {
    const v = this.view!;
    return { layerVisible: () => true, highContrastLayers: v.highContrast && v.activeLayer ? new Set([v.activeLayer]) : null };
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

  private helperFor(tool: string): PcbGridHelper {
    const v = this.view!;
    if (!this.helper || this.tool !== tool) {
      this.cancelTimer();
      this.helper = new PcbGridHelper(this.env(), this.scene, v.magnetic ?? FOOTPRINT_EDITOR_MAGNETIC, this.visibility());
      this.helper.setClock(() => performance.now());
      this.tool = tool;
      this.cachedOverlay = null;
    }
    this.helper.visibility = this.visibility();
    this.helper.magnetic = v.magnetic ?? FOOTPRINT_EDITOR_MAGNETIC;
    this.helper.setEnv(this.env());
    return this.helper;
  }

  /** `BestSnapAnchor` for the tool `tool`: where a click or a move at `world` goes. */
  point(tool: string, world: Pt, mods: FootprintSnapMods, opts: FootprintPointOpts = {}): Pt {
    if (!this.view) return world;
    const h = this.helperFor(tool);
    h.setUseGrid(!mods.ctrl);
    h.setSnap(!mods.shift);
    const layers: LayerSel = opts.layers ?? (this.view.activeLayer ? new Set([this.view.activeLayer]) : "all");
    const p = whole(h.bestSnapAnchor(world, layers, opts.category ?? "current", opts.skip ?? NO_SKIP));
    this.cachedOverlay = h.overlay();
    this.armTimer(h);
    this.last = p;
    reportSnap({ tool, input: world, output: p, types: this.cachedOverlay.snapPoint?.types ?? 0, anchored: this.cachedOverlay.snapPoint !== null });
    return p;
  }

  /** The cursor of a move of `ids`: `BestSnapAnchor( mouse, { active layer }, selectionGrid, ids )`. */
  moveCursor(mouse: Pt, mods: FootprintSnapMods, ids: readonly string[], category: GridCategory): Pt {
    return this.point("move", mouse, mods, { category, skip: new Set(ids) });
  }

  /** `BestDragOrigin`: the point a move of `ids` starts from. */
  dragOrigin(mouse: Pt, ids: readonly string[]): Pt {
    if (!this.view) return mouse;
    const h = this.helperFor("move");
    const wanted = new Set(ids);
    const items = h.scene.items.filter((i) => wanted.has(i.owner) || wanted.has(i.id));
    return whole(h.bestDragOrigin(mouse, items));
  }

  /** The point the last `point` call put the cursor at (what a rubber band is drawn to); null before a call and after `reset`. */
  lastPoint(): Pt | null {
    return this.last;
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
}
