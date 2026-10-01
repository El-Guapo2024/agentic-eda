// Port of the net-highlight half of pcbnew/pcb_painter.cpp's
// PCB_PAINTER::GetColor (the "Single net highlight mode" branch, lines
// ~407-420 of the KiCad source) and include/gal/color4d.h's
// COLOR4D::Brighten/Darken -- exact per-channel formulas, not an alpha
// trick. This is deliberately a DIFFERENT feature from this app's
// pre-existing `highContrast` layer dimming (`painter.ts`'s `layerAlpha`,
// a flat-alpha approximation of a separate KiCad feature,
// `m_hiContrastFactor`): source applies both independently, but only net
// highlight's brighten/darken is in scope here (the task's item 1 names
// "net highlight (`) with KiCad's dimming of everything else").
//
// color4d.h, byte for byte:
//   Brighten(f): r' = r*(1-f) + f   (lerp toward white)
//   Darken(f):   r' = r*(1-f)       (lerp toward black)
// both per channel, alpha untouched. `f` is COLOR4D's 0..1 range; this
// module works in 0..255 RGB (CSS/canvas's native range) and does the
// 0..1 conversion internally so callers never have to.
//
// pcb_painter.cpp's own constant: `m_highlightFactor = 0.5f`
// (common/render_settings.cpp).
export const HIGHLIGHT_FACTOR = 0.5;

export interface RGB {
  r: number;
  g: number;
  b: number;
}

function clamp255(v: number): number {
  return Math.max(0, Math.min(255, Math.round(v)));
}

/** COLOR4D::Brighten -- lerp each channel toward white by `factor` (0..1). */
export function brighten(c: RGB, factor: number): RGB {
  return {
    r: clamp255(c.r * (1 - factor) + 255 * factor),
    g: clamp255(c.g * (1 - factor) + 255 * factor),
    b: clamp255(c.b * (1 - factor) + 255 * factor),
  };
}

/** COLOR4D::Darken -- lerp each channel toward black by `factor` (0..1). */
export function darken(c: RGB, factor: number): RGB {
  return {
    r: clamp255(c.r * (1 - factor)),
    g: clamp255(c.g * (1 - factor)),
    b: clamp255(c.b * (1 - factor)),
  };
}

/**
 * pcb_painter.cpp's net-highlight branch, applied to one item's base
 * color: `netCode === highlightedNet ? Brighten(HIGHLIGHT_FACTOR) :
 * Darken(HIGHLIGHT_FACTOR)`. Only called when highlighting is enabled at
 * all -- callers skip this entirely (keep the plain color) when
 * `netHighlight` is null, same as source's `m_highlightEnabled` gate.
 */
export function netHighlightColor(base: RGB, isOnHighlightedNet: boolean, factor = HIGHLIGHT_FACTOR): RGB {
  return isOnHighlightedNet ? brighten(base, factor) : darken(base, factor);
}

/** "#rrggbb" (or "#rgb") -> RGB. Not KiCad source -- plain CSS color parsing this module needs since this app's palette (layers.ts) is CSS hex strings, not COLOR4D. */
export function hexToRgb(hex: string): RGB {
  let h = hex.trim().replace(/^#/, "");
  if (h.length === 3) {
    h = h
      .split("")
      .map((c) => c + c)
      .join("");
  }
  const n = parseInt(h.slice(0, 6), 16);
  return { r: (n >> 16) & 0xff, g: (n >> 8) & 0xff, b: n & 0xff };
}

export function rgbToHex(c: RGB): string {
  const toHex = (v: number) => clamp255(v).toString(16).padStart(2, "0");
  return `#${toHex(c.r)}${toHex(c.g)}${toHex(c.b)}`;
}
