// The Footprint Editor's canvas: the footprint's fields and the items `eda_model::fp_items` adds (UI-LEFT F1, F2), drawn by the board's own painters (`PCB_PAINTER`
// is one painter for both editors): the fields by `painter.ts::drawFields`, the text boxes, barcodes and points by `pcbItemsPaint.ts`, the tables by
// `tablePaint.ts`, the dimensions by `painter.ts::drawDimensions` and the rule areas by `drawRuleArea`, each on the footprint's display shapes
// (`GET /api/footprint`'s `display`, `crates/cli/src/fp_items_json.rs`) in the footprint's frame, which is the canvas's.
//
// A reference image is drawn with its pixels (`GET /api/pcb_image?fp=..`, the browser decoding the file) and a barcode with the symbol kicad-cli plots of it
// (`GET /api/barcode_shape?fp=..`), as the board's; a zone, on each of its layers, by the board's zone painter: its outline and border hatch, and the fill it brought
// (`display.zone_fills`: the Footprint Editor runs no zone filler, `FOOTPRINT_EDIT_FRAME` registers no `ZONE_FILLER_TOOL`, and draws `m_FilledPolysList` as it is) in
// the frame's zone display mode, filled (UI-LEFT-2 FF6).
import type { FieldInfo } from "../../api/types";
import type { FootprintItemsDisplay } from "../../api/fpItemTypes";
import type { ViewTransform } from "../../kicad-port/view";
import { drawDimensions, drawFields, drawRuleArea, layerAlpha, paintZoneOnLayer, realLayerKey, withAlpha, type PaintOptions } from "../canvas/painter";
import { PLAIN_APPEARANCE } from "../canvas/appearancePaint";
import { fpObjectOpacity, fpOpacities } from "../../kicad-port/fpAppearance";
import { drawBarcode, drawPcbItems, paintRefImage } from "../canvas/pcbItemsPaint";
import { requestBarcodeShape } from "../canvas/barcodeCache";
import { pcbImageUrl } from "../../kicad-port/refImage";
import { parseCssColor } from "../../kicad-port/appearance";
import { drawTables } from "../canvas/tablePaint";
import { layerColor } from "../canvas/layers";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { objectOn } from "../../kicad-port/appearance";
import type { BoardState, Dimension, Zone } from "../../api/types";

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
  /** The footprint's name in the library (`GET /api/footprint?name=`): the image and barcode routes find its items by it. */
  footprint?: string;
  /** Flip Board View is on: a text, field, text box or dimension not on one side's layer is drawn to read as it did (`IsFlippedX() && !IsSideSpecific()`, UI-LEFT-3 F3-9). */
  viewFlipped?: boolean;
}

/** The slice of the board painter's options these painters read, from the footprint editor's own. */
function boardOptions(o: FpItemsPaintOptions): PaintOptions {
  return {
    textRecordTag: "fp",
    viewFlipped: !!o.viewFlipped,
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
    // High Contrast Mode as `GetColor` dims: the colour mixed into the background, not a lowered alpha (UI-LEFT-2 FF5)
    dimByColour: true,
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

/** The dimension Draw Dimension holds, as the server works it out (`POST /api/dimension/preview`): `DrawDimension`'s preview is the dimension itself
 * (`m_view->AddToPreview( dimension )`), its arrows and text included, in its layer's colour (UI-LEFT-2 FF8). */
export function paintFpDimensionPreview(ctx: CanvasRenderingContext2D, view: ViewTransform, dim: Dimension): void {
  const o: FpItemsPaintOptions = { selection: new Set(), layerVisible: {}, layerOpacity: {}, highContrast: false, activeLayer: dim.layer, sketchText: false, sketchGraphics: false, movePreview: null };
  drawDimensions(ctx, view, { drawings: { shapes: [], texts: [], groups: [], dimensions: [dim], tables: [] } } as unknown as BoardState, boardOptions(o));
}

/** The dimensions, zones, text boxes, points, barcodes, images and tables of the footprint. */
export function paintFpItems(ctx: CanvasRenderingContext2D, view: ViewTransform, d: FootprintItemsDisplay, o: FpItemsPaintOptions): void {
  const opts = boardOptions(o);
  const board = (drawings: Partial<NonNullable<BoardState["drawings"]>>) => ({ drawings: { shapes: [], texts: [], groups: [], dimensions: [], tables: [], ...drawings } }) as unknown as BoardState;
  // zones first, under the rest (`ZONE` sits under the graphics in the view's order)
  if (objectOn(o.layerVisible, "zones")) {
    // `PCB_PAINTER::draw( const ZONE* )` on each layer of the zone (`ZONE::ViewGetLayers`), in that layer's colour: the fill the zone brought, then its outline and
    // border hatch; a rule area has its outline and hatch only
    const fillOpts = { ...opts, zoneFill: { zones: d.zone_fills ?? [], source: null }, zoneDisplayMode: "filled" } as PaintOptions;
    const zoneAlpha = fpObjectOpacity(o.layerOpacity, "zones");
    inTwoPasses(ctx, d.zones, o, (zones) => {
      for (const z of zones) {
        const selected = o.selection.has(z.id);
        const asBoard = { ...z, outline: z.outline_xy, holes: z.holes_xy } as unknown as Zone;
        const layers = (z as { layers?: string[] }).layers ?? [z.layer];
        for (const layer of layers) {
          if (!layerIsVisible(o.layerVisible, layer)) continue;
          const color = layerColor(realLayerKey(layer));
          const dim = layerAlpha(opts, layerStateKey(layer));
          if (z.is_rule_area) {
            withAlpha(ctx, dim, () => drawRuleArea(ctx, view, asBoard, selected, color));
            continue;
          }
          paintZoneOnLayer(ctx, view, asBoard, layer, color, zoneAlpha * dim, selected, fillOpts, dim);
        }
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
  // a barcode: `drawBarcode` with the shape kicad-cli plots of it (`GET /api/barcode_shape?fp=..`), its box crossed until it arrives
  inTwoPasses(ctx, d.barcodes, o, (barcodes) => {
    for (const b of barcodes) {
      if (!layerIsVisible(o.layerVisible, b.layer)) continue;
      requestBarcodeShape(b, o.footprint);
      withAlpha(ctx, layerAlpha(opts, layerStateKey(b.layer)), () => drawBarcode(ctx, view, b, o.selection.has(b.id)));
    }
  });
  // a reference image: `PCB_PAINTER::draw( const PCB_REFERENCE_IMAGE* )` with its pixels (`GET /api/pcb_image?fp=..`), at its layer colour's alpha times the Images opacity
  if (objectOn(o.layerVisible, "bitmaps")) {
    const opacity = fpObjectOpacity(o.layerOpacity, "bitmaps");
    for (const i of d.images) {
      if (!layerIsVisible(o.layerVisible, i.layer) || opacity <= 0) continue;
      const mv = moved(o, i.id);
      const shown = mv ? { ...i, x: i.x + mv.dxNm, y: i.y + mv.dyNm } : i;
      const colourAlpha = parseCssColor(layerColor(realLayerKey(i.layer)))?.a ?? 1;
      withAlpha(ctx, layerAlpha(opts, layerStateKey(i.layer)), () => paintRefImage(ctx, view, shown, colourAlpha * opacity, o.selection.has(i.id), pcbImageUrl(i.id, i.hash, o.footprint)));
    }
  }
}
