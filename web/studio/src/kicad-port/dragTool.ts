// Pure glue between the backend's inline-drag session (crates/pns::dragger,
// driven through crates/cli/src/route_api.rs's drag_{start,move,finish}
// endpoints) and this app's `DrawState` -- the `D` counterpart of
// routeTool.ts's route glue, same split: this module turns a `DragPreview`
// reply into the "drag" DrawState patch Canvas.tsx's painter already knows
// how to draw, and stays on the dependency-free side of the line every
// other kicad-port module is already on (no React/DOM/JSX, no import of
// state/store.tsx -- see routeTool.ts's own header comment on why). The
// request-guard/move-throttle this needs are the exact same ones
// routeTool.ts already exports (route and drag sessions are mutually
// exclusive -- never both active at once -- so Canvas.tsx reuses one pair
// of refs for both).
//
// router_tool.cpp's InlineDrag loop this ports (the state machine, not the
// PNS algorithm itself -- that's crates/pns::dragger):
//   D (nothing selected, or a single track/via already selected)
//                  -> grab whatever track segment/corner or via is under
//                     the cursor (components/canvas/dragging.ts's
//                     `findDraggableAt`, approximating
//                     `ACTIONS::selectionCursor` + `NeighboringSegmentFilter`)
//   mouse move     -> live preview (shove/mark-obstacles already resolved
//                     server-side, same as the route tool)
//   click          -> fix the drag at the cursor (one commit -- a drag has
//                     no multi-leg concept the way a route does); over a position
//                     the drag cannot be resolved the last valid one is committed
//                     (`DRAGGER::FixRoute`), and Ctrl+click commits a violation of
//                     a drag that only marks obstacles
//   Esc            -> cancel, nothing committed
//
// `D` with footprints selected (or a footprint under the cursor) is `COMPONENT_DRAGGER` (`dragKind: "component"`): the footprints follow the cursor
// -- held on the anchor of a single one, where KiCad warps the pointer, and on the wire grid (`updateEndItem` with no net: `snapToItem( nullptr )`)
// -- and every track on their pads follows them (`fanout`). `D` with several tracks selected is `MULTI_DRAGGER` (`"multi"`). Both are `InlineDrag`'s:
// a click before the cursor has moved does nothing, and a click the router refuses ends the drag with nothing committed (`refused`).
import { previewClearancesOf, type PreviewClearances } from "./pcbDisplayOptions";
import type { BoardState, DragPreview } from "../api/types";
import { itemKind, padParent } from "./pcbItems";
import { isLocked } from "./pcbTransform";

/** Mirrors `state/store.tsx`'s `DrawState`'s "drag" variant exactly --
 * duplicated rather than imported, same convention `RouteDrawState` already
 * uses (see routeTool.ts). store.tsx is the source of truth for the real
 * shape; keep this in sync with it by hand. */
export interface DragDrawState {
  kind: "drag";
  /** `"corner"`: `pts` is the whole stretched line, one vertex following the
   * cursor (`DM_CORNER`). `"segment"`: `pts` is the whole line with one segment
   * slid sideways and its neighbours turned or stretched to follow (`DM_SEGMENT`).
   * `"via"`: `pts` is just the via's own live position (one point) -- its attached
   * tracks' live shape is `fanout`, not `pts`. */
  dragKind: "corner" | "segment" | "via" | "arc" | "component" | "multi";
  /** Not echoed by `DragPreview` -- captured once from the board at
   * `drag_start` and kept across every subsequent preview, same as
   * `RouteDrawState.net`. */
  net: string | null;
  layer: string;
  /** Corner drag only (the dragged track's own width); 0 for a via drag. */
  width: number;
  /** Via drag only -- the via's own diameter, for drawing it at its live
   * position (DragPreview carries no size of its own, see that type's doc). */
  viaDiameter?: number;
  /** A via drag of a virtual via (`VVIA`: where tracks meet with no pad or via, `DRAGGER::checkVirtualVia`): the junction is dragged, and no via is drawn. */
  virtualVia?: boolean;
  pts: [number, number][];
  colliding?: boolean;
  displaced?: { layer: string; pts: [number, number][] }[];
  displacedVias?: { source_via: string; x: number; y: number }[];
  /** The board ids of the tracks of the line being dragged (`DRAGGER::GetOriginalLine`): the cursor does not snap onto them (`TOOL_BASE::checkSnap`). Captured at `drag_start`. */
  lineTracks?: string[];
  /** The cursor is where the drag cannot be resolved: `pts` is the last position that could be, and a click commits that one (`DRAGGER::Drag`, "Restore last solution"). */
  restored?: boolean;
  /** The first position failed, so the drag only marks obstacles; a violation is committed with Ctrl+click (`GetForceMarkObstaclesMode`). */
  forcedMark?: boolean;
  /** Via drag only: the via's own directly-attached tracks, already
   * stretched to follow it live -- see `DragPreview.fanout`'s own doc. */
  fanout?: { layer: string; width: number; pts: [number, number][] }[];
  /** The clearances of what the preview shows, for the clearance outlines (kicad-port/pcbDisplayOptions.ts `PreviewClearances`, UI-LEFT-3 P3-6). */
  clearances?: PreviewClearances;
  /** `"component"` (`COMPONENT_DRAGGER`) only: the footprints dragged (drawn moved by `offset`), how far they have moved (`aP - m_p0`), where the
   * drag started (`start`: the footprint's anchor, onto which KiCad warps the cursor) and where the pointer was at `D` (`grabbed`). `fanout` is then
   * the lines that follow the pads and the tracks that move with them; for `"multi"` it is the dragged lines. */
  footprints?: string[];
  offset?: [number, number] | null;
  start?: [number, number];
  grabbed?: [number, number];
  /** `"component"` and `"multi"`: the cursor has moved since `D` (`InlineDrag`'s `hasMouseMoved`); a click before that does nothing. */
  moved?: boolean;
  /** `"multi"`: the dragged lines' nets (`MULTI_DRAGGER::CurrentNets`), the only ones the cursor snaps onto. */
  nets?: string[];
  /** `InlineDrag` (`D` from the selection tool): the selection as it was when the drag started (`const PCB_SELECTION selection`), which the drag
   * cleared (`ACTIONS::selectionClear`) and which Esc puts back (`restoreSelection( selection )`). Absent for the router tool's own drag
   * (`performDragging`), which leaves the selection alone. */
  restoreSelection?: string[];
  /** The nets highlighted when the drag ends (`highlightNets( false )`: `m_startHighlightNetcodes`, see `dragHighlight`). Absent: the drag did not
   * change the highlight. */
  restoreHighlight?: string[];
}

/**
 * `TOOL_BASE::highlightNets( true, nets )` at the start of a drag (pcbnew/router/pns_tool_base.cpp at 8303b2ad), with `current` the nets highlighted
 * now: the drag shows `nets` highlighted (`rs->SetHighlight( netcodes, true )`), and at its end (`highlightNets( false )`) the highlight goes back
 * to `current` when it already held one of them ("If the user has previously set some of the routed nets to be highlighted, we assume they want
 * to keep them highlighted after routing"), else off. `null` when `nets` is empty (`if( closestItem->Net() )`: no call). **Differs:** with no net
 * KiCad's `highlightNets( false )` at the end still puts back `m_startHighlightNetcodes` -- whatever an earlier route left there -- where the
 * studio leaves the highlight as it was.
 */
export function dragHighlight(current: readonly string[], nets: readonly string[]): { show: string[]; restore: string[] } | null {
  const shown = [...new Set(nets.filter((n) => n !== ""))];
  if (shown.length === 0) return null;
  const keep = current.some((n) => shown.includes(n));
  return { show: shown, restore: keep ? [...current] : [] };
}

/** What `ROUTER_TOOL::InlineDrag` keeps of the editor as the drag starts (pcbnew/router/router_tool.cpp at 8303b2ad): the selection (`const
 * PCB_SELECTION selection`, which `ACTIONS::selectionClear` then empties and Esc puts back) and the nets highlighted now (`highlightNets`). */
export interface InlineDragContext {
  selection: readonly string[];
  highlighted: readonly string[];
}

/** `InlineDrag` once the router has started the drag: whether the selection is cleared (`m_toolMgr->RunAction( ACTIONS::selectionClear )`, the
 * selection tool's drag; `clearsSelection`), the nets to highlight now (`highlightNets( true, { closestItem->Net() } )`, `net`), and the draw
 * state with what the end puts back. Without `context` nothing changes. */
export function inlineDragBegin(draw: DragDrawState, context: InlineDragContext | undefined, clearsSelection: boolean, net: string | null | undefined): { draw: DragDrawState; clearSelection: boolean; highlight: string[] | null } {
  if (!context) return { draw, clearSelection: false, highlight: null };
  const highlight = dragHighlight(context.highlighted, net ? [net] : []);
  return { draw: { ...draw, restoreSelection: clearsSelection ? [...context.selection] : undefined, restoreHighlight: highlight?.restore }, clearSelection: clearsSelection, highlight: highlight?.show ?? null };
}

/** The end of an `InlineDrag` (Esc, or the click that ends it): the nets highlighted again (`highlightNets( false )`, `null`: unchanged), and the
 * selection to make (`null`: none) -- `select` (`else if( leaderSegments.size() )`: `selectItems` of the leader segments' tracks), or with Esc the
 * one the drag cleared (`if( itemsToDrag.Size() && hasMultidragCancelled ) restoreSelection( selection )`). A drag that is not the selection tool's
 * (`restoreSelection` absent) leaves the selection alone; an empty one stays clear. */
export function inlineDragEnd(draw: DragDrawState | null | undefined, how: { cancelled: true } | { cancelled: false; select?: readonly string[] }): { highlight: string[] | null; selection: string[] | null } {
  if (!draw) return { highlight: null, selection: null };
  const highlight = draw.restoreHighlight ? [...draw.restoreHighlight] : null;
  if (!draw.restoreSelection) return { highlight, selection: null };
  const next = how.cancelled ? draw.restoreSelection : [...(how.select ?? [])];
  return { highlight, selection: next.length > 0 ? next : null };
}

/** The `DrawState` "drag" patch for a successful `DragPreview` reply,
 * keeping whatever session-local fields (`dragKind`, `net`, `layer`,
 * `width`, `viaDiameter`) the server doesn't echo back -- same shape as
 * `routeTool.ts`'s `drawStateFromPreview`. */
export function dragStateFromPreview(current: DragDrawState, preview: DragPreview): DragDrawState {
  return {
    ...current,
    offset: preview.offset ?? current.offset ?? null,
    moved: true,
    pts: preview.pts,
    colliding: preview.colliding,
    restored: preview.restored ?? false,
    forcedMark: preview.forced_mark_obstacles ?? false,
    displaced: preview.displaced,
    displacedVias: preview.displaced_vias,
    fanout: preview.fanout,
    clearances: previewClearancesOf(preview),
  };
}

/** What `D` drags when it is not one track or via: the footprints of the selection (`COMPONENT_DRAGGER`) or two or more tracks (`MULTI_DRAGGER`). */
export type GroupDragTarget = { kind: "footprints"; refs: string[] } | { kind: "tracks"; ids: string[] };

/**
 * `EDIT_TOOL::Drag`'s `RequestSelection` filter and `ROUTER_TOOL::CanInlineDrag`, for the selection drags (pcbnew/tools/edit_tool.cpp, router_tool.cpp):
 *   * a selection that is not the hover one and holds a footprint keeps its footprints only ("Remove non-footprints so box-selection will drag
 *     footprints"); with nothing selected, the footprint under the cursor (a pad stands for it: `FilterCollectorForFreePads`) is the hover selection;
 *   * locked items are dropped (`FilterCollectorForLockedItems`): `lockedOut` says so (`ReportFilteredLockedItems`);
 *   * footprints drag together, but not freely ("Footprints cannot be dragged freely": `G` on more than one does nothing -- one footprint is a
 *     `DraggableItems` item, so `G` on one drags it as `D` does);
 *   * two or more tracks, all of them tracks (`CountType( PCB_TRACE_T ) == Size()`: an arc track or a via in the selection refuses it), drag together.
 * `null` when the selection is none of these: `D` then drags the one track or via under the cursor (`findDraggableAt`).
 */
export function findGroupDragTarget(board: BoardState, selection: readonly string[], hovered: string | null, freeAngle: boolean): { target: GroupDragTarget | null; lockedOut: boolean } {
  const isHover = selection.length === 0;
  const items = isHover ? (hovered ? [hovered] : []) : [...selection];
  const asItem = (id: string) => (itemKind(board, id) === "pad" ? (padParent(board, id) ?? id) : id);
  const ids = [...new Set(items.map(asItem))];
  const footprints = ids.filter((id) => itemKind(board, id) === "part");
  if (footprints.length > 0 && (!isHover || ids.length === 1)) {
    const refs = footprints.filter((r) => !isLocked(board, r));
    const lockedOut = refs.length < footprints.length;
    if (refs.length === 0) return { target: null, lockedOut };
    if (freeAngle && refs.length > 1) return { target: null, lockedOut };
    return { target: { kind: "footprints", refs }, lockedOut };
  }
  if (!isHover && ids.length > 1) {
    const unlocked = ids.filter((id) => !isLocked(board, id));
    const tracks = unlocked.filter((id) => itemKind(board, id) === "track" && !board.routing?.tracks.find((t) => t.id === id)?.arc_mid);
    if (tracks.length > 1 && tracks.length === unlocked.length) return { target: { kind: "tracks", ids: tracks }, lockedOut: unlocked.length < ids.length };
  }
  return { target: null, lockedOut: false };
}
