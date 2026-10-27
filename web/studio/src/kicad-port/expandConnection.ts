// Port of PCB_SELECTION_TOOL::selectAllConnectedTracks (pcbnew/tools/pcb_selection_tool.cpp at 8303b2ad), the walk behind Select/Expand Connection (U):
// from the start items' points through the tracks, vias and pads that meet there, stopping by `aStopCondition`. kicad-port/pcbConnection.ts runs the stages
// (`expandConnection`: STOP_AT_JUNCTION, then STOP_AT_PAD, then STOP_NEVER, until the selection grows).
//
// Line by line:
//   - The items walked are `GetConnectedItems( startItem, EXCLUDE_ZONES | IGNORE_NETS )`: every track, via and pad, whatever its net (a track of another net that
//     ends on the same point is walked into, as in KiCad). Zones are not walked.
//   - `trackMap` holds each track segment under both its ends (a polyline here is a run of KiCad's `PCB_TRACK`s, so an inner vertex is a point two of its
//     segments meet at, and a branch from it is found); `viaMap` each via under its position; `padMap` each pad under its position (`PAD::GetPosition`).
//   - An active point carries the layers of what pushed it (`GetLayerSet()`), and only items on its copper layers count there: a track (`layerSetCu.Contains(
//     track->GetLayer() )`), a via (`( via->GetLayerSet() & layerSetCu ).any()`), a pad. So two tracks on different layers that end on one point do not join
//     without a via.
//   - A via is hit at the exact point, else when the point is within its radius on a shared layer (the first such via in `viaMap`'s order, x then y).
//   - STOP_AT_JUNCTION drops a point with more than two non-degenerate tracks on its layers, with a via, or with a pad that is not a start pad
//     (`pt_count > 2 || gotVia || gotNonStartPad`); STOP_AT_PAD drops a point with a pad that is not a start pad.
//   - Otherwise the pad there pushes its own position (with its layers), each track there is selected and pushes its other end, and the via there is selected and
//     pushes every track point inside its pad on a layer of its, with the via's layers.
//   - `SKIP_STRUCT` marks what has pushed already, for the whole call (all start items), so the walk ends.
// Not ported: the selection filter (`itemPassesFilter`), and graphic shapes on copper (`shapeMap`).
//
// Pure: no store, no DOM. Unit-tested in expandConnection.test.ts.

export type Pt = readonly [number, number];

/** One `PCB_TRACK`: segment `seg` of the track (polyline) `track`, on `layer`; an arc track is one segment, its two ends. */
export interface ConnSeg {
  track: string;
  seg: number;
  layer: string;
  start: Pt;
  end: Pt;
}

export interface ConnVia {
  id: string;
  at: Pt;
  /** The via's width (the same on every layer here). */
  diameter: number;
  /** Its copper layers (`GetLayerSet()`): the span. */
  layers: readonly string[];
}

export interface ConnPad {
  id: string;
  /** `PAD::GetPosition()`. */
  at: Pt;
  /** Its copper layers. */
  layers: readonly string[];
}

export interface ConnWorld {
  segs: readonly ConnSeg[];
  vias: readonly ConnVia[];
  pads: readonly ConnPad[];
}

/** A start item (`BOARD_CONNECTED_ITEM`): a track (each of its segments), a via or a pad. */
export type ConnStart = { kind: "track"; id: string } | { kind: "via"; id: string } | { kind: "pad"; id: string };

export type StopCondition = "junction" | "pad" | "never";

/** What a walk selects: tracks (by id) and vias. */
export interface ConnSelected {
  trackIds: Set<string>;
  viaIds: Set<string>;
}

const key = (p: Pt) => `${p[0]},${p[1]}`;
const copper = (layers: readonly string[]) => layers.filter((l) => l.endsWith(".Cu"));
const overlap = (a: readonly string[], b: readonly string[]) => a.some((l) => b.includes(l));

/**
 * `selectAllConnectedTracks( aStartItems, aStopCondition )`: adds to `selected` what the walk from `starts` reaches. `selected` is the live selection, which the
 * stages of `expandConnection` keep adding to.
 */
export function selectAllConnectedTracks(world: ConnWorld, starts: readonly ConnStart[], stop: StopCondition, selected: ConnSelected): void {
  const trackMap = new Map<string, ConnSeg[]>();
  const trackPoints = new Map<string, Pt>();
  for (const s of world.segs) {
    for (const p of [s.start, s.end]) {
      const list = trackMap.get(key(p));
      if (list) list.push(s);
      else trackMap.set(key(p), [s]);
      trackPoints.set(key(p), p);
    }
  }
  // `std::map<VECTOR2I, PCB_VIA*>`: one via per position (the last one put there), in x-then-y order
  const viaMap = new Map<string, ConnVia>();
  for (const v of world.vias) viaMap.set(key(v.at), v);
  const viasInOrder = [...viaMap.values()].sort((a, b) => a.at[0] - b.at[0] || a.at[1] - b.at[1]);
  const padMap = new Map<string, ConnPad>();
  for (const p of world.pads) padMap.set(key(p.at), p);

  const startPads = new Set(starts.filter((s) => s.kind === "pad").map((s) => s.id));
  const segsOf = (track: string) => world.segs.filter((s) => s.track === track);
  // "Select any starting track items"
  for (const s of starts) {
    if (s.kind === "track" && segsOf(s.id).length > 0) selected.trackIds.add(s.id);
    if (s.kind === "via" && world.vias.some((v) => v.id === s.id)) selected.viaIds.add(s.id);
  }

  // `SKIP_STRUCT`, for the whole call
  const doneSegs = new Set<ConnSeg>();
  const donePads = new Set<string>();
  const doneVias = new Set<string>();

  // each start item on its own (`for( startItem : aStartItems )`), a track standing for each of its segments
  const startItems: Array<{ pts: Array<[Pt, readonly string[]]>; done: () => boolean }> = [];
  for (const s of starts) {
    if (s.kind === "track") {
      for (const seg of segsOf(s.id)) startItems.push({ pts: [[seg.start, [seg.layer]], [seg.end, [seg.layer]]], done: () => doneSegs.has(seg) });
    } else if (s.kind === "via") {
      const v = world.vias.find((q) => q.id === s.id);
      if (v) startItems.push({ pts: [[v.at, v.layers]], done: () => doneVias.has(v.id) });
    } else {
      const p = world.pads.find((q) => q.id === s.id);
      if (p) startItems.push({ pts: [[p.at, p.layers]], done: () => donePads.has(p.id) });
    }
  }

  for (const item of startItems) {
    if (item.done()) continue; // "Skip already visited items"
    const active: Array<[Pt, readonly string[]]> = [...item.pts];
    let guard = 0;
    while (active.length > 0 && guard++ < 1_000_000) {
      const [pt, layers] = active.pop()!;
      const layerSetCu = copper(layers);
      const exact = viaMap.get(key(pt));
      let hitVia: ConnVia | null = exact && overlap(exact.layers, layerSetCu) ? exact : null;
      // off-centre via connection
      if (!hitVia) hitVia = viasInOrder.find((v) => overlap(v.layers, layerSetCu) && (pt[0] - v.at[0]) ** 2 + (pt[1] - v.at[1]) ** 2 <= (v.diameter / 2) ** 2) ?? null;
      const padHere = padMap.get(key(pt));
      const gotPad = padHere != null && overlap(padHere.layers, layerSetCu);
      const gotNonStartPad = gotPad && !startPads.has(padHere!.id);
      const here = trackMap.get(key(pt)) ?? [];

      if (stop === "junction") {
        const ptCount = here.filter((s) => (s.start[0] !== s.end[0] || s.start[1] !== s.end[1]) && layerSetCu.includes(s.layer)).length;
        if (ptCount > 2 || hitVia || gotNonStartPad) continue;
      } else if (stop === "pad") {
        if (gotNonStartPad) continue;
      }

      if (gotPad && !donePads.has(padHere!.id)) {
        donePads.add(padHere!.id);
        active.push([padHere!.at, padHere!.layers]);
      }
      for (const s of here) {
        if (!layerSetCu.includes(s.layer)) continue;
        selected.trackIds.add(s.track);
        if (!doneSegs.has(s)) {
          doneSegs.add(s);
          active.push([key(s.start) === key(pt) ? s.end : s.start, [s.layer]]);
        }
      }
      if (hitVia) {
        selected.viaIds.add(hitVia.id);
        if (!doneVias.has(hitVia.id)) {
          doneVias.add(hitVia.id);
          const r2 = (hitVia.diameter / 2) ** 2;
          for (const p of trackPoints.values()) {
            if ((p[0] - hitVia.at[0]) ** 2 + (p[1] - hitVia.at[1]) ** 2 > r2) continue;
            // "Verify point is inside the VIA pad on at least one track layer"
            if ((trackMap.get(key(p)) ?? []).some((s) => hitVia!.layers.includes(s.layer))) active.push([p, hitVia.layers]);
          }
        }
      }
    }
  }
}
