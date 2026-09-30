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

export function Viewer3DToolbar({ api }: { api: Viewer3DApi | null }) {
  return (
    <div className="toolbar" data-toolbar="viewer3d">
      {PRESET_BUTTONS.map(({ preset, label }) => (
        <button
          key={preset}
          type="button"
          disabled={!api}
          onClick={() => api?.setView(preset)}
          title={`${label} view`}
          style={{
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
            cursor: api ? "default" : "not-allowed",
            flexShrink: 0,
          }}
          onMouseEnter={(e) => {
            if (api) e.currentTarget.style.background = "var(--chrome-hover)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
          }}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
