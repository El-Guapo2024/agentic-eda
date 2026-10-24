// The small icon beside the name of a pin that has alternate functions (View > Show Pin Alternate Icons, `eeschema.EditorControl.togglePinAltIcons`).
// Ports of `PIN_LAYOUT_CACHE::getUntransformedAltIconBox` + `transformBoxForPin` (eeschema/pin_layout_cache.cpp) and `drawAltPinModesIcon`
// (eeschema/sch_painter.cpp, called from `SCH_PAINTER::draw( SCH_PIN )` with the base style selected and the pin name's colour). All lengths are
// nm in the canvas' internal space (+y down). Pure: no DOM (compiled by `npm run test:unit`).
import { pinTextOffsetNm } from "./symPinText";

/** `schIUScale.mmToIU( 1.5 )`: the icon is never larger than this. */
export const MAX_ALT_ICON_SIZE_NM = 1_500_000;

export interface PinAltIcon {
  /** The icon's centre, in the canvas' space. */
  center: [number, number];
  /** `aSize`: the width of the icon, which is as high as it is wide. */
  sizeNm: number;
  /** `nameInfo->m_Angle == ANGLE_VERTICAL`: a pin that points up or down turns its icon a quarter counter-clockwise (`aRotate`). */
  vertical: boolean;
}

/** `GetPenSizeForNormal`: an eighth of the text size, rounded. */
function penFor(sizeNm: number): number {
  return Math.round(sizeNm / 8);
}

/**
 * Where the icon of a pin goes. `tip` is the pin's connection point and `root` the end that meets the body; `name` is the pin name as drawn (the advance of
 * its text and the size it is set in); `nameOffsetNm` is the symbol's pin name offset: above zero the names are inside the body and the icon goes after the
 * name, otherwise the name is over the pin line and the icon goes before it.
 *
 * `getUntransformedAltIconBox` lays the pin out running right from the origin: the name's box is `StringBoundaryLimits` (the text and the pen's reach on every
 * side) -- from `length + offset` on, centred on the line, for a name inside; centred along the pin and lifted by its height and the text offset for a name over
 * the pin -- and the icon, `min( name size, 1.5 mm )` square, sits three quarters of its size beyond the box's end (inside) or before its start (over).
 * `transformBoxForPin` turns that to the pin's direction: a pin running left mirrors it, one running up or down puts it at the left of the line.
 */
export function pinAltIcon(tip: [number, number], root: [number, number], name: { widthNm: number; sizeNm: number }, nameOffsetNm: number): PinAltIcon {
  const length = Math.hypot(root[0] - tip[0], root[1] - tip[1]);
  // the way the pin runs, from the connection point to the body (a pin of no length runs right)
  const body: [number, number] = length === 0 ? [1, 0] : [(root[0] - tip[0]) / length, (root[1] - tip[1]) / length];
  const horizontal = Math.abs(body[0]) >= Math.abs(body[1]);
  const reach = Math.round(1.5 * penFor(name.sizeNm));
  const boxW = name.widthNm + 2 * reach;
  const boxH = name.sizeNm + 2 * reach;
  const iconSize = Math.min(name.sizeNm, MAX_ALT_ICON_SIZE_NM);
  let cx: number;
  let cy: number;
  if (nameOffsetNm > 0) {
    // name inside: the box starts `offset` past the pin's inner end and is centred on the line
    cx = length + nameOffsetNm + boxW + iconSize * 0.75;
    cy = 0;
  } else {
    // name over the pin: centred along it, lifted by half its height and the text offset
    cx = length / 2 - boxW / 2 - iconSize * 0.75;
    cy = -boxH / 2 - pinTextOffsetNm();
  }
  // from the pin's own frame (x along the pin toward the body, y across it with "over" negative) to the canvas: the way the box is rotated for each direction
  const across: [number, number] = horizontal ? [0, 1] : [1, 0];
  return {
    center: [tip[0] + body[0] * cx + across[0] * cy, tip[1] + body[1] * cx + across[1] * cy],
    sizeNm: iconSize,
    vertical: !horizontal,
  };
}

export interface AltIconStrokes {
  /** Segments `[x1, y1, x2, y2]` in the icon's own frame (origin at its centre, +x right, +y down). */
  lines: [number, number, number, number][];
  /** The one arc: its centre, radius, the angle it starts at and the angle it ends at (radians, counter-clockwise on the screen from start to end). */
  arc: { cx: number; cy: number; r: number; from: number; to: number };
  /** `aSize / 10`: the pen. */
  widthNm: number;
}

/**
 * `drawAltPinModesIcon` with `aBaseSelected`: two arrows pointing right, a full one above and a short one below, and the quarter arc that bends
 * from the middle down to the lower one -- the glyph for "this pin can take another role".
 */
export function altIconStrokes(sizeNm: number): AltIconStrokes {
  const s = sizeNm;
  const lineYOffset = s / 4;
  const arrowHead = s / 8;
  const top: [number, number] = [s / 2, -lineYOffset];
  const btm: [number, number] = [s / 2, lineYOffset];
  const lines: [number, number, number, number][] = [
    // top line and its arrowhead
    [top[0], top[1], top[0] - s, top[1]],
    [top[0], top[1], top[0] - arrowHead * 1.2, top[1] - arrowHead],
    [top[0], top[1], top[0] - arrowHead * 1.2, top[1] + arrowHead],
    // bottom line and its arrowhead
    [btm[0], btm[1], btm[0] - s / 2, btm[1]],
    [btm[0], btm[1], btm[0] - arrowHead * 1.2, btm[1] - arrowHead],
    [btm[0], btm[1], btm[0] - arrowHead * 1.2, btm[1] + arrowHead],
  ];
  // DrawArc( top - { size - lineYOffset * 2, -lineYOffset }, lineYOffset, ANGLE_180, -ANGLE_90 ): from the left of the middle round to its bottom
  return { lines, arc: { cx: top[0] - (s - lineYOffset * 2), cy: top[1] + lineYOffset, r: lineYOffset, from: Math.PI, to: Math.PI / 2 }, widthNm: s / 10 };
}
