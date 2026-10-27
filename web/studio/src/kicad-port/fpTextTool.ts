// The Footprint Editor's Text tool (`pcbnew.InteractiveDrawing.text`). Ported from pcbnew/tools/drawing_tool.cpp DRAWING_TOOL::PlaceText:
//   - the first click makes a text on the active layer with the layer class's attributes
//       textAttrs.m_Size = bds.GetTextSize( layer );  textAttrs.m_StrokeWidth = bds.GetTextThickness( layer );
//       textAttrs.m_Italic = bds.GetTextItalic( layer ); textAttrs.m_KeepUpright = bds.GetTextUpright( layer );
//       textAttrs.m_Mirrored = m_board->IsBackLayer( layer );
//       textAttrs.m_Halign = GR_TEXT_H_ALIGN_LEFT;  textAttrs.m_Valign = GR_TEXT_V_ALIGN_BOTTOM;
//     and opens DIALOG_TEXT_PROPERTIES on it; a cancelled dialog or a text with no printable character (`NoPrintableChars`) drops it;
//   - a text whose position the dialog changed is placed at once, any other follows the cursor until the next click, which commits it ("Draw Text");
//   - the tool stays armed; Escape drops a carried text, a second Escape leaves the tool.
// The sizes are the Footprint Editor's defaults (footprint_editor_settings.cpp: silk 1 mm / 0.1 mm, copper 1.5 / 0.3, fab and others 1 / 0.15; the edges and courtyard
// classes keep board_design_settings.cpp's DEFAULT_TEXT_SIZE / DEFAULT_TEXT_WIDTH), which are the factory table of kicad-port/layerDefaults.ts.
// Differs: the text model (`eda_model::Text`) has no vertical justification, italic or keep-upright, so the text is bottom-justified only in KiCad's file; ours has
// the model's own vertical centre.

import type { CmdText, PointXY } from "../api/types";
import { TEXT_MAX_NM, TEXT_MIN_NM } from "./fpFields";
import { isBackLayer, textStyleOf } from "./layerDefaults";

/** The text PlaceText's first click makes on `layer` at `at`, before the dialog fills its content. */
export function fpNewText(layer: string, at: PointXY): CmdText {
  const s = textStyleOf(undefined, layer);
  return { content: "", at: { x: at.x, y: at.y }, angle: 0, layer, size_nm: s.heightNm, stroke_width: s.thicknessNm, justify: "left", mirror: isBackLayer(layer) };
}

/** `NoPrintableChars`: nothing but white space. */
export function noPrintableChars(text: string): boolean {
  return text.trim() === "";
}

/** `EDA_UNIT_UTILS::UI::StringFromValue`'s millimetres with their unit: "%.10f", the trailing zeros (and a bare point) off, then " mm" (`GetText`). */
function mmText(nmValue: number): string {
  const text = (nmValue / 1e6).toFixed(10);
  const trimmed = text.includes(".") ? text.replace(/0+$/, "").replace(/\.$/, "") : text;
  return `${trimmed} mm`;
}

/**
 * `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`'s size check, through `UNIT_BINDER::Validate` (common/widgets/unit_binder.cpp): under `TEXT_MIN_SIZE_MM` the
 * message is `_( "%s must be at least %s." )`, over `TEXT_MAX_SIZE_MM` `_( "%s must be less than %s." )`, the first `%s` the label without its colon (`Width`)
 * and the second the bound as `StringFromValue` prints it. Our one Size field is the width, so its wording is KiCad's. Null when the size is in range.
 */
export function fpTextSizeMessage(sizeNm: number): string | null {
  if (!(sizeNm >= TEXT_MIN_NM)) return `Width must be at least ${mmText(TEXT_MIN_NM)}.`;
  if (sizeNm > TEXT_MAX_NM) return `Width must be less than ${mmText(TEXT_MAX_NM)}.`;
  return null;
}
