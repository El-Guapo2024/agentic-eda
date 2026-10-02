// Helpers for the inline-drag tool (Canvas.tsx, `D`) -- the
// router_tool.cpp `InlineDrag` counterpart of routing.ts's route-tool
// helpers, same split: this is the thin client-side glue (hit-test the
// cursor, turn a click/move into the right `/api/route/drag_*` call and
// `DrawState` update), not a second copy of `eda_pns::dragger::Dragger`'s
// own algorithm.
import type { BoardState, RouteMode } from "../../api/types";
import { routeDragFinish, routeDragStart } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import type { DragDrawState } from "../../kicad-port/dragTool";
import { pickSelectionCandidates, type SelectionFilter } from "./selectionCandidates";

export interface DraggableHit {
  kind: "track" | "via";
  id: string;
  /** A layer the hit item actually sits on -- `drag_start` needs one to
   * hit-test with (`Node::item_at`'s own layer-overlap filter), even
   * though a via spans every layer it's placed on and so matches almost
   * any choice. */
  layer: string;
}

/** `ROUTER_TOOL::CanInlineDrag`/`InlineDrag`'s own item-finding step,
 * approximated client-side: whatever track or via is directly under
 * `(xUm, yUm)` (reusing the exact same hit-test the click-to-select tool
 * uses, restricted to the two kinds `crate::pns::dragger::Dragger` can
 * actually drag -- see that module's own doc comment on why footprints
 * and segment-sideways-slide aren't in scope), falling back to a single
 * already-selected track/via if nothing is directly under the cursor
 * (source's other branch: drag the existing selection when there is
 * exactly one draggable item in it). `null` either way means "nothing to
 * drag here" -- the real pcbnew just rings the bell and does nothing. */
export function findDraggableAt(
  board: BoardState,
  selection: ReadonlySet<string>,
  xUm: number,
  yUm: number,
  toleranceUm: number,
  onePixelUm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean
): DraggableHit | null {
  const fallbackLayer = activeLayer ?? board.layers[0] ?? "F.Cu";
  const candidates = pickSelectionCandidates(board, xUm, yUm, toleranceUm, onePixelUm, filter, layerVisible, activeLayer, highContrast, selection, false, false);
  const hit = candidates.find((c) => c.kind === "track" || c.kind === "via");
  if (hit) return { kind: hit.kind as "track" | "via", id: hit.id, layer: hit.layer ?? fallbackLayer };

  if (selection.size === 1) {
    const id = [...selection][0]!;
    const track = board.routing?.tracks.find((t) => t.id === id);
    if (track) return { kind: "track", id, layer: track.layer };
    const via = board.routing?.vias.find((v) => v.id === id);
    if (via) return { kind: "via", id, layer: fallbackLayer };
  }
  return null;
}

/** `D`: grab `hit` at `(x, y)` and arm the drag tool. Shows an error toast
 * and leaves `drawState` untouched if the backend refuses (e.g. the item
 * named by `hit` no longer exists -- a stale client-side hit against a
 * board that changed a moment ago). `mode` is `state.routerSettings.mode`
 * (`Ctrl+<`'s dialog) -- `Dragger` honors the same walkaround/shove/
 * mark-obstacles choice a route session does. `freeAngle` is `G`
 * (`PCB_ACTIONS::dragFreeAngle` -> `DM_ANY | DM_FREE_ANGLE`): the drag only
 * marks obstacles, whatever `mode` says. */
export async function startInlineDrag(x: number, y: number, hit: DraggableHit, board: BoardState, mode: RouteMode, dispatch: Dispatch<Action>, freeAngle = false): Promise<void> {
  const preview = await routeDragStart(x, y, hit.layer, mode, freeAngle);
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Nothing to drag there.", kind: "error" });
    return;
  }
  const track = hit.kind === "track" ? board.routing?.tracks.find((t) => t.id === hit.id) : undefined;
  const via = hit.kind === "via" ? board.routing?.vias.find((v) => v.id === hit.id) : undefined;
  const draw: DragDrawState = {
    kind: "drag",
    dragKind: hit.kind === "via" ? "via" : "corner",
    net: track?.net ?? via?.net ?? null,
    layer: track?.layer ?? hit.layer,
    width: track?.width ?? 0,
    viaDiameter: via?.d,
    pts: preview.pts,
    colliding: preview.colliding,
    displaced: preview.displaced,
    displacedVias: preview.displaced_vias,
    fanout: preview.fanout,
  };
  dispatch({ type: "SET_DRAW_STATE", draw });
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "drag" });
}

/** A click while dragging: commit at `(x, y)` -- a drag has no multi-leg
 * concept the way a route does, so (unlike `fixInteractiveRoute`) this is
 * always the final commit, not "fix this leg and keep going". Refreshes
 * the board so the moved item shows up without waiting for the next
 * version poll, same as `finishInteractiveRoute`, and always returns the
 * tool to Select (KiCad's own `InlineDrag` is a one-shot action, not a
 * persistently-armed tool the way the route tool's `X` is). */
export async function finishInlineDrag(x: number, y: number, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await routeDragFinish(x, y);
  if (!reply.ok) dispatch({ type: "TOAST", message: reply.message || "Can't drop it there -- still colliding.", kind: "error" });
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  await api.refresh();
}
