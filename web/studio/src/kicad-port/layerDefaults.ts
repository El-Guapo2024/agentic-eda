// What a new item starts with: the Text & Graphics defaults of the layer class its layer belongs to (Board Setup > Text & Graphics > Defaults). Ported from
//   pcbnew/board_design_settings.cpp   GetLayerClass, GetLineThickness, GetTextSize, GetTextThickness, GetTextItalic, GetTextUpright
//   pcbnew/tools/drawing_tool.cpp      `m_stroke.SetWidth( bds.GetLineThickness( m_layer ) )` when a drawing tool starts and when its layer changes, incWidth /
//                                      decWidth stepping it by WIDTH_STEP; PlaceText's `textAttrs` (size, thickness and italic of the layer class, mirrored on a back
//                                      layer, left aligned); the dimension tools' SetTextSize / SetTextThickness / SetLineThickness
//   pcbnew/pcb_shape.cpp, pcb_text.cpp, pcb_dimension.cpp   StyleFromSettings
//
// The backend has the same table (`eda_model::rules::TextGraphicsDefaults::line_thickness_nm` and friends: `eda board shape add` and `text add` start from it too).

import type { LayerClassDefaults, TextGraphicsDefaults, Nm } from "../api/types";
import { stepStrokeWidth, type DrawStrokeStep } from "./pcbParityState";
import { activeLayerName } from "./layerPresets";

/** `LAYER_CLASS_*`: the row of the Text & Graphics grid a layer belongs to. */
export type LayerClass = "silk" | "copper" | "edges" | "courtyard" | "fab" | "others";

/** `BOARD_DESIGN_SETTINGS::GetLayerClass( aLayer )`, by the layer's name in the board file. */
export function layerClassOf(layer: string): LayerClass {
  if (layer === "F.SilkS" || layer === "B.SilkS") return "silk";
  if (layer === "Edge.Cuts") return "edges";
  if (layer === "F.CrtYd" || layer === "B.CrtYd") return "courtyard";
  if (layer === "F.Fab" || layer === "B.Fab") return "fab";
  if (layer.endsWith(".Cu")) return "copper";
  return "others";
}

const row = (line: Nm, size: Nm, thickness: Nm): LayerClassDefaults => ({ line_width_nm: line, text_width_nm: size, text_height_nm: size, text_thickness_nm: thickness, italic: false, upright: true });

/** KiCad's own table (`BOARD_DESIGN_SETTINGS`, board_design_settings.h), for a backend that sends none: the same as `TextGraphicsDefaults::default()` in crates/model/src/rules.rs. */
export const FACTORY_TEXT_GRAPHICS: TextGraphicsDefaults = {
  silk: row(100_000, 1_000_000, 100_000),
  copper: row(200_000, 1_500_000, 300_000),
  edge_cuts_line_width_nm: 50_000,
  courtyard_line_width_nm: 50_000,
  fab: row(100_000, 1_000_000, 150_000),
  others: row(100_000, 1_000_000, 150_000),
};

/** `DEFAULT_TEXT_SIZE` and `DEFAULT_TEXT_WIDTH`: the text of the edges and courtyard classes, which have a line thickness in Board Setup and no text cells. */
const PLAIN_TEXT_SIZE_NM = 1_000_000;
const PLAIN_TEXT_THICKNESS_NM = 150_000;

/** `BOARD_DESIGN_SETTINGS::GetLineThickness( aLayer )`. */
export function lineThicknessNm(d: TextGraphicsDefaults | undefined, layer: string): Nm {
  const t = d ?? FACTORY_TEXT_GRAPHICS;
  switch (layerClassOf(layer)) {
    case "silk":
      return t.silk.line_width_nm;
    case "copper":
      return t.copper.line_width_nm;
    case "edges":
      return t.edge_cuts_line_width_nm;
    case "courtyard":
      return t.courtyard_line_width_nm;
    case "fab":
      return t.fab.line_width_nm;
    default:
      return t.others.line_width_nm;
  }
}

/** The text of a layer class: `GetTextSize`, `GetTextThickness`, `GetTextItalic`, `GetTextUpright`. */
export interface TextStyle {
  widthNm: Nm;
  heightNm: Nm;
  thicknessNm: Nm;
  italic: boolean;
  upright: boolean;
}

export function textStyleOf(d: TextGraphicsDefaults | undefined, layer: string): TextStyle {
  const t = d ?? FACTORY_TEXT_GRAPHICS;
  const c = layerClassOf(layer);
  const r: LayerClassDefaults | null = c === "silk" ? t.silk : c === "copper" ? t.copper : c === "fab" ? t.fab : c === "others" ? t.others : null;
  return r
    ? { widthNm: r.text_width_nm, heightNm: r.text_height_nm, thicknessNm: r.text_thickness_nm, italic: r.italic, upright: r.upright }
    : { widthNm: PLAIN_TEXT_SIZE_NM, heightNm: PLAIN_TEXT_SIZE_NM, thicknessNm: PLAIN_TEXT_THICKNESS_NM, italic: false, upright: false };
}

/** `IsBackLayer`: a text placed on one is mirrored (`textAttrs.m_Mirrored = m_board->IsBackLayer( layer )`). */
export function isBackLayer(layer: string): boolean {
  return ["B.Cu", "B.Adhes", "B.Paste", "B.SilkS", "B.Mask", "B.CrtYd", "B.Fab"].includes(layer);
}

/**
 * What a text placed with the Text tool starts as on `layer` (`DRAWING_TOOL::PlaceText`'s `textAttrs`): the layer class's size (`GetTextSize`: the height, and the
 * width when it differs) and thickness, its italic, mirrored on a back layer, aligned to the left and the bottom (`GR_TEXT_H_ALIGN_LEFT`, `GR_TEXT_V_ALIGN_BOTTOM`).
 * (`m_KeepUpright` is a footprint text's: a board text has no parent footprint to be kept upright against.)
 */
export function newTextStyle(d: TextGraphicsDefaults | undefined, layer: string): { sizeNm: Nm; widthNm: Nm; strokeWidthNm: Nm; bold: boolean; italic: boolean; mirror: boolean; justify: "left"; valign: "bottom" } {
  const s = textStyleOf(d, layer);
  return { sizeNm: s.heightNm, widthNm: s.widthNm, strokeWidthNm: s.thicknessNm, bold: inferBold(s.thicknessNm, s.widthNm, s.heightNm), italic: s.italic, mirror: isBackLayer(layer), justify: "left", valign: "bottom" };
}

/**
 * `InferBold` (gr_text.h:82-89), which `PlaceText` runs on the class's size and thickness (drawing_tool.cpp:1058-1060; round 6 A6-3): a text is bold when its
 * pen is nearer the bold pen than the normal one, `abs( penSize - GetPenSizeForBold( textSize ) ) < abs( penSize - GetPenSizeForNormal( textSize ) )`, with
 * `GetPenSizeForBold` = `KiROUND( size / 5.0 )` and `GetPenSizeForNormal` = `KiROUND( size / 8.0 )` of the smaller of the width and the height (gr_text.cpp:37-64).
 * So copper text at KiCad's defaults (1.5 mm, 0.3 mm) is bold, silkscreen text (1.0 mm, 0.1 mm) is not.
 */
export function inferBold(penNm: Nm, widthNm: Nm, heightNm: Nm): boolean {
  const size = Math.min(widthNm, heightNm);
  return Math.abs(penNm - Math.round(size / 5)) < Math.abs(penNm - Math.round(size / 8));
}

/**
 * The width a graphic being drawn on `activeLayer` (as the state holds it: a key like "f_silks" or a copper layer's name) gets: the layer class's line thickness, unless `incWidth` / `decWidth` have stepped it on this layer (KiCad
 * resets `m_stroke` to the class's width when a drawing tool finds another layer than its `m_layer`, so a step on another layer is gone; see `strokeStepKept`).
 */
export function drawStrokeWidthNm(d: TextGraphicsDefaults | undefined, activeLayer: string | null | undefined, step: DrawStrokeStep | null): Nm {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  return step && step.layer === layer ? step.um : lineThicknessNm(d, layer);
}

/**
 * The tools that carry `DRAWING_TOOL::m_stroke` and step it with `incWidth` / `decWidth`: `drawShape` (line, rectangle, circle; drawing_tool.cpp:2690), `drawArc`
 * (:2995) and the Bezier tool (:3311). The graphic polygon is `DrawZone` (`ZONE_CREATE_HELPER`), which never reads `m_stroke`; the dimension tool steps only a
 * dimension already started and is not counted here.
 */
export const STROKE_TOOLS: ReadonlySet<string> = new Set(["draw_segment", "draw_rect", "draw_circle", "draw_arc", "draw_bezier"]);

/**
 * What becomes of a stepped width when `tool` sees `activeLayer` -- when it is armed, or when the layer changes while it runs. `m_stroke` belongs to
 * `DRAWING_TOOL`, shared by every drawing tool, and is reset to the class's width only when the layer the tool finds differs from `m_layer`
 * (drawing_tool.cpp:2369-2372 at the start of drawShape, 2792-2796 drawArc, 3084-3087 the Bezier tool; 2503-2508 and 2938-2943 on `layerChanged` in their
 * loops) and in `DRAWING_TOOL::Reset` (a board load, Board Setup OK): `if( m_layer != m_frame->GetActiveLayer() ) { m_layer = ...; m_stroke.SetWidth(
 * bds.GetLineThickness( m_layer ) ); }`. So a step survives re-arming any of these tools on its layer (round 6 A6-1), and a layer change no stroke tool sees
 * (none armed, or the polygon tool) leaves it.
 */
export function strokeStepKept(step: DrawStrokeStep | null, tool: string, activeLayer: string | null | undefined): DrawStrokeStep | null {
  if (!step || !STROKE_TOOLS.has(tool)) return step;
  return step.layer === activeLayerName(activeLayer, "F.SilkS") ? step : null;
}

/** `incWidth` / `decWidth`: one `WIDTH_STEP` more or less than the width now (the decrement is refused at the last step). */
export function steppedDrawStroke(d: TextGraphicsDefaults | undefined, activeLayer: string | null | undefined, step: DrawStrokeStep | null, dir: 1 | -1): DrawStrokeStep {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  return { layer, um: stepStrokeWidth(drawStrokeWidthNm(d, layer, step), dir) };
}

/**
 * The graphic polygon tool's new shape (`ZONE_CREATE_HELPER::commitZone`, `ZONE_MODE::GRAPHIC_POLYGON`, zone_create_helper.cpp:236-251): filled except on
 * Edge.Cuts and the courtyards, at the layer class's line thickness with a solid line -- `DrawZone` never reads `m_stroke`, so an incWidth / decWidth step
 * of the other drawing tools does not reach it (round 6 A6-2; the Footprint Editor's own tool is `fpDrawDefaults.ts`'s):
 * `poly->SetFilled( layer != Edge_Cuts && layer != F_CrtYd && layer != B_CrtYd ); poly->SetStroke( STROKE_PARAMS( bds.GetLineThickness( layer ), LINE_STYLE::SOLID ) );`
 */
export function graphicPolygonStyle(d: TextGraphicsDefaults | undefined, layer: string): { filled: boolean; strokeWidthNm: Nm } {
  return { filled: layer !== "Edge.Cuts" && layer !== "F.CrtYd" && layer !== "B.CrtYd", strokeWidthNm: lineThicknessNm(d, layer) };
}
