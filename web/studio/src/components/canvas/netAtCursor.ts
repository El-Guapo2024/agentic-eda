// Port of board_inspection_tool.cpp's BOARD_INSPECTION_TOOL::highlightNet
// cursor-lookup half (the `!aUseSelection` branch -- this app has no
// separate "highlight the selection's net(s)" action, only the plain
// cursor-driven `` ` `` key): collect pads/vias/tracks/shapes at the
// cursor first, falling back to zones only if nothing else is there,
// and return the first candidate's net.
import type { BoardState } from "../../api/types";
import { distToPolyline, pointInPolygon } from "./itemHitTest";

/** Nearest pad/via/track/shape net at (xNm, yNm) within `toleranceNm`, falling back to a zone only if nothing else qualifies -- source's own collector order (`{PAD_T, VIA_T, TRACE_T, ARC_T, SHAPE_T}` first, then `{ZONE_T}` only `if (collector.GetCount() == 0)`). Shapes without a net (most graphics) never match since `net` is null for them in this app's model -- harmless, they just never win here (matching source's `BOARD_CONNECTED_ITEM` cast, which a plain PCB_SHAPE_T without a net wouldn't survive anyway). */
export function findNetAtCursor(board: BoardState, xNm: number, yNm: number, toleranceNm: number): string | null {
  let best: { net: string; d: number } | null = null;
  const consider = (net: string | null | undefined, d: number) => {
    if (!net || d > toleranceNm) return;
    if (!best || d < best.d) best = { net, d };
  };

  for (const part of board.parts) {
    if (!part.placed) continue;
    for (const pad of part.pads ?? []) consider(pad.net, Math.hypot(xNm - pad.x, yNm - pad.y));
  }
  for (const v of board.routing?.vias ?? []) consider(v.net, Math.hypot(xNm - v.x, yNm - v.y) - v.d / 2);
  for (const t of board.routing?.tracks ?? []) consider(t.net, distToPolyline(xNm, yNm, t.pts) - t.width / 2);
  // Shapes carry no net in this app's model (PCB_SHAPE_T graphics never
  // do in source either unless they're a copper-pour-adjacent special
  // case this app doesn't model) -- nothing to `consider` for them.

  if (best) return (best as { net: string; d: number }).net;

  for (const z of board.routing?.zones ?? []) {
    // A hole of the outline is not the zone's.
    const inside = pointInPolygon(xNm, yNm, z.outline) && !(z.holes ?? []).some((h) => h.length >= 3 && pointInPolygon(xNm, yNm, h));
    const d = inside ? 0 : distToPolyline(xNm, yNm, [...z.outline, z.outline[0]!]);
    if (d <= toleranceNm) return z.net;
  }
  return null;
}
