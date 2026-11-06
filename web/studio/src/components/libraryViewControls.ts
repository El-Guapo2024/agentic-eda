// The view half of `WX_VIEW_CONTROLS` (common/view/wx_view_controls.cpp) for the two library editors' canvases (FootprintCanvas.tsx, SymbolEditorCanvas.tsx),
// one copy for both -- the board (canvas/Canvas.tsx) and the schematic (SchematicView.tsx) do the same in their own handlers:
//
//   onButton   a middle or right press starts DRAG_PANNING or DRAG_ZOOMING as Preferences > Mouse and Touchpad's "Middle button drag:" / "Right button drag:" say, or
//              nothing ("None"); a right press that never becomes a drag is a click, and the context menu opens (`justPanned` says when it must not);
//   onMotion   the pan follows the pointer, the zoom scales about the press point by `exp( dy * zoomSpeed * 0.001 )`; with the key of "Pan on mouse movement with
//              key:" held the view follows the pointer (`m_motionPanModifier`);
//   handleAutoPanning / onTimer   with "Automatically pan while moving object" on, while a tool is busy -- an item carried, moved or being drawn, the selection
//              rectangle dragged -- and the pointer is in the border of the canvas, the view pans every frame (the frame loop stands in for the 250/60 ms
//              one-shot timer), and the last pointer position is replayed so what is held follows (`refreshMouse`).
import { useEffect, useRef, type RefObject } from "react";
import { computeAutoPanDirection, computeAutoPanStep, DEFAULT_VIEW_CONTROL_SETTINGS, dragActionOf, dragZoomView, motionPan, type ViewControlSettings } from "../kicad-port/viewControls";
import { panByWorldDelta, type ViewTransform } from "../kicad-port/view";
import { isMac } from "../platform";

type ViewDrag = { kind: "pan"; button: 1 | 2; startScreen: [number, number]; startView: [number, number]; moved: boolean } | { kind: "zoom"; button: 1 | 2; anchor: { x: number; y: number }; lastY: number; moved: boolean };

export interface LibraryViewControlsOptions {
  containerRef: RefObject<HTMLDivElement | null>;
  view: ViewTransform;
  setView: (view: ViewTransform) => void;
  settings: ViewControlSettings;
  /** `input.zoom_speed`, what a Zoom drag reads (`m_settings.m_zoomSpeed`). */
  zoomSpeed: number;
  /** `input.auto_pan` and `input.auto_pan_acceleration`. */
  autoPan: boolean;
  autoPanAcceleration: number;
  /** A tool is busy and would pan at the border (`VIEW_CONTROLS::SetAutoPan( true )` of the tool running). */
  busy: () => boolean;
  /** The canvas is on screen (its tab is the open one). */
  shown: boolean;
  /** The user moved the view by hand (no fit to the content follows). */
  onUserMoved: () => void;
}

export interface LibraryViewControls {
  /** A press: true when it is the middle or the right button's (handled here, or ignored for "None"). */
  press(e: React.PointerEvent): boolean;
  /** A motion (`dragging`: the press has become a drag, tool_dispatcher.cpp's rule): true when a pan or zoom drag took it. Records the pointer for the auto-pan and
   * runs the pan on movement with a key either way. */
  move(e: React.PointerEvent, dragging: boolean): boolean;
  /** The release of a press. */
  release(): void;
  /** A right drag just ended: the `contextmenu` that follows it opens nothing (asked once). */
  takeJustPanned(): boolean;
}

export function useLibraryViewControls(o: LibraryViewControlsOptions): LibraryViewControls {
  const live = useRef(o);
  live.current = o;
  const drag = useRef<ViewDrag | null>(null);
  const justPanned = useRef(false);
  const metaPanStart = useRef<{ x: number; y: number } | null>(null);
  const lastPointer = useRef<{ x: number; y: number; clientX: number; clientY: number; buttons: number } | null>(null);
  const autoPanned = useRef(false);

  // `onTimer`'s AUTO_PANNING: one frame loop, reading what it needs from `live`
  useEffect(() => {
    let raf = 0;
    const tick = () => {
      raf = requestAnimationFrame(tick);
      const c = live.current;
      const el = c.containerRef.current;
      const p = lastPointer.current;
      if (!c.autoPan || !c.shown || !el || !p || drag.current || !(c.view.scale > 0) || !c.busy()) return;
      const rect = el.getBoundingClientRect();
      const size = { width: rect.width, height: rect.height };
      const dir = computeAutoPanDirection(p, size, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin);
      const step = computeAutoPanStep(dir, size, c.view.scale, DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin, c.autoPanAcceleration);
      if (!step) return;
      c.onUserMoved();
      autoPanned.current = true;
      c.setView(panByWorldDelta(c.view, step.x, step.y));
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);
  // After a pan step the pointer is over another place of the drawing: replay its last position so what is held or drawn follows (`refreshMouse`).
  useEffect(() => {
    if (!autoPanned.current) return;
    autoPanned.current = false;
    const p = lastPointer.current;
    const el = live.current.containerRef.current;
    if (!p || !el) return;
    el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: p.clientX, clientY: p.clientY, pointerId: 1, pointerType: "mouse", buttons: p.buttons }));
  }, [o.view]);

  return {
    press(e) {
      if (e.button !== 1 && e.button !== 2) return false;
      justPanned.current = false;
      const c = live.current;
      const action = dragActionOf(e.button, c.settings);
      if (action === "pan") drag.current = { kind: "pan", button: e.button, startScreen: [e.clientX, e.clientY], startView: [c.view.x, c.view.y], moved: false };
      else if (action === "zoom") {
        const rect = c.containerRef.current!.getBoundingClientRect();
        drag.current = { kind: "zoom", button: e.button, anchor: { x: e.clientX - rect.left, y: e.clientY - rect.top }, lastY: e.clientY, moved: false };
      } else drag.current = null;
      return true;
    },
    move(e, dragging) {
      const c = live.current;
      const rect = c.containerRef.current?.getBoundingClientRect();
      if (rect) lastPointer.current = { x: e.clientX - rect.left, y: e.clientY - rect.top, clientX: e.clientX, clientY: e.clientY, buttons: e.buttons };
      const meta = motionPan(c.view, c.settings.motionPanModifier, { altKey: e.altKey, ctrlOrCmd: isMac() ? e.metaKey : e.ctrlKey, shiftKey: e.shiftKey }, metaPanStart.current, { x: e.clientX, y: e.clientY });
      metaPanStart.current = meta.start;
      if (meta.view) {
        c.onUserMoved();
        c.setView(meta.view);
      }
      const d = drag.current;
      if (!d) return false;
      c.onUserMoved();
      if (dragging) d.moved = true;
      if (d.kind === "pan") c.setView({ ...c.view, x: d.startView[0] + (e.clientX - d.startScreen[0]), y: d.startView[1] + (e.clientY - d.startScreen[1]) });
      else {
        c.setView(dragZoomView(c.view, d.anchor, e.clientY - d.lastY, c.zoomSpeed));
        d.lastY = e.clientY;
      }
      return true;
    },
    release() {
      const d = drag.current;
      drag.current = null;
      if (d) justPanned.current = d.button === 2 && d.moved;
    },
    takeJustPanned() {
      const was = justPanned.current;
      justPanned.current = false;
      return was;
    },
  };
}
