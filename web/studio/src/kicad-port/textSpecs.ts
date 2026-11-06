// What `FONT::Draw` is given for each kind of text the studio draws: the position, `TEXT_ATTRIBUTES` and the face (`EDA_TEXT::GetAttributes`, `GetDrawRotation`,
// `GetEffectiveTextPenWidth`, `GetFontName`), from the item as the server sends it -- the one place the painters and the hit tests get a text's layout from (UI-LEFT-2 PP1).
//
// KiCad 8303b2ad: `pcbnew/pcb_painter.cpp::draw( const PCB_TEXT* )` (the attributes, `GetDrawRotation`, the effective pen), `draw( const PCB_DIMENSION_BASE* )` (the label at
// its text position and angle), `pcbnew/pcb_text.cpp::GetDrawRotation` (a footprint's text kept upright, a board's normalised), `pcbnew/pcb_textbox.cpp::GetDrawPos`.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardText, Dimension, FieldInfo } from "../api/types";
import type { BoardTextBox } from "../api/pcbItemTypes";
import { drawAngle } from "./fpFields";
import { effectiveTextPenWidth, type HJustify, type KicadTextSpec, type VJustify } from "./kicadText";

/** A text's spec and the face it asks for (null: the stroke font). */
export interface FacedSpec {
  spec: KicadTextSpec;
  face: string | null;
}

const hOf = (v: number): HJustify => (v < 0 ? "left" : v > 0 ? "right" : "center");
const vOf = (v: number): VJustify => (v < 0 ? "top" : v > 0 ? "bottom" : "center");

/** `EDA_ANGLE::Normalize`: [0, 360). */
const norm = (deg: number): number => ((deg % 360) + 360) % 360;

/** A text of the board (`PCB_TEXT` with no footprint): its glyph width when it has one, its automatic pen when it has no thickness, its angle normalised. */
export function boardTextSpec(t: BoardText): FacedSpec {
  const size: [number, number] = [t.width ?? t.size, t.size];
  const bold = !!t.bold;
  return {
    spec: {
      text: t.shown ?? t.content,
      pos: [t.x, t.y],
      size,
      pen: effectiveTextPenWidth(t.auto_thickness ? 0 : t.stroke_width, size, bold),
      halign: t.justify,
      valign: t.valign ?? "center",
      angleDeg: norm(t.angle / 1000),
      mirror: t.mirror,
      italic: !!t.italic,
      bold,
    },
    face: t.face ?? null,
  };
}

/**
 * A footprint's field or text (`PCB_FIELD`, `PCB_TEXT` of a footprint; a `FpTextInfo` through `artTextAsField`), on the board: its width and height, the pen of its
 * thickness (none: the automatic one), `GetDrawRotation` (kept upright: ]-90, 90]).
 */
export function fieldSpec(f: FieldInfo): FacedSpec {
  const size: [number, number] = [f.w || f.h, f.h];
  return {
    spec: {
      text: f.shown ?? f.text,
      pos: [f.x, f.y],
      size,
      pen: effectiveTextPenWidth(f.thickness, size, f.bold),
      halign: hOf(f.halign),
      valign: vOf(f.valign),
      angleDeg: drawAngle(f.angle, f.upright) / 1000,
      mirror: f.mirror,
      italic: f.italic,
      bold: f.bold,
    },
    face: f.face ?? null,
  };
}

/** A dimension's label (`PCB_DIMENSION_BASE`, a `PCB_TEXT`): at its text position and angle, centred both ways, square, its own pen (15 % of its size unless it says). */
export function dimensionTextSpec(d: Pick<Dimension, "text" | "text_at" | "computed_text_angle" | "text_size_nm" | "text_thickness_nm">): FacedSpec {
  const size: [number, number] = [d.text_size_nm, d.text_size_nm];
  return {
    spec: {
      text: d.text,
      pos: [d.text_at[0], d.text_at[1]],
      size,
      pen: effectiveTextPenWidth(d.text_thickness_nm ?? Math.round(d.text_size_nm * 0.15), size, false),
      halign: "center",
      valign: "center",
      angleDeg: d.computed_text_angle / 1000,
    },
    face: null,
  };
}

/** A text box's text (`PCB_TEXTBOX`) once wrapped: at `GetDrawPos`, its justification, angle and looks. */
export function textBoxSpec(b: BoardTextBox, wrapped: string, pos: [number, number]): FacedSpec {
  const size: [number, number] = [b.width || b.size, b.size];
  return {
    spec: {
      text: wrapped,
      pos,
      size,
      pen: effectiveTextPenWidth(b.thickness_auto ? 0 : b.thickness, size, b.bold),
      halign: b.halign,
      valign: b.valign,
      angleDeg: b.angle / 1000,
      mirror: b.mirror,
      italic: b.italic,
      bold: b.bold,
      lineSpacing: (b.line_spacing_permille || 1000) / 1000,
    },
    face: b.face,
  };
}
