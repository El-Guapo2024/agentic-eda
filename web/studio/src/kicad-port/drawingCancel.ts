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
