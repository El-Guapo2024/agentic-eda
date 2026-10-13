// What a new item starts with: the Text & Graphics defaults of the layer class its layer belongs to (Board Setup > Text & Graphics > Defaults). Ported from
//   pcbnew/board_design_settings.cpp   GetLayerClass, GetLineThickness, GetTextSize, GetTextThickness, GetTextItalic, GetTextUpright
//   pcbnew/tools/drawing_tool.cpp      `m_stroke.SetWidth( bds.GetLineThickness( m_layer ) )` when a drawing tool starts and when its layer changes, incWidth /
//                                      decWidth stepping it by WIDTH_STEP; PlaceText's `textAttrs` (size, thickness and italic of the layer class, mirrored on a back
//                                      layer, left aligned); the dimension tools' SetTextSize / SetTextThickness / SetLineThickness
//   pcbnew/pcb_shape.cpp, pcb_text.cpp, pcb_dimension.cpp   StyleFromSettings
//
// The backend has the same table (`eda_model::rules::TextGraphicsDefaults::line_thickness_um` and friends: `eda board shape add` and `text add` start from it too).

import type { LayerClassDefaults, TextGraphicsDefaults, Um } from "../api/types";
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

const row = (line: Um, size: Um, thickness: Um): LayerClassDefaults => ({ line_width_um: line, text_width_um: size, text_height_um: size, text_thickness_um: thickness, italic: false, upright: true });

/** KiCad's own table (`BOARD_DESIGN_SETTINGS`, board_design_settings.h), for a backend that sends none: the same as `TextGraphicsDefaults::default()` in crates/model/src/rules.rs. */
export const FACTORY_TEXT_GRAPHICS: TextGraphicsDefaults = {
  silk: row(100, 1000, 100),
  copper: row(200, 1500, 300),
  edge_cuts_line_width_um: 50,
  courtyard_line_width_um: 50,
  fab: row(100, 1000, 150),
  others: row(100, 1000, 150),
};

/** `DEFAULT_TEXT_SIZE` and `DEFAULT_TEXT_WIDTH`: the text of the edges and courtyard classes, which have a line thickness in Board Setup and no text cells. */
const PLAIN_TEXT_SIZE_UM = 1000;
const PLAIN_TEXT_THICKNESS_UM = 150;

/** `BOARD_DESIGN_SETTINGS::GetLineThickness( aLayer )`. */
export function lineThicknessUm(d: TextGraphicsDefaults | undefined, layer: string): Um {
  const t = d ?? FACTORY_TEXT_GRAPHICS;
  switch (layerClassOf(layer)) {
    case "silk":
      return t.silk.line_width_um;
    case "copper":
      return t.copper.line_width_um;
    case "edges":
      return t.edge_cuts_line_width_um;
    case "courtyard":
      return t.courtyard_line_width_um;
    case "fab":
      return t.fab.line_width_um;
    default:
      return t.others.line_width_um;
  }
}

/** The text of a layer class: `GetTextSize`, `GetTextThickness`, `GetTextItalic`, `GetTextUpright`. */
export interface TextStyle {
  widthUm: Um;
  heightUm: Um;
  thicknessUm: Um;
  italic: boolean;
  upright: boolean;
}

export function textStyleOf(d: TextGraphicsDefaults | undefined, layer: string): TextStyle {
  const t = d ?? FACTORY_TEXT_GRAPHICS;
  const c = layerClassOf(layer);
  const r: LayerClassDefaults | null = c === "silk" ? t.silk : c === "copper" ? t.copper : c === "fab" ? t.fab : c === "others" ? t.others : null;
  return r
    ? { widthUm: r.text_width_um, heightUm: r.text_height_um, thicknessUm: r.text_thickness_um, italic: r.italic, upright: r.upright }
    : { widthUm: PLAIN_TEXT_SIZE_UM, heightUm: PLAIN_TEXT_SIZE_UM, thicknessUm: PLAIN_TEXT_THICKNESS_UM, italic: false, upright: false };
}

/** `IsBackLayer`: a text placed on one is mirrored (`textAttrs.m_Mirrored = m_board->IsBackLayer( layer )`). */
export function isBackLayer(layer: string): boolean {
  return ["B.Cu", "B.Adhes", "B.Paste", "B.SilkS", "B.Mask", "B.CrtYd", "B.Fab"].includes(layer);
}

/** What a text placed with the Text tool starts as on `layer` (`DRAWING_TOOL::PlaceText`): the layer class's size and thickness, mirrored on a back layer, aligned to the left. */
export function newTextStyle(d: TextGraphicsDefaults | undefined, layer: string): { sizeUm: Um; strokeWidthUm: Um; mirror: boolean; justify: "left" } {
  const s = textStyleOf(d, layer);
  return { sizeUm: s.heightUm, strokeWidthUm: s.thicknessUm, mirror: isBackLayer(layer), justify: "left" };
}

/**
 * The width a graphic being drawn on `activeLayer` (as the state holds it: a key like "f_silks" or a copper layer's name) gets: the layer class's line thickness, unless `incWidth` / `decWidth` have stepped it on this layer (KiCad
 * resets `m_stroke` to the class's width when the tool starts and when the layer changes, so a step on another layer is gone).
 */
export function drawStrokeWidthUm(d: TextGraphicsDefaults | undefined, activeLayer: string | null | undefined, step: DrawStrokeStep | null): Um {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  return step && step.layer === layer ? step.um : lineThicknessUm(d, layer);
}

/** `incWidth` / `decWidth`: one `WIDTH_STEP` more or less than the width now (the decrement is refused at the last step). */
export function steppedDrawStroke(d: TextGraphicsDefaults | undefined, activeLayer: string | null | undefined, step: DrawStrokeStep | null, dir: 1 | -1): DrawStrokeStep {
  const layer = activeLayerName(activeLayer, "F.SilkS");
  return { layer, um: stepStrokeWidth(drawStrokeWidthUm(d, layer, step), dir) };
}
