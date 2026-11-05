// The Footprint Editor's canvas: the footprint's fields and the items `eda_model::fp_items` adds (UI-LEFT F1, F2), drawn by the board's own painters (`PCB_PAINTER`
// is one painter for both editors): the fields by `painter.ts::drawFields`, the text boxes, barcodes and points by `pcbItemsPaint.ts`, the tables by
// `tablePaint.ts`, the dimensions by `painter.ts::drawDimensions` and the rule areas by `drawRuleArea`, each on the footprint's display shapes
// (`GET /api/footprint`'s `display`, `crates/cli/src/fp_items_json.rs`) in the footprint's frame, which is the canvas's.
//
// Differences: a reference image is drawn as its box with a cross (its pixels are served for a board's images only, `GET /api/pcb_image`), and so is a barcode,
// whose symbol kicad-cli plots for a board's barcodes only (`GET /api/barcode_shape`); a zone that is no rule area is its outline, dashed, in its layer's colour
// (kicad-cli fills zones, and a footprint is filled on the board it is placed on).
import type { FieldInfo } from "../../api/types";
import type { FootprintItemsDisplay } from "../../api/fpItemTypes";
import type { ViewTransform } from "../../kicad-port/view";
import { hairlineNm } from "../../kicad-port/view";
import { drawDimensions, drawFields, drawRuleArea, layerAlpha, realLayerKey, withAlpha, type PaintOptions } from "../canvas/painter";
import { PLAIN_APPEARANCE } from "../canvas/appearancePaint";
import { fpObjectOpacity, fpOpacities } from "../../kicad-port/fpAppearance";
import { drawBarcode, drawPcbItems } from "../canvas/pcbItemsPaint";
import { drawTables } from "../canvas/tablePaint";
import { layerColor } from "../canvas/layers";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { objectOn } from "../../kicad-port/appearance";
import type { BoardState, Zone } from "../../api/types";

export interface FpItemsPaintOptions {
  selection: Set<string>;
  /** The Appearance manager's switches: layers by name or state key, objects under `obj:<id>`. */
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;
  highContrast: boolean;
  /** Inactive layers "Hide" (`HIGH_CONTRAST_MODE::HIDDEN`): with `highContrast`, what is not on the active layer is not drawn. */
  contrastHidden?: boolean;
  activeLayer: string;
  sketchText: boolean;
  sketchGraphics: boolean;
  /** What a drag or a Duplicate pick-up carries, drawn where it would drop. */
  movePreview: { refs: string[]; dxNm: number; dyNm: number } | null;
}

/** The slice of the board painter's options these painters read, from the footprint editor's own. */
function boardOptions(o: FpItemsPaintOptions): PaintOptions {
  return {
    selection: o.selection,
    hot: new Set(),
    netHighlight: null,
    showRatsnest: false,
    ratsnestCurved: false,
    ratsnestEdges: null,
    layerVisible: o.layerVisible,
    layerOpacity: o.layerOpacity,
    activeLayer: o.highContrast ? layerStateKey(o.activeLayer) : null,
    highContrast: o.highContrast,
    // the Objects tab's opacities (`PCB_DISPLAY_OPTIONS`, all 1 to begin with in this editor) and the HIDDEN contrast mode, as the board's painters read them
    appearance: { ...PLAIN_APPEARANCE, opacity: fpOpacities(o.layerOpacity), contrastHidden: !!o.contrastHidden },
    gridNm: 0,
    gridVisible: false,
    movePreview: null,
    drawState: null,
    cursorNm: null,
    activeTool: "select",
    sketchPads: false,
    sketchTracks: false,
    sketchVias: false,
    drcViolations: null,
    zoneFill: null,
    zoneDisplayMode: "outline",
    sketchGraphics: o.sketchGraphics,
    sketchText: o.sketchText,
    currentViaPreset: null,
    units: "mm",
    zoneCornerPreview: null,
    shapePointPreview: null,
    drcSelected: null,
  } as unknown as PaintOptions;
}

const moved = (o: FpItemsPaintOptions, id: string) => (o.movePreview && o.movePreview.refs.includes(id) ? o.movePreview : null);

/** Draw `draw` once for the items that stay and once, translated, for the ones in the hand. */
function inTwoPasses<T extends { id: string }>(ctx: CanvasRenderingContext2D, items: readonly T[], o: FpItemsPaintOptions, draw: (subset: T[]) => void) {
  const mv = o.movePreview;
  const carried = mv ? items.filter((i) => mv.refs.includes(i.id)) : [];
  const staying = mv ? items.filter((i) => !mv.refs.includes(i.id)) : [...items];
  if (staying.length > 0) draw(staying);
  if (carried.length > 0 && mv) {
    ctx.save();
    ctx.translate(mv.dxNm, mv.dyNm);
    draw(carried);
    ctx.restore();
  }
}

/**
 * The footprint's fields as `PCB_FIELD::ViewGetLOD` shows them in this editor: a hidden field is not drawn (unless selected, dimmed, as the board's painter does),
 * the Reference needs the References object, the Value the Values object, every field the Footprint Text object, and its own layer.
 */
export function paintFpFields(ctx: CanvasRenderingContext2D, view: ViewTransform, fields: readonly FieldInfo[], o: FpItemsPaintOptions): void {
  if (!objectOn(o.layerVisible, "footprint_text")) return;
  const shown = fields.filter((f) => (f.name === "Reference" ? objectOn(o.layerVisible, "footprint_references") : f.name === "Value" ? objectOn(o.layerVisible, "footprint_values") : true));
  const opts = boardOptions(o);
  inTwoPasses(ctx, shown, o, (subset) => drawFields(ctx, view, subset.map((field) => ({ field })), opts));
}

/** The dimensions, zones, text boxes, points, barcodes, images and tables of the footprint. */
export function paintFpItems(ctx: CanvasRenderingContext2D, view: ViewTransform, d: FootprintItemsDisplay, o: FpItemsPaintOptions): void {
  const opts = boardOptions(o);
  const board = (drawings: Partial<NonNullable<BoardState["drawings"]>>) => ({ drawings: { shapes: [], texts: [], groups: [], dimensions: [], tables: [], ...drawings } }) as unknown as BoardState;
  // zones first, under the rest (`ZONE` sits under the graphics in the view's order)
  if (objectOn(o.layerVisible, "zones")) {
    inTwoPasses(ctx, d.zones, o, (zones) => {
      for (const z of zones) {
        if (!layerIsVisible(o.layerVisible, z.layer)) continue;
        const selected = o.selection.has(z.id);
        const asBoard = { ...z, outline: z.outline_xy, holes: z.holes_xy } as unknown as Zone;
        if (z.is_rule_area) {
          drawRuleArea(ctx, view, asBoard, selected);
          continue;
        }
        withAlpha(ctx, 0.8 * fpObjectOpacity(o.layerOpacity, "zones") * layerAlpha(opts, layerStateKey(z.layer)), () => {
          ctx.beginPath();
          for (const ring of [z.outline_xy, ...(z.holes_xy ?? [])]) {
            ring.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
            ctx.closePath();
          }
          ctx.strokeStyle = selected ? layerColor("selection") : layerColor(realLayerKey(z.layer));
          ctx.lineWidth = hairlineNm(view, selected ? 2.5 : 1.5);
          ctx.setLineDash([hairlineNm(view, 5), hairlineNm(view, 3)]);
          ctx.stroke();
          ctx.setLineDash([]);
        });
      }
    });
  }
  inTwoPasses(ctx, d.tables, o, (tables) => drawTables(ctx, view, board({ tables }), opts));
  inTwoPasses(ctx, d.dimensions, o, (dimensions) => drawDimensions(ctx, view, board({ dimensions }), opts));
  // text boxes and points
  const extras = [...d.text_boxes.map((t) => ({ id: t.id, t })), ...d.points.map((p) => ({ id: p.id, p }))];
  inTwoPasses(ctx, extras, o, (subset) =>
    drawPcbItems(
      ctx,
      view,
      board({
        text_boxes: subset.flatMap((x) => ("t" in x ? [x.t] : [])),
        points: subset.flatMap((x) => ("p" in x ? [x.p] : [])),
      }),
      opts
    )
  );
  // a barcode: `drawBarcode` without asking the board's `GET /api/barcode_shape` (which knows a board's barcodes only): its box with a cross
  inTwoPasses(ctx, d.barcodes, o, (barcodes) => {
    for (const b of barcodes) if (layerIsVisible(o.layerVisible, b.layer)) withAlpha(ctx, layerAlpha(opts, layerStateKey(b.layer)), () => drawBarcode(ctx, view, b, o.selection.has(b.id)));
  });
  // a reference image: its box, crossed, in its layer's colour
  if (objectOn(o.layerVisible, "bitmaps")) {
    for (const i of d.images) {
      if (!layerIsVisible(o.layerVisible, i.layer)) continue;
      const mv = moved(o, i.id);
      const [x0, y0] = [i.x - i.w / 2 + (mv?.dxNm ?? 0), i.y - i.h / 2 + (mv?.dyNm ?? 0)];
      ctx.save();
      ctx.globalAlpha *= fpObjectOpacity(o.layerOpacity, "bitmaps") * layerAlpha(opts, layerStateKey(i.layer)); // Images' opacity (`m_imageOpacity`), the contrast mode
      ctx.strokeStyle = o.selection.has(i.id) ? layerColor("selection") : layerColor(realLayerKey(i.layer));
      ctx.lineWidth = hairlineNm(view, 1.5);
      ctx.strokeRect(x0, y0, i.w, i.h);
      ctx.beginPath();
      ctx.moveTo(x0, y0);
      ctx.lineTo(x0 + i.w, y0 + i.h);
      ctx.moveTo(x0 + i.w, y0);
      ctx.lineTo(x0, y0 + i.h);
      ctx.stroke();
      ctx.restore();
    }
  }
}
