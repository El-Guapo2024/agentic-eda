// What an inspection pick takes from a click (`BOARD_INSPECTION_TOOL::pickItemForInspection` and `filterCollectorForInspection`, pcbnew/tools/board_inspection_tool.cpp at
// 8303b2ad): the items under the pointer, and when one of them is a pad, a via, a track or a zone the footprint they sit in is dropped, so that a click on a pad is the pad. The
// canvas has already guessed one item for the click (`GuessSelectionCandidates`); only when that is a footprint is the collector looked at again here.
import type { BoardState } from "../api/types";
import { collectSelectionCandidates, type SelectableKind, type SelectionFilter } from "../components/canvas/selectionCandidates";
import { guessSelectionCandidates } from "./selection";
import { itemKind } from "./pcbItems";

export interface InspectPickContext {
  filter: SelectionFilter;
  layerVisible: Record<string, boolean>;
  activeLayer: string | null;
  highContrast: boolean;
  selection: ReadonlySet<string>;
  /** Board micrometres per screen pixel's reciprocal: `1 / view.scale`. */
  onePixelUm: number;
  toleranceUm: number;
}

/** The kinds of item `clearanceTypes` (pads, vias, tracks, arcs, zones, shapes, footprints) and `constraintTypes` (those and fields and texts) collect. */
export const CLEARANCE_KINDS: readonly SelectableKind[] = ["pad", "via", "track", "zone", "shape", "part"];
export const CONSTRAINT_KINDS: readonly SelectableKind[] = [...CLEARANCE_KINDS, "field", "text"];

/** `filterCollectorForInspection` and the guess: the id of the item a click at `(x, y)` inspects, among the `kinds`. */
export function inspectionPick(board: BoardState, x: number, y: number, ctx: InspectPickContext, kinds: readonly SelectableKind[]): string | null {
  let all = collectSelectionCandidates(board, x, y, ctx.toleranceUm, ctx.filter, ctx.layerVisible, ctx.activeLayer, ctx.highContrast, ctx.selection, false).filter((c) => kinds.includes(c.kind));
  const copper = all.some((c) => c.kind === "pad" || c.kind === "via" || c.kind === "track" || c.kind === "zone");
  if (copper) all = all.filter((c) => c.kind !== "part");
  const guessed = all.length > 1 ? guessSelectionCandidates(all, ctx.onePixelUm, ctx.activeLayer) : all;
  return guessed[0]?.id ?? null;
}

/** What the canvas picked, made what the inspection takes: a footprint under a click that is also on a pad or a track is that pad or track. */
export function refineInspectionPick(board: BoardState, id: string, at: { x: number; y: number }, ctx: InspectPickContext, kinds: readonly SelectableKind[]): string {
  if (itemKind(board, id) !== "part") return id;
  return inspectionPick(board, at.x, at.y, ctx, kinds) ?? id;
}
