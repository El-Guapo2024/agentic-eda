// The pointer half of the shared tools, for every editor's canvas (the preview half is components/CommonOverlay.tsx).
//
//   common/tool/picker_tool.cpp          PICKER_TOOL::Main -- a left click answers the running picker session, motion feeds its motion handler
//   pcbnew/tools/pcb_selection_tool.cpp  PCB_SELECTION_TOOL::SelectPolyArea, eeschema/tools/sch_selection_tool.cpp selectLasso -- the lasso
//
// The board canvas answers a picker session itself (Canvas.tsx calls `picker.click`); the schematic, the footprint editor and the symbol editor
// do not, so a capture-phase listener on the canvas column answers it for them -- before the canvas sees the press, exactly as the picker tool,
// sitting on top of the tool stack, gets the event first. The same listener runs the lasso: with the selection mode on "lasso", a left drag that
// starts on empty space (or with Shift held) draws a polygon instead of the rectangle; the polygon grows with the drag and with further clicks,
// a double click or End closes it and selects what it hits (clockwise = items fully inside, counterclockwise = items it touches), Escape drops it.
import { useEffect, useRef } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { useFpApi, useFpDispatch, useFpState } from "../state/footprintEditorStore";
import { useSymApi, useSymDispatch, useSymState } from "../state/symbolEditorStore";
import { useCommonOptions, type CommonOptions } from "../state/commonOptions";
import { closeSelectionMenu, getCommonTool, setLasso, setLastPointer, setPickerHover, useCommonTool } from "../state/commonTool";
import { picker, usePickerSession } from "../actions/pcbPicker";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { isCanvasTab, makeEditorAdapter, type EditorAdapter } from "../actions/editorAdapter";
import { screenToWorld } from "../kicad-port/view";
import { flipLocalX } from "../kicad-port/boardControl";
import { alignToGrid } from "../kicad-port/gridSnap";
import { getSnapOrigin } from "./canvas/gridHelper";
import { appendLassoPoint, applyAreaSelection, lassoContained, lassoHits } from "../kicad-port/lasso";

/** A press that moves less than this many pixels is a click, not the start of a drag (KiCad starts a drag after 8 px of travel on most platforms; a lasso uses the same threshold). */
const DRAG_THRESHOLD_PX = 8;
/** The lasso's freehand samples closer than this many pixels are dropped. */
const LASSO_SAMPLE_PX = 3;

interface Latest {
  adapter: EditorAdapter | null;
  opts: CommonOptions;
}

export function CommonToolHost() {
  const studio = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const fp = useFpState();
  const fpDispatch = useFpDispatch();
  const fpApi = useFpApi();
  const sym = useSymState();
  const symDispatch = useSymDispatch();
  const symApi = useSymApi();
  const opts = useCommonOptions();
  const latest = useRef<Latest>({ adapter: null, opts });
  latest.current = {
    adapter: isCanvasTab(studio.tab) ? makeEditorAdapter({ tab: studio.tab, studio, fp, sym, dispatch, fpDispatch, symDispatch, api, fpApi, symApi }) : null,
    opts,
  };

  // The pointer shown over the canvas while a picker that asks for one runs (`SetCursor( KICURSOR::REMOVE )` of the delete tool).
  const session = usePickerSession();
  useEffect(() => {
    const el = document.querySelector<HTMLElement>(".pcb-canvas-container");
    if (!el) return;
    el.style.cursor = session?.cursor === "remove" ? "not-allowed" : "";
    return () => {
      el.style.cursor = "";
    };
  }, [session, studio.tab]);

  // Where the pointer is, for a menu an action opens (`GetMousePosition()`).
  useEffect(() => {
    const onMove = (e: PointerEvent) => setLastPointer(e.clientX, e.clientY);
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  // The clarification menu of `ACTIONS::selectionMenu`: "1  Footprint U1", "2  Track GND on F.Cu", ..., then "Select All".
  const menu = useCommonTool().menu;

  useEffect(() => {
    const col = document.querySelector<HTMLElement>(".canvas-col");
    if (!col) return;

    /** A press that may still turn into a lasso drag, or an empty-space click. */
    let press: { id: number; x: number; y: number; world: [number, number]; additive: boolean; subtractive: boolean; started: boolean } | null = null;

    const container = (e: Event) => (e.target as Element | null)?.closest?.(".pcb-canvas-container") as HTMLElement | null;
    const toWorld = (a: EditorAdapter, c: HTMLElement, e: MouseEvent): [number, number] => {
      const rect = c.getBoundingClientRect();
      return screenToWorld(a.view, flipLocalX(a.flipped, rect.width, e.clientX - rect.left), e.clientY - rect.top);
    };
    /** The click answered with the grid-snapped point and, for an item session, the item under it (`SelectPointInteractively` / `SelectItemInteractively`). */
    const answerPicker = (a: EditorAdapter, w: [number, number]) => {
      const p = alignToGrid({ x: w[0], y: w[1] }, a.gridUm, getSnapOrigin(), { ctrlOrCmd: false });
      picker.click({ point: { x: p.x, y: p.y }, item: () => a.candidatesAt(w[0], w[1])[0]?.id ?? null });
    };
    /** The lasso as it stands, closed to the cursor, selects what it hits (`SelectMultiple`). */
    const finishLasso = (a: EditorAdapter) => {
      const lasso = getCommonTool().lasso;
      if (!lasso) return;
      const poly = [...lasso.pts, ...(a.cursor ? ([[a.cursor.x, a.cursor.y]] as [number, number][]) : [])];
      setLasso(null);
      if (poly.length < 3) return;
      const hits = lassoHits(a.itemBoxes(), poly, lassoContained(poly));
      const mode = lasso.subtractive ? "subtract" : lasso.additive ? "add" : "set";
      a.setSelection(applyAreaSelection([...a.selection], hits, mode));
    };

    const onPointerDown = (e: PointerEvent) => {
      const a = latest.current.adapter;
      const c = container(e);
      if (!a || !c || (e.target as Element).closest(".menubar-dropdown")) return;
      const w = toWorld(a, c, e);

      // A picker session on an editor whose canvas does not answer it: the click is the picker's.
      if (picker.session() && a.tab !== "pcb") {
        e.stopPropagation();
        e.preventDefault();
        if (e.button === 0) answerPicker(a, w);
        else if (e.button === 2) picker.cancel();
        return;
      }
      if (e.button !== 0) return;

      // The lasso: a click adds a point to the one being drawn; a press on empty space (or with Shift) may start one.
      const lasso = getCommonTool().lasso;
      if (lasso) {
        e.stopPropagation();
        e.preventDefault();
        setLasso({ ...lasso, pts: appendLassoPoint(lasso.pts, w, 0), dragging: true });
        try {
          c.setPointerCapture(e.pointerId);
        } catch {
          /* synthetic pointer */
        }
        return;
      }
      if (latest.current.opts.selectionMode !== "lasso" || !a.toolIdle || picker.session()) return;
      const hits = a.candidatesAt(w[0], w[1]).filter((h) => h.kind !== "zone" || a.selection.has(h.id)); // "Don't allow starting a drag from a zone filled area that isn't already selected"
      if (hits.length > 0 && !e.shiftKey) return;
      e.stopPropagation();
      e.preventDefault();
      press = { id: e.pointerId, x: e.clientX, y: e.clientY, world: w, additive: e.shiftKey && !(e.ctrlKey || e.metaKey), subtractive: e.shiftKey && (e.ctrlKey || e.metaKey), started: false };
      try {
        c.setPointerCapture(e.pointerId);
      } catch {
        /* synthetic pointer */
      }
    };

    const onPointerMove = (e: PointerEvent) => {
      const a = latest.current.adapter;
      const c = container(e);
      if (!a || !c) return;
      const s = picker.session();
      if (s?.hover) {
        const w = toWorld(a, c, e);
        setPickerHover(s.hover({ x: w[0], y: w[1] }));
      } else if (getCommonTool().hover !== null) setPickerHover(null);

      // A drag that began on empty space becomes the lasso once it has travelled far enough (`evt->IsDrag( BUT_LEFT )`).
      if (press && e.pointerId === press.id && e.buttons & 1) {
        const w = toWorld(a, c, e);
        if (!press.started) {
          if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_THRESHOLD_PX) return;
          press.started = true;
          // a drag without a modifier drops the selection first (`ClearSelection( true )` when the lasso gets its first point)
          if (!press.additive && !press.subtractive) a.setSelection([]);
          setLasso({ pts: [press.world, w], dragging: true, additive: press.additive, subtractive: press.subtractive });
          return;
        }
        const lasso = getCommonTool().lasso;
        if (lasso) setLasso({ ...lasso, pts: appendLassoPoint(lasso.pts, w, LASSO_SAMPLE_PX / (a.view.scale || 1)) });
      }
    };

    const onPointerUp = (e: PointerEvent) => {
      const a = latest.current.adapter;
      if (!press || e.pointerId !== press.id) {
        const lasso = getCommonTool().lasso;
        if (lasso && lasso.dragging) setLasso({ ...lasso, dragging: false }); // the release ends the freehand stretch; the lasso goes on until it is closed
        if (lasso && container(e)) {
          e.stopPropagation();
          e.preventDefault();
        }
        return;
      }
      const wasClick = !press.started;
      const mods = { additive: press.additive, subtractive: press.subtractive };
      press = null;
      e.stopPropagation();
      e.preventDefault();
      if (!a) return;
      if (wasClick) {
        // an ordinary click on empty space: the selection is cleared unless a modifier is held (the canvas's "nothing hit" branch)
        if (!mods.additive && !mods.subtractive) a.setSelection([]);
        return;
      }
      const lasso = getCommonTool().lasso;
      if (lasso) setLasso({ ...lasso, dragging: false });
    };

    const onDoubleClick = (e: MouseEvent) => {
      const a = latest.current.adapter;
      if (!a || !container(e)) return;
      if (picker.session() && a.tab !== "pcb") {
        e.stopPropagation(); // "Not currently used, but we don't want to pass them either"
        e.preventDefault();
        return;
      }
      if (getCommonTool().lasso) {
        e.stopPropagation();
        e.preventDefault();
        finishLasso(a);
      }
    };

    const onContextMenu = (e: MouseEvent) => {
      if (!container(e) || (e.target as Element).closest(".menubar-dropdown")) return;
      if (picker.session() && latest.current.adapter?.tab !== "pcb") {
        e.stopPropagation();
        e.preventDefault();
      }
    };

    col.addEventListener("pointerdown", onPointerDown, true);
    col.addEventListener("pointermove", onPointerMove, true);
    col.addEventListener("pointerup", onPointerUp, true);
    col.addEventListener("dblclick", onDoubleClick, true);
    col.addEventListener("contextmenu", onContextMenu, true);
    return () => {
      col.removeEventListener("pointerdown", onPointerDown, true);
      col.removeEventListener("pointermove", onPointerMove, true);
      col.removeEventListener("pointerup", onPointerUp, true);
      col.removeEventListener("dblclick", onDoubleClick, true);
      col.removeEventListener("contextmenu", onContextMenu, true);
      setLasso(null);
    };
  }, []);

  if (!menu) return null;
  const entries: MenuEntry[] = [
    ...menu.items.map((it, i) => ({ label: `${i < 9 ? `${i + 1}  ` : ""}${it.label}`, onSelect: () => closeSelectionMenu([it.id]) })),
    { label: "Select All", onSelect: () => closeSelectionMenu(menu.items.map((it) => it.id)) },
  ];
  return <ContextMenu x={menu.at.x} y={menu.at.y} entries={entries} onClose={() => closeSelectionMenu(null)} />;
}
