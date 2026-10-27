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
