// The 3D viewer's view-preset toolbar: Top/Bottom/Front/Back/Left/Right/
// Iso/Reset, exact KiCad 3D-viewer semantics (see scene.ts's
// PRESET_DIRECTIONS comment). App.tsx mounts this in place of the
// normal <Toolbar id="main"/> for the "3d" tab (main-toolbar-row) --
// this app's own equivalent of KiCad's 3D viewer having its own toolbar
// rather than the PCB editor's; 3d-viewer/'s source wasn't available
// this session, so button order/labels are this app's own reasonable
// choice, not a pixel-accurate port.
//
// Wired to Viewer3D's camera through a small imperative handle
// (`Viewer3DApi`, defined in ./Viewer3D) rather than a shared
// prop-callback/context: Viewer3D creates the handle once its
// camera/controls exist and hands it to App.tsx via `onReady`; App.tsx
// passes it straight through here as `api`. Buttons are disabled until
// `api` is non-null (i.e. until Viewer3D has actually mounted its
// Three.js scene).
//
// Root element reuses this app's existing ".toolbar" class (same one
// Toolbar.tsx's own root uses) so it sits in main-toolbar-row with the
// same background/border/height as every other toolbar; the buttons
// themselves are plain text labels (no icon set exists for camera
// presets) styled inline + with this app's --chrome-* custom
// properties, since this task may only create/edit the three viewer3d/
// files, not add a new stylesheet.
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { Viewer3DApi } from "./Viewer3D";
import type { ViewPreset } from "./scene";

const PRESET_BUTTONS: ReadonlyArray<{ preset: ViewPreset; label: string }> = [
  { preset: "top", label: "Top" },
  { preset: "bottom", label: "Bottom" },
  { preset: "front", label: "Front" },
  { preset: "back", label: "Back" },
  { preset: "left", label: "Left" },
  { preset: "right", label: "Right" },
  { preset: "iso", label: "Iso" },
  { preset: "reset", label: "Reset" },
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

/**
 * View presets (KiCad 3D-viewer semantics, see scene.ts's
 * PRESET_DIRECTIONS comment) plus KiCad's real 3D-viewer view-option
 * toggles: hide silkscreen / solder mask / the rendered component
 * models, flip the board to see the other side, and an orthographic/
 * perspective projection switch (3d-viewer/'s own source wasn't
 * available this session -- button grouping/labels are this app's own
 * reasonable choice, not a pixel-accurate port of its toolbar). The
 * toggles live in the global store (state.viewer3d) since Viewer3D
 * itself (not just this toolbar) needs to react to them; the view-preset
 * buttons stay wired through the imperative Viewer3DApi handle, same as
 * before.
 */
export function Viewer3DToolbar({ api }: { api: Viewer3DApi | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const opts = state.viewer3d;
  const set = (patch: Partial<typeof opts>) => dispatch({ type: "SET_VIEWER3D_OPTIONS", options: patch });

  return (
    <div className="toolbar" data-toolbar="viewer3d" style={{ display: "flex" }}>
      {PRESET_BUTTONS.map(({ preset, label }) => (
        <ToolbarButton key={preset} label={label} title={`${label} view`} enabled={!!api} onClick={() => api?.setView(preset)} />
      ))}
      <Sep />
      <ToolbarButton label="Flip Board" title="View the board flipped to the other side" active={opts.flipped} onClick={() => set({ flipped: !opts.flipped })} />
      <ToolbarButton label={opts.orthographic ? "Ortho" : "Persp"} title="Toggle orthographic / perspective projection" active={opts.orthographic} onClick={() => set({ orthographic: !opts.orthographic })} />
      <Sep />
      <ToolbarButton label="Silkscreen" title="Show/hide silkscreen" active={opts.showSilkscreen} onClick={() => set({ showSilkscreen: !opts.showSilkscreen })} />
      <ToolbarButton label="Solder Mask" title="Show/hide solder mask" active={opts.showSolderMask} onClick={() => set({ showSolderMask: !opts.showSolderMask })} />
      <ToolbarButton label="Components" title="Show/hide the rendered component models" active={opts.showComponents} onClick={() => set({ showComponents: !opts.showComponents })} />
      <Sep />
      <ToolbarButton
        label="KiCad Models"
        title="Show GET /api/board.glb's real KiCad render (actual 3D models, kicad-cli's own colors) instead of this app's own procedural scene. Falls back to the procedural scene on its own if the GLB hasn't loaded."
        active={opts.kicadModels}
        onClick={() => set({ kicadModels: !opts.kicadModels })}
      />
    </div>
  );
}
