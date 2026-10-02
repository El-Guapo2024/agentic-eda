// Small glue shared by the board canvas and the footprint editor canvas for the
// "Draw Arc" and "Draw Bezier Curve" tools (kicad-port/arcGeom.ts and
// bezierGeom.ts hold the ported construction managers; this only turns their
// results into the `CmdShape` a commit sends, and reads the angle-snap setting).
import type { CmdShape, PointXY } from "../../api/types";
import { ARC_SET_START, arcStartRadiusEnd, type ArcGeom } from "../../kicad-port/arcGeom";
import type { BezierCurve } from "../../kicad-port/bezierGeom";
import type { AngleSnapMode } from "../../kicad-port/pcbParityState";
import { isMac } from "../../platform";

export const ptXY = (p: readonly [number, number]): PointXY => ({ x: p[0], y: p[1] });

/**
 * `drawArc`: `angleSnap = GetAngleSnapMode(); if( evt->Modifier( MD_CTRL ) ) angleSnap = LEADER_MODE::DIRECT;`
 * then `arcManager.SetAngleSnap( angleSnap != LEADER_MODE::DIRECT )`.
 */
export function arcAngleSnap(mode: AngleSnapMode, e: { ctrlKey: boolean; metaKey: boolean }): boolean {
  const ctrlOrCmd = isMac() ? e.metaKey : e.ctrlKey;
  return mode !== "direct" && !ctrlOrCmd;
}

/** The points locked in so far (centre, then the start radius end) -- what `DrawState.pts` carries for an arc in progress. */
export function arcClickPoints(g: ArcGeom): [number, number][] {
  const pts: [number, number][] = [[g.origin[0], g.origin[1]]];
  if (g.step > ARC_SET_START) pts.push(arcStartRadiusEnd(g));
  return pts;
}

/** `bezier->SetStart/SetBezierC1/SetEnd/SetBezierC2` as the shape `add_shape` / `add_footprint_graphic` takes. */
export function bezierShape(curve: BezierCurve, layer: string, strokeWidthUm: number): CmdShape {
  return { kind: "bezier", layer, stroke_width: strokeWidthUm, filled: false, start: ptXY(curve.start), c1: ptXY(curve.c1), c2: ptXY(curve.c2), end: ptXY(curve.end) };
}
