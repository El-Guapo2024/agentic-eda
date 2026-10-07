// Ports of pcbnew selection-tool operations (pcbnew/tools/
// pcb_selection_tool.cpp at 8303b2ad) that are pure functions over the
// studio's board view:
//
//   Select / Deselect All Tracks in Net   selectNet -> SelectAllItemsOnNet
//   Filter Selected Items...              filterSelection, itemIsIncludedByFilter
//                                         (dialog_filter_selection.cpp options)
//   Unroute Selected                      unrouteSelected, via selectAllConnectedTracks
//   selectConnections                     the "with nets" part of Sync Selection
//
// Not ported: connectivity through zones (the C++ passes EXCLUDE_ZONES to
// the flood anyway) and PCB_GENERATOR promotion (no generator items here).

import type { BoardState } from "../api/types";
import { itemKind } from "./pcbItems";

// ------------------------------------------------------------ select net

/** `BOARD_CONNECTED_ITEM`s of the selection that carry a net: tracks, vias and zones (pads are not separately selectable here). */
export function netsOfItems(board: BoardState, ids: readonly string[]): Set<string> {
  const nets = new Set<string>();
  for (const id of ids) {
    const kind = itemKind(board, id);
    const net = kind === "track" ? board.routing?.tracks.find((t) => t.id === id)?.net : kind === "via" ? board.routing?.vias.find((v) => v.id === id)?.net : kind === "zone" ? board.routing?.zones.find((z) => z.id === id)?.net : undefined;
    if (net) nets.add(net);
  }
  return nets;
}

/** `SelectAllItemsOnNet`'s item set: `conn->GetNetItems( net, { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_SHAPE_T } )` -- the tracks and vias of the nets (copper shapes carry no net here). */
export function netItems(board: BoardState, nets: ReadonlySet<string>): string[] {
  const out: string[] = [];
  for (const t of board.routing?.tracks ?? []) if (nets.has(t.net)) out.push(t.id);
  for (const v of board.routing?.vias ?? []) if (nets.has(v.net)) out.push(v.id);
  return out;
}

// --------------------------------------------------------- filter dialog

/** `DIALOG_FILTER_SELECTION::OPTIONS` (every option on by default). */
export interface FilterOptions {
  includeFootprints: boolean;
  includeLockedFootprints: boolean;
  includeTracks: boolean;
  includeVias: boolean;
  includeZones: boolean;
  includeItemsOnTechLayers: boolean;
  includeBoardOutlineLayer: boolean;
  includePcbTexts: boolean;
}

export const DEFAULT_FILTER_OPTIONS: FilterOptions = {
  includeFootprints: true,
  includeLockedFootprints: true,
  includeTracks: true,
  includeVias: true,
  includeZones: true,
  includeItemsOnTechLayers: true,
  includeBoardOutlineLayer: true,
  includePcbTexts: true,
};

/** `itemIsIncludedByFilter` (pcb_selection_tool.cpp): a filter that is inclusive -- an item that is not listed (a group, ...) is dropped. */
export function itemIsIncludedByFilter(board: BoardState, id: string, opts: FilterOptions, locked: ReadonlySet<string>): boolean {
  switch (itemKind(board, id)) {
    case "part":
      return opts.includeFootprints && (opts.includeLockedFootprints || !locked.has(id));
    case "track":
      return opts.includeTracks;
    case "via":
      return opts.includeVias;
    case "zone":
      return opts.includeZones;
    case "shape": {
      const s = board.drawings?.shapes.find((q) => q.id === id);
      return s?.layer === "Edge.Cuts" ? opts.includeBoardOutlineLayer : opts.includeItemsOnTechLayers;
    }
    case "dimension": {
      const d = board.drawings?.dimensions.find((q) => q.id === id);
      return d?.layer === "Edge.Cuts" ? opts.includeBoardOutlineLayer : opts.includeItemsOnTechLayers;
    }
    case "text":
      return opts.includePcbTexts;
    default:
      return false;
  }
}

/** `filterSelection`: what stays selected. */
export function filterSelection(board: BoardState, ids: readonly string[], opts: FilterOptions): string[] {
  const locked = new Set(board.locked ?? []);
  return ids.filter((id) => itemIsIncludedByFilter(board, id, opts, locked));
}

// ------------------------------------------------- selectAllConnectedTracks

export type StopCondition = "pad" | "junction" | "never";

interface FloodBoard {
  tracks: readonly { id: string; layer: string; pts: readonly (readonly [number, number])[]; arc_mid?: readonly [number, number] | null }[];
  vias: readonly { id: string; x: number; y: number; d: number; from: string; to: string }[];
  /** Pad centres with the copper layers they are on and a unique key (`ref.number`). */
  pads: readonly { key: string; x: number; y: number; layers: readonly string[] }[];
  /** Copper layers in stackup order. */
  layers: readonly string[];
}

export interface FloodStart {
  kind: "track" | "via" | "pad";
  /** A track/via id, or a pad key. */
  id: string;
}

const key = (x: number, y: number): string => `${x},${y}`;

/**
 * `PCB_SELECTION_TOOL::selectAllConnectedTracks( aStartItems, aStopCondition )` over this model: starting
 * tracks/vias are selected; from each start item's connection points the flood follows tracks (any net, like
 * `IGNORE_NETS`) and vias (across the layers they span), and `"pad"` stops at any pad that is not itself a
 * starting pad. A polyline's every vertex is a connection point (each of its segments is one KiCad track).
 */
export function selectAllConnectedTracks(board: FloodBoard, starts: readonly FloodStart[], stop: StopCondition): { trackIds: Set<string>; viaIds: Set<string> } {
  const trackIds = new Set<string>();
  const viaIds = new Set<string>();
  const startPads = new Set(starts.filter((s) => s.kind === "pad").map((s) => s.id));
  const byId = new Map(board.tracks.map((t) => [t.id, t]));
  const viaById = new Map(board.vias.map((v) => [v.id, v]));
  const padByKey = new Map(board.pads.map((p) => [p.key, p]));

  const viaLayers = (v: { from: string; to: string }): string[] => {
    const a = board.layers.indexOf(v.from);
    const b = board.layers.indexOf(v.to);
    if (a < 0 || b < 0) return [v.from, v.to];
    return board.layers.slice(Math.min(a, b), Math.max(a, b) + 1);
  };

  // trackMap: connection point -> tracks (a polyline joins at every vertex, an arc only at its ends)
  const trackMap = new Map<string, string[]>();
  for (const t of board.tracks) {
    const pts = t.arc_mid ? [t.pts[0], t.pts[t.pts.length - 1]] : t.pts;
    for (const p of pts) {
      if (!p) continue;
      const k = key(p[0], p[1]);
      const list = trackMap.get(k);
      if (list) {
        if (!list.includes(t.id)) list.push(t.id);
      } else trackMap.set(k, [t.id]);
    }
  }
  const viaMap = new Map<string, string[]>();
  for (const v of board.vias) {
    const k = key(v.x, v.y);
    const list = viaMap.get(k);
    if (list) list.push(v.id);
    else viaMap.set(k, [v.id]);
  }
  const padMap = new Map<string, string[]>();
  for (const p of board.pads) {
    const k = key(p.x, p.y);
    const list = padMap.get(k);
    if (list) list.push(p.key);
    else padMap.set(k, [p.key]);
  }

  // "Select any starting track items"
  for (const s of starts) {
    if (s.kind === "track" && byId.has(s.id)) trackIds.add(s.id);
    if (s.kind === "via" && viaById.has(s.id)) viaIds.add(s.id);
  }

  const visitedTracks = new Set<string>();
  const visitedVias = new Set<string>();
  const visitedPads = new Set<string>();

  for (const s of starts) {
    let active: { p: readonly [number, number]; layers: readonly string[] }[] = [];
    if (s.kind === "track") {
      const t = byId.get(s.id);
      if (!t || visitedTracks.has(t.id)) continue;
      const pts = t.arc_mid ? [t.pts[0]!, t.pts[t.pts.length - 1]!] : t.pts;
      for (const p of pts) active.push({ p, layers: [t.layer] });
    } else if (s.kind === "via") {
      const v = viaById.get(s.id);
      if (!v || visitedVias.has(v.id)) continue;
      active.push({ p: [v.x, v.y], layers: viaLayers(v) });
    } else {
      const pad = padByKey.get(s.id);
      if (!pad) continue;
      active.push({ p: [pad.x, pad.y], layers: pad.layers });
    }

    let guard = 0;
    while (active.length > 0 && guard++ < 100000) {
      const next: typeof active = [];
      for (const { p, layers } of active) {
        const k = key(p[0], p[1]);
        const hitVia = (viaMap.get(k) ?? []).map((id) => viaById.get(id)!).find((v) => viaLayers(v).some((l) => layers.includes(l)));
        const padHere = (padMap.get(k) ?? []).map((pk) => padByKey.get(pk)!).find((pad) => pad.layers.some((l) => layers.includes(l)));
        const gotNonStartPad = padHere !== undefined && !startPads.has(padHere.key);

        if (stop === "junction") {
          const count = (trackMap.get(k) ?? []).filter((id) => layers.includes(byId.get(id)!.layer)).length;
          if (count > 2 || hitVia || gotNonStartPad) continue;
        } else if (stop === "pad" && gotNonStartPad) {
          continue;
        }

        if (padHere && !visitedPads.has(padHere.key)) {
          visitedPads.add(padHere.key);
          next.push({ p: [padHere.x, padHere.y], layers: padHere.layers });
        }

        for (const id of trackMap.get(k) ?? []) {
          const t = byId.get(id)!;
          if (!layers.includes(t.layer)) continue;
          trackIds.add(id);
          if (visitedTracks.has(id)) continue;
          visitedTracks.add(id);
          for (const q of t.arc_mid ? [t.pts[0]!, t.pts[t.pts.length - 1]!] : t.pts) {
            if (q[0] === p[0] && q[1] === p[1]) continue;
            next.push({ p: q, layers: [t.layer] });
          }
        }

        if (hitVia) {
          viaIds.add(hitVia.id);
          if (!visitedVias.has(hitVia.id)) {
            visitedVias.add(hitVia.id);
            // every track point inside the via's pad joins, on all the via's layers
            const spans = viaLayers(hitVia);
            const radiusSq = (hitVia.d / 2) ** 2;
            for (const t of board.tracks) {
              if (!spans.includes(t.layer)) continue;
              for (const q of t.arc_mid ? [t.pts[0]!, t.pts[t.pts.length - 1]!] : t.pts) {
                if ((q[0] - hitVia.x) ** 2 + (q[1] - hitVia.y) ** 2 <= radiusSq) next.push({ p: q, layers: spans });
              }
            }
          }
        }
      }
      active = next;
    }
  }
  return { trackIds, viaIds };
}

// ------------------------------------------------------------- unroute

export interface UnroutePart {
  ref: string;
  placed: boolean;
  side?: string;
  pads?: readonly { num: string; x: number; y: number; th: boolean }[];
}

/** The pads of the board as flood nodes: through-hole pads are on every copper layer, SMD pads on their footprint's side. */
export function floodPads(parts: readonly UnroutePart[], layers: readonly string[]): FloodBoard["pads"] {
  const first = layers[0] ?? "F.Cu";
  const last = layers[layers.length - 1] ?? "B.Cu";
  const out: { key: string; x: number; y: number; layers: readonly string[] }[] = [];
  for (const p of parts) {
    if (!p.placed) continue;
    for (const pad of p.pads ?? []) out.push({ key: `${p.ref}.${pad.num}`, x: pad.x, y: pad.y, layers: pad.th ? layers : [p.side === "bottom" ? last : first] });
  }
  return out;
}

/**
 * `PCB_SELECTION_TOOL::unrouteSelected`: every selected footprint contributes its pads, every selected
 * track/via itself; the tracks and vias connected to them up to the next pad are deleted. The footprints
 * stay selected.
 */
export function unrouteSelected(board: BoardState, ids: readonly string[]): { trackIds: string[]; viaIds: string[]; reselect: string[] } {
  const layers = board.layers;
  const pads = floodPads(board.parts, layers);
  const starts: FloodStart[] = [];
  const reselect: string[] = [];
  for (const id of ids) {
    const kind = itemKind(board, id);
    if (kind === "part") {
      reselect.push(id);
      for (const p of pads) if (p.key.startsWith(`${id}.`)) starts.push({ kind: "pad", id: p.key });
    } else if (kind === "track" || kind === "via") {
      starts.push({ kind, id });
    }
  }
  const flood: FloodBoard = {
    tracks: (board.routing?.tracks ?? []).map((t) => ({ id: t.id, layer: t.layer, pts: t.pts, arc_mid: t.arc_mid })),
    vias: (board.routing?.vias ?? []).map((v) => ({ id: v.id, x: v.x, y: v.y, d: v.d, from: v.from, to: v.to })),
    pads,
    layers,
  };
  const { trackIds, viaIds } = selectAllConnectedTracks(flood, starts, "pad");
  return { trackIds: [...trackIds], viaIds: [...viaIds], reselect };
}

// ------------------------------------------------------ selectConnections

/**
 * `PCB_SELECTION_TOOL::selectConnections( footprints )` (the "with nets" half of Sync Selection and the
 * body of Select Items in Same Sheet): the tracks and vias reached from the footprints' pads (stopping at
 * other pads), plus every track and via of each net whose pads ALL belong to those footprints.
 */
export function selectConnections(board: BoardState, partRefs: readonly string[]): string[] {
  const layers = board.layers;
  const pads = floodPads(board.parts, layers);
  const refs = new Set(partRefs);
  const padList = pads.filter((p) => refs.has(p.key.slice(0, p.key.lastIndexOf("."))));
  const flood: FloodBoard = {
    tracks: (board.routing?.tracks ?? []).map((t) => ({ id: t.id, layer: t.layer, pts: t.pts, arc_mid: t.arc_mid })),
    vias: (board.routing?.vias ?? []).map((v) => ({ id: v.id, x: v.x, y: v.y, d: v.d, from: v.from, to: v.to })),
    pads,
    layers,
  };
  const { trackIds, viaIds } = selectAllConnectedTracks(
    flood,
    padList.map((p) => ({ kind: "pad" as const, id: p.key })),
    "pad"
  );

  // Nets of the connected pads; a net stays only if every pad on it is in the list.
  const padNet = new Map<string, string>();
  for (const part of board.parts) for (const pad of part.pads ?? []) if (pad.net) padNet.set(`${part.ref}.${pad.num}`, pad.net);
  const listed = new Set(padList.map((p) => p.key));
  const nets = new Set<string>();
  for (const p of padList) {
    const net = padNet.get(p.key);
    if (net) nets.add(net);
  }
  for (const [padKey, net] of padNet) if (nets.has(net) && !listed.has(padKey)) nets.delete(net);

  const out = new Set<string>([...trackIds, ...viaIds]);
  for (const id of netItems(board, nets)) out.add(id);
  return [...out];
}
