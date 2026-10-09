// What the last snap did, for the scripted test hook (`window.__eda.state().snap`, actions/useEdaTestHook.ts): where the grid helper of the tool in force put a cursor and what
// kind of point it was. Nothing reads it but the hook; the helpers (components/canvas/pcbSnap.ts, components/schematic/schSnap.ts) write it after each call.
export interface SnapReport {
  /** The tool the helper belongs to ("draw_segment", "move", "route", ...). */
  tool: string;
  /** The cursor handed in, and the point the helper returned, um. */
  input: readonly [number, number];
  output: readonly [number, number];
  /** The `POINT_TYPE` bits of the marker shown (0: the cursor went to the grid, a snap line or nowhere). */
  types: number;
  /** True when an anchor, an intersection or a point on an element was snapped to (a marker is on show). */
  anchored: boolean;
}

let last: SnapReport | null = null;

export function reportSnap(report: SnapReport | null): void {
  last = report;
}

export function lastSnapReport(): SnapReport | null {
  return last;
}
