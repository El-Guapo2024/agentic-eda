// Port of PCB_SELECTION_TOOL::expandConnection/selectAllConnectedTracks
// (pcbnew/tools/pcb_selection_tool.cpp, U -- "Select/Expand Connection"):
// flood-fill outward from the current selection's connection points
// through tracks and vias at exact coordinate matches, trying
// STOP_AT_JUNCTION, then STOP_AT_PAD, then STOP_NEVER in turn and taking
// the first stage that actually grows the selection -- so pressing U
// repeatedly on the same selection widens one stage further each time.
//
// The exact per-point stop rule, read directly from
// selectAllConnectedTracks (lines ~2072-2095 of the source file):
//
//   if( aStopCondition == STOP_AT_JUNCTION )
//   {
//       // pt_count: how many *real* (non-degenerate) same-layer tracks
//       // touch this point at all (whether we arrived via one of them
//       // or not).
//       if( pt_count > 2 || gotVia || gotNonStartPad )
//       {
//           activePts.erase( ... ); continue;   // <-- bails out BEFORE
//       }                                       //     anything at this
//   else if( aStopCondition == STOP_AT_PAD )     //     point is select()ed
//   {
//       if( gotNonStartPad ) { activePts.erase( ... ); continue; }
//   }
//
// The important, easy-to-get-wrong detail: when a point IS treated as a
// stop, NOTHING there gets selected or traversed further -- not even the
// other tracks that happen to terminate at that same point. A track
// already reached from its OTHER (non-junction) end was already
// select()ed during that earlier step; the branches on the far side of
// the junction are not. A **via** at a point is a hard stop for
// STOP_AT_JUNCTION specifically (a layer change is never treated as a
// plain pass-through at that tier) but is not checked at all for
// STOP_AT_PAD -- so a via only becomes "transparent" starting at the pad
// tier.
//
// Scope, honestly stated: source's traversal is connectivity-graph-based
// (CONNECTIVITY_DATA, which already knows pad/track/via adjacency from a
// full ratsnest build) and explicitly passes `IGNORE_NETS` -- it can walk
// through touching items even if their nets briefly disagree (useful for
// a board mid-repair). This port restricts each flood to a single net
// instead (grouping start points by net and never crossing a net
// boundary) -- safe because two different-net copper items touching at a
// point is a DRC violation in the first place, not a case this app's
// simpler model needs to tolerate. Zones are excluded from the traversal
// entirely, same as source's own `EXCLUDE_ZONES` flag.
//
// Pads are not graph nodes in this model (this app has no connectivity
// data structure to query pad-to-pad adjacency within a footprint) --
// they are only ever a *start* point, never something the flood crosses
// through or is halted by mid-walk (`gotNonStartPad` can never fire
// here). That makes STOP_AT_PAD and STOP_NEVER compute identically:
// once the flood has walked every track/via reachable without being
// halted by a junction/via, there is nothing left on the same net that
// a pad could additionally unlock. Both stages are still implemented and
// tried, matching source's structure (and so a caller counting "how many
// U presses did this take" behaves the same); the third stage is simply
// guaranteed to never add anything new beyond the second. See
// PARITY-pcb.md.

export interface ConnTrack {
  id: string;
  net: string;
  start: readonly [number, number];
  end: readonly [number, number];
}

export interface ConnVia {
  id: string;
  net: string;
  at: readonly [number, number];
}

export interface StartPoint {
  point: readonly [number, number];
  net: string;
}

export interface ExpandResult {
  trackIds: Set<string>;
  viaIds: Set<string>;
}

export type StopCondition = "junction" | "pad" | "never";

function pointKey(net: string, p: readonly [number, number]): string {
  return `${net}|${p[0]},${p[1]}`;
}

/**
 * One net's flood, seeded with whatever of that net is already selected
 * (so re-encountering it mid-walk is a no-op, not a re-add) plus the
 * caller's start points (every point of the currently-selected items on
 * this net).
 */
function floodOneNet(tracks: readonly ConnTrack[], vias: readonly ConnVia[], net: string, seedTrackIds: ReadonlySet<string>, seedViaIds: ReadonlySet<string>, startPoints: readonly (readonly [number, number])[], stopCondition: StopCondition): ExpandResult {
  const byPoint = new Map<string, Array<{ kind: "track" | "via"; id: string }>>();
  const push = (p: readonly [number, number], entry: { kind: "track" | "via"; id: string }) => {
    const k = pointKey(net, p);
    const list = byPoint.get(k);
    if (list) list.push(entry);
    else byPoint.set(k, [entry]);
  };

  const trackById = new Map<string, ConnTrack>();
  const viaById = new Map<string, ConnVia>();
  for (const t of tracks) {
    if (t.net !== net || (t.start[0] === t.end[0] && t.start[1] === t.end[1])) continue; // pt_count only counts non-degenerate tracks
    trackById.set(t.id, t);
    push(t.start, { kind: "track", id: t.id });
    push(t.end, { kind: "track", id: t.id });
  }
  for (const v of vias) {
    if (v.net !== net) continue;
    viaById.set(v.id, v);
    push(v.at, { kind: "via", id: v.id });
  }

  const selectedTracks = new Set(seedTrackIds);
  const selectedVias = new Set(seedViaIds);
  const visited = new Set<string>();
  const frontier: Array<readonly [number, number]> = [...startPoints];

  while (frontier.length > 0) {
    const p = frontier.pop()!;
    const k = pointKey(net, p);
    if (visited.has(k)) continue;
    visited.add(k);

    const here = byPoint.get(k) ?? [];
    const trackEntries = here.filter((e) => e.kind === "track");
    const viaEntries = here.filter((e) => e.kind === "via");

    if (stopCondition === "junction" && (trackEntries.length > 2 || viaEntries.length > 0)) {
      continue; // a junction or a via halts STOP_AT_JUNCTION outright -- nothing here is selected or crossed.
    }

    for (const entry of trackEntries) {
      if (selectedTracks.has(entry.id)) continue;
      selectedTracks.add(entry.id);
      const t = trackById.get(entry.id)!;
      const other = pointKey(net, t.start) === k ? t.end : t.start;
      frontier.push(other);
    }
    for (const entry of viaEntries) {
      selectedVias.add(entry.id);
    }
  }

  return { trackIds: selectedTracks, viaIds: selectedVias };
}

function unionResults(results: readonly ExpandResult[]): ExpandResult {
  const trackIds = new Set<string>();
  const viaIds = new Set<string>();
  for (const r of results) {
    for (const id of r.trackIds) trackIds.add(id);
    for (const id of r.viaIds) viaIds.add(id);
  }
  return { trackIds, viaIds };
}

export interface AlreadySelected {
  trackIds: readonly string[];
  viaIds: readonly string[];
}

/**
 * `startPoints` is grouped by net internally -- a multi-pad footprint
 * selection expands each pad's own net independently and unions the
 * results, same as source seeding one `activePts` set per start item
 * rather than pooling them under one net. `alreadySelected` should be
 * every track/via id the caller already has selected (so the "did this
 * stage actually grow the selection" check -- and the flood's own
 * dedup -- both see the real starting point, same as source mutating
 * the live `m_selection` across calls rather than starting fresh).
 */
export function expandConnection(tracks: readonly ConnTrack[], vias: readonly ConnVia[], startPoints: readonly StartPoint[], alreadySelected: AlreadySelected): ExpandResult {
  const nets = [...new Set(startPoints.map((s) => s.net))];
  const pointsForNet = (net: string) => startPoints.filter((s) => s.net === net).map((s) => s.point);
  const seedTrackIds = new Set(alreadySelected.trackIds);
  const seedViaIds = new Set(alreadySelected.viaIds);
  const baseCount = seedTrackIds.size + seedViaIds.size;

  const stages: StopCondition[] = ["junction", "pad", "never"];
  for (const stage of stages) {
    const perNet = nets.map((net) => floodOneNet(tracks, vias, net, seedTrackIds, seedViaIds, pointsForNet(net), stage));
    const result = unionResults(perNet);
    if (result.trackIds.size + result.viaIds.size > baseCount) return result;
  }

  // Nothing grew at any stage (already at the full extent) -- return the
  // widest (STOP_NEVER) result, same shape every caller expects.
  return unionResults(nets.map((net) => floodOneNet(tracks, vias, net, seedTrackIds, seedViaIds, pointsForNet(net), "never")));
}
