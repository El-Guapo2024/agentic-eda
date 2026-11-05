// A label's spin (`SCH_LABEL_BASE::GetSpinStyle`: which side of its anchor the text runs to) when none was set, read off the wire that ends at the label --
// see components/schematic/labelShape.ts's header for why. Pure (no React, no DOM): the Properties panel and the painter share it.
import type { SchematicWire } from "../api/types";

export type LabelSpin = "left" | "up" | "right" | "bottom";

/** Finds the one wire whose first or last point lands on `at` (within a small tolerance) and reports which way it approaches -- used to infer a label's spin, since this app's label data has no explicit orientation of its own. */
function approachDirection(wires: SchematicWire[], at: [number, number]): [number, number] | null {
  const tol = 50_000; // nm -- generous enough for float/rounding slop in generated wire endpoints, tight enough to not match an unrelated nearby point
  for (const w of wires) {
    const pts = w.pts;
    if (pts.length < 2) continue;
    const first = pts[0]!;
    const last = pts[pts.length - 1]!;
    const near = (p: [number, number]) => Math.abs(p[0] - at[0]) <= tol && Math.abs(p[1] - at[1]) <= tol;
    if (near(last)) {
      const prev = pts[pts.length - 2]!;
      return [last[0] - prev[0], last[1] - prev[1]];
    }
    if (near(first)) {
      const next = pts[1]!;
      return [first[0] - next[0], first[1] - next[1]];
    }
  }
  return null;
}

/** See this module's header comment: recovers KiCad's SPIN_STYLE from the wire approaching the label's anchor, defaulting to "right" when none matches. */
export function inferSpin(wires: SchematicWire[], at: [number, number]): LabelSpin {
  const d = approachDirection(wires, at);
  if (!d) return "right";
  const [dx, dy] = d;
  if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0 ? "right" : "left";
  return dy >= 0 ? "bottom" : "up";
}

