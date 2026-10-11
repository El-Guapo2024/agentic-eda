// `TOOL_BASE::pickSingleItem` as the router's tools use it (pcbnew/router/pns_tool_base.cpp at 8303b2ad): which pad, via or track a route or a drag takes
// as "the item under the cursor". Pads and vias come before tracks; of each kind the nearest centre (a pad, a via) or the nearest end (a track) wins; the
// pick is tried at slop 0 (the cursor is on the item) and then within a grid of it. `components/canvas/pcbSnap.ts`'s `routePoint` snaps to what this finds.
//
// A drag does not snap to the line it is dragging (`TOOL_BASE::checkSnap`: "if we're dragging a track segment, don't try to snap to items that are part of
// the original line"): `skip` is the board ids of those tracks. Without it the cursor within a grid of the dragged track would stick to it and the track
// could not be slid sideways by less than a grid.
import type { BoardState } from "../api/types";
import type { PadItem, SnapScene, TrackItem, ViaItem } from "./snapScene";
import { dist, distanceTo, type Pt } from "./snapGeom";

const NONE: ReadonlySet<string> = new Set();

/** The net of the pad item (its footprint's pad by number). */
export function padNet(board: BoardState, pad: PadItem): string | null {
  const part = board.parts.find((p) => p.ref === pad.footprint);
  const num = pad.id.slice(pad.footprint.length + 1).split("#")[0];
  return part?.pads?.find((q) => q.num === num)?.net ?? null;
}

export function trackNet(board: BoardState, item: TrackItem): string | null {
  return board.routing?.tracks.find((t) => t.id === item.owner)?.net ?? null;
}

/**
 * Pads and vias first (the nearest centre), then tracks (the nearest end); each tried at slop 0 and then within `gridSlop`. Only items of `net` when it is
 * given, only visible ones (`visible` of the item's layers), and no track whose board id is in `skip`.
 */
export function pickRouteItem(board: BoardState, scene: SnapScene, p: Pt, gridSlop: number, net: string | null, visible: (layers: readonly string[]) => boolean, skip: ReadonlySet<string> = NONE): PadItem | ViaItem | TrackItem | null {
  for (const slop of [0, gridSlop]) {
    let best: PadItem | ViaItem | null = null;
    let bestD = Infinity;
    let bestSeg: TrackItem | null = null;
    let bestSegD = Infinity;
    for (const item of scene.items) {
      if (item.type !== "pad" && item.type !== "via" && item.type !== "track") continue;
      if (!visible(item.layers)) continue;
      if (item.type === "track") {
        if (skip.has(item.owner)) continue;
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
