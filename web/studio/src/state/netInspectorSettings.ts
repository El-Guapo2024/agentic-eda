// The Net Inspector's settings (`PANEL_NET_INSPECTOR_SETTINGS`, kicad-port/netInspector.ts): KiCad keeps them in the project's local settings (`.kicad_prl`'s
// `net_inspector_panel`), the panel saving them on each change (`SaveSettings` from `onContextMenuSelection`, `OnSearchTextChanged`, `OnColumnSorted`,
// `OnExpandCollapseRow`). The studio keeps them with the person's other options, in the browser's local storage (like state/commonOptions.ts and the dock
// layout): they are how one person likes the panel, not part of the design, so they never enter `design.json`, undo or `appearance.json`.
import { useSyncExternalStore } from "react";
import { DEFAULT_NET_INSPECTOR_SETTINGS, parseNetInspectorSettings, type NetInspectorSettings } from "../kicad-port/netInspector";

const STORAGE_KEY = "eda-studio.net-inspector.v1";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

function load(): NetInspectorSettings {
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    return parseNetInspectorSettings(raw ? JSON.parse(raw) : null);
  } catch {
    return DEFAULT_NET_INSPECTOR_SETTINGS;
  }
}

let current: NetInspectorSettings = load();
const listeners = new Set<() => void>();

export function getNetInspectorSettings(): NetInspectorSettings {
  return current;
}

/** `SaveSettings`: the new settings, kept at once. */
export function setNetInspectorSettings(next: NetInspectorSettings): void {
  if (next === current) return;
  current = next;
  try {
    storage()?.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    /* private window or blocked site data: the settings still apply for this session */
  }
  for (const l of [...listeners]) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useNetInspectorSettings(): NetInspectorSettings {
  return useSyncExternalStore(subscribe, getNetInspectorSettings, getNetInspectorSettings);
}
