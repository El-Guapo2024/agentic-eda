// A board footprint's text boxes, barcodes and points (UI-LEFT P17) and its dimensions and tables (UI-LEFT PP14) on the canvas: `PCB_PAINTER::draw( const PCB_TEXTBOX* )`,
// `draw( const PCB_BARCODE* )`, `draw( const PCB_POINT* )`, `draw( const PCB_DIMENSION_BASE* )` and `draw( const PCB_TABLE* )` for the items KiCad parents to a footprint, by the
// painters of the board's own items (pcbItemsPaint.ts, painter.ts `drawDimensions`, tablePaint.ts `drawTables`). They are in board space (the backend placed them,
// kicad-port/fpBoardItems.ts), so the transform of a footprint in the hand (painter.ts `drawFootprint`) carries them with the rest; a selected footprint draws them selected
// (`PCB_SELECTION_TOOL::highlight` marks its children). Its reference images are drawn under everything with the board's (`drawRefImages`).
import type { BoardState, Part } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { layerIsVisible } from "../../kicad-port/layerPresets";
import { fpBarcodes, fpDimensions, fpPoints, fpTables, fpTextBoxes } from "../../kicad-port/fpBoardItems";
import { requestBarcodeShape } from "./barcodeCache";
import { drawBarcode, drawPoint, drawTextBox } from "./pcbItemsPaint";
import { footprintShown } from "./appearancePaint";
import { drawDimensions, withLayer, type PaintOptions } from "./painter";
import { drawTables } from "./tablePaint";
import { layerStateKey } from "../../kicad-port/layerPresets";

/** Draw the text boxes, barcodes, points, dimensions and tables of `part`; nothing when it has none or its side is not shown. */
export function drawFootprintItems(ctx: CanvasRenderingContext2D, view: ViewTransform, part: Part, opts: PaintOptions): void {
  if (!part.items || !footprintShown(opts.layerVisible, part)) return;
  const selected = opts.selection.has(part.ref);
  for (const b of fpTextBoxes(part)) {
    if (!layerIsVisible(opts.layerVisible, b.layer)) continue;
    withLayer(ctx, opts, layerStateKey(b.layer), 1, () => drawTextBox(ctx, view, b, selected, !!opts.sketchText));
  }
  for (const b of fpBarcodes(part)) {
    if (!layerIsVisible(opts.layerVisible, b.layer)) continue;
    requestBarcodeShape(b);
    withLayer(ctx, opts, layerStateKey(b.layer), 1, () => drawBarcode(ctx, view, b, selected));
  }
  for (const p of fpPoints(part)) drawPoint(ctx, view, p, selected, opts);
  // the board's own painters, each item selected with its footprint
  const dimensions = fpDimensions(part);
  const tables = fpTables(part);
  if (dimensions.length || tables.length) {
    const own = new Set<string>(selected ? [...dimensions.map((d) => d.id), ...tables.map((t) => t.id)] : []);
    const itemOpts: PaintOptions = { ...opts, dimensionPreview: null, selection: own };
    if (dimensions.length) drawDimensions(ctx, view, { drawings: { dimensions } } as unknown as BoardState, itemOpts);
    if (tables.length) drawTables(ctx, view, { drawings: { tables } } as unknown as Pick<BoardState, "drawings">, itemOpts);
  }
}
