// Auto-rotate on placement: the spin a global or hierarchical label with "Auto" on takes from what it is connected to.
//
// Ported from eeschema at 8303b2ad: `SCH_SCREEN::GetLabelOrientationForPoint` (sch_screen.cpp) and `GetPinSpinStyle` (symb_transforms_utils.cpp), which
// `SCH_EDIT_FRAME::AutoRotateItem` runs when a label is placed (`SCH_DRAWING_TOOLS::TwoClickPlace`), moved or dragged (`SCH_MOVE_TOOL`) and when Label Properties is
// closed with "Auto" checked (`DIALOG_LABEL_PROPERTIES::TransferDataFromWindow`). The backend runs it for real (crates/ops/src/sch_labels.rs); this is the studio's copy,
// used by the label dialogs to show the spin "Auto" will give before OK is pressed.
//
// The C++ walks `Items().Overlapping( aPosition )`, which visits the item types in `KICAD_T` order -- bus wire entries, lines, symbols -- and lets each match overwrite the
// answer, so the last one wins: a pin over a wire end, a wire over a bus entry.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Schematic } from "../api/types";
import { applyMatrix, symbolTransformMatrix } from "../components/schematic/transform";
import { resolveLibSymbol } from "../components/schematic/libSymbol";

/** `SPIN_STYLE`: which way a label's text runs from its anchor. */
export type Spin = "right" | "up" | "left" | "bottom";
type P = readonly [number, number];

/** What a label's anchor can be over, in the shape `GetLabelOrientationForPoint` reads it. */
export interface OrientationScene {
  /** Wires, buses and graphic lines, one entry per segment, in the direction they were drawn (`SCH_LINE_T`). */
  lines: ReadonlyArray<{ a: P; b: P; bus: boolean; half?: number }>;
  /** Bus wire entries (`SCH_BUS_WIRE_ENTRY_T`): where they start and the step to their other end. */
  busEntries: ReadonlyArray<{ at: P; size: P }>;
  /** Each placed symbol (a power symbol is one) with its pins: where the pin's connection point is and the way its line runs from there into the body, on the sheet's axes (y down). */
  symbols: ReadonlyArray<ReadonlyArray<{ at: P; body: P }>>;
}

/** `GetPinSpinStyle`: a label on a pin reads away from the symbol's body. `body` is the pin line's step from its connection point into the body. */
export function pinSpinStyle(body: P): Spin | null {
  const sx = Math.sign(body[0]);
  const sy = Math.sign(body[1]);
  if (sx === 1 && sy === 0) return "left";
  if (sx === -1 && sy === 0) return "right";
  if (sx === 0 && sy === -1) return "bottom";
  if (sx === 0 && sy === 1) return "up";
  return null;
}

const samePoint = (a: P, b: P): boolean => a[0] === b[0] && a[1] === b[1];

/** `IsPointOnSegment`: `p` lies on the segment `a`-`b`, ends included. */
function onSegment(a: P, b: P, p: P): boolean {
  if (samePoint(a, b)) return samePoint(p, a);
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  return cross === 0 && p[0] >= Math.min(a[0], b[0]) && p[0] <= Math.max(a[0], b[0]) && p[1] >= Math.min(a[1], b[1]) && p[1] <= Math.max(a[1], b[1]);
}

/**
 * Half the pen of a line as `SCH_LINE::GetPenWidth` resolves it with nothing set: a wire's 6 mil (the Default net class), a bus's 12 mil, a graphic line's
 * `m_DefaultLineWidth` (6 mil); in micrometres.
 */
export const HALF_PEN_UM = { wire: 76, bus: 152, line: 76 };

/**
 * `Items().Overlapping( aPosition )` for a line: the R-tree holds `SCH_LINE::GetBoundingBox`, the box of its two ends grown by half its pen (and 1 IU on the far
 * side), so a point anywhere in that box counts, on the line or not (a point inside a diagonal line's box, or beside an orthogonal one by less than half its pen).
 */
function inLineBox(a: P, b: P, p: P, half: number): boolean {
  return p[0] >= Math.min(a[0], b[0]) - half && p[0] <= Math.max(a[0], b[0]) + half && p[1] >= Math.min(a[1], b[1]) - half && p[1] <= Math.max(a[1], b[1]) + half;
}

/**
 * `SCH_SCREEN::GetLabelOrientationForPoint( aPosition, aDefaultOrientation )`: the spin a label at `at` should have given what overlaps that point; `current` when nothing does.
 */
export function labelOrientationForPoint(scene: OrientationScene, at: P, current: Spin): Spin {
  let ret = current;

  // SCH_BUS_WIRE_ENTRY_T
  for (const b of scene.busEntries) {
    const end: P = [b.at[0] + b.size[0], b.at[1] + b.size[1]];
    if (at[0] < Math.min(b.at[0], end[0]) || at[0] > Math.max(b.at[0], end[0]) || at[1] < Math.min(b.at[1], end[1]) || at[1] > Math.max(b.at[1], end[1])) continue;
    // `m_connected_bus_item`: the bus that one of the entry's two ends is on
    const bus = scene.lines.find((l) => l.bus && (onSegment(l.a, l.b, b.at) || onSegment(l.a, l.b, end)));
    if (!bus) continue;
    // "bus->Angle().AsDegrees() == 90.0": the angle of the line as drawn, not normalised, so only a bus drawn downwards or to the right counts
    const dx = bus.b[0] - bus.a[0];
    const dy = bus.b[1] - bus.a[1];
    if (dx === 0 && dy > 0) {
      // "bus is vertical -> label shall be horizontal and shall be placed to the side where the bus entry is"
      if (at[0] < bus.a[0]) ret = "left";
      else if (at[0] > bus.a[0]) ret = "right";
    } else if (dy === 0 && dx >= 0) {
      // "bus is horizontal -> label shall be vertical and shall be placed to the side where the bus entry is"
      if (at[1] < bus.a[1]) ret = "up";
      else if (at[1] > bus.a[1]) ret = "bottom";
    }
  }

  // SCH_LINE_T
  for (const l of scene.lines) {
    if (!inLineBox(l.a, l.b, at, l.half ?? (l.bus ? HALF_PEN_UM.bus : HALF_PEN_UM.wire))) continue;
    // "line angles goes between -90 and 90 degrees, but normalize": `Angle().Normalize90()` is in (-90, 90], so -45 < angle <= 45 is -dx < dy <= dx for the direction
    // turned to point right (or down, for a vertical line)
    let dx = l.b[0] - l.a[0];
    let dy = l.b[1] - l.a[1];
    if (dx < 0 || (dx === 0 && dy < 0)) {
      dx = -dx;
      dy = -dy;
    }
    const atEnd = samePoint(l.b, at);
    if ((dx === 0 && dy === 0) || (dx > 0 && -dx < dy && dy <= dx)) {
      ret = l.a[0] <= l.b[0] ? (atEnd ? "right" : "left") : atEnd ? "left" : "right";
    } else {
      ret = l.a[1] <= l.b[1] ? (atEnd ? "bottom" : "up") : atEnd ? "up" : "bottom";
    }
  }

  // SCH_SYMBOL_T: the first pin of each symbol that is on the point
  for (const pins of scene.symbols) {
    const pin = pins.find((p) => samePoint(p.at, at));
    if (!pin) continue;
    const spin = pinSpinStyle(pin.body);
    if (spin) ret = spin;
  }
  return ret;
}

/** The sheet's items as `labelOrientationForPoint` reads them. */
export function orientationSceneOf(sch: Pick<Schematic, "wires" | "lines" | "bus_entries" | "symbols" | "power_symbols" | "lib_symbols">): OrientationScene {
  const lines: Array<{ a: P; b: P; bus: boolean; half?: number }> = [];
  for (const w of sch.wires) for (let i = 0; i + 1 < w.pts.length; i++) lines.push({ a: w.pts[i]!, b: w.pts[i + 1]!, bus: !!w.bus });
  for (const l of sch.lines ?? []) {
    const half = l.width_um > 0 ? Math.floor(l.width_um / 2) : HALF_PEN_UM.line;
    for (let i = 0; i + 1 < l.pts.length; i++) lines.push({ a: l.pts[i]!, b: l.pts[i + 1]!, bus: false, half });
  }
  const symbols: Array<Array<{ at: P; body: P }>> = [];
  const pinsOf = (instance: Parameters<typeof resolveLibSymbol>[0]): Array<{ at: P; body: P }> => {
    const real = resolveLibSymbol(instance, sch.lib_symbols);
    if (!real) return [];
    const m = symbolTransformMatrix(instance.rot, instance.mirror);
    return real.pins.map((r) => {
      // the library angle is the way the pin runs from its connection point to the body (0 right, 90 up, 180 left, 270 down in a y-up frame), turned by the symbol's orientation
      const a = (((r.pin.angle_deg % 360) + 360) % 360) | 0;
      const local: P = a === 0 ? [1, 0] : a === 90 ? [0, -1] : a === 180 ? [-1, 0] : [0, 1];
      return { at: r.tip, body: applyMatrix(m, local[0], local[1]) };
    });
  };
  for (const s of sch.symbols) symbols.push(pinsOf(s));
  for (const ps of sch.power_symbols) {
    symbols.push(pinsOf({ id: ps.id, lib_id: ps.lib_id, at: ps.at, rot: ps.rot, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [ps.pin] }));
  }
  return { lines, busEntries: sch.bus_entries.map((b) => ({ at: b.at, size: b.size })), symbols };
}
