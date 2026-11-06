// The 3D viewer's settings that are the person's, not the project's: its layer presets (`3d_viewer.json`'s `layer_presets`) and its preferences (`render.*`, `camera.*`;
// Preferences > 3D Viewer). KiCad keeps them in the user's settings directory; the studio has no settings file for them, so they are this browser's (localStorage, a
// per-viewer convenience, as `state/designBlocks.ts` keeps its own). Plain module state with subscribers: the viewer, the Appearance manager and the Preferences dialog
// read the same object. The saved viewports are the project's and live in `appearance.json` (kicad-port/viewer3dSettings.ts).
import { useSyncExternalStore } from "react";
import { DEFAULT_VIEWER3D_PREFS, legacyColorsPreset, presetFromJson, sanitizeViewer3DPrefs, type LayerPreset3D, type Viewer3DPrefs } from "../kicad-port/viewer3dSettings";

export interface Viewer3DUserSettings {
  prefs: Viewer3DPrefs;
  presets: readonly LayerPreset3D[];
}

const STORAGE_KEY = "eda-studio.viewer3d";

function load(): Viewer3DUserSettings {
  let saved: { prefs?: unknown; presets?: unknown } | null = null;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    saved = raw ? (JSON.parse(raw) as { prefs?: unknown; presets?: unknown }) : null;
  } catch {
    // no storage (private window, blocked site data): the defaults
  }
  // `EDA_3D_VIEWER_FRAME::LoadSettings`: the first time (`m_CurrentPreset == LEGACY_PRESET_FLAG`) the "legacy colors" preset is added to the person's presets.
  const presets = saved && Array.isArray(saved.presets) ? saved.presets.map(presetFromJson).filter((p): p is LayerPreset3D => p !== null) : [legacyColorsPreset()];
  return { prefs: sanitizeViewer3DPrefs(saved?.prefs ?? DEFAULT_VIEWER3D_PREFS), presets };
}

let current: Viewer3DUserSettings = load();
const listeners = new Set<() => void>();

function save(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ prefs: current.prefs, presets: current.presets }));
  } catch {
    // ignore: it is a convenience
  }
}

export function getViewer3DUserSettings(): Viewer3DUserSettings {
  return current;
}

export function setViewer3DPrefs(prefs: Viewer3DPrefs): void {
  current = { ...current, prefs: sanitizeViewer3DPrefs(prefs) };
  save();
  for (const l of [...listeners]) l();
}

export function setViewer3DPresets(presets: readonly LayerPreset3D[]): void {
  current = { ...current, presets: [...presets] };
  save();
  for (const l of [...listeners]) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useViewer3DUserSettings(): Viewer3DUserSettings {
  return useSyncExternalStore(subscribe, getViewer3DUserSettings, getViewer3DUserSettings);
}
