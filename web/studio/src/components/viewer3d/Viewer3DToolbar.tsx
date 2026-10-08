// The 3D viewer's toolbar: KiCad's own (3d-viewer/3d_viewer/toolbars_3d.cpp, EDA_3D_VIEWER_TOOLBAR_SETTINGS::DefaultToolbarConfig, TOP_MAIN), extracted into
// viewer3d_toolbars.json with the actions it names (eda_3d_actions.cpp's EDA_3D_ACTIONS, which actions.json does not hold): Reload board, Copy 3D image,
// Raytracing, Zoom Redraw / In / Out / Fit, Rotate X / Y / Z clockwise and counterclockwise, Flip Board, Move Left / Right / Up / Down, Orthographic
// projection and the Appearance manager -- each with KiCad's icon, in KiCad's order, the same hotkey in its tooltip.
//
// The studio's own additions of the old toolbar (the six face views, the visibility toggles, "KiCad Models") moved to the Appearance manager, where KiCad
// has the visibility switches (components/viewer3d/Viewer3DAppearancePanel.tsx).
//
// Wired to Viewer3D's camera through a small imperative handle (`Viewer3DApi`, defined in ./Viewer3D) rather than a shared prop-callback/context: Viewer3D
// creates the handle once its camera exists and hands it to App.tsx via `onReady`; App.tsx passes it straight through here as `api`. Buttons are disabled until
// `api` is non-null (i.e. until Viewer3D has actually mounted its Three.js scene).
import actionsData from "../../kicad/actions.json";
import toolbarData from "../../kicad/viewer3d_toolbars.json";
import type { ActionsFile, Viewer3dAction, Viewer3dToolbarsFile } from "../../kicad/types";
import { displayHotkey, effectiveHotkey } from "../../actions/hotkeys";
import type { PanDirection, RotateAxis, RotateDir } from "../../kicad-port/actions3d";
import { setDockColumnCollapsed, useDockLayout } from "../../state/dockLayoutStore";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { ActionIcon } from "../Toolbar";
import type { Viewer3DApi } from "./Viewer3D";

const file = toolbarData as unknown as Viewer3dToolbarsFile;
const own = new Map<string, Viewer3dAction>(file.actions.map((a) => [a.name, a]));
const common = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a]));

/** What a toolbar item shows: its label, tooltip, hotkey and KiCad icon, from the 3D action table or, for the shared zoom actions, actions.json. */
function describe(name: string): { label: string; tooltip: string; hotkey: string | null; icon: string | null } {
  const a = own.get(name);
  if (a) return { label: a.label, tooltip: a.tooltip, hotkey: a.hotkey, icon: a.icon };
  const c = common.get(name);
  return { label: c?.label ?? name, tooltip: c?.tooltip ?? "", hotkey: c ? effectiveHotkey(c).hotkey : null, icon: c?.icon ?? null };
}

const ROTATE: Record<string, { axis: RotateAxis; dir: RotateDir }> = {
  "3DViewer.Control.rotateXclockwise": { axis: "x", dir: "cw" },
  "3DViewer.Control.rotateXcounterclockwise": { axis: "x", dir: "ccw" },
  "3DViewer.Control.rotateYclockwise": { axis: "y", dir: "cw" },
  "3DViewer.Control.rotateYcounterclockwise": { axis: "y", dir: "ccw" },
  "3DViewer.Control.rotateZclockwise": { axis: "z", dir: "cw" },
  "3DViewer.Control.rotateZcounterclockwise": { axis: "z", dir: "ccw" },
};

const MOVE: Record<string, PanDirection> = {
  "3DViewer.Control.moveLeft": "left",
  "3DViewer.Control.moveRight": "right",
  "3DViewer.Control.moveUp": "up",
  "3DViewer.Control.moveDown": "down",
};

interface Handler {
  run: () => void;
  enabled: boolean;
  /** Pressed or not, for a toggle; undefined for a plain button. */
  checked?: boolean;
  /** Why a disabled button is dead, for its tooltip. */
  reason?: string;
}

export function Viewer3DToolbar({ api }: { api: Viewer3DApi | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const dock = useDockLayout();
  const opts = state.viewer3d;
  const set = (patch: Partial<typeof opts>) => dispatch({ type: "SET_VIEWER3D_OPTIONS", options: patch });
  const toast = (message: string, kind: "info" | "error") => dispatch({ type: "TOAST", message, kind });
  const ready = !!api;

  const handlers: Record<string, Handler> = {
    "3DViewer.Control.reloadBoard": { run: () => api?.reload(), enabled: ready },
    "3DViewer.Control.copyToClipboard": {
      run: () => void api?.copyImage().then((ok) => toast(ok ? "3D image copied to the clipboard." : "The browser did not allow copying the image to the clipboard.", ok ? "info" : "error")),
      enabled: ready,
    },
    "3DViewer.Control.toggleRaytacing": { run: () => {}, enabled: false, reason: "not ported: this viewer has one render engine (KiCad's own render, or the live scene), not a raytracer" },
    // Zoom Redraw repaints from current state every frame already; Zoom In / Out step the camera, Zoom to Fit is the home view.
    "common.Control.zoomRedraw": { run: () => {}, enabled: ready },
    "common.Control.zoomInCenter": { run: () => api?.dispatchAction({ kind: "zoomIn" }), enabled: ready },
    "common.Control.zoomOutCenter": { run: () => api?.dispatchAction({ kind: "zoomOut" }), enabled: ready },
    "common.Control.zoomFitScreen": { run: () => api?.setView("reset"), enabled: ready },
    "3DViewer.Control.flipView": { run: () => set({ flipped: !opts.flipped }), enabled: ready, checked: opts.flipped },
    "3DViewer.Control.toggleOrtho": { run: () => set({ orthographic: !opts.orthographic }), enabled: true, checked: opts.orthographic },
    "3DViewer.Control.showLayersManager": { run: () => setDockColumnCollapsed("right", !dock.rightCollapsed), enabled: true, checked: !dock.rightCollapsed },
  };
  for (const [name, { axis, dir }] of Object.entries(ROTATE)) handlers[name] = { run: () => api?.dispatchAction({ kind: "rotate", axis, dir }), enabled: ready };
  for (const [name, direction] of Object.entries(MOVE)) handlers[name] = { run: () => api?.dispatchAction({ kind: "pan", direction }), enabled: ready };

  return (
    <div className="toolbar" data-toolbar="viewer3d" data-editor="3d">
      {file.toolbars[0]!.items.map((item, i) => {
        if (item.type === "separator") return <div key={i} className="toolbar-separator" role="separator" />;
        if (item.type !== "action") return null;
        const h = handlers[item.action];
        const d = describe(item.action);
        const enabled = !!h?.enabled;
        const tooltip = enabled ? [d.label, d.hotkey ? displayHotkey(d.hotkey) : null].filter(Boolean).join(" — ") : `${d.label} (${h?.reason ?? "not ported yet"})`;
        const pressed = enabled ? h?.checked : undefined;
        return (
          <button key={i} className={`toolbar-button${pressed ? " active" : ""}`} aria-pressed={pressed} disabled={!enabled} title={tooltip} onClick={() => h?.run()}>
            <ActionIcon iconName={d.icon} />
          </button>
        );
      })}
    </div>
  );
}
