// Which board texts, graphics and dimensions "Edit Text & Graphics Properties..." changes: the dialog's Scope box and Filter Items box
// (pcbnew/dialogs/dialog_global_edit_text_and_graphics.cpp at 8303b2ad: `TransferDataFromWindow` walks `Drawings()` and picks the kind each scope box
// names, `visitItem` applies the filters). The change itself is `Cmd::EditTextAndGraphics` on the ids this module returns.
//
//     TransferDataFromWindow   "PCB text items" -> texts, "PCB graphic items" -> shapes, "PCB dimensions" -> dimensions
//     visitItem                selected items only (the item, or a group above it, selected) -> by layer (`GetLayer()`)
//
// What this model has of the dialog's items is the free-standing board ones; the footprint reference, value, other fields, text, graphics and
// dimension scopes, and the "By parent reference designator" / "By parent footprint library link" filters (which only ever look at an item's parent
// footprint), have nothing to select here (PARITY-pcb.md section 13).
//
// Pure: no store, no DOM. Unit-tested in globalEditTextGraphics.test.ts.
import type { BoardState } from "../api/types";
import { enabledOptionalLayers } from "./boardLayers";
import { ancestors } from "./groupTree";

/** The Scope box. */
export interface TextGraphicsScope {
  graphics: boolean;
  text: boolean;
  dimensions: boolean;
}

export const NOTHING_IN_TEXT_SCOPE: TextGraphicsScope = { graphics: false, text: false, dimensions: false };

/** The Filter Items box. */
export interface TextGraphicsFilter {
  layerOn: boolean;
  layer: string;
  selectedOnly: boolean;
}

/** Every layer a text, a graphic item or a dimension can be on: the board's copper, then the technical and user layers it has (Board Editor Layers, in the order of the layer selector `PCB_LAYER_BOX_SELECTOR`), then any other layer an item of the board is on. */
export function textGraphicsLayers(board: BoardState | null): string[] {
  const out = [...(board?.layers ?? []), ...enabledOptionalLayers(board?.board_rules?.layer_setup)];
  const known = new Set(out);
  const dr = board?.drawings;
  for (const layer of [...(dr?.shapes ?? []).map((s) => s.layer), ...(dr?.texts ?? []).map((t) => t.layer), ...(dr?.dimensions ?? []).map((d) => d.layer)]) {
    if (layer && !known.has(layer)) {
      known.add(layer);
      out.push(layer);
    }
  }
  return out;
}

/** The dialog as it opens: nothing in scope, every filter off, the layer list on the active layer (else the first layer). */
export function initialTextGraphicsFilter(board: BoardState | null, activeLayer: string | null): TextGraphicsFilter {
  const layers = textGraphicsLayers(board);
  return { layerOn: false, layer: activeLayer != null && layers.includes(activeLayer) ? activeLayer : (layers[0] ?? ""), selectedOnly: false };
}

/** The ids of the shapes, texts and dimensions the dialog would change, in board order. */
export function matchingTextAndGraphics(board: BoardState | null, scope: TextGraphicsScope, filter: TextGraphicsFilter, selection: ReadonlySet<string>): { shapeIds: string[]; textIds: string[]; dimensionIds: string[] } {
  const out = { shapeIds: [] as string[], textIds: [] as string[], dimensionIds: [] as string[] };
  const dr = board?.drawings;
  if (!dr) return out;
  const groups = dr.groups ?? [];
  // `aItem->IsSelected()`, or a group above it is
  const isSelected = (id: string) => selection.has(id) || ancestors(groups, id).some((g) => selection.has(g.id));
  const passes = (id: string, layer: string): boolean => {
    if (filter.selectedOnly && !isSelected(id)) return false;
    if (filter.layerOn && filter.layer !== "" && layer !== filter.layer) return false;
    return true;
  };
  if (scope.graphics) for (const s of dr.shapes) if (passes(s.id, s.layer)) out.shapeIds.push(s.id);
  if (scope.text) for (const t of dr.texts) if (passes(t.id, t.layer)) out.textIds.push(t.id);
  if (scope.dimensions) for (const d of dr.dimensions ?? []) if (passes(d.id, d.layer)) out.dimensionIds.push(d.id);
  return out;
}
