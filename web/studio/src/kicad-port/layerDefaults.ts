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

/** What a text placed with the Text tool starts as on `layer` (`DRAWING_TOOL::PlaceText`): the layer class's size and thickness, mirrored on a back layer, aligned to the left. */
export function newTextStyle(d: TextGraphicsDefaults | undefined, layer: string): { sizeNm: Nm; strokeWidthNm: Nm; mirror: boolean; justify: "left" } {
  const s = textStyleOf(d, layer);
  return { sizeNm: s.heightNm, strokeWidthNm: s.thicknessNm, mirror: isBackLayer(layer), justify: "left" };
}

/**
 * The width a graphic being drawn on `activeLayer` (as the state holds it: a key like "f_silks" or a copper layer's name) gets: the layer class's line thickness, unless `incWidth` / `decWidth` have stepped it on this layer (KiCad
 * resets `m_stroke` to the class's width when the tool starts and when the layer changes, so a step on another layer is gone).
 */
export function drawStrokeWidthNm(d: TextGraphicsDefaults | undefined, activeLayer: string | null | undefined, step: DrawStrokeStep | null): Nm {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  return step && step.layer === layer ? step.um : lineThicknessNm(d, layer);
}

/** `incWidth` / `decWidth`: one `WIDTH_STEP` more or less than the width now (the decrement is refused at the last step). */
export function steppedDrawStroke(d: TextGraphicsDefaults | undefined, activeLayer: string | null | undefined, step: DrawStrokeStep | null, dir: 1 | -1): DrawStrokeStep {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  return { layer, um: stepStrokeWidth(drawStrokeWidthNm(d, layer, step), dir) };
}
