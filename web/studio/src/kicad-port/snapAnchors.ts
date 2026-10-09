// The points of the board's items the cursor can snap to. A port of PCB_GRID_HELPER::computeAnchors( BOARD_ITEM*, ... ) (pcbnew/tools/pcb_grid_helper.cpp) and the
// ANCHOR_FLAGS / ANCHOR of GRID_HELPER (include/tool/grid_helper.h), commit 8303b2ad.
//
// An anchor is a position, flags saying what kind of point it is, the point types (centre, end, mid, corner, quadrant, intersection ...) the snap marker is drawn from,
// and the items it belongs to (two for an intersection, none for a point of pure construction geometry). The flags:
//
//   SNAPPABLE    the cursor snaps to it (BestSnapAnchor looks at these only)
//   ORIGIN       the point an item is picked up by: its position, a pad's centre, a circle's centre (BestDragOrigin)
//   CORNER       an item's corner or end
//   OUTLINE      a point on an item's outline, other than the ones above: the nearest point of a polygon to the cursor, a pad's quadrants
//   CONSTRUCTED  not intrinsic to an item: an intersection, a point of extension geometry
//
// `from` is `aFrom`: the anchors are for picking an item up (BestDragOrigin) rather than for snapping onto it. For a pad that means its centre only; every kind of item is then
// filtered by the selection filter instead of by the magnetic settings.
import type { FootprintItem, PadItem, ShapeItem, SnapItem, TrackItem, ViaItem, ZoneItem, DimensionItem } from "./snapScene";
import { PT, OVAL, circleKeyPoints, ovalKeyPoints, polylineNearest, type Pt } from "./snapGeom";

/** `GRID_HELPER::ANCHOR_FLAGS`. */
export const ANCHOR = { CORNER: 1, OUTLINE: 2, SNAPPABLE: 4, ORIGIN: 8, VERTICAL: 16, HORIZONTAL: 32, CONSTRUCTED: 64, ALL: 127 } as const;

export interface Anchor {
  pos: Pt;
  flags: number;
  /** `POINT_TYPE` bits (kicad-port/snapGeom.ts `PT`). */
  pointTypes: number;
  /** The items that make the anchor; null stands for construction geometry that belongs to no item (a snap line). */
  items: (SnapItem | null)[];
}

/** `MAGNETIC_OPTIONS`: never (`NO_EFFECT`), only while a track is being routed (`CAPTURE_CURSOR_IN_TRACK_TOOL`), or always (`CAPTURE_ALWAYS`). */
export type MagneticOption = "never" | "track-tool" | "always";

/** `MAGNETIC_SETTINGS` (pcbnew_settings.h): the constructor's defaults are pads and tracks "in track tool", graphics off, the active layer only. */
export interface PcbMagnetic {
  pads: MagneticOption;
  tracks: MagneticOption;
  graphics: boolean;
  allLayers: boolean;
}

export const DEFAULT_PCB_MAGNETIC: PcbMagnetic = { pads: "track-tool", tracks: "track-tool", graphics: false, allLayers: false };

/** `PCB_SELECTION_FILTER_OPTIONS`, the members the anchors read (an absent member counts as allowed). */
export interface DragFilter {
  footprints?: boolean;
  pads?: boolean;
  tracks?: boolean;
  vias?: boolean;
  zones?: boolean;
  graphics?: boolean;
  text?: boolean;
  dimensions?: boolean;
}

export interface AnchorContext {
  magnetic: PcbMagnetic;
  /** `checkVisibility`: the item is shown (a layer of it is visible, and in high-contrast mode on an active layer). */
  visible(item: SnapItem): boolean;
  /** `view->IsLayerVisible( LAYER_FOOTPRINTS_FR / _BK / _ANCHOR )`. */
  footprintLayerVisible(fp: FootprintItem): boolean;
  /** `GetGrid()`: the current grid size, for the footprint-centre rule. */
  grid: Pt;
  filter?: DragFilter;
}

/** `addAnchor`'s mask: an anchor whose flags are not all in the mask is dropped. */
export class AnchorList {
  readonly list: Anchor[] = [];
  constructor(public mask: number = ANCHOR.ALL) {}

  add(pos: Pt, flags: number, items: (SnapItem | null)[], pointTypes: number = PT.NONE): void {
    if ((flags & this.mask) === flags) this.list.push({ pos, flags, pointTypes, items });
  }

  clear(): void {
    this.list.length = 0;
  }
}

const allowed = (filter: DragFilter | undefined, key: keyof DragFilter): boolean => !filter || filter[key] !== false;

const SNAP = ANCHOR.SNAPPABLE;

/** `handlePadShape`: a pad's centre and, unless picking up, the key points of its outline. */
function padAnchors(out: AnchorList, pad: PadItem, from: boolean): void {
  out.add(pad.at, ANCHOR.ORIGIN | SNAP, [pad], PT.CENTER);
  if (from) return;
  const { w, h } = pad;
  if (pad.round && w === h) {
    for (const k of circleKeyPoints(pad.at, w / 2, false)) out.add(k.pt, ANCHOR.OUTLINE | SNAP, [pad], k.types);
  } else if (pad.round) {
    for (const k of ovalKeyPoints(pad.at, w, h, 0, OVAL.CENTER | OVAL.CAP_TIPS | OVAL.SIDE_MIDPOINTS | OVAL.CARDINAL_EXTREMES)) out.add(k.pt, ANCHOR.OUTLINE | SNAP, [pad], k.types);
  } else {
    // `corners.Append( -hx, hy ) ... ( hx, hy ) ... ( hx, -hy ) ... ( -hx, -hy )`, closed; each side gives its start as a corner and its middle as a mid point.
    const hx = w / 2;
    const hy = h / 2;
    const corners: Pt[] = [
      [pad.at[0] - hx, pad.at[1] + hy],
      [pad.at[0] + hx, pad.at[1] + hy],
      [pad.at[0] + hx, pad.at[1] - hy],
      [pad.at[0] - hx, pad.at[1] - hy],
    ];
    corners.forEach((a, i) => {
      const b = corners[(i + 1) % corners.length]!;
      out.add(a, ANCHOR.OUTLINE | SNAP, [pad], PT.CORNER);
      out.add([(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], ANCHOR.OUTLINE | SNAP, [pad], PT.MID);
      if (i === corners.length - 1) out.add(b, ANCHOR.OUTLINE | SNAP, [pad], PT.CORNER);
    });
  }
}

/** `addRectPoints`: the centre, the four corners and the four side middles of a box, as corners. */
function rectPoints(out: AnchorList, b: { x0: number; y0: number; x1: number; y1: number }, owner: SnapItem): void {
  const flags = ANCHOR.CORNER | SNAP;
  const origin: Pt = [b.x0, b.y0];
  const topRight: Pt = [b.x1, b.y0];
  const end: Pt = [b.x1, b.y1];
  const bottomLeft: Pt = [b.x0, b.y1];
  out.add([(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2], flags, [owner], PT.CENTER);
  for (const [a, c] of [
    [origin, topRight],
    [topRight, end],
    [end, bottomLeft],
    [bottomLeft, origin],
  ] as const) {
    out.add(a, flags, [owner], PT.CORNER);
    out.add([(a[0] + c[0]) / 2, (a[1] + c[1]) / 2], flags, [owner], PT.MID);
  }
}

/** `handleShape`. */
function shapeAnchors(out: AnchorList, item: ShapeItem, ref: Pt): void {
  const s = item.shape;
  switch (s.kind) {
    case "circle": {
      const [cx, cy] = s.center;
      const r = Math.hypot(s.center[0] - s.end[0], s.center[1] - s.end[1]);
      out.add(s.center, ANCHOR.ORIGIN | SNAP, [item], PT.CENTER);
      for (const [dx, dy] of [
        [-r, 0],
        [r, 0],
        [0, -r],
        [0, r],
      ] as const)
        out.add([cx + dx, cy + dy], ANCHOR.OUTLINE | SNAP, [item], PT.QUADRANT);
      break;
    }
    case "arc": {
      out.add(s.start, ANCHOR.CORNER | SNAP, [item], PT.END);
      out.add(s.end, ANCHOR.CORNER | SNAP, [item], PT.END);
      out.add(s.mid, ANCHOR.CORNER | SNAP, [item], PT.MID);
      const a = item.geom?.t === "arc" ? item.geom : null;
      if (a) out.add(a.c, ANCHOR.ORIGIN | SNAP, [item], PT.CENTER);
      break;
    }
    case "rect":
      rectPoints(out, { x0: Math.min(s.start[0], s.end[0]), y0: Math.min(s.start[1], s.end[1]), x1: Math.max(s.start[0], s.end[0]), y1: Math.max(s.start[1], s.end[1]) }, item);
      break;
    case "segment":
      out.add(s.start, ANCHOR.CORNER | SNAP, [item], PT.END);
      out.add(s.end, ANCHOR.CORNER | SNAP, [item], PT.END);
      out.add([(s.start[0] + s.end[0]) / 2, (s.start[1] + s.end[1]) / 2], ANCHOR.CORNER | SNAP, [item], PT.MID);
      break;
    case "polygon":
      for (const p of s.pts) out.add(p, ANCHOR.CORNER | SNAP, [item], PT.CORNER);
      if (s.pts.length > 0) out.add(polylineNearest(s.pts, true, ref), ANCHOR.OUTLINE, [item], PT.NONE);
      break;
    case "bezier":
      out.add(s.start, ANCHOR.CORNER | SNAP, [item], PT.END);
      out.add(s.end, ANCHOR.CORNER | SNAP, [item], PT.END);
      out.add(s.start, ANCHOR.ORIGIN | SNAP, [item]); // `default:` of the switch: `shape->GetPosition()`
      break;
  }
}

/** The anchors of a dimension (`PCB_DIM_*` cases): its end points and the points of its crossbar, leader or knee. */
function dimensionAnchors(out: AnchorList, item: DimensionItem): void {
  const d = item.dim;
  const flags = ANCHOR.CORNER | SNAP;
  const [sx, sy] = d.start;
  const [ex, ey] = d.end;
  switch (d.kind) {
    case "aligned": {
      const dx = ex - sx;
      const dy = ey - sy;
      const h = d.height ?? 0;
      const len = Math.hypot(dx, dy);
      // `extension = height > 0 ? ( -dy, dx ) : ( dy, -dx )`, the crossbar is that, resized to the height, from each end.
      const n: Pt = len === 0 ? [0, 0] : h > 0 ? [-dy / len, dx / len] : [dy / len, -dx / len];
      const off: Pt = [n[0] * Math.abs(h), n[1] * Math.abs(h)];
      out.add([sx + off[0], sy + off[1]], flags, [item]);
      out.add([ex + off[0], ey + off[1]], flags, [item]);
      out.add(d.start, flags, [item]);
      out.add(d.end, flags, [item]);
      break;
    }
    case "orthogonal": {
      const h = d.height ?? 0;
      const horizontal = d.horizontal !== false;
      const crossStart: Pt = horizontal ? [sx, sy + h] : [sx + h, sy];
      const crossEnd: Pt = horizontal ? [ex, crossStart[1]] : [crossStart[0], ey];
      out.add(crossStart, flags, [item]);
      out.add(crossEnd, flags, [item]);
      out.add(d.start, flags, [item]);
      out.add(d.end, flags, [item]);
      break;
    }
    case "center": {
      out.add(d.start, flags, [item]);
      out.add(d.end, flags, [item]);
      // `radial = end - start`, then twice `RotatePoint( radial, -ANGLE_90 )` -- (x, y) becomes (-y, x) -- and each result added to the start.
      let rx = ex - sx;
      let ry = ey - sy;
      for (let i = 0; i < 2; i++) {
        [rx, ry] = [-ry, rx];
        out.add([sx + rx, sy + ry], flags, [item]);
      }
      break;
    }
    case "radial":
    case "leader":
      out.add(d.start, flags, [item]);
      out.add(d.end, flags, [item]);
      out.add(d.text_at, flags, [item]);
      break;
  }
}

function trackAnchors(out: AnchorList, item: TrackItem): void {
  out.add(item.start, ANCHOR.CORNER | SNAP, [item], PT.END);
  out.add(item.end, ANCHOR.CORNER | SNAP, [item], PT.END);
  out.add(item.center, ANCHOR.ORIGIN, [item], PT.MID);
}

function zoneAnchors(out: AnchorList, item: ZoneItem, ref: Pt): void {
  for (const p of item.outline) out.add(p, ANCHOR.CORNER | SNAP, [item], PT.CORNER);
  if (item.outline.length > 0) out.add(polylineNearest(item.outline, true, ref), ANCHOR.OUTLINE, [item], PT.NONE);
}

/**
 * `PCB_GRID_HELPER::computeAnchors( BOARD_ITEM*, aRefPos, aFrom, aSelectionFilter )`. The magnetic settings gate pads, tracks and vias (always-on only), graphics (on only)
 * when snapping onto an item (`!from`); zones, dimensions and text have no such gate. When picking items up (`from`) the selection filter gates each kind instead.
 */
export function computeItemAnchors(out: AnchorList, item: SnapItem, ref: Pt, from: boolean, ctx: AnchorContext): void {
  const { magnetic, filter } = ctx;
  switch (item.type) {
    case "footprint": {
      const fp: FootprintItem = item;
      const fpVisible = ctx.visible(fp);
      for (const pad of fp.pads) {
        if (from) {
          if (!allowed(filter, "pads")) continue;
        } else if (magnetic.pads !== "always") continue;
        if (!ctx.visible(pad)) continue;
        // Only the pads under the cursor: `if( !pad->GetBoundingBox().Contains( aRefPos ) ) continue;`
        if (ref[0] < pad.bbox.x0 || ref[0] > pad.bbox.x1 || ref[1] < pad.bbox.y0 || ref[1] > pad.bbox.y1) continue;
        padAnchors(out, pad, from);
      }
      // "When computing drag origins, always proceed to add the footprint position anchor regardless of the visibility state."
      if (!fpVisible && !from) break;
      if (from && !allowed(filter, "footprints")) break;
      if (!ctx.footprintLayerVisible(fp)) break;
      out.add(fp.at, ANCHOR.ORIGIN | SNAP, [fp], PT.CENTER);
      const dx = fp.center[0] - fp.at[0];
      const dy = fp.center[1] - fp.at[1];
      if (dx * dx + dy * dy > ctx.grid[0] * ctx.grid[0] + ctx.grid[1] * ctx.grid[1]) out.add(fp.center, ANCHOR.ORIGIN | SNAP, [fp], PT.CENTER);
      break;
    }
    case "pad":
      if (from) {
        if (!allowed(filter, "pads")) break;
      } else if (magnetic.pads !== "always") break;
      if (ctx.visible(item)) padAnchors(out, item, from);
      break;
    case "shape":
      if (from) {
        if (!allowed(filter, "graphics")) break;
      } else if (!magnetic.graphics) break;
      if (ctx.visible(item)) shapeAnchors(out, item, ref);
      break;
    case "track":
      if (from) {
        if (!allowed(filter, "tracks")) break;
      } else if (magnetic.tracks !== "always") break;
      if (ctx.visible(item)) trackAnchors(out, item);
      break;
    case "via":
      if (from) {
        if (!allowed(filter, "vias")) break;
      } else if (magnetic.tracks !== "always") break;
      if (ctx.visible(item)) out.add((item as ViaItem).at, ANCHOR.ORIGIN | ANCHOR.CORNER | SNAP, [item], PT.CENTER);
      break;
    case "zone":
      if (from && !allowed(filter, "zones")) break;
      if (ctx.visible(item)) zoneAnchors(out, item, ref);
      break;
    case "dimension":
      if (from && !allowed(filter, "dimensions")) break;
      if (ctx.visible(item)) dimensionAnchors(out, item);
      break;
    case "text":
      if (from && !allowed(filter, "text")) break;
      if (ctx.visible(item)) out.add(item.at, ANCHOR.ORIGIN, [item]);
      break;
  }
}

/** The nearest anchor of the list to `pos` that has all of `flags`, by straight distance; the first on a tie. */
export function nearestFlagged(anchors: readonly Anchor[], pos: Pt, flags: number): Anchor | null {
  let best: Anchor | null = null;
  let bestD = Infinity;
  for (const a of anchors) {
    if ((flags & a.flags) !== flags) continue;
    const d = (a.pos[0] - pos[0]) ** 2 + (a.pos[1] - pos[1]) ** 2;
    if (d < bestD) {
      bestD = d;
      best = a;
    }
  }
  return best;
}
