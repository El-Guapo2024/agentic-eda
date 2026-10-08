// `common.Control.zoomTool` (Ctrl+F5) -- common/tool/zoom_tool.cpp's
// ZOOM_TOOL::Main/selectRegion. While the tool is armed (state.activeTool ===
// "zoom_area") this transparent layer sits over the canvas, takes the drag,
// shows the selection rectangle (KIGFX::PREVIEW::SELECTION_AREA) and, on
// release, applies kicad-port/cursorControl.ts's zoomToAreaView: left button
// zooms in so the box fills the screen, right button zooms out by the same
// ratio, then the tool exits (Main()'s `break`). A click with no drag (a
// zero-size box) just exits, Escape cancels (ESCAPE reducer -> select tool).
//
// Mounted once from App.tsx; it measures the live `.pcb-canvas-container`
// (the PCB canvas and the schematic view share that class and only one is
// mounted at a time) rather than owning a canvas ref.
import { useRef, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { screenToWorld } from "../kicad-port/view";
import { zoomToAreaView } from "../kicad-port/cursorControl";
import { flipLocalX } from "../kicad-port/boardControl";

interface Drag {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  button: number;
}

export function ZoomAreaOverlay() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);

  const armed = state.activeTool === "zoom_area" && (state.tab === "pcb" || state.tab === "schematic");
  if (!armed) return null;
  const container = document.querySelector(".pcb-canvas-container");
  const rect = container?.getBoundingClientRect();
  if (!rect) return null;

  const isSch = state.tab === "schematic";
  const view = isSch ? state.schematicView : state.view;

  const finish = (d: Drag) => {
    // The Flip Board View mirror (pcbnew.Control.flipBoard): the box is dragged on the mirrored picture.
    const flipped = !isSch && state.bcx.boardFlipped;
    const a = screenToWorld(view, flipLocalX(flipped, rect.width, d.x0 - rect.left), d.y0 - rect.top);
    const b = screenToWorld(view, flipLocalX(flipped, rect.width, d.x1 - rect.left), d.y1 - rect.top);
    const next = zoomToAreaView(view, rect.width, rect.height, { x: a[0], y: a[1] }, { x: b[0], y: b[1] }, d.button === 2);
    if (next) dispatch(isSch ? { type: "SET_SCHEMATIC_VIEW", view: next } : { type: "SET_VIEW", view: next });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  };

  const box = drag && { left: Math.min(drag.x0, drag.x1), top: Math.min(drag.y0, drag.y1), width: Math.abs(drag.x1 - drag.x0), height: Math.abs(drag.y1 - drag.y0) };

  return (
    <div
      style={{ position: "fixed", left: rect.left, top: rect.top, width: rect.width, height: rect.height, zIndex: 50, cursor: "zoom-in", touchAction: "none" }}
      onContextMenu={(e) => e.preventDefault()}
      onPointerDown={(e) => {
        if (e.button !== 0 && e.button !== 2) return;
        try {
          (e.target as Element).setPointerCapture(e.pointerId);
        } catch {
          /* synthetic pointer */
        }
        const d = { x0: e.clientX, y0: e.clientY, x1: e.clientX, y1: e.clientY, button: e.button };
        dragRef.current = d;
        setDrag(d);
      }}
      onPointerMove={(e) => {
        const d = dragRef.current;
        if (!d) return;
        const next = { ...d, x1: e.clientX, y1: e.clientY };
        dragRef.current = next;
        setDrag(next);
      }}
      onPointerUp={(e) => {
        const d = dragRef.current;
        dragRef.current = null;
        setDrag(null);
        if (d) finish({ ...d, x1: e.clientX, y1: e.clientY });
      }}
    >
      {box && <div style={{ position: "fixed", ...box, border: "1px solid #4aa3ff", background: "rgba(74,163,255,0.12)", pointerEvents: "none" }} />}
    </div>
  );
}
