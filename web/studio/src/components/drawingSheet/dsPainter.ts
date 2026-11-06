// The drawing sheet on a canvas: `KIGFX::DS_PAINTER` (common/drawing_sheet/ds_painter.cpp, commit 8303b2ad) over the draw items of
// `kicad-port/dsDataModel.ts`, as `DS_PROXY_VIEW_ITEM::ViewDraw` runs it -- the board editor's in `LAYER_DRAWINGSHEET`'s colour, the schematic's in
// `LAYER_SCHEMATIC_DRAWINGSHEET`'s (the caller passes the colour). One painter for both editors and for the Page Settings sketch.
//
//   - lines and rectangles are stroked at their width (`std::max( GetPenWidth(), GetDefaultPenWidth() )`, the DS render settings' default being 0),
//     never thinner than a pixel on screen;
//   - polygons are filled, without an outline (`SetIsStroke( false )`);
//   - texts in the stroke font, each line where `FONT::getLinePositions` puts it, turned by the text's angle about its position, in its own colour when
//     the sheet gives one (`DS_RENDER_SETTINGS::GetColor`), with the pen `GetEffectiveTextPenWidth` gives;
//   - bitmaps centred on their position at their size (`DS_DRAW_ITEM_BITMAP::GetBoundingBox`); KiCad draws them before everything else
//     (`DS_DRAW_ITEM_LIST::Print`), and so does this.
//
// "Draw the title block normally even if the view is flipped": a canvas whose transform mirrors x (the board seen from below) draws the sheet mirrored
// back about the page's width, as `ViewDraw` does.
//
// Differs: a text under a pixel and a half high is not drawn (it could not be read; KiCad strokes it anyway), and an image is drawn once the browser has
// decoded it (the next repaint).
//
// The browser checks read what was last painted through `window.__edaDrawingSheet` (`{ pcb, sch, sketch }`: the item count and the texts of each).
import { buildDrawItems, parseDrawingSheet, textLines, effectiveTextPen, type DsDrawItem, type DsMeasure, type DsModel, type DsPageContext, type DsColor } from "../../kicad-port/dsDataModel";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";

/** The stroke font's advance, the width scaling with the text's width. */
export const strokeMeasure: DsMeasure = (text, sizeX) => measureStrokeText(text, sizeX);

const modelCache = new Map<string, { model: DsModel; error: string | null }>();

/**
 * The model of a `.kicad_wks` text, read once per text (the board and the sheet repaint often); no text is KiCad's default sheet (`null` model, which
 * `buildDrawItems` takes as the default).
 */
export function sheetModel(text: string | null | undefined, source = "drawing sheet"): { model: DsModel | null; error: string | null } {
  if (!text) return { model: null, error: null };
  let hit = modelCache.get(text);
  if (!hit) {
    hit = parseDrawingSheet(text, source);
    if (modelCache.size > 8) modelCache.delete(modelCache.keys().next().value as string);
    modelCache.set(text, hit);
  }
  return hit;
}

const images = new Map<string, HTMLImageElement>();

function imageOf(png: string): HTMLImageElement | null {
  if (typeof Image === "undefined") return null;
  let img = images.get(png);
  if (!img) {
    img = new Image();
    img.src = `data:image/png;base64,${png}`;
    if (images.size > 16) images.delete(images.keys().next().value as string);
    images.set(png, img);
  }
  return img.complete && img.naturalWidth > 0 ? img : null;
}

function cssColor(c: DsColor): string {
  return `rgba(${Math.round(c.r * 255)}, ${Math.round(c.g * 255)}, ${Math.round(c.b * 255)}, ${c.a})`;
}

export interface DsPaintOptions {
  /** Which canvas this is, for `window.__edaDrawingSheet`. */
  tag?: "pcb" | "sch" | "sketch";
  /** The sheet's colour. */
  color: string;
  /** One screen pixel, in nm (the thinnest line drawn). */
  pixelNm: number;
  /** The page's width, nm: a mirrored canvas is mirrored back about it. */
  pageWidthNm: number;
}

/** `DS_PROXY_VIEW_ITEM::ViewDraw`: the items painted, bitmaps first. */
export function paintDrawItems(ctx: CanvasRenderingContext2D, items: readonly DsDrawItem[], o: DsPaintOptions): void {
  ctx.save();
  const t = typeof ctx.getTransform === "function" ? ctx.getTransform() : null;
  if (t && t.a < 0) {
    ctx.translate(o.pageWidthNm, 0);
    ctx.scale(-1, 1);
  }
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const item of items) {
    if (item.kind !== "bitmap") continue;
    const img = imageOf(item.png);
    if (img) ctx.drawImage(img, item.pos[0] - item.size[0] / 2, item.pos[1] - item.size[1] / 2, item.size[0], item.size[1]);
  }
  for (const item of items) {
    switch (item.kind) {
      case "line":
        ctx.strokeStyle = o.color;
        ctx.lineWidth = Math.max(item.widthNm, o.pixelNm);
        ctx.beginPath();
        ctx.moveTo(item.start[0], item.start[1]);
        ctx.lineTo(item.end[0], item.end[1]);
        ctx.stroke();
        break;
      case "rect": {
        ctx.strokeStyle = o.color;
        ctx.lineWidth = Math.max(item.widthNm, o.pixelNm);
        const x = Math.min(item.start[0], item.end[0]);
        const y = Math.min(item.start[1], item.end[1]);
        ctx.strokeRect(x, y, Math.abs(item.end[0] - item.start[0]), Math.abs(item.end[1] - item.start[1]));
        break;
      }
      case "poly":
        ctx.fillStyle = o.color;
        ctx.beginPath();
        for (const outline of item.outlines) {
          outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
          ctx.closePath();
        }
        ctx.fill("nonzero");
        break;
      case "text": {
        if (!item.text || item.size[1] < 1.5 * o.pixelNm) break;
        const color = item.color ? cssColor(item.color) : o.color;
        const pen = Math.max(effectiveTextPen(item), o.pixelNm);
        const [sx, sy] = item.size;
        ctx.save();
        ctx.translate(item.pos[0], item.pos[1]);
        // an EDA_ANGLE turns counter-clockwise on the y-down page
        if (item.angleDeg) ctx.rotate((-item.angleDeg * Math.PI) / 180);
        for (const line of textLines(item, strokeMeasure)) {
          if (!line.text) continue;
          if (sx === sy) {
            drawStrokeText(ctx, line.text, line.x, line.y, { sizeNm: sy, thicknessNm: pen, color, italic: item.italic });
          } else {
            ctx.save();
            ctx.translate(line.x, line.y);
            ctx.scale(sx / sy, 1);
            drawStrokeText(ctx, line.text, 0, 0, { sizeNm: sy, thicknessNm: pen, color, italic: item.italic });
            ctx.restore();
          }
        }
        ctx.restore();
        break;
      }
      case "bitmap":
        break;
    }
  }
  ctx.restore();
}

/** One page's drawing sheet painted: the sheet's model (null: KiCad's default) built for the page and drawn. */
export function paintDrawingSheet(ctx: CanvasRenderingContext2D, model: DsModel | null, page: DsPageContext, o: Omit<DsPaintOptions, "pageWidthNm">): DsDrawItem[] {
  const items = buildDrawItems(model, page, strokeMeasure);
  paintDrawItems(ctx, items, { ...o, pageWidthNm: page.widthNm });
  if (o.tag && typeof window !== "undefined") {
    const w = window as unknown as { __edaDrawingSheet?: Record<string, { items: number; texts: string[]; page: [number, number] }> };
    (w.__edaDrawingSheet ??= {})[o.tag] = { items: items.length, texts: items.flatMap((i) => (i.kind === "text" ? [i.text] : [])), page: [page.widthNm, page.heightNm] };
  }
  return items;
}
