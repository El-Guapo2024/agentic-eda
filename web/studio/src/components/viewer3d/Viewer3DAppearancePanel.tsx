// The 3D viewer's Appearance manager (`EDA_3D_ACTIONS::showLayersManager`, the last button of the toolbar): where KiCad keeps what the viewer shows
// (3d-viewer/dialogs/appearance_controls_3D.cpp). Its layer tree lists KiCad's rows (kicad-port/appearance3d.ts: board body, plated barrels, copper, adhesive, solder paste,
// silkscreen, solder mask, the user layers the board has something on, the through-hole / SMD / virtual models, the bounding boxes, the references, zones and the background),
// each with an eye (the checkbox), a colour swatch where the row has a colour, and the shortcut of its action. "Use board stackup colors" paints the board body,
// silkscreen, solder mask and copper with the colours of Board Setup's physical stackup (the swatches of those rows are read-only then, as in KiCad); "Use PCB editor copper
// colors" takes the 2D editor's copper colours. The six face views are buttons too, with KiCad's axis icons, so they stay one click away. Render says whether the live scene
// (each part's own model, loaded here) or KiCad's whole-board export is shown.
import { useSyncExternalStore } from "react";
import toolbarData from "../../kicad/viewer3d_toolbars.json";
import type { Viewer3dAction, Viewer3dToolbarsFile } from "../../kicad/types";
import { displayHotkey } from "../../actions/hotkeys";
import { STACKUP_ROWS, isVisible, parseColor, rowsFor, toHex, toRgbHex, usedUserRows, type Row, type RowGroup } from "../../kicad-port/appearance3d";
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { Viewer3DOptions } from "../../state/store";
import { ActionIcon } from "../Toolbar";
import type { Viewer3DApi, ViewPreset } from "./Viewer3D";
import { useViewerColors } from "./useViewerColors";
import { viewer3dProbe } from "./viewer3dProbe";

const other = new Map<string, Viewer3dAction>((toolbarData as unknown as Viewer3dToolbarsFile).otherActions.map((a) => [a.name, a]));

/** The six face views, in KiCad's View menu order, and the action each is. */
const FACE_VIEWS: ReadonlyArray<{ action: string; preset: ViewPreset }> = [
  { action: "3DViewer.Control.viewTop", preset: "top" },
  { action: "3DViewer.Control.viewBottom", preset: "bottom" },
  { action: "3DViewer.Control.viewFront", preset: "front" },
  { action: "3DViewer.Control.viewBack", preset: "back" },
  { action: "3DViewer.Control.viewLeft", preset: "left" },
  { action: "3DViewer.Control.viewRight", preset: "right" },
];

const GROUPS: ReadonlyArray<{ id: RowGroup; title: string }> = [
  { id: "board", title: "Board" },
  { id: "user", title: "User layers" },
  { id: "models", title: "Models" },
  { id: "display", title: "Display" },
];

/** What KiCad's swatch says when it is read-only (`SetReadOnlyCallback`). */
const STACKUP_OWNS = "Uncheck 'Use board stackup colors' to allow color editing.";

export function Viewer3DAppearancePanel({ api }: { api: Viewer3DApi | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const opts = state.viewer3d;
  const set = (patch: Partial<Viewer3DOptions>) => dispatch({ type: "SET_VIEWER3D_OPTIONS", options: patch });
  const colors = useViewerColors();
  const probe = useSyncExternalStore(viewer3dProbe.subscribe, viewer3dProbe.snapshot);

  // The user layers the board has a drawing or a text on are the ones listed (KiCad lists the layers the board has enabled).
  const used = usedUserRows([...(state.board?.drawings?.shapes ?? []).map((s) => s.layer), ...(state.board?.drawings?.texts ?? []).map((t) => t.layer)]);
  const rows = rowsFor(used);

  const setVisible = (id: string, shown: boolean) => set({ layers: { ...opts.layers, [id]: shown } });
  const setColor = (row: Row, text: string) => {
    const c = parseColor(text);
    if (!c) return;
    // The swatch has no alpha: keep the row's own (the solder mask is 83 % opaque).
    const alpha = colors[row.id]?.a ?? 1;
    set({ colors: { ...opts.colors, [row.id]: toHex({ ...c, a: alpha }) } });
  };
  const resetColor = (id: string) => {
    const next = { ...opts.colors };
    delete next[id];
    set({ colors: next });
  };

  // Quiet failure per this route's design (Viewer3D.tsx's fetch effect): no popup, no repeated retries -- the panel says so, and Reload board tries again.
  const render =
    state.glbStatus === "pending"
      ? "Building KiCad's export…"
      : state.glbStatus === "failed"
        ? `KiCad's export failed, so the live scene is shown: ${(state.glbError ?? "unknown error").slice(0, 240)}`
        : state.glbStatus === "loaded"
          ? "Showing KiCad's export."
          : null;
  const m = probe.models;
  const failed = probe.detail.filter((d) => d.status === "failed" || d.status === "missing");

  return (
    <>
      <div className="panel-section">
        <h3>View</h3>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 34px)", gap: 4 }}>
          {FACE_VIEWS.map(({ action, preset }) => {
            const a = other.get(action);
            return (
              <button key={action} className="toolbar-button" style={{ width: 34, height: 34 }} disabled={!api} title={[a?.label ?? action, a?.hotkey ? displayHotkey(a.hotkey) : null].filter(Boolean).join(" — ")} onClick={() => api?.setView(preset)}>
                <ActionIcon iconName={a?.icon ?? null} />
              </button>
            );
          })}
        </div>
      </div>

      <div className="panel-section">
        <h3>Layers</h3>
        <label className="filter-row" title="Paint the board body, silkscreen, solder mask and copper with the colors of the board's physical stackup (Board Setup > Board Stackup)">
          <input type="checkbox" checked={opts.useStackupColors} onChange={(e) => set({ useStackupColors: e.target.checked })} />
          <span>Use board stackup colors</span>
        </label>
        <label className="filter-row" title="Use the PCB editor's colors for F.Cu and B.Cu (the live scene only)">
          <input type="checkbox" checked={opts.useEditorCopperColors} onChange={(e) => set({ useEditorCopperColors: e.target.checked })} />
          <span>Use PCB editor copper colors</span>
        </label>
        {GROUPS.map((g) => {
          const inGroup = rows.filter((r) => r.group === g.id);
          if (inGroup.length === 0) return null;
          return (
            <details key={g.id} open className="viewer3d-layer-group" data-group={g.id}>
              <summary style={{ cursor: "pointer", fontSize: 11, color: "var(--chrome-text-dim)", margin: "6px 0 2px" }}>{g.title}</summary>
              {inGroup.map((r) => {
                const c = colors[r.id];
                const readOnly = opts.useStackupColors && STACKUP_ROWS.has(r.id);
                const overridden = r.id in opts.colors && !readOnly;
                return (
                  <div key={r.id} className="filter-row" data-row={r.id} title={r.tooltip} style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <input type="checkbox" aria-label={`Show ${r.label}`} checked={isVisible(opts.layers, r.id)} onChange={(e) => setVisible(r.id, e.target.checked)} />
                    {r.color && c ? (
                      <input
                        type="color"
                        aria-label={`${r.label} color`}
                        value={toRgbHex(c)}
                        disabled={readOnly}
                        title={readOnly ? STACKUP_OWNS : "Change color"}
                        onChange={(e) => setColor(r, e.target.value)}
                        style={{ width: 22, height: 16, padding: 0, border: "1px solid var(--chrome-border, #444)", background: "none" }}
                      />
                    ) : (
                      <span style={{ width: 22 }} />
                    )}
                    <span style={{ flex: 1 }}>{r.label}</span>
                    {overridden && (
                      <button className="toolbar-button" style={{ width: 18, height: 18, fontSize: 11 }} title="Back to the default color" aria-label={`Reset ${r.label} color`} onClick={() => resetColor(r.id)}>
                        ↺
                      </button>
                    )}
                    {r.hotkey && <span style={{ color: "var(--chrome-text-dim)", fontSize: 10 }}>{r.hotkey}</span>}
                  </div>
                );
              })}
              {g.id === "models" && m.requested > 0 && (
                <div className="panel-empty" style={{ marginTop: 4 }}>
                  {m.loading > 0 ? `Loading models: ${m.ready} of ${m.requested}…` : `${m.ready} of ${m.requested} models loaded.`}
                  {failed.length > 0 && (
                    <>
                      {" "}
                      {failed.length} could not be loaded and are drawn as boxes: {failed.map((f) => `${f.name.split("/").pop()} (${(f.error ?? f.status).slice(0, 80)})`).join("; ")}.
                    </>
                  )}
                </div>
              )}
            </details>
          );
        })}
      </div>

      <div className="panel-section">
        <h3>Render</h3>
        <label className="filter-row" title="Show KiCad's own render of the whole board (kicad-cli: every model through OpenCascade, seconds to minutes, kicad-cli's colors) instead of the live scene, which loads each part's model by itself. The live scene is shown while the export is built, or if it fails.">
          <input type="checkbox" checked={opts.kicadModels} onChange={(e) => set({ kicadModels: e.target.checked })} />
          Exact export (kicad-cli)
        </label>
        {render && <div className="panel-empty" style={{ marginTop: 4 }}>{render}</div>}
      </div>
    </>
  );
}
