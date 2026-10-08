// Re-dispatching a mouse event on the live canvas, for the actions that stand in for the warped pointer.
import type { EditorAdapter } from "./editorAdapter";
import { worldToScreen } from "../kicad-port/view";
import { flipLocalX } from "../kicad-port/boardControl";

/** The canvas of whichever editor is on screen (they all use this class; only one is mounted at a time). */
export function canvasRect(): DOMRect | null {
  return document.querySelector(".pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

/**
 * Stand-in for `m_toolMgr->ProcessEvent( TC_MOUSE ... )` after the pointer is warped: re-dispatches a mouse event on the live canvas
 * at a world position, so every tool that follows the pointer (move preview, route and wire rubber bands, a lasso, a context menu)
 * sees the keyboard cursor exactly as it would a real mouse at that spot.
 */
export function emitCanvasEvent(adapter: EditorAdapter, type: "pointermove" | "contextmenu" | "dblclick", world: { x: number; y: number }): void {
  const el = document.querySelector(".pcb-canvas-container canvas") ?? document.querySelector(".pcb-canvas-container");
  const rect = canvasRect();
  if (!el || !rect) return;
  const [sx, sy] = worldToScreen(adapter.view, world.x, world.y);
  const init = { bubbles: true, cancelable: true, composed: true, clientX: rect.left + flipLocalX(adapter.flipped, rect.width, sx), clientY: rect.top + sy };
  if (type === "contextmenu") el.dispatchEvent(new MouseEvent("contextmenu", { ...init, button: 2, buttons: 2 }));
  else if (type === "dblclick") el.dispatchEvent(new MouseEvent("dblclick", { ...init, button: 0, detail: 2 }));
  else el.dispatchEvent(new PointerEvent("pointermove", { ...init, button: 0, buttons: 0, pointerId: 1, pointerType: "mouse", isPrimary: true }));
}
