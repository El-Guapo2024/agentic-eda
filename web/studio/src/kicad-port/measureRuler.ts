// The measure tool's ruler (`common.Interactive.measureTool`, `ACTIONS::measureTool`, Ctrl+Shift+M): two clicks and the straight-line distance between them
// with its dx and dy, read where the ruler lies. `PCB_VIEWER_TOOLS::MeasureTool` (pcbnew/tools/pcb_viewer_tools.cpp) draws a `KIGFX::PREVIEW::RULER_ITEM` from the
// first click to the cursor and fixes it at the second; a click after that starts a new ruler. The board draws it in painter.ts; the Footprint Editor's is
// drawn by components/CommonOverlay.tsx. Pure here: the click flow and the label.
import { formatLength, type LengthUnit } from "../state/units";

export type RulerPoint = readonly [number, number];

/** A click of the tool: the first click starts a ruler, the second fixes its end, and a click on a finished one starts the next (`pts` holds one or two points). */
export function measureClick(pts: readonly RulerPoint[], at: RulerPoint): RulerPoint[] {
  return pts.length === 1 ? [pts[0]!, at] : [at];
}

/** The text on the ruler: "<distance>  (dx <x>, dy <y>)", in the status line's unit. */
export function measureLabel(a: RulerPoint, b: RulerPoint, units: LengthUnit): string {
  const dx = Math.abs(b[0] - a[0]);
  const dy = Math.abs(b[1] - a[1]);
  return `${formatLength(Math.hypot(dx, dy), units)}  (dx ${formatLength(dx, units)}, dy ${formatLength(dy, units)})`;
}
