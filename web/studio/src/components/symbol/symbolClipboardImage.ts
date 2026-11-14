// Copy's images of the selection (UI-LEFT-2 YY5): `SYMBOL_EDITOR_EDIT_TOOL::Copy` (eeschema/tools/symbol_editor_edit_tool.cpp at KiCad 8303b2ad) puts beside the symbol's text an SVG
// (`plotSymbolToSvg`) and a PNG (`renderSymbolToImageWithAlpha`) of the selected items -- the symbol with the others deleted, plotted over the selection's box, merged from each
// item's `GetBoundingBox()` and grown by 2% of its size on each side (`clipboardBboxInflation`). The PNG is drawn at the view's scale, at most 4096 pixels a side
// (`clipboardMaxBitmapSize`), on a transparent background (KiCad renders on white and on black and works out the alpha); the SVG is kicad-cli's plot of that symbol
// (`POST /api/sym/svg` with the copied text: KiCad's own SVG plotter, `SCH_PLOTTER`'s), cropped to the selection's box as `plotSymbolToSvg`'s page is (UI-LEFT-3 Y3-6).
import type { LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin } from "../../api/types";
import type { SymPaintOptions } from "./symbolPainter";
import { paintSymbol } from "./symbolPainter";
import { postSymbolSvg } from "../../api/schControlClient";
import { bitmapSize, svgCroppedTo, type ClipBox as Box } from "../../kicad-port/symClipboard";
export { clipboardBox } from "../../kicad-port/symClipboard";

/** The PNG of the symbol `sym` (its selected items only) over `box`, or null when there is nothing to draw or no canvas. */
export async function selectionPng(sym: LibrarySymbol, box: Box, scalePxPerNm: number, opts: Omit<SymPaintOptions, "itemsOnly" | "selection" | "drawState" | "cursorNm" | "movePreview">): Promise<Blob | null> {
  const { width, height, scale } = bitmapSize(box, scalePxPerNm);
  if (width <= 0 || height <= 0 || typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  const view = { x: -box[0] * scale, y: -box[1] * scale, scale };
  ctx.save();
  ctx.translate(view.x, view.y);
  ctx.scale(scale, scale);
  paintSymbol(ctx, view as never, width, height, sym, { ...opts, itemsOnly: true, selection: new Set(), drawState: null, cursorNm: null, movePreview: null, pointEditor: null, carried: null });
  ctx.restore();
  return new Promise((resolve) => canvas.toBlob((b) => resolve(b), "image/png"));
}

/**
 * The SVG of the copied symbol's text, plotted by kicad-cli in the unit and body style being edited and in the editor's theme (`renderSettings.LoadColors(
 * aFrame->GetColorSettings() )`: the theme Export Symbol as SVG sends), on a page of the selection's box (`svgCroppedTo`, UI-LEFT-3 Y3-6); null when it could not be.
 * The text it puts on the clipboard is kept as `window.__edaSymClipboardSvg` (a headless browser's clipboard holds no image).
 */
export async function selectionSvg(libId: string, text: string, unit: number, bodyStyle: number, box: Box): Promise<Blob | null> {
  try {
    // Export Symbol as SVG's theme (actions/schControlExports.ts `SYMBOL_EDITOR_THEME`)
    const r = await postSymbolSvg({ lib_id: libId, text, unit, body_style: bodyStyle, theme: "_builtin_default" });
    const svg = r.ok && r.svg ? svgCroppedTo(r.svg.text, box) : null;
    (globalThis as { __edaSymClipboardSvg?: string | null }).__edaSymClipboardSvg = svg;
    return svg ? new Blob([svg], { type: "image/svg+xml" }) : null;
  } catch {
    return null;
  }
}

/** The symbol with only the selected pins and graphics (`item.SetFlags( STRUCT_DELETED )` of the others; its fields stay). */
export function selectedOnly(sym: LibrarySymbol, pins: readonly LibrarySymbolPin[], graphics: readonly LibrarySymbolGraphic[]): LibrarySymbol {
  return { ...sym, pins: [...pins], graphics: [...graphics] };
}

/**
 * `SaveClipboard( prettyData, mimeData )`: the text with the images beside it, as one clipboard item -- the SVG left out where the browser cannot hold one
 * (`ClipboardItem.supports`), then the images where it can hold none; false when not even the text could be written.
 */
export async function writeClipboardWithImages(text: string, svg: Promise<Blob | null>, png: Promise<Blob | null>): Promise<boolean> {
  const clip = typeof navigator !== "undefined" ? navigator.clipboard : undefined;
  const CI = (globalThis as { ClipboardItem?: typeof ClipboardItem }).ClipboardItem;
  if (clip?.write && CI) {
    const supports = (t: string) => (typeof (CI as unknown as { supports?: (t: string) => boolean }).supports === "function" ? (CI as unknown as { supports: (t: string) => boolean }).supports(t) : t !== "image/svg+xml");
    const textBlob = new Blob([text], { type: "text/plain" });
    const [svgBlob, pngBlob] = await Promise.all([svg, png]);
    const tries: Record<string, Blob>[] = [];
    const full: Record<string, Blob> = { "text/plain": textBlob };
    if (pngBlob && supports("image/png")) full["image/png"] = pngBlob;
    if (svgBlob && supports("image/svg+xml")) full["image/svg+xml"] = svgBlob;
    tries.push(full);
    if (full["image/svg+xml"]) tries.push({ "text/plain": textBlob, ...(full["image/png"] ? { "image/png": full["image/png"] } : {}) });
    for (const items of tries) {
      try {
        await clip.write([new CI(items)]);
        (globalThis as { __edaSymClipboardTypes?: string[] }).__edaSymClipboardTypes = Object.keys(items);
        return true;
      } catch {
        // the next, smaller item
      }
    }
  }
  try {
    await clip?.writeText(text);
    (globalThis as { __edaSymClipboardTypes?: string[] }).__edaSymClipboardTypes = ["text/plain"];
    return !!clip;
  } catch {
    return false;
  }
}
