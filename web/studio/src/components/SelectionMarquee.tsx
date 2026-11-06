// The selection rectangle being dragged on a library editor's canvas (`SELECTION_AREA`, common/preview_items/selection_area.cpp), drawn over the canvas in screen
// pixels: a window (left to right, items wholly inside) solid, a crossing box (right to left, items it touches) dashed -- the board canvas's colours
// (canvas/Canvas.tsx) so the editors look alike.
export interface Marquee {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  crossing: boolean;
}

export function SelectionMarquee({ m }: { m: Marquee | null }) {
  if (!m) return null;
  return (
    <div
      data-testid="selection-marquee"
      style={{
        position: "absolute",
        left: Math.min(m.x0, m.x1),
        top: Math.min(m.y0, m.y1),
        width: Math.abs(m.x1 - m.x0),
        height: Math.abs(m.y1 - m.y0),
        border: `1px ${m.crossing ? "dashed #4aa3ff" : "solid #ffd84a"}`,
        background: m.crossing ? "rgba(74,163,255,0.12)" : "rgba(255,216,74,0.12)",
        pointerEvents: "none",
      }}
    />
  );
}
