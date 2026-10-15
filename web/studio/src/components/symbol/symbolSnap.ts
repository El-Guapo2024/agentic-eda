// The Symbol Editor's snapping, as its canvas uses it: the grid helper of the tool in force (kicad-port/schGridHelper.ts, a port of EE_GRID_HELPER) kept fed with the view, the grid
// list and its overrides and the items of the unit being edited (kicad-port/symSnapItems.ts), answering "where does a click here go?" and, for a move, "where does the held selection
// start from and where does it go?". The same object the schematic's canvas uses (components/schematic/schSnap.ts), with the library symbol as the sheet.
import type { LibrarySymbol } from "../../api/types";
import { SchGridHelper, type SchSnapItem } from "../../kicad-port/schGridHelper";
import { symbolSnapItems } from "../../kicad-port/symSnapItems";
import { computeVisibleGridSize } from "../../kicad-port/grid";
import { gridSizeFor, type GridCategory, type GridOverrides } from "../../kicad-port/gridOverrides";
import type { GridEnv, SnapOverlay } from "../../kicad-port/gridHelperBase";
import type { Pt } from "../../kicad-port/snapGeom";
import { reportSnap } from "../../kicad-port/snapReport";

/** What the snapping needs to know about the editor; the canvas hands it over on every render. */
export interface SymbolSnapView {
  symbol: LibrarySymbol | null;
  unit: number;
  bodyStyle: number;
  showHiddenPins: boolean;
  /** Screen pixels per um. */
  scale: number;
  gridUm: number;
  grids: readonly number[];
  overrides: GridOverrides | null;
}

export interface SymbolSnapMods {
  /** Ctrl (Cmd on a Mac): the grid off. */
  ctrl: boolean;
  /** Shift: anchors off. */
  shift: boolean;
}

/** Whole micrometres, as KiCad's integer coordinates are: with the grid off (Ctrl) a point is wherever the pointer is, and the verbs take integers. */
const whole = (p: Pt): Pt => [Math.round(p[0]), Math.round(p[1])];

export class SymbolSnap {
  private view: SymbolSnapView | null = null;
  private helper: SchGridHelper | null = null;
  private tool: string | null = null;
  private cachedOverlay: SnapOverlay | null = null;
  private last: Pt | null = null;
  private items: SchSnapItem[] = [];
  private signature = "";
  private seen: LibrarySymbol | null = null;

  update(view: SymbolSnapView): void {
    this.view = view;
    const key = `${view.unit}|${view.bodyStyle}|${view.showHiddenPins}`;
    if (view.symbol === this.seen && key === this.key) return;
    this.seen = view.symbol;
    this.key = key;
    // The editor re-reads the symbol every moment and hands over a new object each time: the items are made again only when what is snapped to changed.
    const items = symbolSnapItems(view.symbol, view.unit, view.bodyStyle, view.showHiddenPins);
    const sig = JSON.stringify(items.map((i) => [i.id, i.kind, i.position]));
    if (sig !== this.signature) {
      this.signature = sig;
      this.items = items;
      this.helper?.setItems(items);
    }
  }
  private key = "";

  private env(): GridEnv {
    const v = this.view!;
    const scale = v.scale > 0 ? v.scale : 1;
    return { scale, gridUm: v.gridUm, visibleGridUm: computeVisibleGridSize(v.gridUm, scale), origin: [0, 0], gridSizeOf: (c) => gridSizeFor(c, v.gridUm, v.grids, v.overrides) };
  }

  private helperFor(tool: string): SchGridHelper {
    if (!this.helper || this.tool !== tool) {
      this.helper = new SchGridHelper(this.env(), this.items);
      this.tool = tool;
      this.cachedOverlay = null;
    }
    this.helper.setEnv(this.env());
    return this.helper;
  }

  /** `BestSnapAnchor`: where a click or cursor at `world` goes for the tool `tool`. */
  point(tool: string, world: Pt, mods: SymbolSnapMods, category: GridCategory = "current", skip: ReadonlySet<string> = new Set()): Pt {
    if (!this.view) return world;
    const h = this.helperFor(tool);
    h.setUseGrid(!mods.ctrl);
    h.setSnap(!mods.shift);
    const p = whole(h.bestSnapAnchor(world, category, skip));
    this.cachedOverlay = h.overlay();
    this.last = p;
    reportSnap({ tool, input: world, output: p, types: 0, anchored: this.cachedOverlay.snapPoint !== null });
    return p;
  }

  /** The cursor of a move of `ids`: `BestSnapAnchor( mouse, selectionGrid, selection )`. */
  moveCursor(mouse: Pt, mods: SymbolSnapMods, ids: readonly string[], category: GridCategory): Pt {
    return this.point("move", mouse, mods, category, new Set(ids));
  }

  /** `BestDragOrigin`: the point of the held items a move starts from. */
  dragOrigin(mouse: Pt, ids: readonly string[], category: GridCategory): Pt {
    if (!this.view) return mouse;
    const wanted = new Set(ids);
    const items = this.items.filter((i) => wanted.has(i.owner) || wanted.has(i.id));
    return whole(this.helperFor("move").bestDragOrigin(mouse, category, items));
  }

  /** The point the last `point` call put the cursor at; null before one and after `reset`. */
  lastPoint(): Pt | null {
    return this.last;
  }

  overlay(): SnapOverlay | null {
    return this.cachedOverlay;
  }

  reset(): void {
    this.helper = null;
    this.tool = null;
    this.cachedOverlay = null;
    this.last = null;
    reportSnap(null);
  }
}
