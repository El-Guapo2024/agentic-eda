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
//
// With the 3D viewer's "Customize toolbars" on (Preferences > 3D Viewer > Toolbars) the toolbar is the person's (state/toolbarSettings.ts `resolvedToolbar`):
// separators, spacers and groups as they left them; a group is a button of its first usable tool whose right click lists the others. The zoom box a customised
// toolbar may hold is left out, as KiCad's 3D frame has no factory for it (`ApplyConfiguration`: "Unable to find control factory").
import { useState } from "react";
import actionsData from "../../kicad/actions.json";
import toolbarData from "../../kicad/viewer3d_toolbars.json";
import type { ActionsFile, Viewer3dAction, Viewer3dToolbarsFile } from "../../kicad/types";
import { displayHotkey, effectiveHotkey } from "../../actions/hotkeys";
import type { PanDirection, RotateAxis, RotateDir } from "../../kicad-port/actions3d";
import { setDockColumnCollapsed, useDockLayout } from "../../state/dockLayoutStore";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { ActionIcon } from "../Toolbar";
import { ContextMenu } from "../canvas/ContextMenu";
import { resolvedToolbar, useToolbarSettings } from "../../state/toolbarSettings";
import type { Viewer3DApi } from "./Viewer3D";
import { HOTKEY_TABLE_3D } from "../../actions/hotkeyActions";
import { useUserHotkeysVersion } from "../../state/userHotkeys";
import { openExport3DImage } from "../../state/viewer3dExport";
import { Export3DImageDialog } from "./Export3DImageDialog";
import { setViewer3DPrefs, useViewer3DUserSettings } from "../../state/viewer3dUserSettings";

const file = toolbarData as unknown as Viewer3dToolbarsFile;
const own = new Map<string, Viewer3dAction>([...file.actions, ...(file.otherActions ?? [])].map((a) => [a.name, a]));
const common = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a]));
const keys3d = new Map(HOTKEY_TABLE_3D.map((a) => [a.name, a]));

/** What a toolbar item shows: its label, tooltip, hotkey and KiCad icon, from the 3D action table or, for the shared zoom actions, actions.json. The right-click menu shows the same. */
export function describe(name: string): { label: string; tooltip: string; hotkey: string | null; icon: string | null } {
  const a = own.get(name);
  // the hotkey the person gave it in Preferences > Hotkeys, else KiCad's (`TOOL_ACTION::GetHotKey`; the 3D actions' table src/kicad/actions3d.json)
  const k = keys3d.get(name);
  if (a) return { label: a.label, tooltip: a.tooltip, hotkey: k ? effectiveHotkey(k).hotkey : a.hotkey, icon: a.icon };
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
  useToolbarSettings();
  useUserHotkeysVersion(); // the tooltips' hotkeys follow Preferences > Hotkeys
  const items = resolvedToolbar("3d", "main")?.items ?? file.toolbars[0]!.items;
  const [groupMenu, setGroupMenu] = useState<{ x: number; y: number; items: string[] } | null>(null);

  const prefs3d = useViewer3DUserSettings().prefs;
  const handlers: Record<string, Handler> = {
    "3DViewer.Control.reloadBoard": { run: () => api?.reload(), enabled: ready },
    "3DViewer.Control.copyToClipboard": {
      run: () => void api?.copyImage().then((ok) => toast(ok ? "3D image copied to the clipboard." : "The browser did not allow copying the image to the clipboard.", ok ? "info" : "error")),
      enabled: ready,
    },
    // `ExportImage` with no format: "Export 3D View", then the file (components/viewer3d/Export3DImageDialog.tsx)
    "3DViewer.Control.exportImage": { run: () => openExport3DImage(), enabled: ready },
    "3DViewer.Control.toggleRaytacing": { run: () => {}, enabled: false, reason: "not ported: this viewer has one render engine (KiCad's own render, or the live scene), not a raytracer" },
    // Zoom In / Out step the camera, Zoom to Fit is the home view. Zoom Redraw would repaint a view that repaints every frame already: it stays on the
    // toolbar (KiCad's has it), dimmed with that reason, rather than a button that visibly does nothing.
    "common.Control.zoomRedraw": { run: () => {}, enabled: false, reason: "nothing to do: this view repaints on every frame" },
    "common.Control.zoomInCenter": { run: () => api?.dispatchAction({ kind: "zoomIn" }), enabled: ready },
    "common.Control.zoomOutCenter": { run: () => api?.dispatchAction({ kind: "zoomOut" }), enabled: ready },
    "common.Control.zoomFitScreen": { run: () => api?.setView("reset"), enabled: ready },
    "3DViewer.Control.flipView": { run: () => set({ flipped: !opts.flipped }), enabled: ready, checked: opts.flipped },
    "3DViewer.Control.toggleOrtho": { run: () => set({ orthographic: !opts.orthographic }), enabled: true, checked: opts.orthographic },
    "3DViewer.Control.showLayersManager": { run: () => setDockColumnCollapsed("right", !dock.rightCollapsed), enabled: true, checked: !dock.rightCollapsed },
    // `ToggleShowMissingModels` (a customised toolbar may hold it; UI-LEFT-3 X3-2)
    "3DViewer.Control.toggleShowMissingModels": { run: () => setViewer3DPrefs({ ...prefs3d, showMissingModels: !prefs3d.showMissingModels }), enabled: true, checked: prefs3d.showMissingModels },
  };
  for (const [name, { axis, dir }] of Object.entries(ROTATE)) handlers[name] = { run: () => api?.dispatchAction({ kind: "rotate", axis, dir }), enabled: ready };
  for (const [name, direction] of Object.entries(MOVE)) handlers[name] = { run: () => api?.dispatchAction({ kind: "pan", direction }), enabled: ready };

  return (
    <div className="toolbar" data-toolbar="viewer3d" data-editor="3d">
      {items.map((item, i) => {
        if (item.type === "separator") return <div key={i} className="toolbar-separator" role="separator" />;
        if (item.type === "spacer") return <div key={i} className="toolbar-spacer" aria-hidden data-spacer={item.size} style={{ width: item.size, flexShrink: 0 }} />;
        if (item.type === "control") return null;
        const name = item.type === "group" ? (item.items.find((n) => handlers[n]?.enabled) ?? item.items[0]!) : item.action;
        const h = handlers[name];
        const d = describe(name);
        const enabled = !!h?.enabled;
        const tooltip = enabled ? [d.label, d.hotkey ? displayHotkey(d.hotkey) : null].filter(Boolean).join(" — ") : `${d.label} (${h?.reason ?? "not ported yet"})`;
        const pressed = enabled ? h?.checked : undefined;
        return (
          <button
            key={i}
            className={`toolbar-button${pressed ? " active" : ""}${item.type === "group" ? " has-menu" : ""}`}
            aria-pressed={pressed}
            data-action={name}
            data-group={item.type === "group" ? item.label : undefined}
            disabled={!enabled}
            title={item.type === "group" ? `${tooltip} (${item.label}: right click for the others)` : tooltip}
            onClick={() => h?.run()}
            onContextMenu={
              item.type === "group"
                ? (e) => {
                    e.preventDefault();
                    const r = e.currentTarget.getBoundingClientRect();
                    setGroupMenu({ x: r.left, y: r.bottom, items: item.items });
                  }
                : undefined
            }
          >
            <ActionIcon iconName={d.icon} />
          </button>
        );
      })}
      <Export3DImageDialog api={api} />
      {groupMenu && (
        <ContextMenu
          x={groupMenu.x}
          y={groupMenu.y}
          onClose={() => setGroupMenu(null)}
          entries={groupMenu.items.map((n) => ({ label: describe(n).label, icon: <ActionIcon iconName={describe(n).icon} size={16} />, disabled: !handlers[n]?.enabled, onSelect: () => handlers[n]?.run() }))}
        />
      )}
    </div>
  );
}
