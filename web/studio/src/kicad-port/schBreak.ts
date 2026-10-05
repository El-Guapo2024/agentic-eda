// Break and Slice -- `SCH_MOVE_TOOL::preprocessBreakOrSliceSelection` splitting each selected wire, bus or graphic line with
// `SCH_LINE_WIRE_BUS_TOOL::BreakSegment` (eeschema/tools/sch_move_tool.cpp, sch_line_wire_bus_tool.cpp at 8303b2ad) and then moving the new
// end with the cursor, as the move tool does.
//
//   Break   "Divide into connected segments": the line is split in two and the point they share follows the cursor, so the wire bends.
//   Slice   "Divide into unconnected segments": the line is cut; the first half's new end follows the cursor, the second half stays put.
//
// One selected line is cut where the cursor is (`useCursorForSingleLine`); with several, each is cut at its own midpoint. A studio wire
// is a polyline, so "the line" is its segment nearest the cursor.
export type P = readonly [number, number];
export type BreakMode = "break" | "slice";

/** A wire, bus or graphic line that can be cut. */
export interface BreakSource {
  id: string;
  kind: "wire" | "line";
  pts: readonly P[];
  bus?: boolean;
  widthUm?: number;
}

/** One cut: which segment of which line, and where (`breakPos`). */
export interface BreakCut {
  source: BreakSource;
  /** The segment `pts[seg]`-`pts[seg + 1]` that is cut. */
  seg: number;
  at: P;
}

export interface BreakState {
  mode: BreakMode;
  /** The cursor when the tool started (`m_cursor`): the move is measured from here. */
  origin: P;
  cuts: BreakCut[];
}

function distToSegment(p: P, a: P, b: P): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/** The segment of the polyline nearest `p`. */
export function nearestSegment(pts: readonly P[], p: P): number {
  let best = 0;
  let bestD = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) {
    const d = distToSegment(p, pts[i]!, pts[i + 1]!);
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  }
  return best;
}

/** `preprocessBreakOrSliceSelection`: where each line is cut, or null when there is nothing to cut. */
export function startBreak(mode: BreakMode, sources: readonly BreakSource[], cursor: P): BreakState | null {
  const cuttable = sources.filter((s) => s.pts.length >= 2);
  if (cuttable.length === 0) return null;
  const single = cuttable.length === 1;
  const cuts = cuttable.map((source): BreakCut => {
    const seg = nearestSegment(source.pts, cursor);
    const a = source.pts[seg]!;
    const b = source.pts[seg + 1]!;
    // `VECTOR2I breakPos = useCursorForSingleLine ? cursorPos : line->GetMidPoint();`
    const at: P = single ? cursor : [Math.round((a[0] + b[0]) / 2), Math.round((a[1] + b[1]) / 2)];
    return { source, seg, at };
  });
  return { mode, origin: cursor, cuts };
}

/** The two pieces a cut leaves with the new end moved by `delta` (a piece with fewer than two distinct points is dropped). */
export function cutPieces(state: BreakState, cut: BreakCut, cursor: P): P[][] {
  const delta: P = [cursor[0] - state.origin[0], cursor[1] - state.origin[1]];
  const moved: P = [cut.at[0] + delta[0], cut.at[1] + delta[1]];
  const pts = cut.source.pts;
  const first = [...pts.slice(0, cut.seg + 1), moved];
  // Break: the second piece starts at the moved point too (the segments stay connected). Slice: it stays where the cut was made.
  const second = [state.mode === "break" ? moved : cut.at, ...pts.slice(cut.seg + 1)];
  return [first, second].filter((piece) => piece.some((p, i) => i > 0 && (p[0] !== piece[0]![0] || p[1] !== piece[0]![1])));
}

export interface BreakCmds {
  cmds: Array<{ op: "delete_wire"; id: string } | { op: "add_wire"; pts: Array<{ x: number; y: number }>; bus: boolean } | { op: "delete_sch_line"; id: string } | { op: "add_sch_line"; pts: Array<{ x: number; y: number }>; width_um?: number }>;
}

/** The verbs that replace each cut line by its pieces -- one batch, one undo step (`SCH_COMMIT::Push( "Break Wire" / "Slice Wire" )`). */
export function breakCmds(state: BreakState, cursor: P): BreakCmds["cmds"] {
  const deletes: BreakCmds["cmds"] = [];
  const adds: BreakCmds["cmds"] = [];
  const xy = (p: P) => ({ x: p[0], y: p[1] });
  for (const cut of state.cuts) {
    const s = cut.source;
    deletes.push(s.kind === "wire" ? { op: "delete_wire", id: s.id } : { op: "delete_sch_line", id: s.id });
    for (const piece of cutPieces(state, cut, cursor)) {
      adds.push(s.kind === "wire" ? { op: "add_wire", pts: piece.map(xy), bus: s.bus ?? false } : { op: "add_sch_line", pts: piece.map(xy), ...(s.widthUm ? { width_um: s.widthUm } : {}) });
    }
  }
  return [...deletes, ...adds];
}
