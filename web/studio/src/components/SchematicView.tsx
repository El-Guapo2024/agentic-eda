// The schematic tab: GET /api/schematic.svg, panned/zoomed with plain
// pointer/wheel handlers (ported from the old studio.html, which did the
// same -- this is a read-only rendered view, not an editor).
import { useEffect, useRef, useState } from "react";
import { fetchSchematicSvg } from "../api/client";

export function SchematicView() {
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [box, setBox] = useState({ w: 100, h: 100 });
  const [view, setView] = useState({ scale: 1, x: 0, y: 0 });
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ x: number; y: number; vx: number; vy: number } | null>(null);
  const loaded = useRef(false);

  useEffect(() => {
    if (loaded.current) return;
    loaded.current = true;
    fetchSchematicSvg()
      .then((text) => {
        const m = /viewBox="([^"]+)"/.exec(text);
        const vb = (m?.[1] ?? "0 0 100 100").split(/[ ,]+/).map(Number);
        setBox({ w: (vb[2] ?? 100) * 8, h: (vb[3] ?? 100) * 8 });
        setSvg(text);
      })
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, []);

  useEffect(() => {
    if (!svg || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const scale = Math.min(rect.width / box.w, rect.height / box.h) * 0.95;
    setView({ scale, x: (rect.width - box.w * scale) / 2, y: (rect.height - box.h * scale) / 2 });
  }, [svg, box]);

  if (error) return <div className="pcb-canvas-empty">{error}</div>;
  if (!svg) return <div className="pcb-canvas-empty">Loading schematic…</div>;

  const dataUrl = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;

  return (
    <div
      ref={containerRef}
      style={{ position: "absolute", inset: 0, overflow: "hidden", background: "#fbfaf6", cursor: "grab" }}
      onPointerDown={(e) => {
        (e.target as Element).setPointerCapture(e.pointerId);
        dragRef.current = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y };
      }}
      onPointerMove={(e) => {
        const d = dragRef.current;
        if (!d) return;
        setView((v) => ({ ...v, x: d.vx + e.clientX - d.x, y: d.vy + e.clientY - d.y }));
      }}
      onPointerUp={() => (dragRef.current = null)}
      onWheel={(e) => {
        e.preventDefault();
        const rect = containerRef.current!.getBoundingClientRect();
        const mx = e.clientX - rect.left,
          my = e.clientY - rect.top;
        setView((v) => {
          const s2 = Math.max(0.05, Math.min(20, v.scale * Math.exp(-e.deltaY * 0.0015)));
          return { scale: s2, x: mx - ((mx - v.x) * s2) / v.scale, y: my - ((my - v.y) * s2) / v.scale };
        });
      }}
    >
      <img
        src={dataUrl}
        alt="schematic"
        draggable={false}
        style={{ position: "absolute", left: 0, top: 0, width: box.w, height: box.h, transformOrigin: "0 0", transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})`, userSelect: "none" }}
      />
    </div>
  );
}
