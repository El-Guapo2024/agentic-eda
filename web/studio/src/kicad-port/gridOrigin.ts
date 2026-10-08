// The grid origin (`common.Control.gridSetOrigin` / `gridResetOrigin` / `editGridOrigin`): the point the editing grid is anchored at, so a grid point is
// `origin + n * grid`. Ported from pcbnew/tools/pcb_control.cpp (`PCB_CONTROL::DoSetGridOrigin`, `GridPlaceOrigin`, `GridResetOrigin`, `Reset`: the origin
// marker's colour), common/tool/common_tools.cpp (`COMMON_TOOLS::GridOrigin`: the X / Y entry dialog) and common/origin_viewitem.cpp
// (`ORIGIN_VIEWITEM::ViewDraw`: a circle with an X, 16 px, not drawn at (0, 0)), commit 8303b2ad. Pure: the painters draw it, the snap helpers use it.

export interface Origin {
  x: number;
  y: number;
}

export const NO_ORIGIN: Origin = { x: 0, y: 0 };

/** `m_drawAtZero = false`: the marker is not drawn while the origin is where it starts. */
export function originIsSet(at: Origin | null | undefined): at is Origin {
  return !!at && (at.x !== 0 || at.y !== 0);
}

/** One coordinate on the grid anchored at `origin`: `origin + round( ( v - origin ) / grid ) * grid` (a grid of 0 or less leaves the value as it is, rounded). */
export function snapAxis(value: number, grid: number, origin: number): number {
  if (!(grid > 0)) return Math.round(value);
  return Math.round((value - origin) / grid) * grid + origin;
}

/** The grid origin from a state JSON `[x, y]` (null / absent: (0, 0)). */
export function originOf(pair: readonly [number, number] | null | undefined): Origin {
  return pair ? { x: pair[0], y: pair[1] } : NO_ORIGIN;
}

interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** A CSS colour as the studio writes them -- `#rrggbb`, `#rrggbbaa`, `#rgb`, `rgb( ... )` or `rgba( ... )` -- in 0..1 channels; null when it is none of those. */
export function parseCssColor(css: string): Rgba | null {
  const s = css.trim();
  let m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(s);
  if (m) {
    const n = parseInt(m[1]!, 16);
    return { r: ((n >> 16) & 255) / 255, g: ((n >> 8) & 255) / 255, b: (n & 255) / 255, a: m[2] ? parseInt(m[2], 16) / 255 : 1 };
  }
  m = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(s);
  if (m) return { r: parseInt(m[1]! + m[1]!, 16) / 255, g: parseInt(m[2]! + m[2]!, 16) / 255, b: parseInt(m[3]! + m[3]!, 16) / 255, a: 1 };
  m = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(s);
  if (m) return { r: Number(m[1]) / 255, g: Number(m[2]) / 255, b: Number(m[3]) / 255, a: m[4] === undefined ? 1 : Number(m[4]) };
  return null;
}

/** `COLOR4D::GetBrightness()`: the weighted W3C formula. */
export function brightness(c: Rgba): number {
  return c.r * 0.299 + c.g * 0.587 + c.b * 0.117;
}

const css = (c: Rgba) => `rgba(${Math.round(c.r * 255)}, ${Math.round(c.g * 255)}, ${Math.round(c.b * 255)}, ${Math.round(c.a * 1000) / 1000})`;

/**
 * The marker's colour (`PCB_CONTROL::Reset`): the frame's grid colour, darkened by 0.25 on a bright background (`Darken`: channels times 0.75) and
 * brightened by 0.25 on a dark one (`Brighten`: channel times 0.75 plus 0.25). The grid colour as it is when a colour cannot be read.
 */
export function originMarkerColor(gridColor: string, backgroundColor: string): string {
  const grid = parseCssColor(gridColor);
  const background = parseCssColor(backgroundColor);
  if (!grid || !background) return gridColor;
  const f = 0.25;
  const out = brightness(background) > 0.5 ? { ...grid, r: grid.r * (1 - f), g: grid.g * (1 - f), b: grid.b * (1 - f) } : { ...grid, r: grid.r * (1 - f) + f, g: grid.g * (1 - f) + f, b: grid.b * (1 - f) + f };
  return css(out);
}

/**
 * What `COMMON_TOOLS::GridOrigin`'s dialog (`WX_PT_ENTRY_DIALOG`, "X:" "Y:") hands back: the two entries read as numbers in the display unit
 * (`toUm` converts one), as an origin in um -- or null when either is not a number.
 */
export function originFromEntries(xText: string, yText: string, toUm: (value: number) => number): Origin | null {
  const x = Number(xText.trim().replace(",", "."));
  const y = Number(yText.trim().replace(",", "."));
  if (xText.trim() === "" || yText.trim() === "" || !Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x: Math.round(toUm(x)), y: Math.round(toUm(y)) };
}
