// The 3D viewer's Appearance manager (`EDA_3D_ACTIONS::showLayersManager`, the last button of the toolbar): where KiCad keeps what the viewer shows
// (3d-viewer/dialogs/appearance_controls_3D.cpp). Its layer tree lists KiCad's rows (kicad-port/appearance3d.ts: board body, plated barrels, copper, adhesive, solder paste,
// silkscreen, solder mask, the user layers the board has something on, the through-hole / SMD / virtual models, the bounding boxes, the references, zones and the background),
// each with an eye (the checkbox), a colour swatch where the row has a colour, and the shortcut of its action. "Use board stackup colors" paints the board body,
// silkscreen, solder mask and copper with the colours of Board Setup's physical stackup (the swatches of those rows are read-only then, as in KiCad); "Use PCB editor copper
// colors" takes the 2D editor's copper colours. The six face views are buttons too, with KiCad's axis icons, so they stay one click away. Render says whether the live scene
// (each part's own model, loaded here) or KiCad's whole-board export is shown.
import { useEffect, useState, useSyncExternalStore } from "react";
import toolbarData from "../../kicad/viewer3d_toolbars.json";
import type { Viewer3dAction, Viewer3dToolbarsFile } from "../../kicad/types";
import { displayHotkey } from "../../actions/hotkeys";
import { STACKUP_ROWS, resetSwatch, rowLabel, withSwatch, parseColor, rowsFor, toHex, toRgbHex, usedUserRows, type Row, type RowGroup } from "../../kicad-port/appearance3d";
import { enabledOptionalLayers, layerDisplayName } from "../../kicad-port/boardLayers";
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { Viewer3DOptions } from "../../state/store";
import { ActionIcon } from "../Toolbar";
import type { Viewer3DApi, ViewPreset } from "./Viewer3D";
import { useViewerColors } from "./useViewerColors";
import { viewer3dProbe } from "./viewer3dProbe";
import { useViewer3DRows } from "./useViewer3DRows";
import { setViewer3DPresets, useViewer3DUserSettings } from "../../state/viewer3dUserSettings";
import { FOLLOW_PCB, FOLLOW_PLOT_SETTINGS, afterVisibilityChange, applyPreset, colorsOf, deletePreset, deleteViewport, matchingPreset, presetEntries, savePreset, saveViewport, viewFromKicadMatrix, viewportFromJson, visibleSnapshot, type Viewport3D } from "../../kicad-port/viewer3dSettings";
import { sameView } from "../../kicad-port/camera3d";
import { boardOutlinePolygons } from "../../kicad-port/pcbOutline";
import { MessageDialog, NameDialog, PickDialog } from "../panels/appearance/shared";

/** The values of the two lists' options: a preset or a viewport by name (`p:`, `v:`), the two commands, and the blank entry the list sits on when nothing matches. */
const SAVE = "cmd:save";
const DELETE = "cmd:delete";
type Dlg = null | { kind: "save_preset" } | { kind: "overwrite"; name: string } | { kind: "delete_preset" } | { kind: "save_viewport" } | { kind: "delete_viewport" };

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

  // `rebuildLayers`: User.1 .. User.45 are listed when the board enables them; a board layer's row is labelled with the board's name for it.
  const layerSetup = state.board?.board_rules?.layer_setup;
  const rows = rowsFor(usedUserRows(enabledOptionalLayers(layerSetup)));
  const labelOf = (r: Row) => rowLabel(r, (file) => layerDisplayName(layerSetup, file));

  // What is shown (`GetVisibleLayers`): a "Follow" preset's board rows from what it follows, the rest the viewer's own.
  const rows3d = useViewer3DRows();
  const user = useViewer3DUserSettings();
  const colorsNow = colorsOf(colors);
  // `OnLayerVisibilityChanged`: every row as shown, the change, and the preset it now matches when the change takes the view off a "Follow" one.
  const setVisible = (id: string, shown: boolean) => set(afterVisibilityChange({ layers: opts.layers, preset: opts.preset }, id, shown, { follow: rows3d.follow, presets: user.presets, colors: colorsNow, useStackupColors: opts.useStackupColors }));
  // `onColorSwatchChanged`: the colours, and a preset of the person's that they (and the rows) now match -- a "Follow" preset stays.
  const setColors = (overrides: Record<string, string>) => {
    const following = opts.preset === FOLLOW_PCB || opts.preset === FOLLOW_PLOT_SETTINGS;
    const next = { ...colorsNow, ...overrides };
    set({ colors: overrides, ...(following ? {} : { preset: matchingPreset(user.presets, visibleSnapshot(opts.layers, opts.preset, rows3d.follow), next, opts.useStackupColors) }) });
  };
  const [dlg, setDlg] = useState<Dlg>(null);

  // The saved viewports (`board.3dviewports`), the project's: `appearance.json`'s `viewer3d` section (crates/cli/src/appearance_api.rs).
  const [viewports, setViewports] = useState<Viewport3D[]>([]);
  const [viewport, setViewport] = useState("");
  useEffect(() => {
    let cancelled = false;
    fetch("/api/appearance", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .then((v: { viewer3d?: { viewports?: unknown[] } } | null) => {
        if (cancelled || !v) return;
        setViewports((v.viewer3d?.viewports ?? []).map(viewportFromJson).filter((x): x is Viewport3D => x !== null));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
  const storeViewports = (list: Viewport3D[]) => {
    setViewports(list);
    void fetch("/api/appearance", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ viewer3d: { viewports: list } }) });
  };
  // `onUpdateViewportsCb`: once the camera has moved off the viewport chosen, the list shows none.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!viewport || !api) return;
    const t = setInterval(() => {
      const vp = viewports.find((v) => v.name === viewport);
      if (!vp || !vp.camera || !sameView(api.getCamera(), vp.camera)) setViewport("");
      setTick((n) => n + 1);
    }, 500);
    return () => clearInterval(t);
  }, [viewport, viewports, api]);
  const setColor = (row: Row, text: string) => {
    const c = parseColor(text);
    if (!c) return;
    // The swatch has no alpha: keep the row's own (the solder mask is 83 % opaque).
    const alpha = colors[row.id]?.a ?? 1;
    setColors(withSwatch(opts.colors, row.id, toHex({ ...c, a: alpha })));
  };
  // `DIALOG_COLOR_PICKER::OnResetButton`: the swatch's default, `BOARD_ADAPTER::GetDefaultColors()` (both copper rows at once).
  const resetColor = (id: string) => setColors(resetSwatch(opts.colors, id));

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
                    <input type="checkbox" aria-label={`Show ${labelOf(r)}`} checked={rows3d.shown(r.id)} onChange={(e) => setVisible(r.id, e.target.checked)} />
                    {r.color && c ? (
                      <input
                        type="color"
                        aria-label={`${labelOf(r)} color`}
                        value={toRgbHex(c)}
                        disabled={readOnly}
                        title={readOnly ? STACKUP_OWNS : "Change color"}
                        onChange={(e) => setColor(r, e.target.value)}
                        style={{ width: 22, height: 16, padding: 0, border: "1px solid var(--chrome-border, #444)", background: "none" }}
                      />
                    ) : (
                      <span style={{ width: 22 }} />
                    )}
                    <span style={{ flex: 1 }}>{labelOf(r)}</span>
                    {overridden && (
                      <button className="toolbar-button" style={{ width: 18, height: 18, fontSize: 11 }} title="Back to the default color" aria-label={`Reset ${labelOf(r)} color`} onClick={() => resetColor(r.id)}>
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

      <div className="panel-section" data-section="viewer3d-presets">
        {/* `m_cbLayerPresets` and `m_cbViewports` (`rebuildLayerPresetsWidget`, `rebuildViewportsWidget`). The quick switchers on Ctrl+Tab / Shift+Tab are not
            ported: the browser keeps those keys. */}
        <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <label htmlFor="v3d-presets" style={{ width: 70 }}>
            Presets:
          </label>
          <select
            id="v3d-presets"
            style={{ flex: 1 }}
            title={"Save and restore color and visibility combinations."}
            value={opts.preset === FOLLOW_PCB || opts.preset === FOLLOW_PLOT_SETTINGS || user.presets.some((p) => p.name === opts.preset) ? `p:${opts.preset}` : ""}
            onChange={(e) => {
              const v = e.target.value;
              if (v === SAVE) setDlg({ kind: "save_preset" });
              else if (v === DELETE) setDlg({ kind: "delete_preset" });
              else if (v.startsWith("p:")) {
                // `doApplyLayerPreset`.
                const applied = applyPreset(user.presets, v.slice(2));
                if (applied) set(applied);
              }
            }}
          >
            {presetEntries(user.presets).map((e, i) =>
              e.kind === "follow" ? (
                <option key={e.name} value={`p:${e.name}`}>
                  {e.label}
                </option>
              ) : e.kind === "preset" ? (
                <option key={`u:${e.name}`} value={`p:${e.name}`}>
                  {e.name}
                </option>
              ) : e.kind === "separator" ? (
                <option key={`s${i}`} value="" disabled>
                  ---
                </option>
              ) : e.kind === "save" ? (
                <option key="save" value={SAVE}>
                  Save preset...
                </option>
              ) : (
                <option key="delete" value={DELETE}>
                  Delete preset...
                </option>
              )
            )}
          </select>
        </div>
        <div className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <label htmlFor="v3d-viewports" style={{ width: 70 }}>
            Viewports:
          </label>
          <select
            id="v3d-viewports"
            style={{ flex: 1 }}
            title={"Save and restore camera position and zoom."}
            disabled={!api}
            value={viewport && viewports.some((v) => v.name === viewport) ? `v:${viewport}` : ""}
            onChange={(e) => {
              const v = e.target.value;
              if (v === SAVE) setDlg({ kind: "save_viewport" });
              else if (v === DELETE) setDlg({ kind: "delete_viewport" });
              else if (v.startsWith("v:")) {
                // `doApplyViewport`.
                const vp = viewports.find((x) => x.name === v.slice(2));
                if (vp && api) {
                  // a KiCad project's viewport is its matrix alone: `CAMERA::SetViewMatrix` on the camera as it is (kept with the viewport while the page is open)
                  const camera = vp.camera ?? (vp.matrix ? viewFromKicadMatrix(vp.matrix, boardSpanMm(state.board), api.getCamera()) : null);
                  if (!camera) return;
                  if (!vp.camera) setViewports(viewports.map((x) => (x === vp ? { ...x, camera } : x)));
                  api.setCamera(camera);
                  setViewport(vp.name);
                }
              }
            }}
          >
            {viewports.map((v) => (
              <option key={v.name} value={`v:${v.name}`}>
                {v.name}
              </option>
            ))}
            <option value="" disabled>
              ---
            </option>
            <option value={SAVE}>Save viewport...</option>
            <option value={DELETE}>Delete viewport...</option>
          </select>
        </div>
        {dlg?.kind === "save_preset" && (
          <NameDialog
            title="Save Layer Preset"
            label="Layer preset name:"
            onCancel={() => setDlg(null)}
            onOk={(name) => {
              if (!name) return setDlg(null);
              if (user.presets.some((p) => p.name === name)) return setDlg({ kind: "overwrite", name });
              setViewer3DPresets(savePreset(user.presets, name, visibleSnapshot(opts.layers, opts.preset, rows3d.follow), colorsNow));
              set({ preset: name, layers: visibleSnapshot(opts.layers, opts.preset, rows3d.follow) });
              setDlg(null);
            }}
          />
        )}
        {dlg?.kind === "overwrite" && (
          <MessageDialog
            title="Save Layer Preset"
            message="Overwrite existing preset?"
            onCancel={() => setDlg(null)}
            onOk={() => {
              setViewer3DPresets(savePreset(user.presets, dlg.name, visibleSnapshot(opts.layers, opts.preset, rows3d.follow), colorsNow));
              set({ preset: dlg.name, layers: visibleSnapshot(opts.layers, opts.preset, rows3d.follow) });
              setDlg(null);
            }}
          />
        )}
        {dlg?.kind === "delete_preset" && (
          <PickDialog
            title="Delete Preset"
            header="Presets"
            label="Select preset:"
            items={user.presets.map((p) => p.name)}
            onCancel={() => setDlg(null)}
            onOk={(name) => {
              const next = deletePreset(user.presets, name, opts.preset);
              setViewer3DPresets(next.presets);
              if (next.current !== opts.preset) set({ preset: next.current });
              setDlg(null);
            }}
          />
        )}
        {dlg?.kind === "save_viewport" && (
          <NameDialog
            title="Save Viewport"
            label="Viewport name:"
            onCancel={() => setDlg(null)}
            onOk={(name) => {
              if (name && api) {
                storeViewports(saveViewport(viewports, name, api.getCamera(), boardSpanMm(state.board)));
                setViewport(name);
              }
              setDlg(null);
            }}
          />
        )}
        {dlg?.kind === "delete_viewport" && (
          <PickDialog
            title="Delete Viewport"
            header="Viewports"
            label="Select viewport:"
            items={viewports.map((v) => v.name)}
            onCancel={() => setDlg(null)}
            onOk={(name) => {
              storeViewports(deleteViewport(viewports, name));
              if (viewport === name) setViewport("");
              setDlg(null);
            }}
          />
        )}
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

/**
 * The board's longest outline edge in mm, the span the 3D camera is homed with (Viewer3D.tsx `setBoardGeometry`; KiCad's `m_boardSize`, the Edge.Cuts box, 20 mm for a
 * board with none: `bbbox.Inflate( pcbIUScale.mmToIU( 10 ) )`): what a saved viewport's KiCad view matrix is scaled by (`kicadViewMatrix`).
 */
function boardSpanMm(board: Parameters<typeof boardOutlinePolygons>[0] | null | undefined): number {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of board ? boardOutlinePolygons(board) : [])
    for (const [x, y] of p.outer) {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }
  if (!Number.isFinite(minX)) return 20;
  return Math.max((maxX - minX) / 1e6, (maxY - minY) / 1e6, 1e-3);
}
