// The 3D viewer's toolbar. Two groups of buttons:
//  - KiCad's own 3D-viewer toolbar (3d_viewer/toolbars_3d.cpp's
//    DefaultToolbarConfig, TOP_MAIN): Zoom Fit/In/Out, Rotate X/Y/Z
//    CW/CCW, Flip, Move Left/Right/Up/Down, toggle Orthographic -- same
//    order as source, same KiCad keyboard shortcut (if any) in the
//    tooltip. A few of source's own buttons are left out because they
//    have no equivalent in this app at all (Reload board -- this app's
//    board state already polls and rebuilds live; Copy/Export image, and
//    Toggle Raytracing -- there is only one render engine here; Show
//    Appearance Manager -- deferred to the Appearance work, see
//    PARITY-3d.md); "Zoom Redraw" is also left off since it is a true
//    no-op in this app (no render cache to invalidate -- see
//    runAction3D's own comment in Viewer3D.tsx) and a button that visibly
//    does nothing is worse than no button.
//  - This app's own additions: the 6 face-view preset buttons (real
//    KiCad's interactive 3D viewer has no toolbar button for these at
//    all -- only a hotkey or its right-click context menu reach them;
//    kept here anyway for a mouse-first web UI, see PARITY-3d.md) and the
//    visibility/KiCad-models toggles (pre-existing; their own full
//    Preferences/Appearance-manager-equivalent port is the Appearance
//    phase's job, not this one).
//
// Wired to Viewer3D's camera through a small imperative handle
// (`Viewer3DApi`, defined in ./Viewer3D) rather than a shared
// prop-callback/context: Viewer3D creates the handle once its camera
// exists and hands it to App.tsx via `onReady`; App.tsx passes it
// straight through here as `api`. Buttons are disabled until `api` is
// non-null (i.e. until Viewer3D has actually mounted its Three.js scene).
//
// Root element reuses this app's existing ".toolbar" class (same one
// Toolbar.tsx's own root uses) so it sits in main-toolbar-row with the
// same background/border/height as every other toolbar; the buttons
// themselves are plain text labels (no icon set exists for camera
// actions) styled inline + with this app's --chrome-* custom properties,
// since this task may only touch viewer3d/'s own files, not add a new
// stylesheet.
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { RotateAxis } from "../../kicad-port/actions3d";
import type { Viewer3DApi, ViewPreset } from "./Viewer3D";

// KiCad's own hotkeys for these (eda_3d_viewer/tools/eda_3d_actions.cpp):
// Y/Shift+Y (front/back), X/Shift+X (right/left), Z/Shift+Z (top/bottom),
// Home (reset/"home view" -- observably identical to Top, see
// camera3d.ts's reset() doc comment). Real KiCad has no "Iso" action at
// all in the interactive 3D viewer toolbar/menu/hotkeys -- dropped here
// too (see PARITY-3d.md); these 7 buttons are this app's own UI addition
// for mouse-only discoverability (KiCad reaches the 6 face views only via
// hotkey or its right-click context menu, neither of which this toolbar
// replaces).
const PRESET_BUTTONS: ReadonlyArray<{ preset: ViewPreset; label: string; title: string }> = [
  { preset: "top", label: "Top", title: "View Top (Z)" },
  { preset: "bottom", label: "Bottom", title: "View Bottom (Shift+Z)" },
  { preset: "front", label: "Front", title: "View Front (Y)" },
  { preset: "back", label: "Back", title: "View Back (Shift+Y)" },
  { preset: "left", label: "Left", title: "View Left (Shift+X)" },
  { preset: "right", label: "Right", title: "View Right (X)" },
  { preset: "reset", label: "Reset", title: "Home View (Home)" },
];

/** toolbars_3d.cpp's Rotate X/Y/Z CW/CCW buttons -- no default hotkey for any of these (eda_3d_actions.cpp), toolbar/context-menu only, same here. */
const ROTATE_BUTTONS: ReadonlyArray<{ axis: RotateAxis; dir: "cw" | "ccw"; label: string; title: string }> = [
  { axis: "x", dir: "cw", label: "X↻", title: "Rotate X Clockwise" },
  { axis: "x", dir: "ccw", label: "X↺", title: "Rotate X Counterclockwise" },
  { axis: "y", dir: "cw", label: "Y↻", title: "Rotate Y Clockwise" },
  { axis: "y", dir: "ccw", label: "Y↺", title: "Rotate Y Counterclockwise" },
  { axis: "z", dir: "cw", label: "Z↻", title: "Rotate Z Clockwise (Shift+R)" },
  { axis: "z", dir: "ccw", label: "Z↺", title: "Rotate Z Counterclockwise (R)" },
];

/** toolbars_3d.cpp's Move Left/Right/Up/Down buttons -- same keys as the 4 arrow keys (eda_3d_actions.cpp's moveLeft/Right/Up/Down). */
const MOVE_BUTTONS: ReadonlyArray<{ direction: "left" | "right" | "up" | "down"; label: string; title: string }> = [
  { direction: "left", label: "←", title: "Move Board Left (←)" },
  { direction: "right", label: "→", title: "Move Board Right (→)" },
  { direction: "up", label: "↑", title: "Move Board Up (↑)" },
  { direction: "down", label: "↓", title: "Move Board Down (↓)" },
];

const BUTTON_STYLE: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  height: 30,
  padding: "0 10px",
  border: "1px solid transparent",
  borderRadius: 5,
  background: "transparent",
  color: "var(--chrome-text)",
  font: "12px/1 inherit",
  flexShrink: 0,
};

function ToolbarButton({ label, title, active, enabled = true, onClick }: { label: string; title: string; active?: boolean; enabled?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      disabled={!enabled}
      onClick={onClick}
      title={title}
      style={{
        ...BUTTON_STYLE,
        cursor: enabled ? "default" : "not-allowed",
        background: active ? "var(--chrome-hover)" : "transparent",
        border: active ? "1px solid var(--chrome-border-strong, #666)" : "1px solid transparent",
      }}
      onMouseEnter={(e) => {
        if (enabled) e.currentTarget.style.background = "var(--chrome-hover)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = active ? "var(--chrome-hover)" : "transparent";
      }}
    >
      {label}
    </button>
  );
}

/** A thin vertical rule separating button groups, matching KiCad's own toolbar separators. */
function Sep() {
  return <div style={{ width: 1, alignSelf: "stretch", margin: "4px 4px", background: "var(--chrome-border, #444)", flexShrink: 0 }} />;
}

export function Viewer3DToolbar({ api }: { api: Viewer3DApi | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const opts = state.viewer3d;
  const set = (patch: Partial<typeof opts>) => dispatch({ type: "SET_VIEWER3D_OPTIONS", options: patch });
  const run = (action: Parameters<Viewer3DApi["dispatchAction"]>[0]) => api?.dispatchAction(action);
  // Quiet failure per this route's design (Viewer3D.tsx's fetch effect):
  // no popup, no repeated retries -- just this toggle's own tooltip
  // saying the last attempt didn't work, so it's discoverable on hover
  // without being an interruption. Truncated: a raw kicad-cli stderr
  // capture (studio.rs's build_glb) can be long.
  const kicadModelsTitle =
    state.glbStatus === "failed"
      ? `Show KiCad's real render instead of the procedural scene. The last export failed, so the procedural scene is shown instead: ${(state.glbError ?? "unknown error").slice(0, 300)}`
      : "Show GET /api/board.glb's real KiCad render (actual 3D models, kicad-cli's own colors) instead of this app's own procedural scene. Falls back to the procedural scene on its own if the GLB hasn't loaded.";

  return (
    <div className="toolbar" data-toolbar="viewer3d" style={{ display: "flex" }}>
      {PRESET_BUTTONS.map(({ preset, label, title }) => (
        <ToolbarButton key={preset} label={label} title={title} enabled={!!api} onClick={() => api?.setView(preset)} />
      ))}
      <Sep />
      <ToolbarButton label="Zoom In" title="Zoom In (F1)" enabled={!!api} onClick={() => run({ kind: "zoomIn" })} />
      <ToolbarButton label="Zoom Out" title="Zoom Out (F2)" enabled={!!api} onClick={() => run({ kind: "zoomOut" })} />
      <Sep />
      {ROTATE_BUTTONS.map(({ axis, dir, label, title }) => (
        <ToolbarButton key={`${axis}${dir}`} label={label} title={title} enabled={!!api} onClick={() => run({ kind: "rotate", axis, dir })} />
      ))}
      <Sep />
      <ToolbarButton label="Flip Board" title="View the board flipped to the other side (F)" active={opts.flipped} onClick={() => set({ flipped: !opts.flipped })} />
      <Sep />
      {MOVE_BUTTONS.map(({ direction, label, title }) => (
        <ToolbarButton key={direction} label={label} title={title} enabled={!!api} onClick={() => run({ kind: "pan", direction })} />
      ))}
      <Sep />
      <ToolbarButton label={opts.orthographic ? "Ortho" : "Persp"} title="Toggle orthographic / perspective projection" active={opts.orthographic} onClick={() => set({ orthographic: !opts.orthographic })} />
      <Sep />
      <ToolbarButton label="Silkscreen" title="Show/hide silkscreen" active={opts.showSilkscreen} onClick={() => set({ showSilkscreen: !opts.showSilkscreen })} />
      <ToolbarButton label="Solder Mask" title="Show/hide solder mask" active={opts.showSolderMask} onClick={() => set({ showSolderMask: !opts.showSolderMask })} />
      <ToolbarButton label="Components" title="Show/hide the rendered component models" active={opts.showComponents} onClick={() => set({ showComponents: !opts.showComponents })} />
      <Sep />
      <ToolbarButton label="KiCad Models" title={kicadModelsTitle} active={opts.kicadModels} onClick={() => set({ kicadModels: !opts.kicadModels })} />
    </div>
  );
}
