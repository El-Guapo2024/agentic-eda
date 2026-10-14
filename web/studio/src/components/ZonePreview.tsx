// The Zone Manager's preview: port of pcbnew/zone_manager/zone_preview_notebook.cpp and zone_preview_canvas.cpp. A notebook with one tab per layer of the selected zone
// (`aZone->GetLayerSet().UIOrder()`), each a canvas that shows nothing but that layer and the board edges: the translucent box round the board's edges
// (`BOARD_EDGES_BOUNDING_ITEM`), the Edge.Cuts outline, and the zone as it is on that layer -- its fill (KiCad's, as the last fill made it) or, for a zone with none, its outline.
// Zoomed to fit the board; the wheel zooms and a drag pans, "Zoom to Fit" (the canvas's right-click menu there) brings it back. The zoom is kept when another zone or another
// tab is chosen, as `ZONE_PREVIEW_NOTEBOOK::OnZoneSelectionChanged` keeps it.
import { useMemo, useRef, useState } from "react";
import { useNonPassiveWheel } from "../hooks/useNonPassiveWheel";
import type { FillZone, Zone } from "../api/types";
import { layerColor } from "./canvas/layers";
import { zoneLayers } from "../kicad-port/zoneLayers";

type Pt = [number, number];

interface View {
  /** World µm at the centre of the canvas, and µm per pixel. */
  cx: number;
  cy: number;
  scale: number;
}

const SIZE = { w: 300, h: 260 };

function bounds(rings: readonly (readonly Pt[])[]): [number, number, number, number] | null {
  let box: [number, number, number, number] | null = null;
  for (const ring of rings) {
    for (const [x, y] of ring) {
      box = box ? [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)] : [x, y, x, y];
    }
  }
  return box;
}

function path(rings: readonly (readonly Pt[])[]): string {
  return rings.map((ring) => (ring.length < 3 ? "" : `M${ring.map(([x, y]) => `${x} ${y}`).join("L")}Z`)).join("");
}

/** The view that fits `box` (the board's edges, else the zone) in the canvas, with a margin. */
export function fitView(box: [number, number, number, number] | null): View {
  if (!box) return { cx: 0, cy: 0, scale: 100 };
  const [x0, y0, x1, y1] = box;
  const scale = Math.max((x1 - x0) / (SIZE.w * 0.92), (y1 - y0) / (SIZE.h * 0.92), 1);
  return { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, scale };
}

/** The colour KiCad's theme gives a layer, by its real key (`F.Cu` is `F_Cu`). */
export function colorOfLayer(layer: string): string {
  return layerColor(layer.replace(/\./g, "_"));
}

export function ZonePreview({ zone, fills, outlinePolys, outline }: { zone: Zone | null; fills: readonly FillZone[]; outlinePolys?: { outer: Pt[]; holes: Pt[][] }[]; outline: Pt[] | null }) {
  const layers = useMemo(() => (zone ? zoneLayers(zone) : []), [zone]);
  const [layer, setLayer] = useState<string | null>(null);
  const shownLayer = zone && layers.includes(layer ?? "") ? layer! : (layers[0] ?? null);
  const edges: Pt[][] = useMemo(() => (outlinePolys && outlinePolys.length > 0 ? outlinePolys.flatMap((p) => [p.outer, ...p.holes]) : outline && outline.length >= 3 ? [outline] : []), [outlinePolys, outline]);
  const boardBox = useMemo(() => bounds(edges) ?? (zone ? bounds([zone.outline as Pt[]]) : null), [edges, zone]);
  // The zoom a person chose is kept across zones and tabs (`m_hasSavedZoom`); until they chose one the canvas fits the board.
  const [chosen, setChosen] = useState<View | null>(null);
  const view = chosen ?? fitView(boardBox);
  const drag = useRef<{ x: number; y: number; cx: number; cy: number } | null>(null);

  const fill = zone && shownLayer ? fills.find((f) => f.id === zone.id && f.layer === shownLayer) : undefined;
  const color = shownLayer ? colorOfLayer(shownLayer) : "#888";
  const box = useRef<HTMLDivElement>(null);
  const world = (e: { clientX: number; clientY: number }): Pt => {
    const r = box.current?.getBoundingClientRect() ?? { left: 0, top: 0 };
    return [view.cx + (e.clientX - r.left - SIZE.w / 2) * view.scale, view.cy + (e.clientY - r.top - SIZE.h / 2) * view.scale];
  };
  // The wheel zooms about the cursor (`preventDefault()` keeps the page from scrolling under it, which a React `onWheel` cannot do).
  useNonPassiveWheel(box, (e) => {
    e.preventDefault();
    const [wx, wy] = world(e);
    const scale = Math.min(Math.max(view.scale * (e.deltaY < 0 ? 0.8 : 1.25), 1), 1_000_000);
    const r = scale / view.scale;
    setChosen({ cx: wx - (wx - view.cx) * r, cy: wy - (wy - view.cy) * r, scale });
  });

  return (
    <div data-testid="zone-preview" style={{ width: SIZE.w }}>
      <div role="tablist" style={{ display: "flex", flexWrap: "wrap", gap: 2, marginBottom: 4, minHeight: 24 }}>
        {layers.map((l) => (
          <button key={l} role="tab" aria-selected={l === shownLayer} data-testid={`zone-preview-tab-${l}`} onClick={() => setLayer(l)} style={{ fontSize: 11, padding: "1px 6px", borderBottom: l === shownLayer ? `2px solid ${colorOfLayer(l)}` : "2px solid transparent" }}>
            {l}
          </button>
        ))}
        {layers.length === 0 && <span style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>Select a zone to see it.</span>}
      </div>
      <div ref={box} style={{ width: SIZE.w, height: SIZE.h }}>
      <svg
        width={SIZE.w}
        height={SIZE.h}
        viewBox={`${-SIZE.w / 2} ${-SIZE.h / 2} ${SIZE.w} ${SIZE.h}`}
        style={{ background: "var(--chrome-bg-deep, #1b1d20)", border: "1px solid var(--chrome-border, #444)", cursor: drag.current ? "grabbing" : "grab", touchAction: "none" }}
        onMouseDown={(e) => {
          drag.current = { x: e.clientX, y: e.clientY, cx: view.cx, cy: view.cy };
        }}
        onMouseMove={(e) => {
          const d = drag.current;
          if (d) setChosen({ cx: d.cx - (e.clientX - d.x) * view.scale, cy: d.cy - (e.clientY - d.y) * view.scale, scale: view.scale });
        }}
        onMouseUp={() => (drag.current = null)}
        onMouseLeave={() => (drag.current = null)}
        onDoubleClick={() => setChosen(null)}
        onContextMenu={(e) => {
          e.preventDefault();
          setChosen(null); // the canvas's "Zoom to Fit"
        }}
      >
        <g transform={`scale(${1 / view.scale}) translate(${-view.cx} ${-view.cy})`}>
          {boardBox && <rect data-testid="zone-preview-board-box" x={boardBox[0]} y={boardBox[1]} width={boardBox[2] - boardBox[0]} height={boardBox[3] - boardBox[1]} fill="rgba(84,84,84,0.16)" />}
          {edges.length > 0 && <path d={path(edges)} fill="none" stroke={layerColor("board_edge")} strokeWidth={view.scale * 1.2} fillRule="evenodd" />}
          {zone && shownLayer && fill && fill.filled && fill.fragments.length > 0 && <path data-testid="zone-preview-fill" d={path(fill.fragments as Pt[][])} fill={color} fillOpacity={0.85} stroke="none" fillRule="nonzero" />}
          {zone && shownLayer && (
            <path data-testid="zone-preview-outline" d={path([zone.outline as Pt[]])} fill="none" stroke={color} strokeWidth={view.scale * 1.2} strokeDasharray={fill && fill.filled && fill.fragments.length > 0 ? undefined : `${view.scale * 6} ${view.scale * 4}`} />
          )}
        </g>
      </svg>
      </div>
      <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 4 }}>
        {zone ? (fill?.filled ? `Filled by KiCad (${fill.fragments.length} island${fill.fragments.length === 1 ? "" : "s"}).` : "Not filled: press B (Fill All Zones).") : ""} Wheel zooms, drag pans, double-click fits.
      </div>
    </div>
  );
}
