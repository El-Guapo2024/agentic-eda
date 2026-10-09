// The schematic editor's snapping: where the cursor goes when a symbol, a wire, a label ... is placed, drawn or moved. A port of eeschema/tools/ee_grid_helper.cpp
// (EE_GRID_HELPER: BestSnapAnchor, BestDragOrigin, computeAnchors, nearestAnchor, GetItemGrid, GetSelectionGrid) on top of GRID_HELPER (kicad-port/gridHelperBase.ts),
// commit 8303b2ad.
//
// What differs from the board's: the anchors are an item's connection points (a symbol's pins, a wire's ends, a label's, a junction's, a bus entry's ends), the position of a
// symbol or a sheet, and -- on a wire -- the point of it in line with the aligned cursor; there are no intersections and no extension geometry. The snap range is 55 mil
// (`SNAP_RANGE` of eeschema/default_values.h). And the rule is not "the nearest anchor within the range": with the grid on, an anchor is taken only when no grid point is
// nearer than the diagonal of the range (about 78 mil), which a 50 or 100 mil grid always has -- so the grid decides, and the anchor wins only with the grid off (Ctrl) or on a
// coarse one. Which grid depends on the item: connected items and wires snap to their own override (50 mil), text to its own (10 mil) -- kicad-port/gridOverrides.ts.
//
// Units: the studio's um (1 mil = 25.4 um).
import { ANCHOR, AnchorList, type Anchor } from "./snapAnchors";
import { GridHelper, samePos, type GridEnv } from "./gridHelperBase";
import { dist, type Box, type Pt } from "./snapGeom";
import { schItemGrid, selectionGrid, type GridCategory, type SchItemKind } from "./gridOverrides";

/** `SNAP_RANGE` (eeschema/default_values.h), mil. */
export const SCH_SNAP_RANGE_MIL = 55;
export const UM_PER_MIL = 25.4;

/** An item of the sheet as the grid helper sees it. */
export interface SchSnapItem {
  /** The item's own id, and the id of the thing it belongs to (a symbol for its fields): what the skip list names. */
  id: string;
  owner: string;
  kind: SchItemKind;
  /** `SCH_ITEM::IsConnectable()`. */
  connectable: boolean;
  /** `GetPosition()`. */
  position: Pt;
  /** `GetConnectionPoints()`: a symbol's pin ends, a wire's ends, a label's anchor, a bus entry's two ends ... */
  connectionPoints: readonly Pt[];
  bbox: Box;
  /** For a wire, a bus or a graphic line: its two ends (`graphic` for the lines of the notes layer, which are not connectable). */
  line?: { a: Pt; b: Pt; graphic: boolean };
  /** For a text box or a table: the second corner (`GetEnd()`). */
  end?: Pt;
}

function pointOnSegment(p: Pt, a: Pt, b: Pt, tol = 1e-6): boolean {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  if (len2 === 0) return dist(p, a) <= tol;
  const t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2;
  if (t < -1e-9 || t > 1 + 1e-9) return false;
  return Math.hypot(a[0] + t * dx - p[0], a[1] + t * dy - p[1]) <= tol;
}

export class SchGridHelper extends GridHelper {
  private items: readonly SchSnapItem[];
  private anchorItems = new Map<Anchor, SchSnapItem>();

  constructor(env: GridEnv, items: readonly SchSnapItem[]) {
    super(env);
    this.items = items;
  }

  /** A new sheet: what the helper remembers refers to items that are gone. */
  setItems(items: readonly SchSnapItem[]): void {
    if (items === this.items) return;
    this.items = items;
    this.fullReset();
  }

  /** `GetItemGrid`. */
  getItemGrid(item: Pick<SchSnapItem, "kind"> | null | undefined): GridCategory {
    return item ? schItemGrid(item.kind) : "current";
  }

  /** `GetSelectionGrid`: the coarsest grid of the items. */
  getSelectionGrid(items: readonly Pick<SchSnapItem, "kind">[]): GridCategory {
    return selectionGrid(
      items.map((i) => this.getItemGrid(i)),
      (c) => this.env.gridSizeOf(c)
    );
  }

  /** `snapRange`: 55 mil. */
  snapRange(): number {
    return SCH_SNAP_RANGE_MIL * UM_PER_MIL;
  }

  /** `computeAnchors( item, refPos, aFrom, aIncludeText )`. */
  private computeAnchors(item: SchSnapItem, ref: Pt, includeText: boolean): void {
    const add = (pos: Pt, flags: number): void => {
      const before = this.anchors.list.length;
      this.anchors.add(pos, flags, []);
      if (this.anchors.list.length > before) this.anchorItems.set(this.anchors.list[before]!, item);
    };
    const isGraphicLine = item.kind === "graphic_line" || (item.line?.graphic ?? false);
    let connection = false;
    switch (item.kind) {
      case "text":
      case "field":
        if (includeText) add(item.position, ANCHOR.ORIGIN);
        break;
      case "text_box":
        if (includeText) {
          add(item.position, ANCHOR.SNAPPABLE | ANCHOR.CORNER);
          if (item.end) add(item.end, ANCHOR.SNAPPABLE | ANCHOR.CORNER);
        }
        break;
      case "symbol":
      case "sheet":
        add(item.position, ANCHOR.ORIGIN);
        connection = true;
        break;
      case "junction":
      case "no_connect":
      case "wire":
      case "bus":
      case "graphic_line":
        // "Don't add anchors for graphic lines unless we're including text, they may be on a non-connectable grid"
        connection = !(isGraphicLine && !includeText);
        break;
      case "global_label":
      case "hier_label":
      case "label":
      case "directive_label":
      case "bus_entry":
      case "sheet_pin":
        connection = true;
        break;
      case "pin":
        add(item.position, ANCHOR.SNAPPABLE | ANCHOR.ORIGIN);
        break;
      default:
        break;
    }
    if (connection) for (const p of item.connectionPoints) add(p, ANCHOR.SNAPPABLE | ANCHOR.CORNER);

    // A wire: the point of it in line with the aligned cursor (a vertical wire at the cursor's height, a horizontal one at its x).
    if (item.line && (includeText || !isGraphicLine)) {
      const { a, b } = item.line;
      const pt = this.align(ref);
      if (a[0] === b[0]) {
        const possible: Pt = [a[0], pt[1]];
        if (pointOnSegment(possible, a, b)) add(possible, ANCHOR.SNAPPABLE | ANCHOR.VERTICAL);
      } else if (a[1] === b[1]) {
        const possible: Pt = [pt[0], a[1]];
        if (pointOnSegment(possible, a, b)) add(possible, ANCHOR.SNAPPABLE | ANCHOR.HORIZONTAL);
      }
    }
  }

  /**
   * `nearestAnchor( aPos, aFlags, aGrid )`: an anchor of an item that is not on the category's kind of grid is skipped -- the connectable grid takes connectable items' anchors
   * only, the graphics grid the others'.
   */
  private nearestAnchor(pos: Pt, flags: number, category: GridCategory): Anchor | null {
    let minDist = Infinity;
    let best: Anchor | null = null;
    for (const a of this.anchors.list) {
      if ((flags & a.flags) !== flags) continue;
      const item = this.anchorItems.get(a);
      if (item) {
        if (category === "connectable" && !item.connectable) continue;
        if (category === "graphics" && item.connectable) continue;
      }
      const d = dist(a.pos, pos);
      if (d < minDist) {
        minDist = d;
        best = a;
      }
    }
    return best;
  }

  private clearAnchors(): void {
    this.anchors = new AnchorList();
    this.anchorItems.clear();
  }

  /**
   * `BestDragOrigin`: the point of the items picked up that the move starts from. Connectable items are what counts when any is among them (text anchors are often off-grid): the
   * nearest of their origins and connection points to the mouse, or -- if both are further than 50 px -- the nearest point on an outline.
   */
  bestDragOrigin(mouse: Pt, category: GridCategory, items: readonly SchSnapItem[]): Pt {
    this.clearAnchors();
    const hasConnectables = items.some((i) => {
      const g = this.getItemGrid(i);
      return g === "connectable" || g === "wires";
    });
    for (const item of items) this.computeAnchors(item, mouse, !hasConnectables);
    const lineSnapMinCornerDistance = 50 / this.env.scale;
    const outline = this.nearestAnchor(mouse, ANCHOR.OUTLINE, category);
    const corner = this.nearestAnchor(mouse, ANCHOR.CORNER, category);
    const origin = this.nearestAnchor(mouse, ANCHOR.ORIGIN, category);
    let best: Anchor | null = null;
    let minDist = Infinity;
    if (origin) {
      minDist = dist(origin.pos, mouse);
      best = origin;
    }
    if (corner) {
      const d = dist(corner.pos, mouse);
      if (d < minDist) {
        minDist = d;
        best = corner;
      }
    }
    if (outline) {
      const d = dist(outline.pos, mouse);
      if (minDist > lineSnapMinCornerDistance && d < minDist) best = outline;
    }
    return best ? best.pos : mouse;
  }

  /**
   * `BestSnapAnchor( aOrigin, aGrid, aSkip )`. `skip` holds the ids (an item's or its owner's) to ignore -- the items being moved.
   */
  bestSnapAnchor(origin: Pt, category: GridCategory = "current", skip: ReadonlySet<string> = new Set()): Pt {
    const snapRange = this.snapRange();
    let pt = origin;
    let snapDist: Pt = [snapRange, snapRange];
    let gridChecked = false;
    let snappedToAnchor = false;
    const bb = { x0: origin[0] - snapRange / 2, y0: origin[1] - snapRange / 2, x1: origin[0] + snapRange / 2, y1: origin[1] + snapRange / 2 };

    this.clearAnchors();
    this.snapItem = null;

    for (const item of this.items) {
      if (item.bbox.x1 < bb.x0 || item.bbox.x0 > bb.x1 || item.bbox.y1 < bb.y0 || item.bbox.y0 > bb.y1) continue;
      if (skip.has(item.id) || skip.has(item.owner)) continue;
      this.computeAnchors(item, origin, false);
    }

    const nearest = this.nearestAnchor(origin, ANCHOR.SNAPPABLE, category);
    const nearestGrid = this.align(origin, category);
    this.constructionVisible = this.enableSnap;

    const lines = this.snapManager.snapLines;
    const gridSize = this.gridSize(category);
    const range = Math.hypot(snapDist[0], snapDist[1]);

    let guideSnap: Pt | null = null;
    if (this.enableSnapLine) guideSnap = this.snapToConstructionLines(origin, nearestGrid, gridSize, snapRange);

    if (this.enableSnap && nearest && dist(nearest.pos, origin) < range) {
      if (this.canUseGrid() && dist(nearestGrid, origin) < range) {
        pt = nearestGrid;
        snapDist = [Math.abs(nearestGrid[0] - origin[0]), Math.abs(nearestGrid[1] - origin[1])];
        gridChecked = true;
      } else {
        pt = nearest.pos;
        snapDist = [Math.abs(nearest.pos[0] - origin[0]), Math.abs(nearest.pos[1] - origin[1])];
        snappedToAnchor = true;
        gridChecked = true;
      }
    }

    if (guideSnap && !(this.skipPoint && samePos(this.skipPoint, guideSnap))) {
      lines.setSnapLineEnd(guideSnap);
      this.hideSnapPoint();
      this.snapItem = null;
      return guideSnap;
    }

    if (snappedToAnchor && nearest) {
      this.snapItem = nearest;
      this.updateSnapPoint(pt, 0);
      lines.setSnapLineOrigin(pt);
      lines.setSnapLineEnd(null);
      return pt;
    }

    this.snapItem = null;
    if (this.canUseGrid() && !gridChecked) pt = nearestGrid;
    lines.setSnapLineEnd(null);
    this.hideSnapPoint();
    return pt;
  }

  /** `GetSnapped`: the item of the anchor last snapped to. */
  getSnapped(): SchSnapItem | null {
    return this.snapItem ? (this.anchorItems.get(this.snapItem) ?? null) : null;
  }
}
