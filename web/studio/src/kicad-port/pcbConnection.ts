// Select Connection (`U`) on the board: the start of `PCB_SELECTION_TOOL::expandConnection` (pcbnew/tools/pcb_selection_tool.cpp at 8303b2ad) around the flood
// ported in kicad-port/expandConnection.ts.
//
//     unsigned initialCount = 0;
//     for( item : m_selection ) if( footprint || generator || item->IsConnected() ) initialCount++;
//     if( initialCount == 0 )
//     {
//         // First, process any graphic shapes we have ... If no non-copper shapes; fall back to looking for connected items
//         if( !startShapes.empty() ) selectAllConnectedShapes( startShapes ); else selectCursor( true, connectedItemFilter );
//     }
//
// So the command does not need a selection: with no footprint and no connected item (track, via, pad, zone) selected it clears the selection and takes the connected
// item under the cursor (`selectCursor( true, .. )`; `connectedItemFilter` drops every item that is not `BOARD_CONNECTED_ITEM`), then expands from it. A pad is a start
// item like a track (`BOARD_CONNECTED_ITEM::ClassOf`), a footprint stands for all its pads. Graphic lines (`selectAllConnectedShapes`) are not ported: with only such a
// shape selected the command leaves the selection alone.
//
// Pure: no store, no DOM. Unit-tested in pcbConnection.test.ts.
import type { BoardState } from "../api/types";
import { expandConnection, type ConnTrack, type ConnVia, type StartPoint } from "./expandConnection";
import { itemKind, padById, shapeById } from "./pcbItems";

/** What `selectPoint` found under the cursor, best first (`pickSelectionCandidates`). */
export interface HitCandidate {
  kind: string;
  id: string;
}

/** The kinds of selected item that start a connection: footprints (their pads), tracks, vias, zones and pads (`BOARD_CONNECTED_ITEM`). */
const STARTS = new Set(["part", "track", "via", "zone", "pad"]);

/** `initialCount` of `expandConnection`: how many selected items are footprints or connected. */
export function connectedStartCount(board: BoardState, selection: Iterable<string>): number {
  let n = 0;
  for (const id of selection) {
    const kind = itemKind(board, id);
    if (kind != null && STARTS.has(kind)) n++;
  }
  return n;
}

/** `connectedItemFilter`: the first thing under the cursor that is a connected item -- a pad, a track or a via (a zone has no copper path to expand along here). */
export function connectedItemAt(candidates: readonly HitCandidate[]): string | null {
  return candidates.find((c) => c.kind === "track" || c.kind === "via" || c.kind === "pad")?.id ?? null;
}

/** `isExpandableGraphicShape`: a segment, arc, curve or open polygon off the copper layers, which `expandConnection` follows end to end instead. */
function isExpandableGraphicShape(board: BoardState, id: string): boolean {
  const s = shapeById(board, id);
  if (!s || s.layer.endsWith(".Cu")) return false;
  return s.kind === "segment" || s.kind === "arc" || s.kind === "bezier";
}

/**
 * The selection after the flood: `selection` plus every track and via of the connections its tracks, vias, pads and footprints' pads belong to, the stages
 * `STOP_AT_JUNCTION`, `STOP_AT_PAD`, `STOP_NEVER` tried in turn. Null when nothing in `selection` can start one.
 */
export function expandedSelection(board: BoardState, selection: ReadonlySet<string>): string[] | null {
  const tracks: ConnTrack[] = (board.routing?.tracks ?? []).map((t) => ({ id: t.id, net: t.net, start: t.pts[0]!, end: t.pts[t.pts.length - 1]! })).filter((t) => t.start && t.end);
  const vias: ConnVia[] = (board.routing?.vias ?? []).map((v) => ({ id: v.id, net: v.net, at: [v.x, v.y] }));
  const startPoints: StartPoint[] = [];
  const selectedTrackIds: string[] = [];
  const selectedViaIds: string[] = [];
  for (const id of selection) {
    const kind = itemKind(board, id);
    if (kind === "track") {
      const t = board.routing?.tracks.find((q) => q.id === id);
      if (!t) continue;
      selectedTrackIds.push(id);
      startPoints.push({ point: t.pts[0]!, net: t.net }, { point: t.pts[t.pts.length - 1]!, net: t.net });
    } else if (kind === "via") {
      const v = board.routing?.vias.find((q) => q.id === id);
      if (!v) continue;
      selectedViaIds.push(id);
      startPoints.push({ point: [v.x, v.y], net: v.net });
    } else if (kind === "part") {
      const p = board.parts.find((q) => q.ref === id);
      for (const pad of p?.pads ?? []) if (pad.net) startPoints.push({ point: [pad.x, pad.y], net: pad.net });
    } else if (kind === "pad") {
      const pad = padById(board, id)?.pad;
      if (pad?.net) startPoints.push({ point: [pad.x, pad.y], net: pad.net });
    }
  }
  if (startPoints.length === 0) return null;
  const result = expandConnection(tracks, vias, startPoints, { trackIds: selectedTrackIds, viaIds: selectedViaIds });
  const refs = new Set(selection);
  for (const id of result.trackIds) refs.add(id);
  for (const id of result.viaIds) refs.add(id);
  return [...refs];
}

/**
 * What Select Connection does to the selection: `refs` is the new selection, null leaves it as it is. With nothing connected selected the selection is cleared and the
 * connected item under the cursor (`candidates`) taken -- nothing under it leaves the selection empty, as `selectCursor( true, .. )` does -- before the flood; a
 * selection of graphic lines only is left alone (their connection is not ported).
 */
export function planSelectConnection(board: BoardState, selection: ReadonlySet<string>, candidates: readonly HitCandidate[]): { refs: string[] } | null {
  let start: ReadonlySet<string> = selection;
  if (connectedStartCount(board, selection) === 0) {
    if ([...selection].some((id) => isExpandableGraphicShape(board, id))) return null;
    const hit = connectedItemAt(candidates);
    start = new Set(hit ? [hit] : []);
    if (start.size === 0) return selection.size === 0 ? null : { refs: [] };
  }
  return { refs: expandedSelection(board, start) ?? [...start] };
}
