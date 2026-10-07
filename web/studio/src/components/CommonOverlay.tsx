// What the shared tools draw over every editor's canvas: the cursor crosshair and the item bounding boxes.
//
// KiCad paints both through the GAL, whichever editor is open: the cursor in `blitCursor` (small cross, full-window cross
// or 45 degree cross; shown while a tool wants it or "Always show crosshairs" is on) and the boxes in each painter's
// `m_drawBoundingBoxes` branch (PCB_PAINTER::draw, SCH_PAINTER::drawItemBoundingBox). The four canvases of the studio paint
// themselves, so this transparent layer sits over whichever is on screen and draws the parts that are common to them -- the
// canvases keep only their own content. It takes no pointer events.
import { useEffect, useRef, useState } from "react";
import { useStudioState } from "../state/store";
import { useFpState } from "../state/footprintEditorStore";
import { useSymState } from "../state/symbolEditorStore";
import { useCommonOptions } from "../state/commonOptions";
import { crosshairSegments, cursorVisible } from "../kicad-port/crosshair";
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

export function CommonOverlay() {
  const studio = useStudioState();
  const fp = useFpState();
  const sym = useSymState();
  const opts = useCommonOptions();
  const ref = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  // The layer is as large as the canvas column it sits in.
  useEffect(() => {
    const el = ref.current?.parentElement;
    if (!el) return;
    const measure = () => setSize({ width: el.clientWidth, height: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

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
    const { view } = adapter;

    // `RENDER_SETTINGS::GetDrawBoundingBoxes`: one outline per item, red when it is selected.
    if (opts.drawBoundingBoxes) {
      const boardSide = tab === "pcb" || tab === "footprint";
      const parts = new Set(studio.board?.parts.map((p) => p.ref) ?? []);
      ctx.lineWidth = boardSide ? 1 : Math.max(1, SCH_BOX_WIDTH_UM * view.scale);
      for (const [id, [x0, y0, x1, y1]] of adapter.itemBoxes()) {
        const selected = adapter.selection.has(id);
        ctx.strokeStyle = selected ? SELECTED_BOX_COLOR : !boardSide ? SCH_BOX_COLOR : tab === "pcb" && parts.has(id) ? PCB_FOOTPRINT_BOX_COLOR : PCB_BOX_COLOR;
        const sx = x0 * view.scale + view.x;
        const sy = y0 * view.scale + view.y;
        ctx.strokeRect(sx, sy, (x1 - x0) * view.scale, (y1 - y0) * view.scale);
      }
    }

    // `blitCursor`: at the cursor, in the chosen mode, when a tool wants it or the setting forces it.
    if (adapter.cursor && cursorVisible(opts.alwaysShowCursor, !adapter.toolIdle)) {
      const px = adapter.cursor.x * view.scale + view.x;
      const py = adapter.cursor.y * view.scale + view.y;
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

  return <canvas ref={ref} aria-hidden style={{ position: "absolute", left: 0, top: 0, pointerEvents: "none", zIndex: 5 }} />;
}
