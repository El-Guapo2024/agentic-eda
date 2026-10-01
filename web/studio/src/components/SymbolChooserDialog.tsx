// `A` (sch_drawing_tools.cpp PlaceSymbol -> DIALOG_SYMBOL_CHOOSER): a
// search list + live preview over GET /api/symbol_library's catalog
// ("the libraries we already load" -- see that endpoint's own doc for
// what that means and what it deliberately excludes). Real KiCad's
// chooser also has recently-used/already-placed pseudo-library tabs and
// a unit picker for a multi-unit part -- not replicated; this is a
// search-and-preview-and-place MVP, not a full port.
//
// Confirming here doesn't place anything itself -- it arms
// `state.armedSymbol` and the `sch_place_symbol` tool, same two-step
// ("choose, then click to place") flow every other eeschema placement
// tool in this app uses, and SchematicView.tsx's onPointerDown is what
// actually calls `add_symbol` once the user clicks the sheet.
import { useEffect, useRef, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { fetchSymbolLibrary } from "../api/client";
import type { Schematic, SchematicSymbol, SymbolLibrary, SymbolLibraryEntry } from "../api/types";
import { symbolBounds, paintSchematic } from "./schematic/painter";
import { fitTransform } from "./canvas/view";
import { layerColor } from "./canvas/layers";

const PREVIEW_W = 220;
const PREVIEW_H = 160;

function SymbolPreview({ entry, library }: { entry: SymbolLibraryEntry | null; library: SymbolLibrary | null }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, PREVIEW_W, PREVIEW_H);
    ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
    ctx.fillRect(0, 0, PREVIEW_W, PREVIEW_H);
    const resolved = entry && library?.lib_symbols[entry.lib_id];
    if (!resolved) return;
    const fakeSymbol: SchematicSymbol = { id: "?", lib_id: entry.lib_id, at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] };
    const fakeSch: Schematic = { lib_symbols: { [entry.lib_id]: resolved }, symbols: [fakeSymbol], power_symbols: [], wires: [], no_connects: [], labels: [], texts: [], title_block: null };
    const bounds = symbolBounds(fakeSymbol, fakeSch.lib_symbols);
    const view = fitTransform(bounds, PREVIEW_W, PREVIEW_H, 14);
    ctx.save();
    ctx.translate(view.x, view.y);
    ctx.scale(view.scale || 1, view.scale || 1);
    paintSchematic(ctx, view, fakeSch, { selection: new Set(), netHighlight: null });
    ctx.restore();
  }, [entry, library]);

  return <canvas ref={canvasRef} width={PREVIEW_W} height={PREVIEW_H} style={{ border: "1px solid var(--chrome-border)", borderRadius: 4 }} />;
}

export function SymbolChooserDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.symbolChooserOpen;

  const [library, setLibrary] = useState<SymbolLibrary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setSelectedId(null);
    setError(null);
    let cancelled = false;
    fetchSymbolLibrary()
      .then((lib) => {
        if (cancelled) return;
        setLibrary(lib);
        setSelectedId(lib.entries[0]?.lib_id ?? null);
      })
      .catch((e) => !cancelled && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      cancelled = true;
    };
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_SYMBOL_CHOOSER_OPEN", open: false });

  const q = query.trim().toLowerCase();
  const matches = (library?.entries ?? []).filter((e) => !q || e.lib_id.toLowerCase().includes(q) || e.description.toLowerCase().includes(q));
  const selected = matches.find((e) => e.lib_id === selectedId) ?? matches[0] ?? null;

  const place = () => {
    if (!selected) return;
    dispatch({ type: "SET_ARMED_SYMBOL", symbol: { libId: selected.lib_id, referencePrefix: selected.reference_prefix } });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_place_symbol" });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Place Symbol</span>
        </div>
        <div className="dialog-body">
          <input
            autoFocus
            placeholder="Search symbols (e.g. R, Device:C, diode)…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") place();
            }}
            style={{ width: "100%", boxSizing: "border-box", marginBottom: 8 }}
          />
          {error && <div className="field" style={{ color: "var(--chrome-danger)" }}>{error}</div>}
          <div style={{ display: "flex", gap: 10 }}>
            <div style={{ flex: 1, height: 220, overflowY: "auto", border: "1px solid var(--chrome-border)", borderRadius: 4 }}>
              {matches.map((e) => (
                <div
                  key={e.lib_id}
                  onClick={() => setSelectedId(e.lib_id)}
                  onDoubleClick={place}
                  style={{
                    padding: "4px 8px",
                    cursor: "pointer",
                    background: e.lib_id === selected?.lib_id ? "var(--chrome-selected-bg)" : "transparent",
                    color: e.lib_id === selected?.lib_id ? "var(--chrome-selected-text)" : undefined,
                  }}
                >
                  <div>{e.lib_id}</div>
                  {e.description && <div style={{ fontSize: "0.85em", opacity: 0.7 }}>{e.description}</div>}
                </div>
              ))}
              {matches.length === 0 && !error && <div className="field" style={{ padding: 8, opacity: 0.7 }}>No symbols match.</div>}
            </div>
            <SymbolPreview entry={selected} library={library} />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!selected} onClick={place}>
            Place
          </button>
        </div>
      </div>
    </div>
  );
}
