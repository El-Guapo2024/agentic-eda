// A dimension's label as its own text (UI-LEFT-2 PP11), from pcbnew at 8303b2ad:
//
//   DIALOG_DIMENSION_PROPERTIES (dialogs/dialog_dimension_properties.cpp)   the "Dimension Text" box: Font, Width, Height, Thickness and the row of Bold, Italic,
//                                                                         Align left / centre / right and Mirrored -- `TransferDataToWindow` (:235-267),
//                                                                         `onFontSelected` (:324), `onBoldToggle` (:343), `onThickness` (:366) and
//                                                                         `updateDimensionFromDialog` (:419-439)
//   PCB_TEXT_DESC / EDA_TEXT_DESC, inherited by DIMENSION_DESC            the Properties panel's Font, Italic, Bold, Mirrored, Horizontal Justification, Keep Upright,
//   (pcb_text.cpp, common/eda_text.cpp, pcb_dimension.cpp)               Width and Height of a dimension (Text, Vertical Justification, Hyperlink and Knockout
//                                                                         unavailable: the kinds' `OverrideAvailability`)
//   DRAWING_TOOL::DrawDimension (tools/drawing_tool.cpp)                  a new label: `SetMirrored( IsBackLayer( layer ) )`, `SetTextSize`, `SetTextThickness`
//                                                                         and `SetItalic( bds.GetTextItalic( layer ) )` of the layer -- again at a layer change
//                                                                         (`layerChanged`), the mirror left as it was
//
// The label's looks are `Dimension::text_effects` (crates/model/src/ir.rs `DimensionTextEffects`): what a field leaves out is the default (the width the height, the stroke
// font, upright, centred, mirrored as the layer's side says). Pure: no store, no DOM. Unit-tested in dimensionLabel.test.ts.
import type { CmdDimension, Dimension, DimensionTextEffects, TextJustify } from "../api/types";
import { boldTogglePenNm, inferBold, isBackLayer } from "./layerDefaults";

/** The label's looks as the dialog and the Properties panel show them. */
export interface DimensionLabelLooks {
  /** `GetTextWidth()`, nm. */
  widthNm: number;
  /** `GetTextHeight()`, nm. */
  heightNm: number;
  /** `GetTextThickness()`, nm (15 % of the height when the dimension has none). */
  thicknessNm: number;
  /** The face; null is KiCad's stroke font. */
  face: string | null;
  bold: boolean;
  italic: boolean;
  /** `IsMirrored()`. */
  mirrored: boolean;
  hJustify: TextJustify;
  keepUpright: boolean;
}

/** What a dimension without a mirror of its own shows: a back layer's label is mirrored (`DrawDimension`'s `SetMirrored( IsBackLayer( layer ) )`, and what the writer writes). */
export function layerMirrors(layer: string): boolean {
  return isBackLayer(layer);
}

/** The looks of a dimension as `/api/state` gives it (a backend without them: a square, upright, centred stroke-font label mirrored as its layer says). */
export function labelLooksOf(d: Pick<Dimension, "layer" | "text_size_nm" | "text_thickness_nm"> & Partial<Pick<Dimension, "text_width_nm" | "face" | "bold" | "italic" | "mirrored" | "h_justify" | "keep_upright">>): DimensionLabelLooks {
  return {
    widthNm: d.text_width_nm ?? d.text_size_nm,
    heightNm: d.text_size_nm,
    thicknessNm: d.text_thickness_nm ?? Math.round(d.text_size_nm * 0.15),
    face: d.face ?? null,
    bold: !!d.bold,
    italic: !!d.italic,
    mirrored: d.mirrored ?? layerMirrors(d.layer),
    hJustify: d.h_justify ?? "center",
    keepUpright: !!d.keep_upright,
  };
}

/**
 * The looks written into the dimension the verbs take (`edit_dimension` replaces the whole dimension): the height and the pen on the dimension, the rest in its
 * `text_effects`, each left out at its default -- the width when it is the height, the mirror when it is what the dimension's layer gives (so a label that was never
 * mirrored by hand keeps following its layer, and one that was keeps its own: `IsMirrored()` is an attribute in KiCad, which a layer change leaves).
 */
export function withLabelLooks(cmd: CmdDimension, l: DimensionLabelLooks): CmdDimension {
  const fx: DimensionTextEffects = {};
  if (l.widthNm !== l.heightNm) fx.width_nm = l.widthNm;
  if (l.face) fx.face = l.face;
  if (l.bold) fx.bold = true;
  if (l.italic) fx.italic = true;
  if (l.mirrored !== layerMirrors(cmd.layer)) fx.mirrored = l.mirrored;
  if (l.hJustify !== "center") fx.h_justify = l.hJustify;
  if (l.keepUpright) fx.keep_upright = true;
  const out: CmdDimension = { ...cmd, text_size_nm: l.heightNm };
  // The pen: `SetTextThickness` of what is shown. A dimension with no pen of its own (15 % of its height) keeps none while that is still what it shows.
  if (cmd.text_thickness_nm != null || l.thicknessNm !== Math.round(l.heightNm * 0.15)) out.text_thickness_nm = l.thicknessNm;
  if (Object.keys(fx).length) out.text_effects = fx;
  else delete out.text_effects;
  return out;
}

/** The looks of a dimension the verbs take, as [`withLabelLooks`] wrote them. */
export function labelLooksOfCmd(cmd: CmdDimension): DimensionLabelLooks {
  const fx = cmd.text_effects ?? {};
  return {
    widthNm: fx.width_nm ?? cmd.text_size_nm,
    heightNm: cmd.text_size_nm,
    thicknessNm: cmd.text_thickness_nm ?? Math.round(cmd.text_size_nm * 0.15),
    face: fx.face ?? null,
    bold: !!fx.bold,
    italic: !!fx.italic,
    mirrored: fx.mirrored ?? layerMirrors(cmd.layer),
    hJustify: fx.h_justify ?? "center",
    keepUpright: !!fx.keep_upright,
  };
}

/** `onBoldToggle`: the Bold button sets the thickness to the bold or the normal pen of the smaller of the width and the height. */
export function boldToggled(l: DimensionLabelLooks, bold: boolean): DimensionLabelLooks {
  return { ...l, bold, thicknessNm: boldTogglePenNm(bold, l.widthNm, l.heightNm) };
}

/** `onThickness`: a thickness typed in sets the Bold button to the pen it is nearer (`GetPenSizeForBold` or `GetPenSizeForNormal`). */
export function thicknessTyped(l: DimensionLabelLooks, thicknessNm: number): DimensionLabelLooks {
  return { ...l, thicknessNm, bold: inferBold(thicknessNm, l.widthNm, l.heightNm) };
}

/**
 * `onFontSelected`: picking the stroke font shows the Thickness again and sets Bold from it, as `onThickness` does; an outline face hides it (the face's own weight is
 * its bold). `face` null is the stroke font.
 */
export function fontPicked(l: DimensionLabelLooks, face: string | null): DimensionLabelLooks {
  return face ? { ...l, face } : { ...l, face: null, bold: inferBold(l.thicknessNm, l.widthNm, l.heightNm) };
}
