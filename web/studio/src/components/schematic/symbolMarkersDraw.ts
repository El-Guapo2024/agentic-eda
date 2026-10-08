// Strokes the marks of a symbol's attributes (`SCH_PAINTER::draw( SCH_SYMBOL )`): the Do not Populate cross and the Exclude from Simulation frame --
// the geometry is `symbolMarkers.ts`.
import { layerColor } from "../canvas/layers";
import { DEFAULT_LINE_UM, dnpCross, simExclusionMark, type Box } from "./symbolMarkers";

export interface SymbolMarkFlags {
  dnp?: boolean;
  exclude_from_sim?: boolean;
}

/** `markSim` is the View menu's "Mark items excluded from simulation". */
export function drawSymbolMarkers(ctx: CanvasRenderingContext2D, body: Box, all: Box, flags: SymbolMarkFlags, markSim: boolean): void {
  if (flags.dnp) {
    const color = layerColor("LAYER_DNP_MARKER");
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineCap = "round";
    ctx.lineWidth = 3 * DEFAULT_LINE_UM;
    ctx.beginPath();
    for (const [a, b] of dnpCross(body, all)) {
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
    }
    ctx.stroke();
    ctx.restore();
  }
  if (markSim && flags.exclude_from_sim) {
    const m = simExclusionMark(body);
    const color = layerColor("LAYER_EXCLUDED_FROM_SIM");
    ctx.save();
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineJoin = "miter";
    ctx.lineWidth = m.pen;
    ctx.strokeRect(m.frame.minX, m.frame.minY, m.frame.maxX - m.frame.minX, m.frame.maxY - m.frame.minY);
    ctx.globalAlpha = 0.1;
    ctx.beginPath();
    ctx.arc(m.center[0], m.center[1], m.radius, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.lineWidth = m.pen / 2;
    ctx.beginPath();
    ctx.moveTo(m.curve[0][0], m.curve[0][1]);
    ctx.bezierCurveTo(m.curve[1][0], m.curve[1][1], m.curve[2][0], m.curve[2][1], m.curve[3][0], m.curve[3][1]);
    ctx.stroke();
    ctx.restore();
  }
}
