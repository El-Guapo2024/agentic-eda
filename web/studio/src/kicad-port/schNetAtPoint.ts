// The net under a point, for SCH_EDITOR_CONTROL::HighlightNet ("`" -- see
// eeschema/tools/sch_editor_control.cpp::highlightNet): source collects the
// connectable item under the cursor and highlights its net; nothing there
// clears the highlight (returns null). Nearest-wire first (within
// `tolUm`), then a label or power-symbol anchor. Pure.

export interface NetWire {
  net: string;
  pts: ReadonlyArray<readonly [number, number]>;
}
export interface NetAnchor {
  net: string;
  at: readonly [number, number];
}

export function netAtPoint(wires: readonly NetWire[], anchors: readonly NetAnchor[], x: number, y: number, tolUm: number): string | null {
  let best: { net: string; d: number } | null = null;
  for (const a of anchors) {
    const d = Math.hypot(a.at[0] - x, a.at[1] - y);
    if (d <= tolUm && (!best || d < best.d)) best = { net: a.net, d };
  }
  for (const w of wires) {
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const [x1, y1] = w.pts[i]!;
      const [x2, y2] = w.pts[i + 1]!;
      const dx = x2 - x1;
      const dy = y2 - y1;
      const lenSq = dx * dx + dy * dy || 1;
      const t = Math.max(0, Math.min(1, ((x - x1) * dx + (y - y1) * dy) / lenSq));
      const d = Math.hypot(x - (x1 + t * dx), y - (y1 + t * dy));
      if (d <= tolUm && (!best || d < best.d)) best = { net: w.net, d };
    }
  }
  return best?.net ?? null;
}
