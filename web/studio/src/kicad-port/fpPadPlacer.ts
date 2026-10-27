// The pad the Footprint Editor's Pad tool carries before it is placed. Ported from
//   pcbnew/tools/pad_tool.cpp       PAD_TOOL::PlacePad: `doInteractiveItemPlacement( aEvent, &placer, _( "Place pad" ), IPO_REPEAT | IPO_SINGLE_CLICK | IPO_ROTATE | IPO_FLIP )`
//   pcbnew/tools/pcb_tool_base.cpp  PCB_TOOL_BASE::doInteractiveItemPlacement: with IPO_SINGLE_CLICK a new item is made when the tool starts and after each placement, it
//                                   follows the cursor, a rotate action turns it (`newItem->Rotate( newItem->GetPosition(), rotationAngle )`), Flip flips it
//                                   (`newItem->Flip( newItem->GetPosition(), m_FlipDirection )`), and a click places it (`PAD_PLACER::PlaceItem`, which also does
//                                   `m_Pad_Master->ImportSettingsFrom( *pad )`, so the next pad is turned or flipped the same way); Escape leaves the tool.
//   pcbnew/pad.cpp                  PAD::Rotate, PAD::Flip, PAD::FlipPrimitives; common/eda_shape.cpp EDA_SHAPE::flip.
// Differs: a padstack pad with other copper shapes (`stack.back` / `inner` / `layers`) is not flipped here (PADSTACK::FlipLayers is not ported); inner layers keep their
// names (`BOARD::FlipLayer` maps them by the board's copper count, which a footprint does not have).

import type { ChamferCorners, CmdShape, LibraryPad, PointXY } from "../api/types";
import type { FlipDirection } from "./pcbTransform";

const norm = (mdeg: number) => ((Math.round(mdeg) % 360_000) + 360_000) % 360_000;

/** `PAD::Rotate( GetPosition(), angle )`: the orientation turns by `deg` (counter-clockwise positive, KiCad's), the position stays. */
export function rotateCarriedPad(pad: LibraryPad, deg: number): LibraryPad {
  return { ...pad, rot: norm(pad.rot + deg * 1000) };
}

/** `BOARD::FlipLayer` for the layers a pad can be on: front and back swap, everything else stays. */
export function flipLayerName(layer: string): string {
  if (layer.startsWith("F.")) return `B.${layer.slice(2)}`;
  if (layer.startsWith("B.")) return `F.${layer.slice(2)}`;
  return layer;
}

/** `MIRROR( v, {0,0}, dir )`. */
const mirror = (p: PointXY, dir: FlipDirection): PointXY => (dir === "left_right" ? { x: neg(p.x), y: p.y } : { x: p.x, y: neg(p.y) });
/** No negative zero (it would compare unequal to the pad's own 0 in a deep comparison). */
const neg = (v: number) => (v === 0 ? 0 : -v);

/** `EDA_SHAPE::flip` about the pad's origin, then `SetLayer( FlipLayer( GetLayer() ) )` (PCB_SHAPE::Flip). */
function flipShape(s: CmdShape, dir: FlipDirection): CmdShape {
  const m = (p: PointXY) => mirror(p, dir);
  const layer = flipLayerName(s.layer);
  switch (s.kind) {
    case "segment":
    case "rect":
      return { ...s, layer, start: m(s.start), end: m(s.end) };
    case "circle":
      return { ...s, layer, center: m(s.center), end: m(s.end) };
    case "arc":
      // `std::swap( m_start, m_end )`: a mirrored arc keeps its direction of travel
      return { ...s, layer, start: m(s.end), mid: m(s.mid), end: m(s.start) };
    case "polygon":
      return { ...s, layer, pts: s.pts.map(m) };
    case "bezier":
      return { ...s, layer, start: m(s.start), c1: m(s.c1), c2: m(s.c2), end: m(s.end) };
  }
}

/** `mirrorBitFlags`: the chamfered corners trade places across the flip axis. */
function flipChamfers(c: ChamferCorners, dir: FlipDirection): ChamferCorners {
  return dir === "left_right"
    ? { top_left: c.top_right, top_right: c.top_left, bottom_left: c.bottom_right, bottom_right: c.bottom_left }
    : { top_left: c.bottom_left, top_right: c.bottom_right, bottom_left: c.top_left, bottom_right: c.top_right };
}

/**
 * `PAD::Flip( GetPosition(), dir )`: the offset and the trapezoid delta mirrored, the orientation negated, the chamfered corners swapped, the layers to the other
 * side and a custom pad's basic shapes flipped. `null` for a padstack pad with shapes of its own on other layers (not ported).
 */
export function flipCarriedPad(pad: LibraryPad, dir: FlipDirection): LibraryPad | null {
  const st = pad.stack;
  if (st && (st.back || st.inner || (st.layers && Object.keys(st.layers).length > 0))) return null;
  const off = mirror(pad.offset, dir);
  const delta = pad.trapezoid_delta ? mirror({ x: pad.trapezoid_delta[0], y: pad.trapezoid_delta[1] }, dir) : null;
  return {
    ...pad,
    offset: off,
    trapezoid_delta: delta ? [delta.x, delta.y] : null,
    rot: norm(neg(pad.rot)),
    chamfer_corners: flipChamfers(pad.chamfer_corners, dir),
    layers: pad.layers.map(flipLayerName),
    custom: pad.custom ? { ...pad.custom, primitives: pad.custom.primitives.map((p) => ({ ...p, shape: flipShape(p.shape, dir) })) } : pad.custom,
  };
}
