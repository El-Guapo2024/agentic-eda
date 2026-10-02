// Port of eeschema's connection selection:
//   - SCH_SELECTION_TOOL::SelectConnection (Ctrl+4, "Select/Expand
//     Connection", eeschema/tools/sch_selection_tool.cpp): tries
//     STOP_AT_JUNCTION, then STOP_AT_PIN, then STOP_NEVER, taking the first
//     stage that adds something beyond the original selection, so repeated
//     presses widen the selection one stage at a time.
//   - SCH_SELECTION_TOOL::expandConnectionWithGraph's `isStopPoint`: a
//     point stops traversal at a pin (STOP_AT_PIN and up), and additionally
//     at a junction or a label/no-connect anchor (STOP_AT_JUNCTION).
//   - SCH_SELECTION_TOOL::SelectNode (Alt+3): SelectPoint(cursor,
//     connectedTypes) -- selects the connectable items AT the cursor point.
//
// Scope: source walks CONNECTION_GRAPH (pins/symbols/labels included); this
// IR stores one polyline per wire, so the walk is over wires (same bus-ness
// only: a wire never connects to a bus) plus the labels anchored on them.
// Pure; no state.

export interface ConnWire {
  id: string;
  pts: ReadonlyArray<readonly [number, number]>;
  bus?: boolean;
}
export interface ConnLabel {
  id: string;
  at: readonly [number, number];
}
export type StopCondition = "junction" | "pin" | "never";

type P = readonly [number, number];
const eq = (a: P, b: P) => a[0] === b[0] && a[1] === b[1];

function onSegment(p: P, a: P, b: P): boolean {
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  if (cross !== 0) return false;
  return p[0] >= Math.min(a[0], b[0]) && p[0] <= Math.max(a[0], b[0]) && p[1] >= Math.min(a[1], b[1]) && p[1] <= Math.max(a[1], b[1]);
}

function wireContains(w: ConnWire, p: P): boolean {
  for (let i = 0; i + 1 < w.pts.length; i++) if (onSegment(p, w.pts[i]!, w.pts[i + 1]!)) return true;
  return w.pts.length === 1 && eq(w.pts[0]!, p);
}

/** Points where wire `a` touches wire `b` (an endpoint of one on the other). */
function contactPoints(a: ConnWire, b: ConnWire): P[] {
  const out: P[] = [];
  const ends = (w: ConnWire): P[] => (w.pts.length ? [w.pts[0]!, w.pts[w.pts.length - 1]!] : []);
  for (const p of ends(a)) if (wireContains(b, p)) out.push(p);
  for (const p of ends(b)) if (wireContains(a, p) && !out.some((q) => eq(q, p))) out.push(p);
  return out;
}

export interface ExpandInput {
  wires: readonly ConnWire[];
  labels: readonly ConnLabel[];
  /** Every pin tip (and power-symbol anchor) on the sheet. */
  pinPoints: readonly P[];
  /** No-connect anchors (a STOP_AT_JUNCTION stop point, like labels). */
  noConnectPoints?: readonly P[];
}

/** expandConnectionWithGraph, for one stop condition. Returns wire + label ids (including the starting ones). */
export function expandWithStop(input: ExpandInput, startWireIds: readonly string[], stop: StopCondition): { wireIds: Set<string>; labelIds: Set<string> } {
  const byId = new Map(input.wires.map((w) => [w.id, w]));
  const pins = input.pinPoints;
  const labelPts = input.labels.map((l) => l.at);
  const ncPts = input.noConnectPoints ?? [];

  const isStop = (p: P, bus: boolean): boolean => {
    if (stop === "never") return false;
    if (pins.some((q) => eq(q, p))) return true;
    if (stop === "pin") return false;
    // junction: 3+ wires meet at p (an endpoint or a T on an interior)
    let n = 0;
    for (const w of input.wires) if (!!w.bus === bus && wireContains(w, p)) n++;
    if (n >= 3) return true;
    return labelPts.some((q) => eq(q, p)) || ncPts.some((q) => eq(q, p));
  };

  const seen = new Set<string>();
  const queue: string[] = [];
  for (const id of startWireIds) {
    if (byId.has(id) && !seen.has(id)) {
      seen.add(id);
      queue.push(id);
    }
  }
  while (queue.length) {
    const cur = byId.get(queue.pop()!)!;
    for (const other of input.wires) {
      if (seen.has(other.id) || !!other.bus !== !!cur.bus) continue;
      const crossing = contactPoints(cur, other).some((p) => !isStop(p, !!cur.bus));
      if (crossing) {
        seen.add(other.id);
        queue.push(other.id);
      }
    }
  }
  const labelIds = new Set<string>();
  for (const l of input.labels) for (const id of seen) if (wireContains(byId.get(id)!, l.at)) labelIds.add(l.id);
  return { wireIds: seen, labelIds };
}

/**
 * SelectConnection: staged expansion. `selected` is the current selection
 * (wire/label ids); returns the new selection (wire + label ids). Falls back
 * to the original selection when no stage grows it.
 */
export function selectConnection(input: ExpandInput, selected: readonly string[]): string[] {
  const wireIds = new Set(input.wires.map((w) => w.id));
  const start = selected.filter((id) => wireIds.has(id));
  if (start.length === 0) return [...selected];
  const original = new Set(selected);
  let result: { wireIds: Set<string>; labelIds: Set<string> } | null = null;
  for (const stop of ["junction", "pin", "never"] as const) {
    result = expandWithStop(input, start, stop);
    const grew = [...result.wireIds, ...result.labelIds].some((id) => !original.has(id));
    if (grew) break;
  }
  return result ? [...new Set([...result.wireIds, ...result.labelIds])] : [...selected];
}

/** SelectNode: wires and labels sitting exactly on `p` (cursor, un-snapped in source; callers pass a tolerance-snapped point). */
export function selectNodeAt(input: ExpandInput, p: P): string[] {
  const out: string[] = [];
  for (const w of input.wires) if (wireContains(w, p)) out.push(w.id);
  for (const l of input.labels) if (eq(l.at, p)) out.push(l.id);
  return out;
}
