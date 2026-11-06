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
// item like a track (`BOARD_CONNECTED_ITEM::ClassOf`), a footprint stands for all its pads. With graphic lines selected (and nothing connected) the lines they
// meet end to end are followed (`selectAllConnectedShapes`, UI-LEFT P22). The selection filter (`itemPassesFilter( item, true )`) is the caller's `passes`.
//
// Pure: no store, no DOM. Unit-tested in pcbConnection.test.ts.
import type { BoardState, Shape } from "../api/types";
import { selectAllConnectedTracks, type ConnPad, type ConnSeg, type ConnStart, type ConnVia, type ConnWorld, type StopCondition } from "./expandConnection";
import { itemKind, padIds, shapeById } from "./pcbItems";

/** What `selectPoint` found under the cursor, best first (`pickSelectionCandidates`). */
export interface HitCandidate {
  kind: string;
  id: string;
}

/** The kinds of selected item that start a connection: footprints (their pads), tracks, vias, zones and pads (`BOARD_CONNECTED_ITEM`). */
const STARTS = new Set(["part", "track", "via", "zone", "pad"]);

/**
 * `initialCount` of `expandConnection`: how many selected items are footprints, generators (a tuning pattern) or connected
 * (`item->Type() == PCB_FOOTPRINT_T || item->Type() == PCB_GENERATOR_T || IsConnected()`).
 */
export function connectedStartCount(board: BoardState, selection: Iterable<string>): number {
  const generators = new Set((board.drawings?.groups ?? []).filter((g) => g.generator).map((g) => g.id));
  let n = 0;
  for (const id of selection) {
    const kind = itemKind(board, id);
    if (kind != null && (STARTS.has(kind) || (kind === "group" && generators.has(id)))) n++;
  }
  return n;
}

/** `connectedItemFilter`: the first thing under the cursor that is a connected item -- a pad, a track or a via (a zone has no copper path to expand along here). */
export function connectedItemAt(candidates: readonly HitCandidate[]): string | null {
  return candidates.find((c) => c.kind === "track" || c.kind === "via" || c.kind === "pad")?.id ?? null;
}

/**
 * `isExpandableGraphicShape`: a segment, arc or curve off the copper layers, or a polygon there that is not closed -- which `expandConnection` follows end to end
 * instead. (A board polygon here is always closed: `gr_poly` is.)
 */
export function isExpandableGraphicShape(board: BoardState, id: string): boolean {
  const s = shapeById(board, id);
  return s != null && isExpandableShape(s);
}

function isExpandableShape(s: Shape): boolean {
  if (s.layer.endsWith(".Cu")) return false;
  return s.kind === "segment" || s.kind === "arc" || s.kind === "bezier";
}

/** `GetStart()` and `GetEnd()` of a line, an arc or a curve. */
function shapeEnds(s: Shape): [readonly [number, number], readonly [number, number]] | null {
  return s.kind === "segment" || s.kind === "arc" || s.kind === "bezier" ? [s.start, s.end] : null;
}

/**
 * `PCB_SELECTION_TOOL::selectAllConnectedShapes( aStartItems )`: a depth-first walk (a stack) from the start shapes. A shape not yet seen is marked (`SKIP_STRUCT`);
 * one the filter refuses goes no further; one it lets through is selected, and at its start and at its end every expandable shape the collector finds there --
 * one the point hits within the collector's accuracy (`HitTest( aWhere, accuracy )`: anywhere along it, not only at its ends), on the layers of the shape
 * being expanded (`guide.SetLayerVisibleBits( shape->GetLayerSet() )`) -- is pushed. `hit` is the shape's hit distance at a point (0 or less on its stroke).
 * Answers the shapes selected, in the order selected.
 */
export function selectAllConnectedShapes(
  board: BoardState,
  starts: readonly string[],
  opts: { accuracyNm: number; hit: (s: Shape, x: number, y: number) => number; passes?: (id: string) => boolean }
): string[] {
  const shapes = (board.drawings?.shapes ?? []).filter(isExpandableShape);
  const byId = new Map(shapes.map((s) => [s.id, s]));
  const stack = starts.filter((id) => byId.has(id)).map((id) => byId.get(id)!);
  const seen = new Set<string>();
  const out: string[] = [];
  while (stack.length > 0) {
    const shape = stack.pop()!;
    if (seen.has(shape.id)) continue;
    seen.add(shape.id);
    if (opts.passes && !opts.passes(shape.id)) continue;
    out.push(shape.id);
    const ends = shapeEnds(shape);
    if (!ends) continue;
    for (const [x, y] of ends) {
      // `collector.Collect( board(), { PCB_SHAPE_T }, aWhere, guide )`: the shapes on this shape's layer the point hits
      for (const other of shapes) {
        if (other.layer !== shape.layer) continue;
        if (opts.hit(other, x, y) <= opts.accuracyNm) stack.push(other);
      }
    }
  }
  return out;
}

/** The board as `selectAllConnectedTracks` walks it: every track's segments, every via with its copper span, every pad of a placed footprint with its copper layers. */
export function connectionWorld(board: BoardState): ConnWorld {
  const copper = board.layers.filter((l) => l.endsWith(".Cu"));
  const segs: ConnSeg[] = [];
  for (const t of board.routing?.tracks ?? []) {
    if (t.pts.length < 2) continue;
    // an arc is one `PCB_ARC`, its two ends; a polyline is a run of `PCB_TRACK`s
    if (t.arc_mid) segs.push({ track: t.id, seg: 0, layer: t.layer, start: t.pts[0]!, end: t.pts[t.pts.length - 1]! });
    else for (let i = 0; i + 1 < t.pts.length; i++) segs.push({ track: t.id, seg: i, layer: t.layer, start: t.pts[i]!, end: t.pts[i + 1]! });
  }
  const span = (a: string, b: string) => {
    const [i, j] = [copper.indexOf(a), copper.indexOf(b)];
    return i < 0 || j < 0 ? [a, b] : copper.slice(Math.min(i, j), Math.max(i, j) + 1);
  };
  const vias: ConnVia[] = (board.routing?.vias ?? []).map((v) => ({ id: v.id, at: [v.x, v.y], diameter: v.d, layers: span(v.from, v.to) }));
  const pads: ConnPad[] = [];
  for (const part of board.parts) {
    if (!part.placed) continue;
    const ids = padIds(part);
    (part.pads ?? []).forEach((pad, i) => {
      const layers = pad.th ? copper : [part.side === "bottom" ? "B.Cu" : "F.Cu"];
      pads.push({ id: ids[i]!, at: [pad.px ?? pad.x, pad.py ?? pad.y], layers });
    });
  }
  return { segs, vias, pads };
}

/**
 * The stages of `expandConnection` from `selection`: STOP_AT_JUNCTION, STOP_AT_PAD, STOP_NEVER, each adding to the selection what `selectAllConnectedTracks` reaches
 * from the selection as it stands (a footprint stands for its pads, a tuning pattern for its tracks), until the selection holds more than `initialCount` items --
 * `initialCount` is counted before a cursor pick, so after one the first stage that finds anything is the last. A track of a tuning pattern is selected as the
 * pattern ("Promote generated members to their PCB_GENERATOR parents"). Null when nothing in `selection` can start a walk.
 */
export function expandedSelection(board: BoardState, selection: ReadonlySet<string>, initialCount = connectedStartCount(board, selection), passes?: (kind: ConnStart["kind"], id: string) => boolean): string[] | null {
  const world = connectionWorld(board);
  const generators = (board.drawings?.groups ?? []).filter((g) => g.generator);
  const generatorOf = new Map(generators.flatMap((g) => g.member_ids.map((m) => [m, g.id] as const)));
  const refs = new Set(selection);
  const startsOf = (ids: Iterable<string>): ConnStart[] => {
    const out: ConnStart[] = [];
    for (const id of ids) {
      const kind = itemKind(board, id);
      if (kind === "track" || kind === "via" || kind === "pad") out.push({ kind, id });
      else if (kind === "part") for (const pid of padIds(board.parts.find((q) => q.ref === id) ?? { ref: id })) out.push({ kind: "pad", id: pid });
      else {
        const g = generators.find((q) => q.id === id);
        for (const m of g?.member_ids ?? []) if (itemKind(board, m) === "track" || itemKind(board, m) === "via") out.push({ kind: itemKind(board, m) as "track" | "via", id: m });
      }
    }
    return out;
  };
  if (startsOf(refs).length === 0) return null;
  for (const stage of ["junction", "pad", "never"] as StopCondition[]) {
    const selected = { trackIds: new Set<string>(), viaIds: new Set<string>() };
    selectAllConnectedTracks(world, startsOf(refs), stage, selected, passes);
    for (const id of [...selected.trackIds, ...selected.viaIds]) refs.add(generatorOf.get(id) ?? id);
    for (const id of [...refs]) if (generatorOf.has(id)) refs.delete(id);
    if (refs.size > initialCount) break;
  }
  return [...refs];
}

/** What Select Connection asks of the board beyond the selection: the filter (`itemPassesFilter( item, true )`) and the graphic lines' hit test. */
export interface SelectConnectionOptions {
  passes?: (kind: "track" | "via" | "pad" | "shape", id: string) => boolean;
  /** `selectAllConnectedShapes`' collector: the accuracy (5 pixels) and a shape's hit distance at a point. */
  shapes?: { accuracyNm: number; hit: (s: Shape, x: number, y: number) => number };
}

/**
 * What Select Connection does to the selection: `refs` is the new selection, null leaves it as it is. With nothing connected selected: graphic lines selected are
 * followed to the lines they meet (`selectAllConnectedShapes`, added to the selection); with none, the selection is cleared and the connected item under the cursor
 * (`candidates`) taken -- nothing under it leaves the selection empty, as `selectCursor( true, .. )` does -- before the flood.
 */
export function planSelectConnection(board: BoardState, selection: ReadonlySet<string>, candidates: readonly HitCandidate[], opts: SelectConnectionOptions = {}): { refs: string[] } | null {
  let start: ReadonlySet<string> = selection;
  const initialCount = connectedStartCount(board, selection);
  const passes = opts.passes;
  if (initialCount === 0) {
    const startShapes = [...selection].filter((id) => isExpandableGraphicShape(board, id));
    if (startShapes.length > 0) {
      if (!opts.shapes) return null;
      const lines = selectAllConnectedShapes(board, startShapes, { ...opts.shapes, passes: passes ? (id) => passes("shape", id) : undefined });
      // `select( shape )` adds to what is selected; the stages after find nothing to walk from lines off copper
      return { refs: [...new Set([...selection, ...lines])] };
    }
    const hit = connectedItemAt(candidates);
    start = new Set(hit ? [hit] : []);
    if (start.size === 0) return selection.size === 0 ? null : { refs: [] };
  }
  return { refs: expandedSelection(board, start, initialCount, passes) ?? [...start] };
}
