// The plot of a simulator tab (`SIM_PLOT_TAB`'s `mpWindow`), painted on a canvas the way mathplot paints its layers: the background, the axes
// (`mpScaleXBase::Plot`, `mpScaleY::Plot`), the legend (`mpInfoLegend::Plot`), the traces (`mpFXY::Plot`), the cursors (`CURSOR::Plot`) and the
// zoom rectangle. The mouse does what `mpWindow`'s handlers do: the wheel zooms (Ctrl pans left and right, Shift up and down), a left drag zooms to
// a rectangle or moves a cursor or the legend, a middle drag pans, a right click opens the zoom menu. The model is kicad-port/sim/mathPlot.ts.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  cssRgb,
  distance,
  fit,
  fitWithUndo,
  mix,
  p2x,
  p2y,
  pan,
  resize,
  toPlot,
  fromPlot,
  x2p,
  xTicks,
  y2p,
  yTicks,
  zoomIn,
  zoomOut,
  zoomRect,
  zoomUndo,
  zoomRedo,
  colorList,
  type Rgb,
  type Scale,
} from "../../kicad-port/sim/mathPlot";
import { cursorValue, displayName, scaleOf, traceDotted, type Cursor, type PlotTab, type Trace } from "../../kicad-port/sim/simPlotTab";
import { moveCursor } from "../../kicad-port/sim/simFrame";
import { getSimUi, touch, useSimUi } from "../../state/simulator";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";

const FONT = "11px sans-serif";
// mathplot.cpp's mpLEGEND_MARGIN, mpLEGEND_LINEWIDTH; CURSOR::DRAG_MARGIN
const LEGEND_MARGIN = 5;
const LEGEND_LINEWIDTH = 10;
const DRAG_MARGIN = 10;

/** The plotted canvases, by tab id, so an export can take a picture of one (`mpWindow::SaveScreenshot`). */
export const plotCanvases = new Map<number, HTMLCanvasElement>();

function textWidth(ctx: CanvasRenderingContext2D, s: string): number {
  return Math.ceil(ctx.measureText(s).width);
}
const TEXT_H = 13;

function line(ctx: CanvasRenderingContext2D, x0: number, y0: number, x1: number, y1: number): void {
  ctx.beginPath();
  ctx.moveTo(x0 + 0.5, y0 + 0.5);
  ctx.lineTo(x1 + 0.5, y1 + 0.5);
  ctx.stroke();
}

/** `mpScaleXBase::Plot`. */
function paintX(ctx: CanvasRenderingContext2D, s: Scale, plot: PlotTab, axis: Rgb, fg: Rgb): void {
  const v = plot.view;
  xTicks(s, v);
  if (!s.visible) return;
  ctx.strokeStyle = cssRgb(axis);
  ctx.fillStyle = cssRgb(fg);
  ctx.lineWidth = 1;
  const orgy = v.scrY - v.marginBottom;
  const startPx = v.marginLeft;
  const endPx = v.scrX - v.marginRight;
  const minYpx = v.marginTop;
  const maxYpx = v.scrY - v.marginBottom;
  for (const tp of s.tickValues) {
    const p = Math.trunc((toPlot(s, tp) - v.posX) * v.scaleX);
    if (p < startPx || p > endPx) continue;
    if (s.ticks) line(ctx, p, orgy, p, orgy + 4);
    else {
      ctx.setLineDash([1, 2]);
      line(ctx, p, orgy + 4, p, minYpx);
      ctx.setLineDash([]);
      line(ctx, p, orgy + 4, p, orgy - 4);
    }
  }
  line(ctx, startPx, minYpx, endPx, minYpx);
  line(ctx, startPx, maxYpx, endPx, maxYpx);
  ctx.textBaseline = "top";
  for (const l of s.tickLabels) {
    if (!l.visible) continue;
    const p = Math.trunc((toPlot(s, l.pos) - v.posX) * v.scaleX);
    if (p < startPx || p > endPx) continue;
    ctx.fillText(l.label, p - textWidth(ctx, l.label) / 2, orgy + 4);
  }
  const tx = textWidth(ctx, s.name);
  ctx.fillText(s.name, (endPx + startPx) / 2 - tx / 2, orgy + 6 + TEXT_H);
}

/** `mpScaleY::Plot`. */
function paintY(ctx: CanvasRenderingContext2D, s: Scale, plot: PlotTab, axis: Rgb, fg: Rgb): void {
  const v = plot.view;
  yTicks(s, v);
  if (!s.visible) return;
  ctx.strokeStyle = cssRgb(axis);
  ctx.fillStyle = cssRgb(fg);
  ctx.lineWidth = 1;
  const orgx = s.align === "left" ? v.marginLeft : s.align === "right" ? v.scrX - v.marginRight : v.scrX - 1;
  const endPx = v.scrX - v.marginRight;
  const minYpx = v.marginTop;
  const maxYpx = v.scrY - v.marginBottom;
  line(ctx, orgx, minYpx, orgx, maxYpx);
  for (const tp of s.tickValues) {
    const p = Math.trunc((v.posY - toPlot(s, tp)) * v.scaleY);
    if (p < minYpx || p > maxYpx) continue;
    if (s.ticks) line(ctx, orgx - 4, p, orgx, p);
    else {
      line(ctx, orgx - 4, p, orgx + 4, p);
      ctx.setLineDash([1, 2]);
      line(ctx, orgx - 4, p, endPx, p);
      ctx.setLineDash([]);
    }
  }
  ctx.textBaseline = "middle";
  for (const l of s.tickLabels) {
    if (!l.visible) continue;
    const p = Math.trunc((v.posY - toPlot(s, l.pos)) * v.scaleY);
    if (p < minYpx || p > maxYpx) continue;
    const tx = textWidth(ctx, l.label);
    ctx.fillText(l.label, s.align === "right" ? orgx + 4 : orgx - 4 - tx, p);
  }
  ctx.textBaseline = "top";
  const tx = textWidth(ctx, s.name);
  if (s.nameAlign === "border_right") ctx.fillText(s.name, orgx - 6 - tx, 4);
  else ctx.fillText(s.name, orgx - tx / 2, minYpx - TEXT_H - 4);
}

/** `mpFXY::Plot` of a continuous trace: a polyline per sweep, clipped to the plot area. */
function paintTrace(ctx: CanvasRenderingContext2D, plot: PlotTab, tr: Trace): void {
  const sx = plot.x;
  const sy = scaleOf(plot, tr);
  if (!sx || !sy || tr.x.length === 0) return;
  const v = plot.view;
  const startPx = v.marginLeft;
  const endPx = v.scrX - v.marginRight;
  const minYpx = v.marginTop;
  const maxYpx = v.scrY - v.marginBottom;
  if (endPx <= startPx || minYpx >= maxYpx) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(startPx, minYpx, endPx - startPx + 1, maxYpx - minYpx + 1);
  ctx.clip();
  ctx.strokeStyle = cssRgb(tr.color);
  ctx.lineWidth = 2;
  ctx.setLineDash(traceDotted(plot, tr) ? [2, 4] : []);
  const size = Number.isFinite(tr.sweepSize) ? tr.sweepSize : tr.x.length;
  for (let sweep = 0; sweep < tr.sweepCount; sweep++) {
    const a = sweep * size;
    const b = Math.min(tr.x.length, a + size);
    ctx.beginPath();
    let first = true;
    for (let i = a; i < b; i++) {
      const px = x2p(v, toPlot(sx, tr.x[i]!));
      const py = y2p(v, toPlot(sy, tr.y[i]!));
      if (!Number.isFinite(px) || !Number.isFinite(py)) continue;
      if (first) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
      first = false;
    }
    ctx.stroke();
  }
  ctx.restore();
}

/** `CURSOR::Plot`: the cross lines in the trace's colour mixed with the foreground, the triangle and the cursor's number above the plot. */
function paintCursor(ctx: CanvasRenderingContext2D, plot: PlotTab, tr: Trace, id: number, c: Cursor, fg: Rgb): void {
  const sx = plot.x;
  const sy = scaleOf(plot, tr);
  if (!sx || !sy || tr.x.length <= 1) return;
  const v = plot.view;
  const x = c.x;
  // `CURSOR::doSetCoordX` (a multi-run trace read on the cursor's run)
  const y = cursorValue(tr, x, c.sweep ?? 0);
  const cx = x2p(v, toPlot(sx, x));
  const cy = y2p(v, toPlot(sy, y));
  const left = v.marginLeft;
  const right = v.scrX - v.marginRight;
  const top = v.marginTop;
  const bottom = v.scrY - v.marginBottom;
  const color = mix(tr.color, fg, 0.6);
  let text: Rgb = fg;
  if (distance(color, text) < 0.66) text = [255 - text[0], 255 - text[1], 255 - text[2]];
  ctx.strokeStyle = cssRgb(color);
  ctx.lineWidth = 1;
  ctx.setLineDash([8, 4]);
  if (top < cy && cy < bottom) line(ctx, left, cy, right, cy);
  if (left < cx && cx < right) {
    line(ctx, cx, top, cx, bottom);
    ctx.setLineDash([]);
    const h = TEXT_H + 3 - ((TEXT_H + 3) % 2);
    ctx.fillStyle = cssRgb(tr.color);
    ctx.beginPath();
    ctx.moveTo(cx - 1 - h / 2, top - h);
    ctx.lineTo(cx + 1 + h / 2, top - h);
    ctx.lineTo(cx, top);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = cssRgb(text);
    ctx.textBaseline = "top";
    const label = `${id}`;
    ctx.fillText(label, cx - textWidth(ctx, label) / 2 - (label === "1" ? 1 : 0), top - 4 - TEXT_H);
  }
  ctx.setLineDash([]);
}

/** `mpInfoLegend::Plot`: the box, and a line and the name of every trace. Returns its box for dragging. */
function paintLegend(ctx: CanvasRenderingContext2D, plot: PlotTab, traces: Trace[], fg: Rgb): { x: number; y: number; w: number; h: number } | null {
  if (!plot.legend.shown) return null;
  const base = LEGEND_MARGIN * 2 + LEGEND_LINEWIDTH;
  let w = base;
  let h = LEGEND_MARGIN;
  for (const tr of traces) {
    const tx = textWidth(ctx, displayName(tr));
    w = w > tx + base ? w : tx + base + LEGEND_MARGIN;
    h += TEXT_H;
  }
  if (h === LEGEND_MARGIN) return null;
  h += LEGEND_MARGIN;
  const { x, y } = plot.legend;
  ctx.strokeStyle = cssRgb(fg);
  ctx.lineWidth = 1;
  ctx.strokeRect(x + 0.5, y + 0.5, w, h);
  ctx.textBaseline = "top";
  traces.forEach((tr, i) => {
    ctx.strokeStyle = cssRgb(tr.color);
    ctx.lineWidth = 2;
    ctx.setLineDash(traceDotted(plot, tr) ? [2, 4] : []);
    const py = y + LEGEND_MARGIN + i * TEXT_H + (TEXT_H >> 1);
    line(ctx, x + LEGEND_MARGIN, py, x + LEGEND_LINEWIDTH + LEGEND_MARGIN, py);
    ctx.setLineDash([]);
    ctx.fillStyle = cssRgb(fg);
    ctx.fillText(displayName(tr), x + base, y + LEGEND_MARGIN + i * TEXT_H);
  });
  return { x, y, w, h };
}

/** The traces in the order mathplot plots them: as they were added (the tab's map keeps insertion order). */
const tracesOf = (plot: PlotTab) => [...plot.traces.values()];

/** Paint `plot` on `ctx` (`mpWindow::OnPaint`). */
export function paintPlot(ctx: CanvasRenderingContext2D, plot: PlotTab, dark: boolean, zoomBox?: { x0: number; y0: number; x1: number; y1: number } | null): { x: number; y: number; w: number; h: number } | null {
  const [bg, fg, axis] = colorList(dark) as [Rgb, Rgb, Rgb];
  const v = plot.view;
  ctx.save();
  ctx.font = FONT;
  ctx.fillStyle = cssRgb(bg);
  ctx.fillRect(0, 0, v.scrX, v.scrY);
  if (plot.x) paintX(ctx, plot.x, plot, axis, fg);
  for (const s of [plot.y1, plot.y2, plot.y3]) if (s) paintY(ctx, s, plot, axis, fg);
  const traces = tracesOf(plot);
  const legend = paintLegend(ctx, plot, traces, fg);
  for (const tr of traces) {
    paintTrace(ctx, plot, tr);
    for (const [id, c] of tr.cursors) paintCursor(ctx, plot, tr, id, c, fg);
  }
  if (zoomBox) {
    ctx.strokeStyle = cssRgb(fg);
    ctx.lineWidth = 1;
    ctx.setLineDash([1, 2]);
    ctx.strokeRect(Math.min(zoomBox.x0, zoomBox.x1) + 0.5, Math.min(zoomBox.y0, zoomBox.y1) + 0.5, Math.abs(zoomBox.x1 - zoomBox.x0), Math.abs(zoomBox.y1 - zoomBox.y0));
    ctx.setLineDash([]);
  }
  ctx.restore();
  return legend;
}

/** The cursor under a point (`CURSOR::Inside`: within DRAG_MARGIN of its vertical or its horizontal line). */
function cursorAt(plot: PlotTab, p: { x: number; y: number }): { tr: Trace; id: number } | null {
  const sx = plot.x;
  if (!sx) return null;
  for (const tr of tracesOf(plot)) {
    const sy = scaleOf(plot, tr);
    for (const [id, c] of tr.cursors) {
      const cx = x2p(plot.view, toPlot(sx, c.x));
      const cy = sy ? y2p(plot.view, toPlot(sy, cursorValue(tr, c.x, c.sweep ?? 0))) : Number.NaN;
      if (Math.abs(p.x - cx) <= DRAG_MARGIN || Math.abs(p.y - cy) <= DRAG_MARGIN) return { tr, id };
    }
  }
  return null;
}

type Drag = { kind: "zoom"; x0: number; y0: number; x1: number; y1: number } | { kind: "cursor"; tr: Trace; id: number } | { kind: "legend"; dx: number; dy: number } | { kind: "pan"; x: number; y: number };

export function SimPlot({ tabId, plot }: { tabId: number; plot: PlotTab }) {
  const ui = useSimUi();
  const dark = ui.frame.dark;
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<Drag | null>(null);
  const legendBox = useRef<{ x: number; y: number; w: number; h: number } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const middleClick = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const fitted = useRef(false);

  const paint = useCallback(() => {
    const c = canvas.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const d = drag.current;
    legendBox.current = paintPlot(ctx, plot, dark, d?.kind === "zoom" ? d : null);
  }, [plot, dark]);

  useEffect(() => {
    const c = canvas.current;
    if (c) plotCanvases.set(tabId, c);
    return () => {
      if (plotCanvases.get(tabId) === c) plotCanvases.delete(tabId);
    };
  }, [tabId]);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const c = canvas.current;
      if (!c) return;
      const w = Math.max(128, Math.floor(el.clientWidth));
      const h = Math.max(128, Math.floor(el.clientHeight));
      c.width = w;
      c.height = h;
      if (!fitted.current) {
        plot.view.scrX = w;
        plot.view.scrY = h;
        fit(plot.view);
        fitted.current = true;
      } else resize(plot.view, w, h);
      paint();
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [plot, paint]);

  useEffect(() => {
    paint();
  }, [paint, ui.version]);

  useNonPassiveWheel(canvas, (e) => onWheelRef.current(e));
  const onWheelRef = useRef<(e: WheelEvent) => void>(() => undefined);

  const pos = (e: { clientX: number; clientY: number }) => {
    const r = canvas.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    setMenu(null);
    const p = pos(e);
    try {
      canvas.current?.setPointerCapture(e.pointerId);
    } catch {
      // a pointer the page did not see go down
    }
    if (e.button === 1) {
      // `OnMouseMiddleDown`: `m_mouseMClick`, where the popup's Zoom In zooms
      middleClick.current = { x: p.x, y: p.y };
      drag.current = { kind: "pan", x: p.x, y: p.y };
      return;
    }
    if (e.button !== 0) return;
    const lb = legendBox.current;
    if (lb && p.x >= lb.x && p.x <= lb.x + lb.w && p.y >= lb.y && p.y <= lb.y + lb.h) {
      drag.current = { kind: "legend", dx: p.x - lb.x, dy: p.y - lb.y };
      return;
    }
    const c = cursorAt(plot, p);
    if (c) {
      drag.current = { kind: "cursor", tr: c.tr, id: c.id };
      return;
    }
    drag.current = { kind: "zoom", x0: p.x, y0: p.y, x1: p.x, y1: p.y };
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    const p = pos(e);
    if (canvas.current) canvas.current.style.cursor = d?.kind === "legend" ? "move" : d?.kind === "cursor" || cursorAt(plot, p) ? "ew-resize" : "crosshair";
    if (!d) return;
    if (d.kind === "zoom") {
      d.x1 = p.x;
      d.y1 = p.y;
      paint();
    } else if (d.kind === "pan") {
      pan(plot.view, d.x - p.x, d.y - p.y);
      d.x = p.x;
      d.y = p.y;
      middleClick.current = { x: p.x, y: p.y };
      touch();
    } else if (d.kind === "legend") {
      plot.legend.x = Math.round(p.x - d.dx);
      plot.legend.y = Math.round(p.y - d.dy);
      paint();
    } else if (d.kind === "cursor" && plot.x) {
      moveCursor(getSimUi().frame, d.tr, d.id, fromPlot(plot.x, p2x(plot.view, p.x)));
      touch();
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    if (d.kind === "zoom") {
      if (d.x0 !== d.x1 || d.y0 !== d.y1) zoomRect(plot.view, { x: d.x0, y: d.y0 }, { x: d.x1, y: d.y1 });
      touch();
    } else if (d.kind === "legend") touch(true);
    void e;
  };

  // `mpWindow::PerformMouseWheelAction` with KiCad's default actions: zoom; with Ctrl pan left and right; with Shift pan up and down.
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const p = pos(e);
    const change = -e.deltaY;
    // PAN_LEFT_RIGHT: `m_posX + change / m_scaleX`; PAN_UP_DOWN: `m_posY + change / m_scaleY` (`pan` takes pixels, y downwards)
    if (e.ctrlKey) pan(plot.view, change, 0);
    else if (e.shiftKey) pan(plot.view, 0, -change);
    else if (change > 0) zoomIn(plot.view, "both", p);
    else if (change < 0) zoomOut(plot.view, "both", p);
    touch();
  };
  onWheelRef.current = onWheel;

  const item = (label: string, enabled: boolean, fn: () => void) => (
    <div className="menu-node-item" role="menuitem" aria-disabled={!enabled} onClick={() => {
      setMenu(null);
      if (enabled) {
        fn();
        touch();
      }
    }}>
      <span>{label}</span>
    </div>
  );

  return (
    <div ref={wrap} className="sim-plot" onContextMenu={(e) => {
      e.preventDefault();
      const r = wrap.current!.getBoundingClientRect();
      setMenu({ x: e.clientX - r.left, y: e.clientY - r.top });
    }}>
      <canvas ref={canvas} data-testid="sim-plot" onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} />
      {menu && (
        // `mpWindow`'s popup menu.
        <div className="menubar-dropdown sim-plot-menu" style={{ left: menu.x, top: menu.y }}>
          {item("Undo Last Zoom", plot.view.undo.length > 0, () => zoomUndo(plot.view))}
          {item("Redo Last Zoom", plot.view.redo.length > 0, () => zoomRedo(plot.view))}
          <div className="menu-separator" role="separator" />
          {item("Zoom In", true, () => zoomIn(plot.view, "both", middleClick.current))}
          {item("Zoom Out", true, () => zoomOut(plot.view))}
          {item("Center on Cursor", true, () => {
            // `mpWindow::OnCenter`: the clicked point to the middle of the plot area.
            const v = plot.view;
            const cx = (v.scrX - v.marginLeft - v.marginRight) / 2;
            const cy = (v.scrY - v.marginTop - v.marginBottom) / 2;
            // `SetPos`: the position alone, not the desired view nor its limits
            const nx = p2x(v, menu.x - cx);
            const ny = p2y(v, menu.y - cy);
            v.posX = nx;
            v.posY = ny;
          })}
          {item("Fit on Screen", true, () => fitWithUndo(plot.view))}
        </div>
      )}
    </div>
  );
}
