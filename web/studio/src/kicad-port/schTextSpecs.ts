// What `FONT::Draw` is given for each text of a sheet and of the Symbol Editor (UI-LEFT-2 PP1, CC1): the position and the `TEXT_ATTRIBUTES` `SCH_PAINTER` hands
// `strokeText` / `DrawGlyphs`, for `kicad-port/kicadText.ts`'s layout -- the stroke font or the item's outline face.
//
// KiCad 8303b2ad, eeschema:
//   - `sch_painter.cpp`: `getTextThickness` (every text's pen: `GetEffectiveTextPenWidth( m_schSettings.GetDefaultPenWidth() )` -- the schematic's default line width,
//     6 mil unless Schematic Setup says otherwise, for a text with no pen of its own; `GetPenSizeForBold` of its width once bold); `draw( SCH_FIELD )` (a field is drawn
//     centred on both axes at the centre of its `GetBoundingBox`, whatever its own justification: "the easier way is to use no justifications (centered text) and use
//     GetBoundingBox to know the text coordinate considered as centered"); `draw( SCH_TEXT )` (a free text or a label at `GetDrawPos() + GetSchematicTextOffset()` with its
//     own attributes; a symbol's text, on `LAYER_DEVICE`, at the centre of its box -- its left or right edge for a left or right justification -- centred vertically);
//     `draw( SCH_TEXTBOX )` (the text broken to the box, `GetShownText`, at `GetDrawPos()`);
//   - `sch_label.cpp`: `SCH_LABEL_BASE::SetSpinStyle` (a spin's angle and horizontal justification, the vertical one bottom), `SCH_GLOBALLABEL::SetSpinStyle` and
//     `SCH_HIERLABEL::SetSpinStyle` (centred vertically), `SCH_LABEL_BASE::GetSchematicTextOffset` (`GetTextOffset() + GetPenWidth()` off the wire),
//     `SCH_GLOBALLABEL::GetSchematicTextOffset` (past the flag's `GetLabelBoxExpansion`, three quarters of the height more past an input, bidirectional or tri-state point,
//     0.0715 of it down "on the center line of E"), `SCH_HIERLABEL::GetSchematicTextOffset` (`GetTextOffset() + GetTextWidth()`), `GetLabelBoxExpansion`,
//     `SCH_GLOBALLABEL::CreateGraphicShape` (the flag as long as `GetTextBox( aRenderSettings ).GetWidth()`, which an outline face changes);
//   - `sch_text.cpp`: `SCH_TEXT::GetTextOffset` (`m_TextOffsetRatio` of the height, 0.15 by default), `GetPenWidth`, `GetBoundingBox`;
//   - `sch_field.cpp::SCH_FIELD::GetBoundingBox` (`GetTextBox` turned by the text angle about the position);
//   - `sch_textbox.cpp`: `SCH_TEXTBOX::GetDrawPos` (the corner or the middle of a side, the margins in), `GetShownText` (`FONT::LinebreakText` to the column).
//
// The positions are the server's (`SchField.at`, the label's and the text's anchors) in nm; KiCad's ints are eeschema's 100 nm (`SCH_IU`). The label offset and margin
// ratios are Schematic Setup > Formatting's for the sheet being drawn, KiCad's defaults in the Symbol Editor (kicad-port/schFormattingLook.ts, UI-LEFT-2 SS4).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { SchematicText } from "../api/types";
import { effectiveTextPenWidth, kicadTextBox, kiRound, lineAdvance, STROKE, type HJustify, type KicadFont, type KicadTextSpec, type VJustify } from "./kicadText";
import { linebreakText } from "./pcbTextBox";
import { FIELD_SIZE_NM } from "./schText";
import { textLie } from "./schTextItem";
import type { LabelSpin } from "./labelSpin";
import { schFormattingLook } from "./schFormattingLook";

/** eeschema's internal unit, in the sheet's nm. */
export const SCH_IU = 100;
/** `DEFAULT_LINE_WIDTH_MILS` (6 mil): `SCH_RENDER_SETTINGS`' default pen, what a text with no pen of its own is drawn with. */
export const SCH_DEFAULT_PEN_NM = 152_400;

/** `KiROUND` and an int's truncation in eeschema's units. */
const iuRound = (v: number): number => kiRound(v / SCH_IU) * SCH_IU;
const iuTrunc = (v: number): number => Math.trunc(v / SCH_IU) * SCH_IU;

/** `SCH_PAINTER::getTextThickness`: `GetEffectiveTextPenWidth( GetDefaultPenWidth() )` of a text of `size`; `thickness` its own pen (`GetTextThickness`, Auto
 *  Thickness unticked: `TextAttrs::thickness_nm`), none or 0 for KiCad's for its size and weight. */
export function schDrawPen(size: number, bold: boolean, defaultPen = SCH_DEFAULT_PEN_NM, thickness?: number | null): number {
  return iuRound(effectiveTextPenWidth(thickness && thickness > 1 ? thickness : 0, [size, size], bold, defaultPen));
}

/** `EDA_TEXT::GetEffectiveTextPenWidth()` with no default (what `GetTextBox`, `GetPenWidth` and the label offsets take): an eighth of the size, a fifth once bold. */
export function schOwnPen(size: number, bold: boolean): number {
  return iuRound(effectiveTextPenWidth(0, [size, size], bold, 0));
}

/** A text's size, the default 50 mil when it has none. */
const sizeOf = (s: number | null | undefined): number => (s && s > 0 ? s : FIELD_SIZE_NM);

/** `BOX2I::Centre` of a box, in eeschema's units (`m_Pos + m_Size / 2`, an integer division). */
function centre(b: { x0: number; y0: number; x1: number; y1: number }): [number, number] {
  return [b.x0 + iuTrunc((b.x1 - b.x0) / 2), b.y0 + iuTrunc((b.y1 - b.y0) / 2)];
}

/** `RotatePoint( p, about, ANGLE_90 )`: a quarter turn counter-clockwise on the screen. */
const quarter = (p: [number, number], about: readonly [number, number]): [number, number] => [about[0] + (p[1] - about[1]), about[1] - (p[0] - about[0])];

export interface SchFieldLook {
  at: readonly [number, number];
  h: HJustify;
  v: VJustify;
  vertical: boolean;
  size_nm?: number | null;
  bold?: boolean;
  italic?: boolean;
  /** The text's own pen (`(thickness ..)`, `EDA_TEXT::GetTextThickness`); absent or 0 is the automatic one. Read by `deviceTextDrawSpec`. */
  pen_nm?: number | null;
}

/**
 * `SCH_TEXT::GetBoundingBox` / `SCH_FIELD::GetBoundingBox` of a text laid out by `spec` (UI-LEFT-3 S3-29): `GetTextBox` in the text's face with its own pen
 * (`GetEffectiveTextPenWidth()`, an eighth of the size, a fifth once bold), turned about the position by the text's angle (`RotatePoint( pos, GetTextPos(),
 * GetTextAngle() )`), normalized. What the hit tests and bounds of a text in an outline face take, as KiCad's do.
 */
export function schTextBoundsInFace(spec: KicadTextSpec, font: KicadFont): { minX: number; minY: number; maxX: number; maxY: number } {
  const own: KicadTextSpec = { ...spec, pen: schOwnPen(spec.size[1], !!spec.bold) };
  const b = kicadTextBox(own, font);
  let [p, q]: [[number, number], [number, number]] = [
    [b.x0, b.y0],
    [b.x1, b.y1],
  ];
  if (Math.round(spec.angleDeg ?? 0) % 180 !== 0) [p, q] = [quarter(p, spec.pos), quarter(q, spec.pos)];
  return { minX: Math.min(p[0], q[0]), minY: Math.min(p[1], q[1]), maxX: Math.max(p[0], q[0]), maxY: Math.max(p[1], q[1]) };
}

/**
 * `SCH_PAINTER::draw( SCH_FIELD )`: the field's box (`SCH_FIELD::GetBoundingBox`: `GetTextBox` with its own justification, at its own pen, turned a quarter about the
 * position for a vertical one) and the text drawn centred on both axes at the box's centre, with the painter's pen.
 */
export function schFieldDrawSpec(text: string, f: SchFieldLook, font: KicadFont = STROKE, defaultPen = SCH_DEFAULT_PEN_NM): KicadTextSpec {
  const size = sizeOf(f.size_nm);
  const bold = !!f.bold;
  const own: KicadTextSpec = { text, pos: f.at, size: [size, size], pen: schOwnPen(size, bold), halign: f.h, valign: f.v, italic: !!f.italic, bold, multiline: false, iu: SCH_IU };
  let c = centre(kicadTextBox(own, font));
  if (f.vertical) c = quarter(c, f.at);
  return { ...own, pos: c, halign: "center", valign: "center", angleDeg: f.vertical ? 90 : 0, pen: schDrawPen(size, bold, defaultPen) };
}

/**
 * `SCH_PAINTER::draw( SCH_TEXT )` of a symbol's text (`LAYER_DEVICE`): its box (`SCH_TEXT::GetBoundingBox`, turned about the position) and the text centred vertically at
 * the box's centre -- at its left edge for a left-justified text, its right edge for a right-justified one (the bottom and the top of a text that runs up).
 */
export function deviceTextDrawSpec(text: string, f: SchFieldLook, font: KicadFont = STROKE, defaultPen = SCH_DEFAULT_PEN_NM): KicadTextSpec {
  const size = sizeOf(f.size_nm);
  const bold = !!f.bold;
  // a pen of its own (`(thickness ..)`): `GetEffectiveTextPenWidth` is it, clamped to the size, for the box and for the drawing (`getTextThickness`)
  const ownPen = f.pen_nm && f.pen_nm > 0 ? iuRound(effectiveTextPenWidth(f.pen_nm, [size, size], bold, 0)) : 0;
  const own: KicadTextSpec = { text, pos: f.at, size: [size, size], pen: ownPen || schOwnPen(size, bold), halign: f.h, valign: f.v, italic: !!f.italic, bold, multiline: true, iu: SCH_IU };
  const b = kicadTextBox(own, font);
  let box = b;
  if (f.vertical) {
    const [p, q] = [quarter([b.x0, b.y0], f.at), quarter([b.x1, b.y1], f.at)];
    box = { x0: Math.min(p[0], q[0]), y0: Math.min(p[1], q[1]), x1: Math.max(p[0], q[0]), y1: Math.max(p[1], q[1]) };
  }
  let [x, y] = centre(box);
  if (f.vertical) {
    if (f.h === "left") y = box.y1;
    else if (f.h === "right") y = box.y0;
  } else if (f.h === "left") x = box.x0;
  else if (f.h === "right") x = box.x1;
  return { ...own, pos: [x, y], valign: "center", angleDeg: f.vertical ? 90 : 0, pen: ownPen || schDrawPen(size, bold, defaultPen) };
}

/**
 * `SCH_PAINTER::draw( SCH_TEXT )` of a free text: at its anchor, with its own justification (`textLie`), a vertical one turned a quarter; lines split at `\n`. A text with a glyph
 * width of its own (an imported DXF text's, `width_nm`) is that wide (`GetTextSize().x`), and its pen is of that width (`GetEffectiveTextPenWidth`: `GetPenSizeForNormal(
 * GetTextWidth() )`, clamped to the smaller side).
 */
export function schTextDrawSpec(t: Pick<SchematicText, "content" | "shown" | "at" | "angle" | "size_nm" | "width_nm" | "h" | "v" | "vertical" | "bold" | "italic"> & { thickness_nm?: number | null }, defaultPen = SCH_DEFAULT_PEN_NM): KicadTextSpec {
  const lie = textLie(t);
  const size = sizeOf(t.size_nm);
  const width = t.width_nm && t.width_nm > 0 ? t.width_nm : size;
  const bold = !!t.bold;
  // its own pen when Auto Thickness is unticked (`GetTextThickness() > 1`)
  const own = t.thickness_nm && t.thickness_nm > 1 ? t.thickness_nm : 0;
  const pen = width === size ? schDrawPen(size, bold, defaultPen, own) : iuRound(effectiveTextPenWidth(own, [width, size], bold, defaultPen));
  return { text: t.shown ?? t.content, pos: t.at, size: [width, size], pen, halign: lie.h, valign: lie.v, angleDeg: lie.vertical ? 90 : 0, italic: !!t.italic, bold, multiline: true, iu: SCH_IU };
}

export interface SchLabelLook {
  net: string;
  at: readonly [number, number];
  scope: "local" | "global" | "hierarchical" | string;
  shape?: string | null;
  size_nm?: number | null;
  bold?: boolean;
  italic?: boolean;
  /** Its own pen (`TextAttrs::thickness_nm`: Auto Thickness unticked). */
  thickness_nm?: number | null;
  /** The justification its spin does not say (`SchExtras::label_justify`): centred on the anchor, its own vertical one. */
  justify?: { center?: boolean; v?: "top" | "center" | "bottom" | null } | null;
}

/** `SCH_LABEL_BASE::SetSpinStyle`: right and up are left-justified, left and bottom right-justified; up and bottom run up the sheet. */
function spinAttrs(spin: LabelSpin): { halign: HJustify; angleDeg: number } {
  return { halign: spin === "right" || spin === "up" ? "left" : "right", angleDeg: spin === "up" || spin === "bottom" ? 90 : 0 };
}

/** `GetLabelBoxExpansion`: `KiROUND( m_LabelSizeRatio * GetTextSize().y )`. */
export const labelBoxExpansion = (size: number): number => iuRound(schFormattingLook().labelSizeRatio * size);

/** `GetSchematicTextOffset` of a label of `scope` (a global label's by its shape): where its text's position is from its anchor. */
export function labelTextOffset(l: Pick<SchLabelLook, "scope" | "shape" | "size_nm" | "bold">, spin: LabelSpin): [number, number] {
  const size = sizeOf(l.size_nm);
  if (l.scope === "global" && l.shape) {
    let horiz = labelBoxExpansion(size);
    // "Use three-quarters-height as proxy for triangle size" (an int's `* 3 / 4`)
    if (l.shape === "input" || l.shape === "bidirectional" || l.shape === "tri_state") horiz += Math.trunc((size / SCH_IU) * 3 / 4) * SCH_IU;
    // "Center the text on the center line of "E" instead of "R" to make room for an overbar"
    const vert = iuTrunc(size * 0.0715);
    return spin === "left" ? [-horiz, vert] : spin === "up" ? [vert, -horiz] : spin === "right" ? [horiz, vert] : [vert, horiz];
  }
  if (l.scope === "hierarchical" && l.shape) {
    const dist = iuRound(schFormattingLook().textOffsetRatio * size) + size;
    return spin === "left" ? [-dist, 0] : spin === "up" ? [0, -dist] : spin === "right" ? [dist, 0] : [0, dist];
  }
  // a local label: "add an offset to x (or y) position to aid readability of text on a wire or line"
  const dist = iuRound(schFormattingLook().textOffsetRatio * size) + schOwnPen(size, !!l.bold);
  return spin === "up" || spin === "bottom" ? [-dist, 0] : [0, -dist];
}

/** `SCH_PAINTER::draw( SCH_TEXT )` of a label: at its anchor plus `GetSchematicTextOffset`, its spin's angle and justification, centred vertically on a global or hierarchical flag, on the wire's side of a local one. */
export function schLabelDrawSpec(l: SchLabelLook, spin: LabelSpin, defaultPen = SCH_DEFAULT_PEN_NM): KicadTextSpec {
  const size = sizeOf(l.size_nm);
  const bold = !!l.bold;
  const [dx, dy] = labelTextOffset(l, spin);
  const flagged = (l.scope === "global" || l.scope === "hierarchical") && !!l.shape;
  return {
    text: l.net,
    pos: [l.at[0] + dx, l.at[1] + dy],
    size: [size, size],
    pen: schDrawPen(size, bold, defaultPen, l.thickness_nm),
    ...spinAttrs(spin),
    // a centred text or its own vertical justification (Edit Text & Graphics' H and V Align; `SetSpinStyle` puts them back)
    ...(l.justify?.center ? { halign: "center" as const } : {}),
    valign: l.justify?.v ?? (flagged ? "center" : "bottom"),
    italic: !!l.italic,
    bold,
    multiline: false,
    iu: SCH_IU,
  };
}

/** `GetTextBox( aRenderSettings ).GetWidth()` of a global label's text (its own pen), which `SCH_GLOBALLABEL::CreateGraphicShape` makes the flag as long as. */
export function labelTextBoxWidth(l: Pick<SchLabelLook, "net" | "size_nm" | "bold" | "italic">, font: KicadFont = STROKE): number {
  const size = sizeOf(l.size_nm);
  const b = kicadTextBox({ text: l.net, pos: [0, 0], size: [size, size], pen: schOwnPen(size, !!l.bold), halign: "left", valign: "bottom", italic: !!l.italic, bold: !!l.bold, multiline: false, iu: SCH_IU }, font);
  return b.x1 - b.x0;
}

/** A text box's corners, its margins (left, top, right, bottom) and its text's looks. */
export interface SchTextBoxLook {
  start: readonly [number, number];
  end: readonly [number, number];
  margins: readonly [number, number, number, number];
  text: string;
  /** 0 or 90 (degrees). */
  angleDeg: number;
  size_nm: number;
  h: HJustify;
  v: VJustify;
  bold?: boolean;
  italic?: boolean;
  /** Its own pen (`TextAttrs::thickness_nm`: Auto Thickness unticked). */
  thickness_nm?: number | null;
}

/** `SCH_TEXTBOX::GetDrawPos`: the box's corner, side middle or centre the justification names, the margins in from it (a text that runs up the box reads the box turned). */
export function schTextBoxDrawPos(b: Pick<SchTextBoxLook, "start" | "end" | "margins" | "angleDeg" | "h" | "v">): [number, number] {
  const [left, right] = [Math.min(b.start[0], b.end[0]), Math.max(b.start[0], b.end[0])];
  const [top, bottom] = [Math.min(b.start[1], b.end[1]), Math.max(b.start[1], b.end[1])];
  const [ml, mt, mr, mb] = b.margins;
  // `( bbox.GetTop() + bbox.GetBottom() ) / 2`: an int's division
  const mid = (a: number, c: number) => iuTrunc((a + c) / 2);
  let x = left + ml;
  let y = bottom - mb;
  if (Math.round(b.angleDeg) % 180 === 90) {
    y = b.h === "left" ? bottom - mb : b.h === "center" ? mid(top, bottom) : top + mt;
    x = b.v === "top" ? left + ml : b.v === "center" ? mid(left, right) : right - mr;
  } else {
    x = b.h === "left" ? left + ml : b.h === "center" ? mid(left, right) : right - mr;
    y = b.v === "top" ? top + mt : b.v === "center" ? mid(top, bottom) : bottom - mb;
  }
  return [x, y];
}

/**
 * `SCH_PAINTER::draw( SCH_TEXTBOX )`: the text broken to the column (`SCH_TEXTBOX::GetShownText`: the box's width less the left and right margins, its height less the top
 * and bottom ones for a text that runs up it; `FONT::LinebreakText` in the box's font, the pen taken off) at `GetDrawPos`, with the box's justification.
 */
export function schTextBoxDrawSpec(b: SchTextBoxLook, font: KicadFont = STROKE, defaultPen = SCH_DEFAULT_PEN_NM): KicadTextSpec {
  const size = b.size_nm;
  const bold = !!b.bold;
  const italic = !!b.italic;
  const vertical = Math.round(b.angleDeg) % 180 === 90;
  const [ml, mt, mr, mb] = b.margins;
  const col = vertical ? Math.abs(b.end[1] - b.start[1]) - (mt + mb) : Math.abs(b.end[0] - b.start[0]) - (ml + mr);
  const measure = (text: string, s: number) => lineAdvance(text, font, [s, s], italic, SCH_IU);
  const lines = linebreakText(b.text, col, size, schOwnPen(size, bold), measure);
  return {
    text: lines.join("\n"),
    pos: schTextBoxDrawPos(b),
    size: [size, size],
    pen: schDrawPen(size, bold, defaultPen, b.thickness_nm),
    halign: b.h,
    valign: b.v,
    angleDeg: vertical ? 90 : 0,
    italic,
    bold,
    multiline: true,
    iu: SCH_IU,
  };
}
