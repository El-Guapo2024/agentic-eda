// What is under a point in the two library editors: the footprint editor's pads, text and graphics, the symbol editor's pins and
// graphics. The editors' canvases pick with the same rules for their own clicks (FootprintCanvas.tsx / SymbolEditorCanvas.tsx
// `hitTest`); the shared tools (the interactive delete tool, the picker, the lasso's "is this empty space") need them without a
// canvas, so they are here as pure functions.
//
// Units: the footprint editor's own um; the symbol editor's internal um (+y down) -- a library symbol is in mm, +y up, and is
// turned with `resolveLibPoint` / `resolvePin` exactly as the painter does.
import type { CmdShape, LibraryPad, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, LibraryFootprint } from "../api/types";
import { distToSegment, shapeHitDistance } from "../components/canvas/itemHitTest";
import { libPointToInternalUm, resolvePin, symbolTransformMatrix } from "../components/schematic/transform";
import { cmdShapeToShape } from "./itemBoxes";

/** The two editors' tolerance: `Math.max( 80, 6 px )` in um. */
export function pickTolerance(scale: number): number {
  return Math.max(80, scale > 0 ? 6 / scale : 80);
}

/** A point in a pad's own frame (`at`, then its rotation). */
function padLocal(pad: LibraryPad, x: number, y: number): { x: number; y: number } {
  const dx = x - pad.at.x;
  const dy = y - pad.at.y;
  const rad = -(pad.rot / 1000) * (Math.PI / 180);
  const cos = Math.cos(rad);
  const sin = Math.sin(rad);
  return { x: dx * cos - dy * sin, y: dx * sin + dy * cos };
}

/** The footprint editor's pad hit test: an ellipse for a circle or an oval, the (rotated) rectangle for every other shape. */
export function padContains(pad: LibraryPad, x: number, y: number): boolean {
  const local = padLocal(pad, x, y);
  const px = local.x - pad.offset.x;
  const py = local.y - pad.offset.y;
  const [w, h] = pad.size;
  const hw = w / 2;
  const hh = h / 2;
  if (pad.shape === "circle" || pad.shape === "oval") return (px / hw) ** 2 + (py / hh) ** 2 <= 1;
  return Math.abs(px) <= hw && Math.abs(py) <= hh;
}

/** Distance from a point to a footprint graphic's outline (0 on a filled one it is inside). */
export function footprintGraphicDistance(g: CmdShape, x: number, y: number): number {
  return shapeHitDistance(cmdShapeToShape(g), x, y);
}

/**
 * The topmost pad, text or graphic under `(x, y)` -- the footprint canvas's own order: pads (copper reads above the art), then
 * text (within its own half height), then graphics (within the tolerance).
 */
export function pickFootprintItem(fp: LibraryFootprint, x: number, y: number, scale: number): { kind: "pad" | "graphic" | "text"; id: string } | null {
  for (let i = fp.pads.length - 1; i >= 0; i--) {
    const p = fp.pads[i]!;
    if (p.id && padContains(p, x, y)) return { kind: "pad", id: p.id };
  }
  const tol = pickTolerance(scale);
  for (let i = fp.texts.length - 1; i >= 0; i--) {
    const t = fp.texts[i]!;
    if (t.id && Math.hypot(x - t.at.x, y - t.at.y) <= Math.max(tol, t.size_um / 2)) return { kind: "text", id: t.id };
  }
  for (let i = fp.graphics.length - 1; i >= 0; i--) {
    const g = fp.graphics[i]!;
    if (g.id && footprintGraphicDistance(g, x, y) <= tol) return { kind: "graphic", id: g.id };
  }
  return null;
}

const um = (p: { x: number; y: number }): [number, number] => libPointToInternalUm([p.x, p.y]);

/** Distance from a point to a library symbol graphic's outline, in internal um. */
export function symbolGraphicDistance(g: LibrarySymbolGraphic, x: number, y: number): number {
  if (g.kind === "circle") {
    const [cx, cy] = um(g.center);
    return Math.abs(Math.hypot(x - cx, y - cy) - g.radius_mm * 1000);
  }
  if (g.kind === "text") {
    const [tx, ty] = um(g.at);
    return Math.hypot(x - tx, y - ty);
  }
  let pts: [number, number][];
  if (g.kind === "rectangle") {
    const [x0, y0] = um(g.start);
    const [x1, y1] = um(g.end);
    pts = [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]];
  } else if (g.kind === "arc") pts = [um(g.start), um(g.mid), um(g.end)];
  else pts = g.pts.map(um);
  let best = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) best = Math.min(best, distToSegment(x, y, pts[i]![0], pts[i]![1], pts[i + 1]![0], pts[i + 1]![1]));
  if (g.kind === "polyline" && pts.length > 2) best = Math.min(best, distToSegment(x, y, pts[pts.length - 1]![0], pts[pts.length - 1]![1], pts[0]![0], pts[0]![1]));
  return best;
}

/** Distance from a point to a pin's body-to-tip line, resolved the way the painter draws it. */
export function symbolPinDistance(pin: LibrarySymbolPin, x: number, y: number): number {
  const rp = resolvePin({ number: pin.number, name: pin.name, electrical_type: pin.electrical_type, shape: pin.shape, at: [pin.at.x, pin.at.y], angle_deg: pin.angle_deg, length_mm: pin.length_mm, unit: pin.unit, body_style: pin.body_style, hidden: pin.hidden }, symbolTransformMatrix(0, null), [0, 0]);
  return distToSegment(x, y, rp.root[0], rp.root[1], rp.tip[0], rp.tip[1]);
}

/** The pin or graphic under `(x, y)` on the open unit and body style; a hidden pin that is not drawn cannot be picked. */
export function pickSymbolItem(sym: LibrarySymbol, x: number, y: number, scale: number, unit: number, bodyStyle: number, showHiddenPins: boolean): { kind: "pin" | "graphic"; id: string } | null {
  const visible = (u: number, b: number) => (u === 0 || u === unit) && (b === 0 || b === bodyStyle);
  const tol = pickTolerance(scale);
  for (let i = sym.pins.length - 1; i >= 0; i--) {
    const p = sym.pins[i]!;
    if (p.id && (!p.hidden || showHiddenPins) && visible(p.unit, p.body_style) && symbolPinDistance(p, x, y) <= tol) return { kind: "pin", id: p.id };
  }
  for (let i = sym.graphics.length - 1; i >= 0; i--) {
    const g = sym.graphics[i]!;
    if (g.id && visible(g.unit, g.body_style) && symbolGraphicDistance(g, x, y) <= tol) return { kind: "graphic", id: g.id };
  }
  return null;
}
