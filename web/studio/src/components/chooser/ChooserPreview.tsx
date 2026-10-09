// The preview panes of the choosers: a library symbol (`SYMBOL_PREVIEW_WIDGET`) and a footprint (`FOOTPRINT_PREVIEW_WIDGET`) drawn on a canvas, fitted to
// their box. They draw with the editors' own painters, so a symbol or footprint looks here as it does when placed or opened.
import { useEffect, useMemo, useRef } from "react";
import type { LibraryFootprint, LibrarySymbol } from "../../api/types";
import { fitTransform } from "../../kicad-port/view";
import { footprintPreviewBounds, symbolPreviewBounds } from "../../kicad-port/libChooserPreview";
import { layerColor } from "../canvas/layers";
import { paintFootprint } from "../footprint/footprintPainter";
import { paintSymbol } from "../symbol/symbolPainter";

const W = 360;

function useCanvas(height: number, draw: (ctx: CanvasRenderingContext2D, w: number, h: number) => void, deps: readonly unknown[]) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.save();
    ctx.scale(dpr, dpr);
    draw(ctx, W, height);
    ctx.restore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return ref;
}

export function SymbolPreview({ symbol, unit, bodyStyle, status, height = 230 }: { symbol: LibrarySymbol | null; unit: number; bodyStyle: number; status: string; height?: number }) {
  const ref = useCanvas(
    height,
    (ctx, w, h) => {
      ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
      ctx.fillRect(0, 0, w, h);
      if (!symbol) return;
      const bounds = symbolPreviewBounds(symbol, unit, bodyStyle);
      if (!bounds) return;
      const view = fitTransform(bounds, w, h, 22);
      ctx.save();
      ctx.translate(view.x, view.y);
      ctx.scale(view.scale, view.scale);
      paintSymbol(ctx, view, w, h, symbol, {
        selection: new Set(),
        gridUm: 1270,
        gridVisible: false,
        activeUnit: unit,
        activeBodyStyle: bodyStyle,
        drawState: null,
        cursorUm: null,
        movePreview: null,
        showElectricalTypes: false,
        showHiddenPins: false,
        showPinNumbers: false,
        pendingText: null,
      });
      ctx.restore();
    },
    [symbol, unit, bodyStyle, height]
  );
  return (
    <div className="chooser-preview" data-preview="symbol">
      <canvas ref={ref} style={{ aspectRatio: `${W} / ${height}` }} />
      {(!symbol || status) && <div className="status">{status || ""}</div>}
    </div>
  );
}

/** The footprint as the preview draws it: its text variables read the way a library footprint shows them (`${REFERENCE}` is `REF**`, `${VALUE}` its name). */
function withTextVariables(fp: LibraryFootprint): LibraryFootprint {
  const value = fp.name.slice(fp.name.indexOf(":") + 1);
  return { ...fp, texts: fp.texts.map((t) => ({ ...t, content: t.content.replace(/\$\{REFERENCE\}/g, "REF**").replace(/\$\{VALUE\}/g, value) })) };
}

export function FootprintPreview({ footprint: raw, status, height = 200 }: { footprint: LibraryFootprint | null; status: string; height?: number }) {
  const footprint = useMemo(() => (raw ? withTextVariables(raw) : null), [raw]);
  const ref = useCanvas(
    height,
    (ctx, w, h) => {
      ctx.fillStyle = layerColor("background");
      ctx.fillRect(0, 0, w, h);
      if (!footprint) return;
      const bounds = footprintPreviewBounds(footprint);
      if (!bounds) return;
      const view = fitTransform(bounds, w, h, 22);
      ctx.save();
      ctx.translate(view.x, view.y);
      ctx.scale(view.scale, view.scale);
      paintFootprint(ctx, view, w, h, footprint, { selection: new Set(), gridUm: 0, gridVisible: false, drawState: null, cursorUm: null, movePreview: null });
      ctx.restore();
    },
    [footprint, height]
  );
  return (
    <div className="chooser-preview" data-preview="footprint">
      <canvas ref={ref} style={{ aspectRatio: `${W} / ${height}` }} />
      {(!footprint || status) && <div className="status">{status || ""}</div>}
    </div>
  );
}
