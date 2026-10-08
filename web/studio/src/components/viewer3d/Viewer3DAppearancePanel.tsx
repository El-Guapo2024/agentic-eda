// The 3D viewer's Appearance manager (`EDA_3D_ACTIONS::showLayersManager`, the last button of the toolbar): where KiCad keeps what the viewer shows. It holds
// the switches the studio's old 3D toolbar carried as text buttons -- board body, silkscreen, solder mask and paste, the component models (through-hole
// and SMD apart, `T` / `S`), their bounding boxes -- and "KiCad Models", the choice between KiCad's own render of the board (the real 3D models, `kicad-cli pcb
// export glb`) and the live placeholder scene. The six face views (View > Top ... in KiCad, hotkeys Z / Shift+Z / Y / Shift+Y / X / Shift+X) are buttons here too,
// with KiCad's axis icons, so they stay one click away.
import toolbarData from "../../kicad/viewer3d_toolbars.json";
import type { Viewer3dAction, Viewer3dToolbarsFile } from "../../kicad/types";
import { displayHotkey } from "../../actions/hotkeys";
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { Viewer3DOptions } from "../../state/store";
import { ActionIcon } from "../Toolbar";
import type { Viewer3DApi, ViewPreset } from "./Viewer3D";

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

const SWITCHES: ReadonlyArray<{ key: keyof Viewer3DOptions; label: string; title: string; icon?: string; hotkey?: string }> = [
  { key: "showBoardBody", label: "Board Body", title: "Show the board substrate (render.show_board_body)" },
  { key: "showSilkscreen", label: "Silkscreen", title: "Show silkscreen" },
  { key: "showSolderMask", label: "Solder Mask", title: "Show solder mask" },
  { key: "showSolderPaste", label: "Solder Paste", title: "Show solder paste (render.show_solderpaste)" },
  { key: "showComponents", label: "3D Models", title: "Show the rendered component models" },
  { key: "showTHT", label: "Through Hole Models", title: "Show 3D models for 'Through hole' type footprints", icon: "show_tht", hotkey: "T" },
  { key: "showSMD", label: "SMD Models", title: "Show 3D models for 'Surface mount' type footprints", icon: "show_smt", hotkey: "S" },
  { key: "showBoundingBoxes", label: "Model Bounding Boxes", title: "Show a bounding box per part (render.opengl_show_model_bbox)" },
];

export function Viewer3DAppearancePanel({ api }: { api: Viewer3DApi | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const opts = state.viewer3d;
  const set = (patch: Partial<Viewer3DOptions>) => dispatch({ type: "SET_VIEWER3D_OPTIONS", options: patch });
  // Quiet failure per this route's design (Viewer3D.tsx's fetch effect): no popup, no repeated retries -- the panel says so, and Reload board tries again.
  const render =
    state.glbStatus === "pending"
      ? "Loading KiCad's render of the board…"
      : state.glbStatus === "failed"
        ? `KiCad's render failed, so the live scene is shown: ${(state.glbError ?? "unknown error").slice(0, 240)}`
        : state.glbStatus === "loaded"
          ? "Showing KiCad's render (the real 3D models)."
          : null;
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
        <h3>Show</h3>
        {SWITCHES.map((s) => (
          <label key={s.key} className="filter-row" title={s.title}>
            <input type="checkbox" checked={Boolean(opts[s.key])} onChange={(e) => set({ [s.key]: e.target.checked } as Partial<Viewer3DOptions>)} />
            {s.icon && <ActionIcon iconName={s.icon} size={16} />}
            <span style={{ flex: 1 }}>{s.label}</span>
            {s.hotkey && <span style={{ color: "var(--chrome-text-dim)", fontSize: 10 }}>{s.hotkey}</span>}
          </label>
        ))}
      </div>
      <div className="panel-section">
        <h3>Render</h3>
        <label className="filter-row" title="Show KiCad's own render of the board (the real 3D models, kicad-cli's colors) instead of the live scene. The live scene is shown while the render is built, or if it fails.">
          <input type="checkbox" checked={opts.kicadModels} onChange={(e) => set({ kicadModels: e.target.checked })} />
          KiCad Models
        </label>
        {render && <div className="panel-empty" style={{ marginTop: 4 }}>{render}</div>}
      </div>
    </>
  );
}
