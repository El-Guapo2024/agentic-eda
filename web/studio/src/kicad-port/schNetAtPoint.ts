// The net under a point, for SCH_EDITOR_CONTROL::HighlightNet ("`" -- see
// eeschema/tools/sch_editor_control.cpp::highlightNet): source collects the
// connectable item under the cursor and highlights its net; nothing there
// clears the highlight (returns null). Nearest-wire first (within
// `tolNm`), then a label or power-symbol anchor. Pure.

export interface NetWire {
  net: string;
  pts: ReadonlyArray<readonly [number, number]>;
}
export interface NetAnchor {
  net: string;
  at: readonly [number, number];
}

/** A pin on the sheet, for the net highlight: the net it is on and how far the point is from its box (0 inside it: `kicad-port/schPins.ts` `pinDistance`). */
export interface NetPin {
  net: string | null;
  distNm: number;
}

/** `Schematic()->Settings().m_PinSymbolSize / 4`: the least accuracy a pin hit test is given (the same floor as `kicad-port/schPins.ts`). */
const PIN_FLOOR_NM = 158_750;

/**
 * `SCH_SELECTION_TOOL::GetNode`: the connectable items at the point -- pins, wires, labels, power symbols -- tried at the accuracies 0, a quarter, a half and the whole of `tolNm` in
 * turn, the first accuracy that finds one deciding; the nearest of those is the item. Its net is the highlighted one (`highlightNet`), none clears it.
 */
export function netAtPoint(wires: readonly NetWire[], anchors: readonly NetAnchor[], x: number, y: number, tolNm: number, pins: readonly NetPin[] = []): string | null {
  for (const t of [0, tolNm / 4, tolNm / 2, tolNm]) {
    let best: { net: string | null; d: number } | null = null;
    const consider = (net: string | null, d: number) => {
      if (!best || d < best.d) best = { net, d };
    };
    for (const a of anchors) {
      const d = Math.hypot(a.at[0] - x, a.at[1] - y);
      if (d <= t) consider(a.net, d);
    }
    for (const w of wires) {
      for (let i = 0; i + 1 < w.pts.length; i++) {
        const [x1, y1] = w.pts[i]!;
        const [x2, y2] = w.pts[i + 1]!;
        const dx = x2 - x1;
        const dy = y2 - y1;
        const lenSq = dx * dx + dy * dy || 1;
        const k = Math.max(0, Math.min(1, ((x - x1) * dx + (y - y1) * dy) / lenSq));
        const d = Math.hypot(x - (x1 + k * dx), y - (y1 + k * dy));
        if (d <= t) consider(w.net, d);
      }
    }
    // `SCH_PIN::HitTest`: the pin's box, at least the pin symbol size over four around it
    for (const p of pins) if (p.distNm <= Math.max(t, PIN_FLOOR_NM)) consider(p.net, p.distNm);
    if (best) return (best as { net: string | null }).net;
  }
  return null;
}
