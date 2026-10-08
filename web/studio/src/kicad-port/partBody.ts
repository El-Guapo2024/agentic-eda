// The box the 3D tab draws for a part when KiCad's own render (`/api/board.glb`) is not there -- the render takes seconds, or it failed -- sized from the
// part's BODY. KiCad draws a part's body on `F.Fab`; the courtyard around it is the body plus a clearance margin plus the pads' reach (a 0603 resistor's
// courtyard is three times as wide as the part), so a box on the courtyard reads as an oversized block. The server sends the body as `part.body`
// (crates/cli/src/body_api.rs: the box of the footprint's `F.Fab` graphics, board-space µm); a footprint with no `F.Fab` has none, and its courtyard stands in.
import type { Um } from "../api/types";

export type BoxUm = readonly [Um, Um, Um, Um];

/** The tallest a fallback box gets: the same stand-in height the 3D tab has always used for a part it knows nothing about. */
export const FALLBACK_MAX_HEIGHT_MM = 2;
/** The lowest: a 0603 is 0.45 mm tall, and a flat sliver would not read as a part at all. */
export const FALLBACK_MIN_HEIGHT_MM = 0.4;

/** `(x0, y0, x1, y1)` the fallback box covers: the part's `F.Fab` body, else its courtyard, else `null` (an unplaced part). */
export function partBodyBoxUm(part: { body?: BoxUm | null; courtyard?: BoxUm | null }): BoxUm | null {
  return part.body ?? part.courtyard ?? null;
}

/**
 * How tall the fallback box is. There is no model data to measure, so it is a plausible height from the body's own footprint: half its narrower side,
 * between {@link FALLBACK_MIN_HEIGHT_MM} and {@link FALLBACK_MAX_HEIGHT_MM} -- a 1.6 x 0.8 mm resistor stays 0.4 mm, a SOIC-16 (3.9 mm wide) is 1.95 mm,
 * a module or a connector shell is capped at 2 mm. Only a stand-in; the real models (KiCad Models, the default) have their true heights.
 */
export function fallbackBodyHeightMm(widthMm: number, depthMm: number): number {
  const narrow = Math.min(widthMm, depthMm);
  return Math.min(FALLBACK_MAX_HEIGHT_MM, Math.max(FALLBACK_MIN_HEIGHT_MM, narrow / 2));
}
