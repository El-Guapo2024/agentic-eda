// What Escape does in a board drawing tool with a shape started. `DRAWING_TOOL::drawShape` (pcbnew/tools/drawing_tool.cpp):
//
//   if( evt->IsCancelInteractive() || ( started && evt->IsAction( &ACTIONS::undo ) ) )
//   {
//       cleanup();
//       if( !started ) { evt->SetPassEvent( false ); m_frame->PopTool( aTool ); cancelled = true; }
//       break;
//   }
//
// and `DrawLine` loops `while( drawShape( .. ) )`: the first Escape drops the line (rectangle, circle, arc, polygon) being drawn and the tool stays armed;
// the next one, with nothing started, leaves the tool. `DrawZone` (`ZONE_CREATE_HELPER`: `IsPolygonInProgress()`), `DrawDimension`, `DrawTable` and the
// measure tool (`MEASURE` in `pcb_viewer_tools.cpp`: `if( originSet ) clearRuler else PopTool`) do the same.

/** The board drawing tools' in-progress kinds whose Escape cancels the item and keeps the tool. */
const BOARD_DRAWING_KINDS = new Set(["shape", "zone", "dimension", "table", "measure"]);

/** True when Escape should drop the item being drawn and leave the tool armed (rather than leave the tool): a board drawing tool with an item started. */
export function escapeKeepsDrawingTool(tab: string, activeTool: string, drawState: { kind: string } | null | undefined): boolean {
  return tab === "pcb" && activeTool !== "select" && drawState != null && BOARD_DRAWING_KINDS.has(drawState.kind);
}

type DrawLike = { kind: string; shapeKind?: string; pts?: readonly unknown[]; arc?: unknown; bezier?: unknown };

/**
 * A polygon being drawn by `DRAWING_TOOL::DrawZone`, which draws zones, rule areas, cutouts, similar zones and graphic polygons
 * (`ZONE_MODE::GRAPHIC_POLYGON`), with a corner placed (`started`):
 *
 *   else if( started && (   evt->IsAction( &PCB_ACTIONS::deleteLastPoint )
 *                        || evt->IsAction( &ACTIONS::doDelete )
 *                        || evt->IsAction( &ACTIONS::undo ) ) )
 *   {
 *       if( std::optional<VECTOR2I> last = polyGeomMgr.DeleteLastCorner() ) { ... } else { cleanup(); }
 *   }
 *
 * so Delete and Undo remove the last corner, as Backspace does, rather than delete the selection or drop the whole outline.
 */
export function drawZoneStarted(tab: string, activeTool: string, drawState: DrawLike | null | undefined): boolean {
  if (tab !== "pcb" || activeTool === "select" || drawState == null) return false;
  // `started` stays true once the first corner is in, even when Delete has taken every corner away again (see `withoutLastCorner`).
  // A graphic polygon's state exists only from its first corner on, so it is `started` with no corner left too (round 5 A5-6).
  return drawState.kind === "zone" || (drawState.kind === "shape" && drawState.shapeKind === "polygon");
}

/**
 * `POLYGON_GEOM_MANAGER::DeleteLastCorner`: the polygon without its last corner. It answers the corner it removed even when that was the only one, so the
 * tool stays `started` with no corner; only the press after that, with nothing to remove, reaches `cleanup()` (null). (KiCad also warps the cursor onto the
 * removed corner; a browser cannot move the pointer.)
 */
export function withoutLastCorner<T extends { pts: readonly unknown[] }>(draw: T): T | null {
  return draw.pts.length === 0 ? null : { ...draw, pts: draw.pts.slice(0, -1) };
}

/** The shapes `DRAWING_TOOL::drawShape` draws (lines, rectangles, circles); arcs and Béziers have loops of their own (`drawArc`, `drawOneBezier`). */
const DRAW_SHAPE_KINDS = new Set(["segment", "rect", "circle"]);

/**
 * Delete (and Backspace) with a line, rectangle or circle started is `drawShape`'s:
 *
 *   else if( started && ( evt->IsAction( &PCB_ACTIONS::doDelete ) || evt->IsAction( &PCB_ACTIONS::deleteLastPoint ) ) )
 *
 * so it never reaches the editor's Delete (the selection, which the first click cleared anyway). An arc or a Bézier handles only Backspace.
 */
export function drawShapeStarted(tab: string, activeTool: string, drawState: DrawLike | null | undefined): boolean {
  if (tab !== "pcb" || activeTool === "select" || drawState?.kind !== "shape") return false;
  return DRAW_SHAPE_KINDS.has(drawState.shapeKind ?? "") && (drawState.pts?.length ?? 0) > 0;
}

type LineDraw = { kind: string; shapeKind?: string; pts: [number, number][]; committed?: [[number, number], [number, number]][] };

/**
 * What that Delete or Backspace does: with a segment of `DrawLine`'s chain already on the board (`aCommittedGraphics` not empty), that segment is taken back
 * -- popped off the chain, its "Draw Line" commit undone (`PopCommandFromUndoList`, `PutDataInPreviousState`) -- and the line in progress starts again from
 * its start (`twoPointMgr.SetOrigin( top->GetStart() )`); otherwise the shape in progress goes (`cleanup()`) and the tool stays.
 */
export function takeBackLastLine<T extends LineDraw>(draw: T): { draw: T | null; undo: boolean } {
  const committed = draw.committed ?? [];
  const top = committed[committed.length - 1];
  if (draw.shapeKind !== "segment" || !top) return { draw: null, undo: false };
  return { draw: { ...draw, pts: [top[0]], committed: committed.slice(0, -1) }, undo: true };
}

/**
 * Redo, and the zone fill actions (`ZONE_FILLER_TOOL::IsZoneFillAction`: Fill, Fill All, Unfill, Unfill All), with an item started in a drawing tool:
 *
 *   else if( started && ( ZONE_FILLER_TOOL::IsZoneFillAction( evt ) || evt->IsAction( &ACTIONS::redo ) ) )
 *       wxBell();
 *
 * in `drawShape`, `drawArc`, `drawOneBezier`, `DrawZone`, `DrawDimension`, `DrawTable` and the image and text placers. The measure tool has no such branch.
 */
export function redoRefused(tab: string, activeTool: string, drawState: DrawLike | null | undefined): boolean {
  return escapeKeepsDrawingTool(tab, activeTool, drawState) && drawState!.kind !== "measure";
}

/**
 * Undo with a board item being drawn: `drawShape` (lines, rectangles, circles, arcs, Béziers), `DrawDimension` and `DrawTable` drop the item as Escape does
 * (`started && evt->IsAction( &ACTIONS::undo )`); `DrawZone` is [`drawZoneStarted`]'s; the measure tool (`PCB_VIEWER_TOOLS::MeasureTool`) handles only
 * `IsCancelInteractive()`, so there Undo reaches the frame and undoes the board.
 */
export function undoDropsDrawing(tab: string, activeTool: string, drawState: DrawLike | null | undefined): boolean {
  return escapeKeepsDrawingTool(tab, activeTool, drawState) && drawState!.kind !== "measure" && !drawZoneStarted(tab, activeTool, drawState);
}
