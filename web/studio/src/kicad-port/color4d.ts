// `COLOR4D`'s mixing operations (include/gal/color4d.h, KiCad 8303b2ad) on the CSS colours the studio's painters set, and the colours the board painter
// derives with them: the ratsnest's (`RATSNEST_VIEW_ITEM::ViewDraw`), High Contrast Mode's dim (`PCB_RENDER_SETTINGS::GetColor`'s `color.Mix( background,
// m_hiContrastFactor )`) and the net highlight's darkening of everything else (`GetColor`'s single-net highlight branch). Pure: the painter applies them.
import { brightness, parseCssColor } from "./gridOrigin";
import { HIGHLIGHT_FACTOR } from "./netHighlight";

/** A colour in COLOR4D's 0..1 channels. */
export interface Color4 {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** `COLOR4D::Brightened( f )`: each channel `c * ( 1 - f ) + f`, the alpha kept. */
export function brightened(c: Color4, f: number): Color4 {
  return { r: c.r * (1 - f) + f, g: c.g * (1 - f) + f, b: c.b * (1 - f) + f, a: c.a };
}

/** `COLOR4D::Darkened( f )`: each channel `c * ( 1 - f )`, the alpha kept. */
export function darkened(c: Color4, f: number): Color4 {
  return { r: c.r * (1 - f), g: c.g * (1 - f), b: c.b * (1 - f), a: c.a };
}

/** `COLOR4D::Mix( aColor, f )`: `aColor * ( 1 - f ) + this * f` per channel -- `f` is how much of this colour is kept -- with this colour's alpha. */
export function mix(c: Color4, other: Color4, f: number): Color4 {
  return { r: other.r * (1 - f) + c.r * f, g: other.g * (1 - f) + c.g * f, b: other.b * (1 - f) + c.b * f, a: c.a };
}

/** `COLOR4D::WithAlpha( a )`. */
export function withAlpha(c: Color4, a: number): Color4 {
  return { ...c, a };
}

/** A colour as `rgba( r, g, b, a )`, the channels rounded to bytes and the alpha to a thousandth. */
export function cssOf(c: Color4): string {
  const byte = (v: number) => Math.max(0, Math.min(255, Math.round(v * 255)));
  const alpha = Math.max(0, Math.min(1, Math.round(c.a * 1000) / 1000));
  return `rgba(${byte(c.r)}, ${byte(c.g)}, ${byte(c.b)}, ${alpha})`;
}

/** A CSS colour the studio writes (`#rrggbb`, `#rrggbbaa`, `#rgb`, `rgb()`, `rgba()`) in 0..1 channels; null for anything else. */
export function parseColor(css: string): Color4 | null {
  return parseCssColor(css);
}

/** `1.0f - COMMON_SETTINGS::m_Appearance.hicontrast_dimming_factor` (`PCB_RENDER_SETTINGS::LoadColors`; the setting's default 0.8, `common_settings.cpp`). */
export const HI_CONTRAST_FACTOR = 1 - 0.8;

/**
 * `dim_factor_Edge_Cuts = std::max( m_hiContrastFactor, 0.3f )`: "Graphics on Edge_Cuts layer are not fully dimmed or hidden because they are useful when
 * working on another layer" (`PCB_RENDER_SETTINGS::GetColor`).
 */
export const EDGE_CUTS_DIM_FACTOR = Math.max(HI_CONTRAST_FACTOR, 0.3);

/** The factor in use: `1.0f - hicontrast_dimming_factor` of the person's Preferences > Common ("High-contrast mode dimming factor:"), read by the painters. */
let hiContrastFactorNow = HI_CONTRAST_FACTOR;

/** `PCB_RENDER_SETTINGS::LoadColors`' `m_hiContrastFactor = 1.0f - Pgm().GetCommonSettings()->m_Appearance.hicontrast_dimming_factor`. */
export function setHiContrastFactor(factor: number): void {
  hiContrastFactorNow = Number.isFinite(factor) ? Math.min(1, Math.max(0, factor)) : HI_CONTRAST_FACTOR;
}

export function hiContrastFactor(): number {
  return hiContrastFactorNow;
}

/** `std::max( m_hiContrastFactor, 0.3f )` of the factor in use. */
export function edgeCutsDimFactor(): number {
  return Math.max(hiContrastFactorNow, 0.3);
}

/**
 * High Contrast Mode's Dim of an item off the active layer (`HIGH_CONTRAST_MODE::DIMMED`): its colour mixed into the board's background, `factor` of the
 * colour kept, the alpha kept -- `color.Mix( backgroundColor, m_hiContrastFactor )`. An inactive item covers what lies under it as an item of that colour does;
 * it is not see-through. A colour that cannot be read is returned as it is.
 */
export function hiContrastDim(css: string, background: string, factor = hiContrastFactorNow): string {
  const c = parseColor(css);
  const bg = parseColor(background);
  if (!c || !bg) return css;
  return cssOf(mix(c, bg, factor));
}

/**
 * The single-net highlight's colour of an item that is not on a highlighted net -- every board item but the ones of the net, a silkscreen line, a text, the
 * board outline as much as a track of another net (`netCode` stays -1 for an item that is not a connected copper item): `color.Darkened( 1.0 -
 * m_highlightFactor )` (`m_layerColorsDark`; `m_highlightFactor` is 0.5, netHighlight.ts).
 */
export function highlightDarken(css: string): string {
  const c = parseColor(css);
  return c ? cssOf(darkened(c, 1 - HIGHLIGHT_FACTOR)) : css;
}

/** The single-net highlight's colour of an item on a highlighted net that has no colour of its own for it: `color.Brightened( m_highlightFactor )` (`m_layerColorsHi`). */
export function highlightBrighten(css: string): string {
  const c = parseColor(css);
  return c ? cssOf(brightened(c, HIGHLIGHT_FACTOR)) : css;
}

/** What `RATSNEST_VIEW_ITEM::ViewDraw` draws a line in. */
export interface RatsnestLineState {
  /** A line of the dynamic ratsnest: one end on an item being moved (`m_data->GetLocalRatsnest()`). */
  dynamic: boolean;
  /** Its net is highlighted (`highlightedNets.count( i )`). */
  highlighted: boolean;
  /** `dimStatic = m_data->GetLocalRatsnest().size() > 0 || highlightedNets.size() > 0`: something is moving or a net is highlighted. */
  dimStatic: boolean;
}

/**
 * The colour of one ratsnest line (`RATSNEST_VIEW_ITEM::ViewDraw`), from its colour (the ratsnest colour, or its net's or net class's when the net colour
 * mode shows them) and the board's background. `adjustColor( c, delta, alpha )` brightens by `delta` on a dark background (brightness below 0.5) and darkens on
 * a light one, and sets the alpha, at most 1:
 * - a dynamic line: `adjustColor( color, 0.5, color.a + 0.3 )`;
 * - a static line: with `dimStatic`, first `adjustColor( color, 0.0, color.a / 2 )` (the alpha halved), then for a highlighted net `adjustColor( color, 0.8,
 *   color.a + 0.4 )`; any other keeps the colour.
 * A colour that cannot be read is returned as it is.
 */
export function ratsnestLineColor(css: string, background: string, line: RatsnestLineState): string {
  let c = parseColor(css);
  const bg = parseColor(background);
  if (!c || !bg) return css;
  const dark = brightness(bg) < 0.5;
  const adjust = (base: Color4, delta: number, alpha: number): Color4 => withAlpha(dark ? brightened(base, delta) : darkened(base, delta), Math.min(alpha, 1));
  if (line.dynamic) return cssOf(adjust(c, 0.5, c.a + 0.3));
  if (line.dimStatic) c = adjust(c, 0, c.a / 2);
  if (line.highlighted) c = adjust(c, 0.8, c.a + 0.4);
  return cssOf(c);
}

/**
 * The curve of "Curved Ratsnest Lines" (`m_DisplayRatsnestLinesCurved`): `DrawCurve( source, center, center, target )`, a cubic Bezier whose two control points
 * are both `center = source + ( 0.5 dx - 0.1 dy, 0.5 dy + 0.1 dx )` -- a bow 0.1 of the length off the middle, to the left of the line as it runs from its
 * source (`VECTOR2I`'s int conversion truncates).
 */
export function ratsnestCurveControl(source: readonly [number, number], target: readonly [number, number]): [number, number] {
  const dx = target[0] - source[0];
  const dy = target[1] - source[1];
  return [Math.trunc(source[0] + 0.5 * dx - 0.1 * dy), Math.trunc(source[1] + 0.5 * dy + 0.1 * dx)];
}
