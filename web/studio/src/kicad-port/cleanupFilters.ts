// The "Filter Items" box of "Cleanup Tracks & Vias..." and its "Refill zones before and after cleanup" box
// (pcbnew/dialogs/dialog_cleanup_tracks_and_vias.cpp at 8303b2ad: the ctor lists the nets, the net classes and the copper layers, TransferDataToWindow picks
// what each starts on, doCleanup's `SetFilter` lambda is what the backend runs -- crates/connectivity/src/cleanup_filter.rs).
//
//     m_netFilter        a net list           starts on the first highlighted net
//     m_netclassFilter   the Default class and the others    starts on `GetCurrentNetClassName()`
//     m_layerFilter      the copper layers    starts on the active layer
//     m_selectedItemsFilter                    "Selected items only"
//
// Every filter is off until its box is ticked; a filter that is on skips the tracks and vias that do not pass it. Changing any of them (`OnCheckBox`,
// `OnNetFilterSelect`, `OnNetclassFilterSelect`, `OnLayerFilterSelect`) sends the dialog back to "Build Changes". "Refill zones" is not wired to that handler.
//
// Pure: no store, no DOM. Unit-tested in cleanupFilters.test.ts.
import type { BoardState } from "../api/types";
import { netsContext } from "./appearanceNets";

/** What the "Filter Items" box holds: each filter's box and what it is set to. */
export interface CleanupFilterBox {
  netOn: boolean;
  net: string;
  netclassOn: boolean;
  netclass: string;
  layerOn: boolean;
  layer: string;
  selectedOnly: boolean;
}

/** The request fields the backend reads (`cleanup_api.rs` `filter_of`); a filter that is off sends nothing. */
export interface CleanupFilterFields {
  filter_net?: string;
  filter_netclass?: string;
  filter_layer?: string;
  /** Present -- even when empty -- is "Selected items only". Groups are among the ids: the backend opens them. */
  selected_ids?: string[];
}

/** What each list offers: the nets, the Default class first and then the others, the copper layers. */
export interface CleanupFilterChoices {
  nets: string[];
  netclasses: string[];
  layers: string[];
}

export function filterChoices(board: BoardState | null): CleanupFilterChoices {
  const ctx = netsContext(board);
  return { nets: ctx.nets, netclasses: [ctx.defaultName, ...ctx.classes.map((c) => c.name)], layers: ctx.copper };
}

/**
 * `TransferDataToWindow`: the net starts on the highlighted net, the layer on the active layer (a layer that is not copper is not in the list, so the first
 * copper layer stands), the class on the Default class -- `GetCurrentNetClassName()` is the class of the net last routed, which is Default until then. Every
 * filter starts off.
 */
export function filterBoxDefaults(board: BoardState | null, highlightedNet: string | null, activeLayer: string | null): CleanupFilterBox {
  const c = filterChoices(board);
  return {
    netOn: false,
    net: highlightedNet != null && c.nets.includes(highlightedNet) ? highlightedNet : (c.nets[0] ?? ""),
    netclassOn: false,
    netclass: c.netclasses[0] ?? "Default",
    layerOn: false,
    layer: activeLayer != null && c.layers.includes(activeLayer) ? activeLayer : (c.layers[0] ?? ""),
    selectedOnly: false,
  };
}

/** The request fields for the filters that are on. */
export function filterFields(box: CleanupFilterBox, selection: Iterable<string>): CleanupFilterFields {
  const out: CleanupFilterFields = {};
  if (box.netOn && box.net) out.filter_net = box.net;
  if (box.netclassOn && box.netclass) out.filter_netclass = box.netclass;
  if (box.layerOn && box.layer) out.filter_layer = box.layer;
  if (box.selectedOnly) out.selected_ids = [...selection];
  return out;
}

/** Whether any filter is on (the dialog says so in its report line). */
export function anyFilterOn(box: CleanupFilterBox): boolean {
  return box.netOn || box.netclassOn || box.layerOn || box.selectedOnly;
}
