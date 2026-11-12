// The Footprint Editor canvas's side of the drawing tools for the items `eda_model::fp_items` adds (UI-LEFT F2): what a click, a motion, Enter or a double click does with
// each of them, and their previews. What each tool makes is kicad-port/fpDrawTools.ts; the commit is one `fp_item add` (the store's `addItem`), after which the new item is
// selected (`m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, .. )`) and the tool stays armed for the next one, as `DRAWING_TOOL`'s loops do (pcbnew/tools/drawing_tool.cpp
// at KiCad 8303b2ad):
//   Draw Dimension (5 kinds)  DrawDimension: origin, end (and height for an aligned or orthogonal one); a leader then opens its dialog ("Run the edit immediately to set the leader text")
//   Draw Rule Area            DrawZone( ZONE_MODE::ADD, rule area): Rule Area Properties at the first corner (`ZONE_CREATE_HELPER::OnFirstPoint` -> `createNewZone`; Cancel
//                             drops the click), then the outline in the line mode (`POLYGON_GEOM_MANAGER`, kicad-port/pcbDrawZone.ts), added as it closes (`OnComplete`)
//   Draw Text Box             DrawRectangle( drawTextBox ): two corners (the same spot twice ends the tool), then Text Box Properties
//   Place Point               PlacePoint (`IPO_REPEAT | IPO_SINGLE_CLICK`): a point at every click
//   Draw Table                DrawTable: the first corner, the cursor sizes the grid, the second click opens Table Properties
//   Add Barcode               DrawBarcode: a click opens Barcode Properties on a new barcode
//   Place Reference Image     PlaceReferenceImage: a file chooser, the image follows the cursor by its centre, a click puts it down; the next click asks for another file
// The dialogs of a tool's new item add it on OK and drop it on Cancel (components/footprint/FpItemDialogs.tsx); a tool's own dialog then waits for the next click.
//
// Differs from KiCad: an aligned dimension's height step stays on the grid when its line is not cardinal (KiCad turns the grid off there).
import type { Dispatch } from "react";
import type { ViewTransform } from "../../kicad-port/view";
import { hairlineNm } from "../../kicad-port/view";
import { FP_DIM_TOOL_KIND, type FootprintEditorApi, type FootprintEditorState, type FpAction, type FpToolId } from "../../state/footprintEditorStore";
import { fpDimensionClick, fpDimensionMotion, fpDimensionPreview, fpNewBarcode, fpNewImage, fpNewPoint, fpNewRuleArea } from "../../kicad-port/fpDrawTools";
import { leaderModeOf, zoneClick, zoneFinish } from "../../kicad-port/pcbDrawZone";
import { shapeSecondPoint } from "../../kicad-port/pcbShapeSnap";
import { newTextBox } from "../../kicad-port/pcbTextBox";
import { drawTableSession } from "../../kicad-port/tables";
import { imageSizeNm, SCALE_PPM } from "../../kicad-port/refImage";
import { chooseImageFile, loadPickedImage, readImageFile, type PickedImage } from "../../actions/imageFiles";
import { DEFAULT_ZONE_SETTINGS } from "../../state/store";
import { drawTablePreview } from "../canvas/tablePaint";
import { layerColor } from "../canvas/layers";
import type { PaintOptions } from "../canvas/painter";
import type { Dimension } from "../../api/types";
import { paintFpDimensionPreview } from "./fpItemsPaint";
import type { FootprintPointOpts } from "./footprintSnap";

export interface FpToolCtx {
  state: FootprintEditorState;
  dispatch: Dispatch<FpAction>;
  api: FootprintEditorApi;
}

type P = [number, number];

/**
 * Where each tool's `BestSnapAnchor` looks and which grid it holds the cursor to: a dimension every layer and the graphics grid (`nullptr, GRID_GRAPHICS`), a rule area, a
 * text box and an image the active layer and the graphics grid, a table and a barcode the active layer and the text grid (`GRID_TEXT`), a point every layer and the
 * current grid (`doInteractiveItemPlacement`'s `BestSnapAnchor( cursorPos, nullptr )`). Null for a tool that is not one of these.
 */
export function fpItemToolSnap(tool: FpToolId): FootprintPointOpts | null {
  if (FP_DIM_TOOL_KIND[tool]) return { category: "graphics", layers: "all" };
  switch (tool) {
    case "rule_area":
    case "textbox":
    case "ref_image":
      return { category: "graphics" };
    case "table":
    case "barcode":
      return { category: "text" };
    case "point":
      return { category: "current", layers: "all" };
    default:
      return null;
  }
}

/** `angleSnap != LEADER_MODE::DIRECT` with Ctrl unheld: a dimension's end is held to 45 degrees. */
const constrainedOf = (state: FootprintEditorState, ctrl: boolean): boolean => state.lineMode !== "direct" && !ctrl;

/** The new item is added as one undo step and selected; a leader's dialog opens on it. */
async function addAndSelect(c: FpToolCtx, item: Parameters<FootprintEditorApi["addItem"]>[0], openDialog?: string): Promise<void> {
  const id = await c.api.addItem(item);
  if (!id) return;
  c.dispatch({ type: "SET_SELECTION", refs: [id] });
  if (openDialog) c.dispatch({ type: "SET_ITEM_DIALOG", dialog: { id, kind: openDialog } });
}

/** A left click (a snapped point `at`) of a tool of the fp_items kinds. */
export function fpItemToolClick(c: FpToolCtx, at: P, ctrl: boolean): void {
  const { state, dispatch } = c;
  const tool = state.activeTool;
  const layer = state.activeLayer;
  const dimKind = FP_DIM_TOOL_KIND[tool];
  if (dimKind) {
    const prev = state.itemDraw?.tool === "dimension" ? state.itemDraw.draw : null;
    if (!prev) dispatch({ type: "SET_SELECTION", refs: [] }); // SET_ORIGIN: `ACTIONS::selectionClear`
    const r = fpDimensionClick(prev, dimKind, layer, at, constrainedOf(state, ctrl));
    dispatch({ type: "SET_ITEM_DRAW", draw: r.draw ? { tool: "dimension", draw: r.draw } : null });
    if (r.commit) void addAndSelect(c, { dimension: r.commit }, dimKind === "leader" ? "dimension" : undefined);
    return;
  }
  switch (tool) {
    case "rule_area": {
      // `DrawZone` with `POLYGON_GEOM_MANAGER` in the line mode: a click on the first corner closes the outline
      const already = state.drawState?.shapeKind === "polygon" ? state.drawState.pts : [];
      if (already.length === 0) {
        // `ZONE_CREATE_HELPER::OnFirstPoint`: `createNewZone` asks for the rule area's settings (the footprint editor's default keepouts on the active layer) before
        // the first corner is taken; OK starts the outline there (components/footprint/FpItemDialogs.tsx), Cancel leaves the tool waiting.
        dispatch({ type: "SET_PENDING_ITEM", item: { kind: "zone", zone: fpNewRuleArea(layer, [at], DEFAULT_ZONE_SETTINGS), first: at } });
        return;
      }
      const mode = leaderModeOf(state.lineMode, ctrl);
      const click = zoneClick(already, at, mode);
      if (click.finish) finishRuleArea(c, zoneFinish(already, at, mode));
      else dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind: "polygon", pts: click.pts } });
      return;
    }
    case "textbox": {
      const first = state.drawState?.shapeKind === "rect" ? state.drawState.pts[0] : undefined;
      if (!first) {
        dispatch({ type: "SET_SELECTION", refs: [] });
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "shape", shapeKind: "rect", pts: [at] } });
        return;
      }
      const end = shapeSecondPoint("rect", state.lineMode, ctrl, first, at);
      dispatch({ type: "SET_DRAW_STATE", draw: null });
      // "User has clicked twice in the same spot, meaning we're finished"
      if (end[0] === first[0] && end[1] === first[1]) {
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
        return;
      }
      dispatch({ type: "SET_PENDING_ITEM", item: { kind: "textbox", box: newTextBox({}, layer, first, end) } });
      return;
    }
    case "point":
      void addAndSelect(c, { point: fpNewPoint(layer, at) });
      return;
    case "table": {
      const held = state.itemDraw?.tool === "table" ? state.itemDraw.origin : null;
      if (!held) {
        dispatch({ type: "SET_SELECTION", refs: [] });
        dispatch({ type: "SET_ITEM_DRAW", draw: { tool: "table", origin: at } });
        return;
      }
      const s = drawTableSession(undefined, layer, held, at, state.gridNm);
      dispatch({ type: "SET_ITEM_DRAW", draw: null });
      dispatch({ type: "SET_PENDING_ITEM", item: { kind: "table", drawn: { layer: s.layer, at: held, cols: s.grid.cols, rows: s.grid.rows, cellW: s.grid.cellW, cellH: s.grid.cellH, lineWidth: s.lineWidth } } });
      return;
    }
    case "barcode":
      dispatch({ type: "SET_SELECTION", refs: [] });
      dispatch({ type: "SET_PENDING_ITEM", item: { kind: "barcode", barcode: fpNewBarcode(layer, at) } });
      return;
    case "ref_image": {
      const held = state.itemDraw?.tool === "ref_image" ? state.itemDraw : null;
      if (!held) {
        void fpChooseImage(c);
        return;
      }
      dispatch({ type: "SET_ITEM_DRAW", draw: null });
      void addAndSelect(c, { image: fpNewImage(layer, at, held.data) });
      return;
    }
    default:
      return;
  }
}

/** `ZONE_CREATE_HELPER::OnComplete`: the closed outline becomes the new rule area with the settings asked at its first corner, added and selected; the next one asks again. */
function finishRuleArea(c: FpToolCtx, outline: [number, number][] | null): void {
  c.dispatch({ type: "SET_DRAW_STATE", draw: null });
  const zone = c.state.itemDraw?.tool === "rule_area" ? c.state.itemDraw.zone : null;
  c.dispatch({ type: "SET_ITEM_DRAW", draw: null });
  if (zone && outline && outline.length >= 3) void addAndSelect(c, { zone: { ...zone, outline: outline.map(([x, y]) => ({ x, y })) } });
}

/** Enter or a double click while a tool of these kinds runs: a rule area's outline is closed; the others take nothing from it (a double click was its clicks). */
export function fpItemToolFinish(c: FpToolCtx, at: readonly [number, number] | null, ctrl: boolean): void {
  const draw = c.state.drawState;
  if (c.state.activeTool !== "rule_area" || draw?.shapeKind !== "polygon") return;
  const mode = leaderModeOf(c.state.lineMode, ctrl);
  finishRuleArea(c, at ? (zoneFinish(draw.pts, [at[0], at[1]], mode) ?? null) : draw.pts.length >= 3 ? draw.pts : null);
}

/** The motion of a tool of these kinds: a dimension's end or height follows the snapped cursor (`DrawDimension`'s `IsMotion`). */
export function fpItemToolMotion(c: FpToolCtx, at: P, ctrl: boolean): void {
  const d = c.state.itemDraw;
  if (d?.tool !== "dimension") return;
  const next = fpDimensionMotion(d.draw, at, constrainedOf(c.state, ctrl));
  if (JSON.stringify(next.dim) !== JSON.stringify(d.draw.dim)) c.dispatch({ type: "SET_ITEM_DRAW", draw: { tool: "dimension", draw: next } });
}

/**
 * `PlaceReferenceImage`'s file: the browser's chooser (or an image already chosen), then the image held at the cursor by its centre, at scale 1. A cancelled chooser leaves
 * the tool waiting; a file that is no image says so (`_( "Could not load image from '%s'." )`).
 */
export async function fpChooseImage(c: Pick<FpToolCtx, "dispatch">, given?: PickedImage | null): Promise<void> {
  try {
    let picked = given ?? null;
    if (!picked) {
      const file = await chooseImageFile();
      if (!file) return;
      picked = await readImageFile(file);
    }
    const loaded = await loadPickedImage(picked);
    const [w, h] = imageSizeNm(loaded.header.px, loaded.header.ppi, SCALE_PPM);
    c.dispatch({ type: "SET_ITEM_DRAW", draw: { tool: "ref_image", name: loaded.name, data: loaded.data, img: loaded.img, w, h } });
  } catch (e) {
    c.dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
  }
}

/** The previews these tools draw themselves: a dimension being drawn, the grid of a table being sized, the image the cursor holds. A polygon's and a box's are `drawState`'s. */
export function paintFpItemToolPreview(ctx: CanvasRenderingContext2D, view: ViewTransform, state: FootprintEditorState, cursor: P | null, dimPreview?: Dimension | null): void {
  const d = state.itemDraw;
  if (!d) return;
  if (d.tool === "dimension" && dimPreview) {
    // `DrawDimension`'s preview: the dimension as it will be, its arrows and text worked out by the server (UI-LEFT-2 FF8)
    paintFpDimensionPreview(ctx, view, dimPreview);
    return;
  }
  if (d.tool === "dimension") {
    // until the server's reply: the dimension's lines
    ctx.save();
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = Math.max(d.draw.dim.stroke_width, hairlineNm(view, 1.5));
    ctx.setLineDash([hairlineNm(view, 5), hairlineNm(view, 3)]);
    ctx.beginPath();
    for (const [a, b] of fpDimensionPreview(d.draw.dim)) {
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
    }
    ctx.stroke();
    ctx.restore();
    return;
  }
  if (!cursor || d.tool === "rule_area") return;
  if (d.tool === "table") {
    const opts = { cursorNm: { x: cursor[0], y: cursor[1] }, snappedCursor: cursor, activeLayer: state.activeLayer, gridNm: state.gridNm } as unknown as PaintOptions;
    drawTablePreview(ctx, view, {}, d.origin, opts);
    return;
  }
  // `m_view->AddToPreview( image )`: the image as it will be, centred on the cursor
  ctx.save();
  ctx.globalAlpha *= 0.8;
  ctx.drawImage(d.img, cursor[0] - d.w / 2, cursor[1] - d.h / 2, d.w, d.h);
  ctx.restore();
}
