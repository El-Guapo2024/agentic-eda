// Which tracks and vias "Edit Track & Via Properties..." changes: the dialog's Scope box and Filter Items box
// (pcbnew/dialogs/dialog_global_edit_tracks_and_vias.cpp at 8303b2ad: `TransferDataFromWindow` picks the scope, `visitItem` applies the filters, and
// `buildFilterLists` / `TransferDataToWindow` say what each starts on). The change itself is `Cmd::EditTracksAndVias` on the ids this module returns.
//
//     TransferDataFromWindow    a track or arc when "Tracks" is ticked; a via when the box of its type -- through, micro, blind, buried -- is
//     visitItem                 selected items only (the item, or a group above it, selected) -> net -> net class -> layer
//                               (`GetLayer()`: a track's layer, a via's top layer) -> a via's diameter / a track's width, exactly
//     buildFilterLists          the net starts on the highlighted net, the layer on the active layer; the net and the class are then remembered for the session
//
// A via's type here is what its layers say: through when it spans the outer copper layers, blind when one end is outer and the other inner, buried when
// both are inner. This model has no microvia type, so that checkbox and `VIATYPE::MICROVIA` are left out.
//
// Pure: no store, no DOM. Unit-tested in globalEditTracksVias.test.ts.
import type { BoardState, Nm } from "../api/types";
import { netsContext } from "./appearanceNets";
import { ancestors } from "./groupTree";

export type ViaKind = "through" | "blind" | "buried";

/** `VIATYPE` of a via, from the copper layers it spans. `copper` is the stackup, front to back. */
export function viaKind(via: { from: string; to: string }, copper: readonly string[]): ViaKind {
  const top = copper[0];
  const bottom = copper[copper.length - 1];
  const outer = (layer: string) => layer === top || layer === bottom;
  if (outer(via.from) && outer(via.to) && via.from !== via.to) return "through";
  if (outer(via.from) || outer(via.to)) return "blind";
  return "buried";
}

/** `PCB_VIA::GetLayer()`: the top layer of the span, whichever way round the span is written. */
export function viaTopLayer(via: { from: string; to: string }, copper: readonly string[]): string {
  const at = (layer: string) => {
    const i = copper.indexOf(layer);
    return i < 0 ? Number.MAX_SAFE_INTEGER : i;
  };
  return at(via.to) < at(via.from) ? via.to : via.from;
}

/** The Scope box. */
export interface TracksViasScope {
  tracks: boolean;
  through: boolean;
  blind: boolean;
  buried: boolean;
}

/** The Filter Items box. The width and size are µm. */
export interface TracksViasFilter {
  selectedOnly: boolean;
  netOn: boolean;
  net: string;
  netclassOn: boolean;
  netclass: string;
  layerOn: boolean;
  layer: string;
  trackWidthOn: boolean;
  trackWidth: Nm;
  viaSizeOn: boolean;
  viaSize: Nm;
}

/** The state of the tri-state "Vias" box (`updateViasCheckbox`): off when no type is ticked, on when all are, and in between otherwise. */
export function viasBoxState(s: Pick<TracksViasScope, "through" | "blind" | "buried">): "off" | "on" | "mixed" {
  const n = [s.through, s.blind, s.buried].filter(Boolean).length;
  return n === 0 ? "off" : n === 3 ? "on" : "mixed";
}

/** `onVias`: clicking the "Vias" box sets every type to the box's new value. */
export function withAllVias(scope: TracksViasScope, on: boolean): TracksViasScope {
  return { ...scope, through: on, blind: on, buried: on };
}

/** What each list of the Filter Items box offers: the nets, the Default class and then the others, and the copper layers. */
export function trackViaChoices(board: BoardState | null): { nets: string[]; netclasses: string[]; layers: string[] } {
  const ctx = netsContext(board);
  return { nets: ctx.nets, netclasses: [ctx.defaultName, ...ctx.classes.map((c) => c.name)], layers: ctx.copper };
}

/**
 * The dialog as it opens: nothing in scope and every filter off (nothing in the base ever calls `SetValue( true )`), with each list on what
 * `buildFilterLists` and `TransferDataToWindow` give it -- the net the session remembered if the board still has it, else the highlighted net; the class
 * the session remembered, else the Default class; the active layer, or the first copper layer when the active one is not copper; the board's own
 * track width and via diameter in the two size filters.
 */
export function initialTracksViasFilter(board: BoardState | null, highlightedNet: string | null, activeLayer: string | null, remembered: { net: string; netclass: string }): TracksViasFilter {
  const c = trackViaChoices(board);
  const net = c.nets.includes(remembered.net) ? remembered.net : highlightedNet != null && c.nets.includes(highlightedNet) ? highlightedNet : (c.nets[0] ?? "");
  const netclass = c.netclasses.includes(remembered.netclass) ? remembered.netclass : (c.netclasses[0] ?? "Default");
  const layer = activeLayer != null && c.layers.includes(activeLayer) ? activeLayer : (c.layers[0] ?? "");
  const rules = board?.board_rules;
  return {
    selectedOnly: false,
    netOn: false,
    net,
    netclassOn: false,
    netclass,
    layerOn: false,
    layer,
    trackWidthOn: false,
    trackWidth: rules?.track_width ?? 0,
    viaSizeOn: false,
    viaSize: rules?.via_diameter ?? 0,
  };
}

export const NOTHING_IN_SCOPE: TracksViasScope = { tracks: false, through: false, blind: false, buried: false };

/** The ids of the tracks (arcs included) and vias the dialog would change, tracks first, in board order. */
export function matchingTracksAndVias(board: BoardState | null, scope: TracksViasScope, filter: TracksViasFilter, selection: ReadonlySet<string>): { trackIds: string[]; viaIds: string[] } {
  const out = { trackIds: [] as string[], viaIds: [] as string[] };
  if (!board?.routing) return out;
  const groups = board.drawings?.groups ?? [];
  const ctx = netsContext(board);
  // `aItem->IsSelected()`, or a group above it is (`while( group && !group->IsSelected() )`)
  const isSelected = (id: string) => selection.has(id) || ancestors(groups, id).some((g) => selection.has(g.id));
  const passes = (id: string, net: string, layer: string): boolean => {
    if (filter.selectedOnly && !isSelected(id)) return false;
    if (filter.netOn && filter.net !== "" && net !== filter.net) return false;
    if (filter.netclassOn && filter.netclass !== "" && ctx.classOf(net) !== filter.netclass) return false;
    if (filter.layerOn && filter.layer !== "" && layer !== filter.layer) return false;
    return true;
  };
  if (scope.tracks) {
    for (const t of board.routing.tracks) {
      if (!passes(t.id, t.net, t.layer)) continue;
      if (filter.trackWidthOn && t.width !== filter.trackWidth) continue;
      out.trackIds.push(t.id);
    }
  }
  if (scope.through || scope.blind || scope.buried) {
    for (const v of board.routing.vias) {
      if (!scope[viaKind(v, board.layers)]) continue;
      if (!passes(v.id, v.net, viaTopLayer(v, board.layers))) continue;
      if (filter.viaSizeOn && v.d !== filter.viaSize) continue;
      out.viaIds.push(v.id);
    }
  }
  return out;
}
