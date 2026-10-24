// The router settings across reloads: KiCad keeps `ROUTING_SETTINGS` in pcbnew.json (`tools.pns`); the studio has no settings file of
// its own to write, so they are this browser's (localStorage, a per-viewer convenience, like the grid and the common options).
// The format is kicad-port/routerSettings.ts's `parseRouterSettings` / `serializeRouterSettings`.
import { DEFAULT_ROUTER_SETTINGS, parseRouterSettings, serializeRouterSettings, type RouterSettings } from "../kicad-port/routerSettings";

const STORAGE_KEY = "eda-studio.router-settings";

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** The settings a previous session saved, or the defaults. */
export function loadRouterSettings(): RouterSettings {
  try {
    return parseRouterSettings(storage()?.getItem(STORAGE_KEY) ?? null);
  } catch {
    return DEFAULT_ROUTER_SETTINGS;
  }
}

/** Keeps `s` for the next session. */
export function saveRouterSettings(s: RouterSettings): void {
  try {
    storage()?.setItem(STORAGE_KEY, serializeRouterSettings(s));
  } catch {
    // private window or blocked storage: the settings last as long as the page
  }
}
