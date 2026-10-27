// What a shape drawn in the Footprint Editor starts with. Ported from
//   pcbnew/tools/drawing_tool.cpp            `m_stroke.SetWidth( bds.GetLineThickness( m_layer ) )` when a drawing tool starts (DrawLine, DrawRectangle, DrawCircle,
//                                           DrawArc, DrawBezier) -- the layer class's line thickness
//   pcbnew/footprint_editor_settings.cpp     the Footprint Editor's own Text & Graphics defaults (silk 0.1, copper 0.2, edges 0.05, courtyard 0.05, fab 0.1, others 0.1 mm),
//                                           which are KiCad's factory table (kicad-port/layerDefaults.ts `FACTORY_TEXT_GRAPHICS`)
//   pcbnew/tools/zone_create_helper.cpp      ZONE_CREATE_HELPER::commitZone, a graphic polygon:
//                                           `poly->SetFilled( layer != Edge_Cuts && layer != F_CrtYd && layer != B_CrtYd )` with the layer's line thickness
//
// The studio has no Footprint Editor preferences page for these defaults, so the factory values are the ones used.

import type { CmdShape } from "../api/types";
import { lineThicknessNm } from "./layerDefaults";

/**
 * The layers the Footprint Editor's layer box offers, in `LSET::UIOrder()` (the footprint editor's board enables every layer; the inner copper layers are left out here,
 * as a library footprint's items are on the outer ones).
 */
export const FP_EDITOR_LAYERS = [
  "F.Cu", "B.Cu", "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User",
  "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab", "User.1", "User.2", "User.3", "User.4", "User.5", "User.6", "User.7", "User.8", "User.9",
];

/** `bds.GetLineThickness( layer )` with the Footprint Editor's defaults. */
export function fpStrokeWidthNm(layer: string): number {
  return lineThicknessNm(undefined, layer);
}

/** `ZONE_CREATE_HELPER::commitZone`: a graphic polygon is filled unless it is on Edge.Cuts or a courtyard. */
export function fpPolygonFilled(layer: string): boolean {
  return layer !== "Edge.Cuts" && layer !== "F.CrtYd" && layer !== "B.CrtYd";
}

export type FpPointShapeKind = "segment" | "rect" | "circle" | "arc" | "polygon";

/** The shape a point-counted drawing tool commits from its clicks, with the layer's width (and, for a polygon, its fill); null while the points are too few. */
export function fpShape(kind: FpPointShapeKind, pts: readonly (readonly [number, number])[], layer: string): CmdShape | null {
  const p = (i: number) => ({ x: pts[i]![0], y: pts[i]![1] });
  const stroke_width = fpStrokeWidthNm(layer);
  switch (kind) {
    case "segment":
      return pts.length >= 2 ? { kind: "segment", layer, stroke_width, filled: false, start: p(0), end: p(1) } : null;
    case "rect":
      return pts.length >= 2 ? { kind: "rect", layer, stroke_width, filled: false, start: p(0), end: p(1) } : null;
    case "circle":
      return pts.length >= 2 ? { kind: "circle", layer, stroke_width, filled: false, center: p(0), end: p(1) } : null;
    case "arc":
      return pts.length >= 3 ? { kind: "arc", layer, stroke_width, filled: false, start: p(0), mid: p(1), end: p(2) } : null;
    case "polygon":
      return pts.length >= 3 ? { kind: "polygon", layer, stroke_width, filled: fpPolygonFilled(layer), pts: pts.map((_, i) => p(i)) } : null;
  }
}
