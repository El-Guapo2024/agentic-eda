// Compare Footprint with Library (`pcbnew.InspectionTool.DiffFootprint`, BOARD_INSPECTION_TOOL::DiffFootprint and FOOTPRINT_DIFF_WIDGET, pcbnew/tools/board_inspection_tool.cpp
// and common/widgets/footprint_diff_widget.cpp at 8303b2ad): the lines of the Summary page, and what the Visual page draws -- the board's footprint and the library's copy
// of it on top of one another, a slider fading from one to the other.
//
// Pure functions over `GET /api/footprint_diff`'s answer (api/fpAssign.ts), units micrometres, the footprint's own frame (y down, as the board's).

import type { FootprintDiff, FootprintDrawing } from "../api/fpAssign";
import type { Shape } from "../api/types";
import type { ReportLine } from "./pcbExchange";

/**
 * The Summary page: "Board vs library diff for:", the footprint (`getItemDescription`: "Footprint U1"), the library and the item; then the differences
 * (`FootprintNeedsUpdate`'s reporter), "No relevant differences detected." when there are none, or why there is no library copy to compare with.
 */
export function summaryLines(diff: FootprintDiff): ReportLine[] {
  const out: ReportLine[] = [
    { severity: "action", text: "Board vs library diff for:" },
    {
      severity: "info",
      text: `    Footprint ${diff.reference || "<no reference designator>"}`,
    },
    { severity: "info", text: `    Library: ${diff.library}` },
    { severity: "info", text: `    Library item: ${diff.item}` },
    { severity: "info", text: "" },
  ];
  if (!diff.found) {
    out.push({
      severity: "warning",
      text: diff.reason ?? "The library no longer contains the item.",
    });
    return out;
  }
  const lines = diff.lines ?? [];
  if (lines.length === 0)
    out.push({ severity: "action", text: "No relevant differences detected." });
  for (const line of lines) out.push({ severity: "action", text: line });
  return out;
}

/** `FOOTPRINT_DIFF_WIDGET::onSlider`: how opaque each footprint is at `percent` of the slider (0 is the board's, 100 the library's, both shown at 50). */
export function diffOpacity(percent: number): {
  board: number;
  library: number;
} {
  const pct = Math.min(1, Math.max(0, percent / 100));
  return {
    board: pct < 0.5 ? 1 : 1 - (pct - 0.5) * 2,
    library: pct > 0.5 ? 1 : pct * 2,
  };
}

/** `FOOTPRINT_DIFF_WIDGET::ToggleAB`: from the board's to the library's, from anywhere else to the board's. */
export function toggleSlider(percent: number): number {
  return percent === 0 ? 100 : 0;
}

type Pt = readonly [number, number];

export interface Bounds {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

function rotatedHalfExtents(
  w: number,
  h: number,
  degrees: number,
): [number, number] {
  const a = (degrees * Math.PI) / 180;
  const c = Math.abs(Math.cos(a));
  const s = Math.abs(Math.sin(a));
  return [(w / 2) * c + (h / 2) * s, (w / 2) * s + (h / 2) * c];
}

/** The rectangle that holds the drawings (pads at their rotation, strokes with their width, the courtyard); null when there is nothing to draw. */
export function drawingBounds(
  drawings: readonly FootprintDrawing[],
): Bounds | null {
  let b: Bounds | null = null;
  const add = (x: number, y: number, rx = 0, ry = rx) => {
    b = b
      ? {
          x0: Math.min(b.x0, x - rx),
          y0: Math.min(b.y0, y - ry),
          x1: Math.max(b.x1, x + rx),
          y1: Math.max(b.y1, y + ry),
        }
      : { x0: x - rx, y0: y - ry, x1: x + rx, y1: y + ry };
  };
  for (const d of drawings) {
    for (const p of d.pads) {
      const [hx, hy] = rotatedHalfExtents(p.w, p.h, p.rot);
      add(p.x, p.y, hx, hy);
    }
    for (const s of d.shapes) {
      const half = s.stroke_width / 2;
      for (const [x, y] of shapePoints(s)) add(x, y, half);
      if (s.kind === "circle") {
        const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
        add(s.center[0], s.center[1], r + half);
      }
    }
    for (const outline of d.courtyard) for (const [x, y] of outline) add(x, y);
  }
  return b;
}

function shapePoints(s: Shape): Pt[] {
  switch (s.kind) {
    case "segment":
    case "rect":
      return [s.start, s.end];
    case "arc":
      return [s.start, s.mid, s.end];
    case "circle":
      return [s.center];
    case "polygon":
      return s.pts;
    case "bezier":
      return [s.start, s.c1, s.c2, s.end];
  }
}

const f = (n: number) => String(Math.round(n * 100) / 100);

/** `M start A r r 0 large sweep end`: the arc through three points as SVG draws it (y down, the sweep flag is the direction of increasing angle). */
export function arcPath(start: Pt, mid: Pt, end: Pt): string {
  const [x1, y1] = start;
  const [x2, y2] = mid;
  const [x3, y3] = end;
  const d = 2 * (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2));
  if (Math.abs(d) < 1e-9) return `M ${f(x1)} ${f(y1)} L ${f(x3)} ${f(y3)}`;
  const s1 = x1 * x1 + y1 * y1;
  const s2 = x2 * x2 + y2 * y2;
  const s3 = x3 * x3 + y3 * y3;
  const cx = (s1 * (y2 - y3) + s2 * (y3 - y1) + s3 * (y1 - y2)) / d;
  const cy = (s1 * (x3 - x2) + s2 * (x1 - x3) + s3 * (x2 - x1)) / d;
  const r = Math.hypot(x1 - cx, y1 - cy);
  const increasing = (x2 - x1) * (y3 - y2) - (y2 - y1) * (x3 - x2) > 0;
  const a1 = Math.atan2(y1 - cy, x1 - cx);
  const a3 = Math.atan2(y3 - cy, x3 - cx);
  const full = Math.PI * 2;
  const swept = increasing
    ? (((a3 - a1) % full) + full) % full
    : (((a1 - a3) % full) + full) % full;
  return `M ${f(x1)} ${f(y1)} A ${f(r)} ${f(r)} 0 ${swept > Math.PI ? 1 : 0} ${increasing ? 1 : 0} ${f(x3)} ${f(y3)}`;
}

export interface ShapePath {
  d: string;
  /** A filled shape is painted solid; the others are strokes only. */
  fill: boolean;
  width: number;
  layer: string;
}

/** A graphic as an SVG path. */
export function shapePath(s: Shape): ShapePath {
  const base = { fill: s.filled, width: s.stroke_width, layer: s.layer };
  switch (s.kind) {
    case "segment":
      return {
        ...base,
        fill: false,
        d: `M ${f(s.start[0])} ${f(s.start[1])} L ${f(s.end[0])} ${f(s.end[1])}`,
      };
    case "arc":
      return { ...base, fill: false, d: arcPath(s.start, s.mid, s.end) };
    case "rect": {
      const [x0, y0] = s.start;
      const [x1, y1] = s.end;
      return {
        ...base,
        d: `M ${f(x0)} ${f(y0)} L ${f(x1)} ${f(y0)} L ${f(x1)} ${f(y1)} L ${f(x0)} ${f(y1)} Z`,
      };
    }
    case "circle": {
      const [cx, cy] = s.center;
      const r = Math.hypot(s.end[0] - cx, s.end[1] - cy);
      return {
        ...base,
        d: `M ${f(cx + r)} ${f(cy)} A ${f(r)} ${f(r)} 0 1 0 ${f(cx - r)} ${f(cy)} A ${f(r)} ${f(r)} 0 1 0 ${f(cx + r)} ${f(cy)} Z`,
      };
    }
    case "polygon":
      return {
        ...base,
        d:
          s.pts
            .map(([x, y], i) => `${i === 0 ? "M" : "L"} ${f(x)} ${f(y)}`)
            .join(" ") + " Z",
      };
    case "bezier":
      return {
        ...base,
        d: `M ${f(s.start[0])} ${f(s.start[1])} C ${f(s.c1[0])} ${f(s.c1[1])} ${f(s.c2[0])} ${f(s.c2[1])} ${f(s.end[0])} ${f(s.end[1])}`,
      };
  }
}

export interface PadFigure {
  num: string;
  /** The centre, the size before the rotation and the rotation about the centre (degrees, as SVG's `rotate`). */
  x: number;
  y: number;
  w: number;
  h: number;
  rot: number;
  /** A circle or an oval is drawn as an ellipse or a pill; the other shapes as a rectangle with this corner radius. */
  shape: "rect" | "ellipse";
  radius: number;
  hole: { w: number; h: number } | null;
}

/** A pad as one figure: a rectangle (a rounded one for a round rectangle, a pill for an oval) or an ellipse, with its drill. Trapezoids and custom pads are drawn as their bounding rectangle. */
export function padFigure(p: FootprintDrawing["pads"][number]): PadFigure {
  const small = Math.min(p.w, p.h);
  let shape: PadFigure["shape"] = "rect";
  let radius = 0;
  if (p.shape === "circle") shape = "ellipse";
  else if (p.shape === "oval") radius = small / 2;
  else if (p.shape === "round_rect") radius = (p.ratio ?? 0.25) * small;
  else if (p.shape === "chamfered_rect") radius = 0;
  const hole = p.slot
    ? { w: p.slot[0], h: p.slot[1] }
    : p.drill
      ? { w: p.drill, h: p.drill }
      : null;
  return {
    num: p.num,
    x: p.x,
    y: p.y,
    w: p.w,
    h: p.h,
    rot: p.rot,
    shape,
    radius,
    hole,
  };
}

/** The order a layer is painted in: documentation under the copper, the silkscreen over it. */
export function layerRank(layer: string): number {
  if (layer.endsWith(".Fab")) return 0;
  if (layer.endsWith(".CrtYd")) return 1;
  if (layer.endsWith(".Cu")) return 2;
  if (layer.endsWith(".Mask") || layer.endsWith(".Paste")) return 3;
  if (layer.endsWith(".SilkS")) return 4;
  return 5;
}
