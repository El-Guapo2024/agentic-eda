// What `DIALOG_SHAPE_PROPERTIES` (eeschema/dialogs/dialog_shape_properties.cpp) asks of a shape of a library symbol in the Symbol Editor: the dialog's title, its Border box
// with the width, the three fills a library shape can have, and the two "Common to all ..." boxes -- read off the graphic when the dialog opens (`TransferDataToWindow`) and
// turned back into the verb's fields when it is accepted (`TransferDataFromWindow`). The border's colour and style, "Fill with:" a colour of its own and "Private to Symbol
// Editor" are kept beside the shape (kicad-port/symLooks.ts, `set_symbol_graphic_looks` in the same batch; UI-LEFT-2 YY2).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibraryFill, LibrarySymbolGraphic } from "../api/types";
import { isMultiBodyStyle, type BodyStyled } from "./symBodyStyles";
import type { SchColor } from "../api/schEditTypes";
import { isUnspecified, type GraphicState } from "./symLooks";

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
    case "bezier":
      return "Curve Properties";
  }
}

/** The help line under the border's width. */
export const BORDER_HELP = "Set border width to 0 to use schematic's default line width.";

/** The Fill Style box (`m_fillBook`'s symbol editor page): `NO_FILL`, `FILLED_SHAPE`, `FILLED_WITH_BG_BODYCOLOR`, `FILLED_WITH_COLOR` ("Fill with:" and its swatch). */
export const FILL_CHOICES: readonly { value: LibraryFill; label: string }[] = [
  { value: "none", label: "Do not fill" },
  { value: "outline", label: "Fill with body outline color" },
  { value: "background", label: "Fill with body background color" },
  { value: "color", label: "Fill with:" },
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
  /** `m_borderColorSwatch`: the stroke's colour, null for `COLOR4D::UNSPECIFIED`. */
  borderColor: SchColor | null;
  /** `m_borderStyleCombo`: the line style, Solid for the default one (`DEFAULT_LINE_STYLE_LABEL`). */
  style: string;
  /** `m_customColorSwatch` of a shape filled with its own colour (`GetFillColor`); null for `COLOR4D::UNSPECIFIED`. The swatch of the other fills shows the layer's
   *  colour, which is drawn, not kept: OK keeps the shape's own fill colour then. */
  fillColor: SchColor | null;
  /** `m_privateCheckbox`. */
  private: boolean;
}

const NO_STATE: GraphicState = { looks: {}, lineStyle: null, thicknessMm: null };

export function shapeFormOf(g: Pick<Shape, "stroke_mm" | "fill" | "unit" | "body_style">, symbol: { unit_count: number } & BodyStyled, state: GraphicState = NO_STATE): ShapeForm {
  const own = (c: SchColor | null | undefined) => (c && !isUnspecified(c) ? c : null);
  return {
    border: g.stroke_mm >= 0,
    widthMm: g.stroke_mm >= 0 ? g.stroke_mm : 0,
    // a hatched fill is none of the dialog's four: `FILLED_WITH_COLOR`'s radio stays off as KiCad's `else` branch leaves "Do not fill" on
    fill: g.fill === "color" && state.looks.hatch ? "none" : g.fill,
    commonUnits: symbol.unit_count > 1 && g.unit === 0,
    commonBodyStyles: isMultiBodyStyle(symbol) && g.body_style === 0,
    borderColor: own(state.looks.stroke_color),
    style: state.lineStyle ?? "solid",
    fillColor: own(state.looks.fill_color),
    private: !!state.looks.private,
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
export function shapeEditOf(form: ShapeForm, editor: { unit: number; bodyStyle: number }, before: GraphicState = NO_STATE): { strokeMm: number; fill: LibraryFill; unit: number; bodyStyle: number; state: GraphicState } {
  // `stroke.SetLineStyle`, `stroke.SetColor`, `SetFillMode` (no hatch: the four radios), `SetFillColor( m_customColorSwatch )`, `SetPrivate`; the link and pen stay
  const looks = { ...before.looks, stroke_color: form.borderColor, hatch: null, private: form.private, fill_color: form.fill === "color" ? form.fillColor : before.looks.fill_color };
  return { strokeMm: strokeOf(form), fill: form.fill, unit: form.commonUnits ? 0 : editor.unit, bodyStyle: form.commonBodyStyles ? 0 : editor.bodyStyle, state: { looks, lineStyle: form.style, thicknessMm: before.thicknessMm } };
}
