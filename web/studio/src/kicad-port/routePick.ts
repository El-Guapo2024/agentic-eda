// `TOOL_BASE::pickSingleItem` as the router's tools use it (pcbnew/router/pns_tool_base.cpp at 8303b2ad): which pad, via or track a route or a drag takes
// as "the item under the cursor". Pads and vias come before tracks; of each kind the nearest centre (a pad, a via) or the nearest end (a track) wins; the
// pick is tried at slop 0 (the cursor is on the item) and then within a grid of it. `components/canvas/pcbSnap.ts`'s `routePoint` snaps to what this finds.
//
// A drag does not snap to the line it is dragging (`TOOL_BASE::checkSnap`: "if we're dragging a track segment, don't try to snap to items that are part of
// the original line"): `skip` is the board ids of those tracks. Without it the cursor within a grid of the dragged track would stick to it and the track
// could not be slid sideways by less than a grid. A route does not end on the item it started on (`pickSingleItem( mousePos, net, layer, false, { m_startItem } )`):
// its `skip` is that item's id, so the cursor within a grid of the start pad does not snap back onto it.
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
 * given, only visible ones (`visible` of the item's layers), and nothing in `skip`: an item's own id (`R1.1`, `trk_x#2`), or -- for a track -- the board id of the
 * track it is a segment of.
 *
 * `layer` is `pickSingleItem`'s top layer `tl` (r5 R5-1): a pad or via on it, then a track on it (`prioritized[0]`, `[1]`), come before a pad or via, then a
 * track, on any layer (`prioritized[2]`, `[3]`) -- a route started on a bottom-side pad with F.Cu active starts there (`getStartLayer`). With `only` (the
 * end of a route: `pickSingleItem( mousePos, net, layer, .. )` with `aLayer >= 0`), an item not on that layer is not picked at all
 * (`aLayer < 0 || item->Layers().Overlaps( aLayer )`). Without `layer`, every layer is the top one.
 */
export function pickRouteItem(board: BoardState, scene: SnapScene, p: Pt, gridSlop: number, net: string | null, visible: (layers: readonly string[]) => boolean, skip: ReadonlySet<string> = NONE, layer?: { top: string; only?: boolean }): PadItem | ViaItem | TrackItem | null {
  const onTop = (layers: readonly string[]) => !layer || layers.includes(layer.top);
  for (const slop of [0, gridSlop]) {
    // prioritized[0..3]: a pad or via on the top layer, a track on it, a pad or via on any layer, a track on any
    const best: [PadItem | ViaItem | null, TrackItem | null, PadItem | ViaItem | null, TrackItem | null] = [null, null, null, null];
    const bestD = [Infinity, Infinity, Infinity, Infinity];
    const offer = (k: number, d: number, item: PadItem | ViaItem | TrackItem) => {
      if (d < bestD[k]!) {
        bestD[k] = d;
        best[k] = item as never;
      }
    };
    for (const item of scene.items) {
      if (item.type !== "pad" && item.type !== "via" && item.type !== "track") continue;
      if (!visible(item.layers) || skip.has(item.id)) continue;
      if (item.type === "track") {
        if (skip.has(item.owner)) continue;
        if (net !== null && trackNet(board, item) !== net) continue;
        if (distanceTo(item.geom!, p) - item.width / 2 > slop) continue;
        const d = Math.min(dist(item.start, p), dist(item.end, p));
        offer(3, d, item);
        if (onTop(item.layers)) offer(1, d, item);
      } else {
        if (item.type === "pad" && net !== null && padNet(board, item) !== net) continue;
        const reach = item.type === "pad" ? Math.max(item.w, item.h) / 2 : item.d / 2;
        if (slop === 0 ? !item.hit(p) : dist(item.at, p) - reach > slop) continue;
        const d = dist(item.at, p);
        offer(2, d, item);
        if (onTop(item.layers)) offer(0, d, item);
      }
    }
    // `haveCandidates()` ends the slop loop; the first of the prioritized that is on `aLayer` (any, for a start) is the pick
    if (best.some((b) => b !== null)) {
      for (const b of best) if (b && (!layer?.only || b.layers.includes(layer.top))) return b;
      return null;
    }
  }
  return null;
}
