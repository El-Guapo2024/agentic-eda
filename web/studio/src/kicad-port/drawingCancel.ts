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
  const polygon = drawState.kind === "zone" || (drawState.kind === "shape" && drawState.shapeKind === "polygon");
  return polygon && (drawState.pts?.length ?? 0) > 0;
}

/** `POLYGON_GEOM_MANAGER::DeleteLastCorner`: the polygon without its last corner, or null (`cleanup()`) when no corner is left. */
export function withoutLastCorner<T extends { pts: readonly unknown[] }>(draw: T): T | null {
  return draw.pts.length <= 1 ? null : { ...draw, pts: draw.pts.slice(0, -1) };
}

/**
 * Undo with a board item being drawn: `drawShape` (lines, rectangles, circles, arcs, Béziers), `DrawDimension` and `DrawTable` drop the item as Escape does
 * (`started && evt->IsAction( &ACTIONS::undo )`); `DrawZone` is [`drawZoneStarted`]'s; the measure tool (`PCB_VIEWER_TOOLS::MeasureTool`) handles only
 * `IsCancelInteractive()`, so there Undo reaches the frame and undoes the board.
 */
export function undoDropsDrawing(tab: string, activeTool: string, drawState: DrawLike | null | undefined): boolean {
  return escapeKeepsDrawingTool(tab, activeTool, drawState) && drawState!.kind !== "measure" && !drawZoneStarted(tab, activeTool, drawState);
}
