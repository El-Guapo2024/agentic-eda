// Helpers for the inline-drag tool (Canvas.tsx, `D`) -- the
// router_tool.cpp `InlineDrag` counterpart of routing.ts's route-tool
// helpers, same split: this is the thin client-side glue (hit-test the
// cursor, turn a click/move into the right `/api/route/drag_*` call and
// `DrawState` update), not a second copy of `eda_pns::dragger::Dragger`'s
// own algorithm.
import type { BoardState, CmdReply } from "../../api/types";
import type { RouterSettings } from "../../kicad-port/routerSettings";
import { routeDragFinish, routeDragStart } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import type { DragDrawState } from "../../kicad-port/dragTool";
import { pickSelectionCandidates, type SelectionFilter } from "./selectionCandidates";
import { askConfirm } from "../library/libraryDialogs";

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
 * `(xNm, yNm)` (reusing the exact same hit-test the click-to-select tool
 * uses, restricted to the two kinds `crate::pns::dragger::Dragger` can
 * actually drag -- a footprint is not in scope), falling back to a single
 * already-selected track/via if nothing is directly under the cursor
 * (source's other branch: drag the existing selection when there is
 * exactly one draggable item in it). `null` either way means "nothing to
 * drag here" -- the real pcbnew just rings the bell and does nothing. */
export function findDraggableAt(
  board: BoardState,
  selection: ReadonlySet<string>,
  xNm: number,
  yNm: number,
  toleranceNm: number,
  onePixelNm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean
): DraggableHit | null {
  const fallbackLayer = activeLayer ?? board.layers[0] ?? "F.Cu";
  const candidates = pickSelectionCandidates(board, xNm, yNm, toleranceNm, onePixelNm, filter, layerVisible, activeLayer, highContrast, selection, false, false);
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
 * board that changed a moment ago). `settings` is `state.routerSettings`
 * (`Ctrl+<`'s dialog) -- `Dragger` honors the same walkaround/shove/
 * mark-obstacles choice, and the rest of the router's settings, a route session does. `freeAngle` is `G`
 * (`PCB_ACTIONS::dragFreeAngle` -> `DM_ANY | DM_FREE_ANGLE`): the drag only
 * marks obstacles, whatever `mode` says. */
export async function startInlineDrag(x: number, y: number, hit: DraggableHit, board: BoardState, settings: RouterSettings, dispatch: Dispatch<Action>, freeAngle = false): Promise<void> {
  let preview = await routeDragStart(x, y, hit.layer, settings, freeAngle);
  // `ROUTER_TOOL::performDragging`: a locked item asks first, and "Drag Anyway" unlocks it for the drag
  if (!preview.ok && preview.locked) {
    const anyway = await askConfirm({ title: "Confirmation", message: "The selected item is locked.", okLabel: "Drag Anyway" });
    if (!anyway) return;
    preview = await routeDragStart(x, y, hit.layer, settings, freeAngle, true);
  }
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Nothing to drag there.", kind: "error" });
    return;
  }
  const track = hit.kind === "track" ? board.routing?.tracks.find((t) => t.id === hit.id) : undefined;
  const via = hit.kind === "via" ? board.routing?.vias.find((v) => v.id === hit.id) : undefined;
  // `DRAGGER::Start` decides: the middle of a segment slides it (`DM_SEGMENT`), within half its width of an end it is that corner
  const draw: DragDrawState = {
    kind: "drag",
    dragKind: preview.drag ?? (hit.kind === "via" ? "via" : "corner"),
    net: track?.net ?? via?.net ?? null,
    layer: track?.layer ?? hit.layer,
    width: track?.width ?? 0,
    viaDiameter: via?.d,
    virtualVia: preview.virtual_via ?? false,
    lineTracks: preview.line_tracks ?? [],
    pts: preview.pts,
    colliding: preview.colliding,
    restored: preview.restored ?? false,
    forcedMark: preview.forced_mark_obstacles ?? false,
    displaced: preview.displaced,
    displacedVias: preview.displaced_vias,
    fanout: preview.fanout,
  };
  dispatch({ type: "SET_DRAW_STATE", draw });
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "drag" });
  // `InlineDrag`: a drag whose first position is already a violation only marks obstacles from then on
  if (draw.forcedMark && draw.colliding) dispatch({ type: "TOAST", message: DRAG_VIOLATION_HINT, kind: "info" });
}

/** `ROUTER_STATUS_VIEW_ITEM`'s message while a drag only marks obstacles and the position violates DRC (router_tool.cpp `InlineDrag`). */
export const DRAG_VIOLATION_HINT = "Track violates DRC. (Ctrl+Click to commit anyway.)";

/** A click while dragging: commit at `(x, y)` -- a drag has no multi-leg
 * concept the way a route does, so (unlike `fixInteractiveRoute`) this is
 * always the final commit, not "fix this leg and keep going". Refreshes
 * the board so the moved item shows up without waiting for the next
 * version poll, same as `finishInteractiveRoute`, and always returns the
 * tool to Select (KiCad's own `InlineDrag` is a one-shot action, not a
 * persistently-armed tool the way the route tool's `X` is). A click where the
 * drag cannot be resolved commits the last position that could be
 * (`DRAGGER::FixRoute`); `forceCommit` (Ctrl+click) commits the violation of a
 * drag that only marks obstacles. */
export async function finishInlineDrag(x: number, y: number, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">, forceCommit = false): Promise<void> {
  const reply = (await routeDragFinish(x, y, forceCommit)) as CmdReply & { dragging?: boolean };
  if (!reply.ok) dispatch({ type: "TOAST", message: reply.message || "Can't drop it there -- still colliding.", kind: "error" });
  // `performDragging`: a click `FixRoute` refuses leaves the drag going (`if( m_router->FixRoute( .. ) ) break;`)
  if (!reply.ok && reply.dragging) return;
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  await api.refresh();
}
