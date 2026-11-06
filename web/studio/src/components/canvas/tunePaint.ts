// The length tuner on the canvas (kicad-port/tuneTool.ts, crates/cli/src/tune_api.rs): the track under the cursor with its status box, the pattern being placed (the
// new line, its outline, the box), the dashed baseline, outline and edit points of a selected pattern. KiCad: `PCB_TUNING_PATTERN::ViewDraw`,
// `TUNING_STATUS_VIEW_ITEM::ViewDraw`, `ROUTER_PREVIEW_ITEM`.
import type { BoardState } from "../../api/types";
import type { TuneHoverReply, TuneReadout, TuningPattern } from "../../api/tuneTypes";
import type { ViewTransform } from "../../state/store";
import { copperColorKey, layerColor } from "./layers";
import { hairlineNm } from "./view";
import { drawStrokeText } from "../text/strokeFont";
import { formatLength, type LengthUnit } from "../../state/units";
import { patternBaselines, patternOutline, readoutBox, statusBoxLayout, type TuneDrawState } from "../../kicad-port/tuneTool";

type Pt = [number, number];

function dashed(ctx: CanvasRenderingContext2D, view: ViewTransform, pts: readonly Pt[], color: string, widthPx: number, closed: boolean, dash: [number, number]) {
  if (pts.length < 2) return;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = hairlineNm(view, widthPx);
  ctx.setLineDash([hairlineNm(view, dash[0]), hairlineNm(view, dash[1])]);
  ctx.beginPath();
  pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  if (closed) ctx.closePath();
  ctx.stroke();
  ctx.restore();
}

/** The four edit points (`EDIT_POINT`): small squares, outlined in the selection colour. */
function drawHandles(ctx: CanvasRenderingContext2D, view: ViewTransform, handles: readonly [number, number][]) {
  const half = hairlineNm(view, 4);
  ctx.save();
  ctx.lineWidth = hairlineNm(view, 1.5);
  ctx.strokeStyle = layerColor("selection");
  ctx.fillStyle = layerColor("background");
  handles.forEach(([x, y]) => {
    ctx.beginPath();
    ctx.rect(x - half, y - half, half * 2, half * 2);
    ctx.fill();
    ctx.stroke();
  });
  ctx.restore();
}

/**
 * `TUNING_STATUS_VIEW_ITEM::ViewDraw` at the mouse position: the drop shadow box (`wxSYS_COLOUR_BTNFACE` at 0.9, a 2 px `wxSYS_COLOUR_BTNTEXT` line) above and to the
 * right of the cursor, the label, `min` and `max` over the length and the range -- each green when it is inside and red when it is not (`FromHSL( 0, 1, 0.3 )` and
 * `FromHSL( 120, 1, 0.2 )` on a light background). The box is the same size on the screen at every zoom ([`statusBoxLayout`]).
 */
export function drawReadoutBox(ctx: CanvasRenderingContext2D, view: ViewTransform, readout: TuneReadout, at: Pt, units: LengthUnit) {
  const box = readoutBox(readout, (nm) => formatLength(nm, units));
  const L = statusBoxLayout(at, (n) => hairlineNm(view, n));
  const [x0, y0, x1, y1] = L.box;
  ctx.save();
  ctx.fillStyle = "rgba(240, 240, 240, 0.9)";
  ctx.strokeStyle = "rgb(0, 0, 0)";
  ctx.lineWidth = hairlineNm(view, 2);
  ctx.beginPath();
  ctx.rect(x0, y0, x1 - x0, y1 - y0);
  ctx.fill();
  ctx.stroke();
  ctx.restore();
  const green = "rgb(0, 102, 0)";
  const red = "rgb(153, 0, 0)";
  const normal = "rgb(0, 0, 0)";
  // (the rows' y is the texts' centre, `GR_TEXT_V_ALIGN_CENTER`; the stroke font is anchored on its baseline, half a glyph below)
  box.header.forEach((label, i) => drawStrokeText(ctx, label, L.header.xs[i]!, L.header.y + L.header.size / 2, { sizeNm: L.header.size, justify: "left", color: normal, thicknessNm: L.header.stroke }));
  box.values.forEach((value, i) => drawStrokeText(ctx, value, L.values.xs[i]!, L.values.y + L.values.size / 2, { sizeNm: L.values.size, justify: "left", color: box.bad[i] ? red : green, thicknessNm: L.values.stroke }));
}

/** The track under the cursor with the tool armed (`updateHoverStatus`): its line highlighted, and the box. */
export function drawTuneHover(ctx: CanvasRenderingContext2D, view: ViewTransform, hover: { reply: TuneHoverReply; at: Pt }, units: LengthUnit) {
  const { reply, at } = hover;
  const width = reply.pick?.width ?? 0;
  const layer = reply.pick?.layer ?? "F.Cu";
  if (reply.path && reply.path.length >= 2) {
    ctx.save();
    ctx.globalAlpha = 0.45;
    ctx.strokeStyle = layerColor("selection");
    ctx.lineWidth = Math.max(width, hairlineNm(view, 2)) + hairlineNm(view, 3);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    reply.path.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
    ctx.restore();
  }
  void layer;
  if (reply.readout) drawReadoutBox(ctx, view, reply.readout, at, units);
}

/** The dashed baseline, the dashed outline and the edit points of a pattern (`PCB_TUNING_PATTERN::ViewDraw` while it is selected or new). */
export function drawPatternFrame(ctx: CanvasRenderingContext2D, view: ViewTransform, p: TuningPattern, handles: readonly [number, number][] | undefined, withBaseline: boolean) {
  const color = layerColor("selection");
  if (withBaseline) for (const line of patternBaselines(p)) dashed(ctx, view, line, color, 1.5, false, [5, 3]);
  for (const poly of patternOutline(p)) dashed(ctx, view, poly, color, 1, true, [8, 4]);
  if (handles && handles.length > 0) drawHandles(ctx, view, handles);
}

/** The patterns of the board that are selected. */
export function drawSelectedPatterns(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, selection: ReadonlySet<string>) {
  for (const p of board.drawings?.tuning_patterns ?? []) {
    if (selection.has(p.id)) drawPatternFrame(ctx, view, p, p.handles, true);
  }
}

/** A pattern being placed (or edited): the new line over the board, its frame, and the box at the cursor. */
export function drawTune(ctx: CanvasRenderingContext2D, view: ViewTransform, draw: TuneDrawState, cursor: Pt | null, units: LengthUnit) {
  for (const piece of draw.pieces) {
    ctx.save();
    ctx.globalAlpha = piece.member ? 0.85 : 0.6;
    ctx.strokeStyle = layerColor(copperColorKey(piece.layer));
    ctx.lineWidth = Math.max(piece.width, hairlineNm(view, 1));
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    piece.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
    ctx.restore();
  }
  if (draw.pattern && draw.moved) drawPatternFrame(ctx, view, draw.pattern, draw.handles, true);
  else if (draw.pattern) {
    // the start of the pattern, before the cursor has moved: a dot on the track
    ctx.save();
    ctx.fillStyle = layerColor("selection");
    ctx.beginPath();
    ctx.arc(draw.origin[0], draw.origin[1], hairlineNm(view, 4), 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
  if (draw.readout && cursor) drawReadoutBox(ctx, view, draw.readout, cursor, units);
}
