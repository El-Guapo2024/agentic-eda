// The in-progress "Draw Arc" overlay (KIGFX::PREVIEW::ARC_ASSISTANT, driven by
// kicad-port/arcGeom.ts's ARC_GEOM_MANAGER port): the radius line(s) of the
// construction plus the arc as it would be drawn, and the radius/angle readout
// ARC_ASSISTANT prints. Shared by the board painter and the footprint editor's
// painter -- both draw the same tool.
import { ARC_SET_START, arcEndRadiusEnd, arcHasPreview, arcPointIndex, arcStartRadiusEnd, arcSubtended, arcSweep, arcUsesExactEndpoints, type ArcGeom } from "../../kicad-port/arcGeom";
import { drawStrokeText } from "../text/strokeFont";
import { formatLength, type LengthUnit } from "../../state/units";

export interface PreviewStyle {
  color: string;
  /** `hairlineNm( view, px )`: a width that is `px` screen pixels wide at the current zoom. */
  hair: (px: number) => number;
  units: LengthUnit;
}

/** `ctx` already carries the view transform (world micrometres); strokes are dashed, like every other not-yet-committed preview. */
export function drawArcPreview(ctx: CanvasRenderingContext2D, g: ArcGeom, style: PreviewStyle): void {
  if (arcUsesExactEndpoints(g)) return drawEndpointGuides(ctx, g, style);
  if (g.step < ARC_SET_START) return;
  const { color, hair, units } = style;
  const [ox, oy] = g.origin;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = hair(1.5);
  ctx.setLineDash([hair(4), hair(3)]);
  ctx.lineCap = "round";

  const [sx, sy] = arcStartRadiusEnd(g);
  if (g.step === ARC_SET_START) {
    // Centre locked, waiting for the start point: the radius line follows the cursor (`setStart` runs on every motion).
    ctx.beginPath();
    ctx.moveTo(ox, oy);
    ctx.lineTo(sx, sy);
    ctx.stroke();
    ctx.setLineDash([]);
    centreMark(ctx, ox, oy, hair);
    if (g.radius > 0) label(ctx, `R ${formatLength(g.radius, units)}`, (ox + sx) / 2, (oy + sy) / 2 - hair(8), style);
    ctx.restore();
    return;
  }

  // Start locked: both radius lines (the start radius and the end radius at the cursor's angle), then the arc.
  const [ex, ey] = arcEndRadiusEnd(g);
  ctx.beginPath();
  ctx.moveTo(sx, sy);
  ctx.lineTo(ox, oy);
  ctx.lineTo(ex, ey);
  ctx.stroke();
  const sweep = arcSweep(g);
  if (sweep) {
    const a0 = (sweep.startAngle * Math.PI) / 180;
    ctx.beginPath();
    // `ctx.arc` measures angles clockwise from +x on the y-down board -- the same direction of increasing `atan2` angle the stored arc runs in.
    ctx.arc(ox, oy, g.radius, a0, a0 + (sweep.sweep * Math.PI) / 180, false);
    ctx.lineWidth = hair(2);
    ctx.stroke();
  }
  ctx.setLineDash([]);
  centreMark(ctx, ox, oy, hair);
  // ARC_ASSISTANT's readout: the radius, and the angle swept so far.
  label(ctx, `R ${formatLength(g.radius, units)}   ${Math.abs(arcSubtended(g)).toFixed(1)}°`, ox, oy - g.radius - hair(14), style);
  ctx.restore();
}

function centreMark(ctx: CanvasRenderingContext2D, x: number, y: number, hair: (px: number) => number): void {
  ctx.beginPath();
  ctx.arc(x, y, hair(2.5), 0, Math.PI * 2);
  ctx.fill();
}

function label(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, style: PreviewStyle): void {
  drawStrokeText(ctx, text, x, y, { sizeNm: style.hair(12), justify: "center", color: style.color, thicknessNm: style.hair(1.4) });
}

/**
 * KiCad 25ec2335's `ARC_ASSISTANT::drawEndpointGuides` for the modes that keep the clicked ends (start-end-mid, start-end-centre, tangent, start-direction-end): the chord
 * the first two clicks fixed (the cursor chasing its end until then) with the bisector guide to the marked mid point or centre and the radii of a centre; or the tangent
 * ray and the line to the cursor (then to the end); the radius and the sweep as a readout. The arc itself is the caller's preview.
 */
function drawEndpointGuides(ctx: CanvasRenderingContext2D, g: ArcGeom, style: PreviewStyle): void {
  if (!arcHasPreview(g)) return;
  const index = arcPointIndex(g);
  // "The first point is only being placed"
  if (index === 0 || !g.pts) return;
  const { color, hair, units } = style;
  const mode = g.mode;
  const sol = g.solution;
  const start = g.pts[0];
  const cursor = g.lastPoint;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = hair(1);
  const line = (a: readonly [number, number], b: readonly [number, number], dashed: boolean) => {
    ctx.setLineDash(dashed ? [hair(12), hair(6)] : []);
    ctx.beginPath();
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
    ctx.stroke();
  };
  // "Long guides, such as the line to a far away center, are cut so the dashes stay bounded"
  const guide = (a: readonly [number, number], b: readonly [number, number]) => {
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const len = Math.hypot(dx, dy);
    const limit = hair(400);
    line(a, len > limit ? [a[0] + (dx * limit) / len, a[1] + (dy * limit) / len] : b, true);
  };
  const readout: string[] = [];
  if (mode === "start_end_mid" || mode === "start_end_center") {
    const chordEnd = index === 1 ? cursor : g.pts[1];
    const mid: [number, number] = [(start[0] + chordEnd[0]) / 2, (start[1] + chordEnd[1]) / 2];
    line(start, chordEnd, index > 1);
    if (index > 1 && sol?.valid) {
      const marked = mode === "start_end_mid" ? sol.mid : sol.center;
      guide(mid, marked);
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.arc(marked[0], marked[1], hair(4), 0, Math.PI * 2);
      ctx.stroke();
      if (mode === "start_end_center") {
        line(marked, sol.start, true);
        line(marked, sol.end, true);
      }
      readout.push(`r ${formatLength(sol.radius, units)}`);
    }
  } else {
    // "The tangent ray is at least as long as the distance to the cursor so it reads as a direction"
    const dir = g.dir ?? [0, 0];
    const len = Math.hypot(dir[0], dir[1]);
    if (len > 0) {
      const l = Math.max(len, hair(100));
      guide(start, [start[0] + (dir[0] * l) / len, start[1] + (dir[1] * l) / len]);
    }
    if (index === 1) line(start, cursor, true);
    else if (sol?.valid) {
      line(start, sol.end, true);
      readout.push(`r ${formatLength(sol.radius, units)}`);
    }
  }
  ctx.setLineDash([]);
  if (readout.length > 0) label(ctx, readout.join("   "), cursor[0], cursor[1] - hair(14), style);
  ctx.restore();
}
