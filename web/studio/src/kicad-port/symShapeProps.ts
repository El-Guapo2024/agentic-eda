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
  /** `m_customColorSwatch`: the colour of the fill chosen -- the border's (else `LAYER_DEVICE`'s) for the outline fill, `LAYER_DEVICE_BACKGROUND`'s for the background one, the
   *  shape's own for "Fill with:", none for no fill (UI-LEFT-3 Y3-5); null for `COLOR4D::UNSPECIFIED`. OK gives the shape this colour (`SetFillColor( m_customColorSwatch->
   *  GetSwatchColor() )`), which a file writes for a colour fill only (`formatFill`). */
  fillColor: SchColor | null;
  /** `m_privateCheckbox`. */
  private: boolean;
}

const NO_STATE: GraphicState = { looks: {}, lineStyle: null, thicknessMm: null };

/** The editor's `LAYER_DEVICE` and `LAYER_DEVICE_BACKGROUND` colours (`m_frame->GetRenderSettings()->GetLayerColor( .. )`), which the fill swatch shows for the two body fills. */
export interface DeviceColors {
  device: SchColor;
  deviceBackground: SchColor;
}

const own = (c: SchColor | null | undefined) => (c && !isUnspecified(c) ? c : null);

/**
 * `m_customColorSwatch` for the fill `fill` (`TransferDataToWindow`'s and `onFillRadioButton`'s branches): the outline fill shows the border colour, or `LAYER_DEVICE`'s when the
 * border has none; the background fill `LAYER_DEVICE_BACKGROUND`'s; no fill none; "Fill with:" the colour it has (`ownFill`; `GetNewSwatchColor()` asks for one when the radio is
 * picked, so the caller keeps the swatch as it is and opens the chooser).
 */
export function fillSwatch(fill: LibraryFill, borderColor: SchColor | null, ownFill: SchColor | null, layer: DeviceColors): SchColor | null {
  switch (fill) {
    case "outline":
      return own(borderColor) ?? layer.device;
    case "background":
      return layer.deviceBackground;
    case "color":
      return own(ownFill);
    default:
      return null;
  }
}

/** `onFillRadioButton`: the fill chosen, and the swatch it shows. */
export function withFillChoice(form: ShapeForm, fill: LibraryFill, layer: DeviceColors): ShapeForm {
  return { ...form, fill, fillColor: fill === "color" ? form.fillColor : fillSwatch(fill, form.borderColor, null, layer) };
}

/** `onBorderSwatch`: a new border colour, which the swatch takes while the outline fill is chosen (`LAYER_DEVICE`'s for none). */
export function withBorderColor(form: ShapeForm, borderColor: SchColor | null, layer: DeviceColors): ShapeForm {
  return { ...form, borderColor, fillColor: form.fill === "outline" ? fillSwatch("outline", borderColor, null, layer) : form.fillColor };
}

/** `onCustomColorSwatch`: a colour picked in the swatch chooses "Fill with:". */
export function withFillColor(form: ShapeForm, fillColor: SchColor | null): ShapeForm {
  return { ...form, fillColor, fill: "color" };
}

/** A CSS colour (`#rrggbb`, `#rrggbbaa`) as the swatch's colour; the layer colours of `layerColor`. */
export function schColorOfCss(css: string, fallback: SchColor = { r: 0, g: 0, b: 0, a: 255 }): SchColor {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})?$/i.exec(css.trim());
  return m ? { r: parseInt(m[1]!, 16), g: parseInt(m[2]!, 16), b: parseInt(m[3]!, 16), a: m[4] ? parseInt(m[4], 16) : 255 } : fallback;
}

export function shapeFormOf(g: Pick<Shape, "stroke_mm" | "fill" | "unit" | "body_style">, symbol: { unit_count: number } & BodyStyled, state: GraphicState = NO_STATE, layer?: DeviceColors): ShapeForm {
  // a hatched fill is none of the dialog's four: `FILLED_WITH_COLOR`'s radio stays off as KiCad's `else` branch leaves "Do not fill" on
  const fill: LibraryFill = g.fill === "color" && state.looks.hatch ? "none" : g.fill;
  const borderColor = own(state.looks.stroke_color);
  return {
    border: g.stroke_mm >= 0,
    widthMm: g.stroke_mm >= 0 ? g.stroke_mm : 0,
    fill,
    commonUnits: symbol.unit_count > 1 && g.unit === 0,
    commonBodyStyles: isMultiBodyStyle(symbol) && g.body_style === 0,
    borderColor,
    style: state.lineStyle ?? "solid",
    // without the layer colours (a test of the other rows) the swatch is the shape's own colour
    fillColor: layer ? fillSwatch(fill, borderColor, own(state.looks.fill_color), layer) : own(state.looks.fill_color),
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
  // `stroke.SetLineStyle`, `stroke.SetColor`, `SetFillMode` (no hatch: the four radios), `SetFillColor( m_customColorSwatch->GetSwatchColor() )` whatever the fill, `SetPrivate`;
  // the link and pen stay
  const looks = { ...before.looks, stroke_color: form.borderColor, hatch: null, private: form.private, fill_color: form.fillColor };
  return { strokeMm: strokeOf(form), fill: form.fill, unit: form.commonUnits ? 0 : editor.unit, bodyStyle: form.commonBodyStyles ? 0 : editor.bodyStyle, state: { looks, lineStyle: form.style, thicknessMm: before.thicknessMm } };
}
