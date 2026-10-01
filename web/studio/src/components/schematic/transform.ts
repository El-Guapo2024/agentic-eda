// KiCad's symbol placement transform -- libs/kimath/include/transform.h's
// TRANSFORM (x1,y1,x2,y2: a row-major 2x2 matrix, `x'=x1*x+y1*y, y'=x2*x+
// y2*y`) and sch_symbol.cpp's `SetOrientation` (the exact 8 matrices a
// rotation/mirror combination produces), ported line-for-line from source
// read this session (not guessed): a schematic symbol's placed rotation is
// always a multiple of 90 degrees, and its `mirror` is a single axis (`x`
// or `y`, never both as one flag) or none -- 4 rotations x 3 mirror states
// is 12 combinations, but several produce the same matrix (e.g. "180 +
// mirror x" is the same matrix as "0 + mirror y"), so there are only 8
// distinct matrices in total, enumerated here directly rather than
// re-deriving the matrix product every time.
//
// Library-symbol graphics/pins are stored Y-up in the file (and in this
// app's own `LibGraphic`/`LibPin` types, which mirror the file's raw mm
// numbers) -- every point gets Y-negated once on the way into this
// module's "internal" space, matching eeschema's own parser
// (`parseXY(true)`: internal = (fileX, -fileY)) -- and from there behaves
// exactly like every other coordinate in this app (Y-down, um). A placed
// instance's own position (`SchematicSymbol.at`) is already in that
// shared Y-down um space and is never itself flipped.
import type { Degrees, LibPin, Mm } from "../../api/types";

/** A row-major 2x2 matrix: `x' = x1*x + y1*y`, `y' = x2*x + y2*y` -- libs/kimath's TRANSFORM, by field name. */
export type Matrix = { x1: number; y1: number; x2: number; y2: number };

const IDENTITY: Matrix = { x1: 1, y1: 0, x2: 0, y2: 1 };

/**
 * The 8 matrices a (rotation, mirror) combination produces --
 * sch_symbol.cpp's `SetOrientation`, read directly (both the 4 primitive
 * matrices -- rotate CW/CCW, mirror X/Y -- and the composition order,
 * `new = old * temp`, i.e. the mirror is applied after the rotation).
 * Keyed by `"<rot>:<mirror>"` with `rot` normalized to 0/90/180/270 and
 * `mirror` to `"none"/"x"/"y"`.
 */
const MATRICES: Record<string, Matrix> = {
  "0:none": { x1: 1, y1: 0, x2: 0, y2: 1 },
  "90:none": { x1: 0, y1: 1, x2: -1, y2: 0 },
  "180:none": { x1: -1, y1: 0, x2: 0, y2: -1 },
  "270:none": { x1: 0, y1: -1, x2: 1, y2: 0 },
  // KiCad's MIRROR_X flips Y (a vertical flip -- "mirror across the X
  // axis"), MIRROR_Y flips X -- the opposite of what the names suggest if
  // you read them as "mirror the X axis" rather than "mirror in X".
  "0:x": { x1: 1, y1: 0, x2: 0, y2: -1 },
  "0:y": { x1: -1, y1: 0, x2: 0, y2: 1 },
  "90:x": { x1: 0, y1: 1, x2: 1, y2: 0 },
  "90:y": { x1: 0, y1: -1, x2: -1, y2: 0 },
  // The remaining four (rot, mirror) combinations are not independent
  // reflections -- they coincide exactly with four of the entries above
  // (sch_symbol.cpp's own `GetOrientation()` resolves them to the same
  // matrix, confirmed directly from source): 180+mirror_x == 0+mirror_y,
  // 180+mirror_y == 0+mirror_x, 270+mirror_x == 90+mirror_y, 270+mirror_y
  // == 90+mirror_x.
  "180:x": { x1: -1, y1: 0, x2: 0, y2: 1 },
  "180:y": { x1: 1, y1: 0, x2: 0, y2: -1 },
  "270:x": { x1: 0, y1: -1, x2: -1, y2: 0 },
  "270:y": { x1: 0, y1: 1, x2: 1, y2: 0 },
};

/** Normalizes an arbitrary rotation to KiCad's own 0/90/180/270 (every real symbol instance is placed at one of these; anything else rounds to the nearest). */
function normalizeRot(rotDeg: Degrees): 0 | 90 | 180 | 270 {
  const r = (((Math.round(rotDeg / 90) * 90) % 360) + 360) % 360;
  return r as 0 | 90 | 180 | 270;
}

export function symbolTransformMatrix(rotDeg: Degrees, mirror: "x" | "y" | null): Matrix {
  const key = `${normalizeRot(rotDeg)}:${mirror ?? "none"}`;
  return MATRICES[key] ?? IDENTITY;
}

export function applyMatrix(m: Matrix, x: number, y: number): [number, number] {
  return [m.x1 * x + m.y1 * y, m.x2 * x + m.y2 * y];
}

/** A library point (mm, file Y-up) to this app's shared internal space (um, Y-down) -- the one Y-negate every library coordinate needs, before any instance transform/translation. */
export function libPointToInternalUm([xMm, yMm]: [Mm, Mm]): [number, number] {
  return [xMm * 1000, -yMm * 1000];
}

/** A library point, fully resolved: internal-space Y-flip, instance transform, then translation by the instance's own (already Y-down, um) placement. The one function every lib_symbols consumer should call -- never apply the Y-flip or the matrix separately, so there is exactly one place this pipeline can be gotten wrong. */
export function resolveLibPoint(pMm: [Mm, Mm], m: Matrix, atUm: [number, number]): [number, number] {
  const [ix, iy] = libPointToInternalUm(pMm);
  const [tx, ty] = applyMatrix(m, ix, iy);
  return [tx + atUm[0], ty + atUm[1]];
}

/** eeschema's `v(orientation)` (sch_pin.cpp) -- the unit vector a pin's own `angle_deg` (0/90/180/270, file convention: 0=right,90=up,180=left,270=down) points, directly in this app's shared internal (Y-down) space: `v(90)=(0,-1)` is "up" because up is negative-Y once Y-down, not because the angle itself gets re-flipped -- the file's pin-orientation convention and this app's Y-down convention already agree once the one Y-flip above has been applied. */
function pinDirectionInternal(angleDeg: Degrees): [number, number] {
  switch (normalizeRot(angleDeg)) {
    case 0:
      return [1, 0];
    case 90:
      return [0, -1];
    case 180:
      return [-1, 0];
    case 270:
      return [0, 1];
  }
}

export interface ResolvedPin {
  pin: LibPin;
  /** The pin's outer, wire-connection end -- world space, um. */
  tip: [number, number];
  /** The body-attachment end -- world space, um. */
  root: [number, number];
  /** Unit vector from `root` to `tip` (always axis-aligned: every real rotation here is a multiple of 90 degrees) -- "which way the pin sticks out", in world space. */
  dir: [number, number];
}

/** A library pin, fully resolved to world space: `tip` (`LibPin.at`) and `root` (`tip` moved `length_mm` into the body, along `angle_deg`) both transformed and translated the same way `resolveLibPoint` resolves any other library point -- see `resolveLibPoint`'s doc comment for why that one shared pipeline matters. */
export function resolvePin(pin: LibPin, m: Matrix, atUm: [number, number]): ResolvedPin {
  const [tipIx, tipIy] = libPointToInternalUm(pin.at);
  const [dx, dy] = pinDirectionInternal(pin.angle_deg);
  const lengthUm = pin.length_mm * 1000;
  const rootIx = tipIx + dx * lengthUm;
  const rootIy = tipIy + dy * lengthUm;
  const [tx, ty] = applyMatrix(m, tipIx, tipIy);
  const [rx, ry] = applyMatrix(m, rootIx, rootIy);
  const tip: [number, number] = [tx + atUm[0], ty + atUm[1]];
  const root: [number, number] = [rx + atUm[0], ry + atUm[1]];
  // Normalize losslessly: the matrix only ever has entries in {-1,0,1} and
  // (dx,dy) is axis-aligned, so (tip-root) is already an exact axis-
  // aligned vector of length `lengthUm` -- dividing by lengthUm (when
  // nonzero) gives dir directly, with no floating-point drift to guard
  // against. A zero-length pin (hidden power pins) has no defined
  // direction; callers of a zero-length pin's `dir` should expect
  // (0,0) and treat tip===root.
  const dir: [number, number] = lengthUm === 0 ? [0, 0] : [(tip[0] - root[0]) / lengthUm, (tip[1] - root[1]) / lengthUm];
  return { pin, tip, root, dir };
}
