// What `DIALOG_SHAPE_PROPERTIES` (eeschema/dialogs/dialog_shape_properties.cpp) asks of a shape of a library symbol in the Symbol Editor: the dialog's title, its Border box
// with the width, the three fills a library shape can have, and the two "Common to all ..." boxes -- read off the graphic when the dialog opens (`TransferDataToWindow`) and
// turned back into the verb's fields when it is accepted (`TransferDataFromWindow`). The border's colour and style, the custom fill colour and Private to Symbol Editor are not
// modelled by the library symbol and are not offered.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibraryFill, LibrarySymbolGraphic } from "../api/types";

type Shape = Exclude<LibrarySymbolGraphic, { kind: "text" | "text_box" }>;

/** The width of a border switched off: -1 of a schematic's internal units, 0.0001 mm (`STROKE_PARAMS`). */
export const NO_BORDER_MM = -0.0001;

/** `SCH_SHAPE::GetFriendlyName` (`EDA_SHAPE::SHAPE_T_asString`'s display names): "%s Properties". A polyline is a `SHAPE_T::POLY`, which KiCad calls a polygon. */
export function shapeTitle(g: Pick<Shape, "kind">): string {
  switch (g.kind) {
    case "rectangle":
      return "Rectangle Properties";
    case "circle":
      return "Circle Properties";
    case "arc":
      return "Arc Properties";
    case "polyline":
      return "Polygon Properties";
  }
}

/** The help line under the border's width. */
export const BORDER_HELP = "Set border width to 0 to use schematic's default line width.";

/** The Fill Style box (`m_fillBook`'s symbol editor page): `NO_FILL`, `FILLED_SHAPE`, `FILLED_WITH_BG_BODYCOLOR`. */
export const FILL_CHOICES: readonly { value: LibraryFill; label: string }[] = [
  { value: "none", label: "Do not fill" },
  { value: "outline", label: "Fill with body outline color" },
  { value: "background", label: "Fill with body background color" },
];

/** What the dialog shows for a shape. */
export interface ShapeForm {
  /** `m_borderCheckbox`: `GetWidth() >= 0`. */
  border: boolean;
  /** `m_borderWidth` in mm: the shape's width, or the default (0) while the border is off, as the dialog fills it when the box is checked again. */
  widthMm: number;
  fill: LibraryFill;
  /** `symbol->IsMultiUnit() && shape->GetUnit() == 0`. */
  commonUnits: boolean;
  /** `symbol->IsMultiBodyStyle() && shape->GetBodyStyle() == 0`. */
  commonBodyStyles: boolean;
}

export function shapeFormOf(g: Pick<Shape, "stroke_mm" | "fill" | "unit" | "body_style">, symbol: { unit_count: number; has_alternate_body_style: boolean }): ShapeForm {
  return {
    border: g.stroke_mm >= 0,
    widthMm: g.stroke_mm >= 0 ? g.stroke_mm : 0,
    fill: g.fill,
    commonUnits: symbol.unit_count > 1 && g.unit === 0,
    commonBodyStyles: symbol.has_alternate_body_style && g.body_style === 0,
  };
}

/** The width as the dialog's own rule reads it: with the border on a width is at least 0 (`std::max( 0, ... )`), with it off the shape gets the no-border width. */
export function strokeOf(form: Pick<ShapeForm, "border" | "widthMm">): number {
  return form.border ? Math.max(0, Number.isFinite(form.widthMm) ? form.widthMm : 0) : NO_BORDER_MM;
}

/**
 * `TransferDataFromWindow`: the verb's fields. A box that is not checked puts the shape on the unit and the body style the editor is showing (`SetUnit( symbolEditor->GetUnit() )`),
 * a checked one on every one (0).
 */
export function shapeEditOf(form: ShapeForm, editor: { unit: number; bodyStyle: number }): { strokeMm: number; fill: LibraryFill; unit: number; bodyStyle: number } {
  return { strokeMm: strokeOf(form), fill: form.fill, unit: form.commonUnits ? 0 : editor.unit, bodyStyle: form.commonBodyStyles ? 0 : editor.bodyStyle };
}
