// What the shared tools draw over every editor's canvas: the cursor crosshair, the item bounding boxes, the lasso being drawn and the item
// the picker highlights.
//
// KiCad paints the first two through the GAL, whichever editor is open: the cursor in `blitCursor` (small cross, full-window cross or 45
// degree cross; shown while a tool wants it or "Always show crosshairs" is on) and the boxes in each painter's `m_drawBoundingBoxes`
// branch (PCB_PAINTER::draw, SCH_PAINTER::drawItemBoundingBox); the lasso is a `KIGFX::PREVIEW::SELECTION_AREA` view item and the picker's
// candidate is `BrightenItem`. The four canvases of the studio paint themselves, so this transparent layer sits over whichever is on screen
// and draws the parts that are common to them -- the canvases keep only their own content. It takes no pointer events.
import { useEffect, useRef, useState } from "react";
import { useStudioState } from "../state/store";
import { useFpState } from "../state/footprintEditorStore";
import { useSymState } from "../state/symbolEditorStore";
import { useCommonOptions } from "../state/commonOptions";
import { useCommonTool } from "../state/commonTool";
import { crosshairSegments, cursorVisible } from "../kicad-port/crosshair";
import { lassoContained } from "../kicad-port/lasso";
import { measureLabel } from "../kicad-port/measureRuler";
import { alignToGrid } from "../kicad-port/gridSnap";
import { getSnapOrigin } from "./canvas/gridHelper";
import { flipLocalX } from "../kicad-port/boardControl";
import { usePickerSession } from "../actions/pcbPicker";
import { makeEditorAdapter, isCanvasTab } from "../actions/editorAdapter";
import { layerColor } from "./canvas/layers";

/** The schematic-side editors draw on a light page, so their cursor colour is the schematic theme's (`LAYER_SCHEMATIC_CURSOR`), the board side's is `LAYER_CURSOR`. */
const cursorColor = (tab: string) => layerColor(tab === "schematic" || tab === "symbol" ? "LAYER_SCHEMATIC_CURSOR" : "LAYER_CURSOR");

const SELECTED_BOX_COLOR = "rgba(255,51,51,1)"; // COLOR4D( 1.0, 0.2, 0.2, 1 )
const PCB_BOX_COLOR = "rgba(102,102,102,1)"; // COLOR4D( 0.4, 0.4, 0.4, 1 )
const PCB_FOOTPRINT_BOX_COLOR = "#ff00ff"; // COLOR4D( MAGENTA )
const SCH_BOX_COLOR = "rgba(51,51,51,1)"; // COLOR4D( 0.2, 0.2, 0.2, 1 )
/** `SCH_PAINTER::drawItemBoundingBox`: `SetLineWidth( schIUScale.MilsToIU( 3 ) )`. */
const SCH_BOX_WIDTH_UM = 76.2;
/** The brightened item of a picker (the interactive delete tool's candidate): a heavy cyan outline. */
const HOVER_BOX_COLOR = "#33ddff";
/** A match of the open Find dialog: the brightened item. */
const FIND_BOX_COLOR = "#ffd84a";

export function CommonOverlay() {
  const studio = useStudioState();
  const fp = useFpState();
  const sym = useSymState();
  const opts = useCommonOptions();
  const tool = useCommonTool();
  const picker = usePickerSession();
  const ref = useRef<HTMLCanvasElement>(null);
  /** Where the editor's canvas is inside the column the layer sits in: the library editors draw their tree and toolbars in the same column, so the layer is placed over the canvas itself. */
  const [place, setPlace] = useState({ left: 0, top: 0, width: 0, height: 0 });
  const size = place;

  // The layer is as large as, and exactly over, the canvas container of the editor on screen (`.pcb-canvas-container`: all four editors use it).
  useEffect(() => {
    const parent = ref.current?.parentElement;
    if (!parent) return;
    const measure = () => {
      const container = parent.querySelector<HTMLElement>(".pcb-canvas-container");
      const outer = parent.getBoundingClientRect();
      const r = container?.getBoundingClientRect() ?? outer;
      const next = { left: Math.round(r.left - outer.left + parent.scrollLeft), top: Math.round(r.top - outer.top + parent.scrollTop), width: Math.round(r.width), height: Math.round(r.height) };
      setPlace((prev) => (prev.left === next.left && prev.top === next.top && prev.width === next.width && prev.height === next.height ? prev : next));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(parent);
    const container = parent.querySelector<HTMLElement>(".pcb-canvas-container");
    if (container) ro.observe(container);
    // The container comes and goes with the tab, and moves when a dock column folds.
    const mo = new MutationObserver(measure);
    mo.observe(parent, { childList: true, subtree: true });
    window.addEventListener("resize", measure);
    return () => {
      ro.disconnect();
      mo.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [studio.tab]);

  const tab = studio.tab;
  const adapter = isCanvasTab(tab) ? makeEditorAdapter({ tab, studio, fp, sym, dispatch: () => {}, fpDispatch: () => {}, symDispatch: () => {} }) : null;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || size.width === 0 || size.height === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.width * dpr);
    canvas.height = Math.round(size.height * dpr);
    canvas.style.width = `${size.width}px`;
    canvas.style.height = `${size.height}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size.width, size.height);
    if (!adapter || !(adapter.view.scale > 0)) return;
    const { view, flipped } = adapter;
    /** World -> canvas pixels, including the board's mirrored view. */
    const sx = (x: number) => flipLocalX(flipped, size.width, x * view.scale + view.x);
    const sy = (y: number) => y * view.scale + view.y;
    const strokeBox = ([x0, y0, x1, y1]: readonly [number, number, number, number]) => {
      const a = sx(x0);
      const b = sx(x1);
      ctx.strokeRect(Math.min(a, b), sy(y0), Math.abs(b - a), (y1 - y0) * view.scale);
    };

    // `RENDER_SETTINGS::GetDrawBoundingBoxes`: one outline per item, red when it is selected.
    if (opts.drawBoundingBoxes) {
      const boardSide = tab === "pcb" || tab === "footprint";
      const parts = new Set(studio.board?.parts.map((p) => p.ref) ?? []);
      ctx.lineWidth = boardSide ? 1 : Math.max(1, SCH_BOX_WIDTH_UM * view.scale);
      for (const [id, box] of adapter.itemBoxes()) {
        const selected = adapter.selection.has(id);
        ctx.strokeStyle = selected ? SELECTED_BOX_COLOR : !boardSide ? SCH_BOX_COLOR : tab === "pcb" && parts.has(id) ? PCB_FOOTPRINT_BOX_COLOR : PCB_BOX_COLOR;
        strokeBox(box);
      }
    }

    // Every item a match of the open Find dialog sits on (`UpdateFind`: `BrightenItem` + `SetForceVisible`).
    if (tab === "schematic" && tool.findHighlights.length > 0) {
      ctx.strokeStyle = FIND_BOX_COLOR;
      ctx.lineWidth = 2;
      const boxes = adapter.itemBoxes();
      for (const id of tool.findHighlights) {
        const box = boxes.get(id);
        if (box) strokeBox(box);
      }
    }

    // The item a picker would take (`BrightenItem` of the delete tool's candidate).
    if (picker && tool.hover) {
      const box = adapter.itemBoxes().get(tool.hover);
      if (box) {
        ctx.strokeStyle = HOVER_BOX_COLOR;
        ctx.lineWidth = 2;
        strokeBox(box);
      }
    }

    // The lasso (`SELECTION_AREA`): clockwise is inside (solid, yellow), counterclockwise touching (dashed, blue), the polygon closed to the cursor.
    if (tool.lasso && tool.lasso.pts.length > 0) {
      const pts = [...tool.lasso.pts, ...(adapter.cursor ? [[adapter.cursor.x, adapter.cursor.y] as [number, number]] : [])];
      const inside = lassoContained(pts);
      ctx.beginPath();
      pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(sx(x), sy(y)) : ctx.lineTo(sx(x), sy(y))));
      ctx.closePath();
      ctx.fillStyle = inside ? "rgba(255,216,74,0.12)" : "rgba(74,163,255,0.12)";
      ctx.strokeStyle = inside ? "#ffd84a" : "#4aa3ff";
      ctx.setLineDash(inside ? [] : [4, 3]);
      ctx.lineWidth = 1;
      ctx.fill();
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // The measure tool's ruler in the library editors (`RULER_ITEM`): a dashed line from the first click to the second, or to the cursor on the grid while the
    // second is still to come, with the distance and its extent at the middle.
    if (tool.measure && tool.measure.pts.length > 0) {
      const start = tool.measure.pts[0]!;
      const snapped = adapter.cursor ? alignToGrid({ x: adapter.cursor.x, y: adapter.cursor.y }, adapter.gridUm, getSnapOrigin(), { ctrlOrCmd: false }) : null;
      const end = tool.measure.pts.length >= 2 ? tool.measure.pts[1]! : snapped ? ([snapped.x, snapped.y] as const) : null;
      if (end) {
        ctx.strokeStyle = layerColor("selection");
        ctx.fillStyle = layerColor("selection");
        ctx.lineWidth = 1.5;
        ctx.setLineDash([6, 4]);
        ctx.beginPath();
        ctx.moveTo(sx(start[0]), sy(start[1]));
        ctx.lineTo(sx(end[0]), sy(end[1]));
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.font = "12px monospace";
        ctx.textAlign = "center";
        ctx.fillText(measureLabel(start, end, studio.units), sx((start[0] + end[0]) / 2), sy((start[1] + end[1]) / 2) - 8);
        ctx.textAlign = "start";
      }
    }

    // The zoom tool's box in the library editors (`ZOOM_TOOL::selectRegion`'s `SELECTION_AREA`): a blue rectangle from where the button went down to the cursor.
    const zoomBox = tool.zoomArea?.drag;
    if (zoomBox) {
      const [xa, xb] = [sx(zoomBox.a[0]), sx(zoomBox.b[0])];
      const [ya, yb] = [sy(zoomBox.a[1]), sy(zoomBox.b[1])];
      ctx.fillStyle = "rgba(74,163,255,0.12)";
      ctx.strokeStyle = "#4aa3ff";
      ctx.lineWidth = 1;
      ctx.fillRect(Math.min(xa, xb), Math.min(ya, yb), Math.abs(xb - xa), Math.abs(yb - ya));
      ctx.strokeRect(Math.min(xa, xb), Math.min(ya, yb), Math.abs(xb - xa), Math.abs(yb - ya));
    }

    // `blitCursor`: at the cursor, in the chosen mode, when a tool wants it or the setting forces it.
    if (adapter.cursor && cursorVisible(opts.alwaysShowCursor, !adapter.toolIdle || picker != null || tool.lasso != null || tool.zoomArea != null || tool.measure != null)) {
      const px = sx(adapter.cursor.x);
      const py = sy(adapter.cursor.y);
      ctx.strokeStyle = cursorColor(tab);
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (const [x0, y0, x1, y1] of crosshairSegments(opts.crossHairMode, px, py, size.width, size.height)) {
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
      }
      ctx.stroke();
    }
  });

  return <canvas ref={ref} aria-hidden style={{ position: "absolute", left: place.left, top: place.top, pointerEvents: "none", zIndex: 5 }} />;
}
