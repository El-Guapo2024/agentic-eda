// Preferences (`common.SuiteControl.openPreferences`, Ctrl+, -- `PANEL_MOUSE_SETTINGS`, "Mouse and Touchpad",
// common/dialogs/panel_mouse_settings.cpp) and the settings it edits (`COMMON_SETTINGS::m_Input`,
// common/settings/common_settings.cpp).
//
// Only the settings a canvas here actually honours are offered -- a control with no effect would be a fake:
//   * the scroll-wheel gestures: which modifier key (none / Ctrl / Shift / Alt) zooms, pans left/right and pans
//     up/down, one action per column, and whether zoom and left/right pan run reversed;
//   * "Automatically pan while moving object" and its speed (`input.auto_pan`, `input.auto_pan_acceleration`);
//   * "Use zoom acceleration" and the zoom speed with its "Automatic" switch (`input.zoom_acceleration`,
//     `input.zoom_speed`, `input.zoom_speed_auto` -> `WX_VIEW_CONTROLS::LoadSettings`);
//   * the board editor's magnetic points (`PCBNEW_SETTINGS::m_MagneticItems`, "Snap to Pads", "Snap to Tracks and Vias", "Snap to graphics": the board editor's
//     Preferences > Editing Options), which decide what the drawing and move tools snap to (kicad-port/snapAnchors.ts).
// Not offered: "Center and warp cursor on zoom" (it moves the real pointer, which a page may not and this studio
// never does), the drag gestures and "Pan on mouse movement with key" (the canvases' own drag handling is not
// configurable), and "Pan left/right with horizontal movement" (stored by KiCad but never read by the wheel code).
import { DEFAULT_VIEW_CONTROL_SETTINGS, type ScrollModifier, type ViewControlSettings } from "./viewControls";
import type { MagneticOption, PcbMagnetic } from "./snapAnchors";
import { AcceleratingZoomController, CONSTANT_SCALE, ConstantZoomController, pickDefaultZoomController, type ZoomController } from "./zoomController";

export type { ScrollModifier };
export type ScrollRow = "zoom" | "panH" | "panV";

export interface Preferences {
  /** `input.scroll_modifier_zoom` / `_pan_h` / `_pan_v`: the key held while scrolling for each action. */
  scrollModifierZoom: ScrollModifier;
  scrollModifierPanH: ScrollModifier;
  scrollModifierPanV: ScrollModifier;
  /** `input.reverse_scroll_zoom` / `reverse_scroll_pan_h`. */
  reverseScrollZoom: boolean;
  reverseScrollPanH: boolean;
  /** `input.auto_pan`, default off. */
  autoPan: boolean;
  /** `input.auto_pan_acceleration`, 1..10, default 5. */
  autoPanAcceleration: number;
  /** `input.zoom_acceleration`, default off (the platform controller ignores it on macOS, as in KiCad). */
  zoomAcceleration: boolean;
  /** `input.zoom_speed_auto`, default on; `input.zoom_speed` 1..10, default 5, used when it is off. */
  zoomSpeedAuto: boolean;
  zoomSpeed: number;
  /** `MAGNETIC_SETTINGS::pads`: snap to pads never / only while routing / always -- default "in track tool". */
  magneticPads: MagneticOption;
  /** `MAGNETIC_SETTINGS::tracks`: tracks and vias -- default "in track tool". */
  magneticTracks: MagneticOption;
  /** `MAGNETIC_SETTINGS::graphics`: snap to graphics -- default off. */
  magneticGraphics: boolean;
}

/** `COMMON_SETTINGS` factory defaults (common_settings.cpp `input.*`): wheel zooms, Ctrl pans left/right, Shift pans up/down. */
export const DEFAULT_PREFERENCES: Preferences = {
  scrollModifierZoom: "none",
  scrollModifierPanH: "ctrl",
  scrollModifierPanV: "shift",
  reverseScrollZoom: false,
  reverseScrollPanH: false,
  autoPan: false,
  autoPanAcceleration: 5,
  zoomAcceleration: false,
  zoomSpeedAuto: true,
  zoomSpeed: 5,
  magneticPads: "track-tool",
  magneticTracks: "track-tool",
  magneticGraphics: false,
};

type ScrollSet = Pick<Preferences, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV" | "reverseScrollZoom" | "reverseScrollPanH">;

/** `onMouseDefaults`: the wheel zooms, Ctrl pans left/right, Shift pans up/down, nothing reversed. */
export const MOUSE_SCROLL_DEFAULTS: ScrollSet = { scrollModifierZoom: "none", scrollModifierPanH: "ctrl", scrollModifierPanV: "shift", reverseScrollZoom: false, reverseScrollPanH: false };
/** `onTrackpadDefaults`: Ctrl+scroll (pinch) zooms, Shift pans left/right, the plain two-finger scroll pans up/down. */
export const TRACKPAD_SCROLL_DEFAULTS: ScrollSet = { scrollModifierZoom: "ctrl", scrollModifierPanH: "shift", scrollModifierPanV: "none", reverseScrollZoom: false, reverseScrollPanH: false };

const KEYS: Record<ScrollRow, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV"> = { zoom: "scrollModifierZoom", panH: "scrollModifierPanH", panV: "scrollModifierPanV" };
const ROWS: readonly ScrollRow[] = ["zoom", "panH", "panV"];
/** `assign_first_available`'s candidate order. */
const CANDIDATES: readonly ScrollModifier[] = ["none", "ctrl", "shift", "alt"];

/** `isScrollModSetValid`: each action on its own column. */
export function scrollModSetValid(p: Pick<Preferences, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV">): boolean {
  return p.scrollModifierZoom !== p.scrollModifierPanH && p.scrollModifierPanH !== p.scrollModifierPanV && p.scrollModifierPanV !== p.scrollModifierZoom;
}

/**
 * `OnScrollRadioButton`: the user picks `modifier` for `row`. A Ctrl/Shift/Alt column can hold one action only, so
 * any other row that held `modifier` moves to the first column nobody holds (none, Ctrl, Shift, Alt in that order).
 * Picking "none" for a row leaves the others alone -- a clash of two "none"s is reported by `scrollModSetValid`.
 */
export function assignScrollModifier(prefs: Preferences, row: ScrollRow, modifier: ScrollModifier): Preferences {
  const next: Preferences = { ...prefs, [KEYS[row]]: modifier };
  if (modifier === "none") return next;
  for (const other of ROWS) {
    if (other === row || next[KEYS[other]] !== modifier) continue;
    const taken = (c: ScrollModifier) => ROWS.some((r) => next[KEYS[r]] === c);
    const free = CANDIDATES.find((c) => c !== modifier && !taken(c));
    if (free) next[KEYS[other]] = free;
  }
  return next;
}

/** The part of the settings `handleWheel` / the auto-pan loop read. */
export function toViewControlSettings(p: Preferences): ViewControlSettings {
  return {
    ...DEFAULT_VIEW_CONTROL_SETTINGS,
    autoPanEnabled: p.autoPan,
    autoPanAcceleration: p.autoPanAcceleration,
    scrollModifierZoom: p.scrollModifierZoom,
    scrollModifierPanH: p.scrollModifierPanH,
    reverseScrollZoom: p.reverseScrollZoom,
    reverseScrollPanH: p.reverseScrollPanH,
  };
}

/**
 * `WX_VIEW_CONTROLS::LoadSettings`: automatic speed -> the platform's controller (zoom acceleration only matters off macOS);
 * a manual speed -> an accelerating controller scaled by it, or a constant one at `zoom_speed * MANUAL_SCALE_FACTOR`.
 */
export function zoomControllerFor(p: Preferences, isMacPlatform: boolean): ZoomController {
  if (p.zoomSpeedAuto) return pickDefaultZoomController(isMacPlatform, p.zoomAcceleration);
  if (p.zoomAcceleration) return new AcceleratingZoomController(p.zoomSpeed);
  return new ConstantZoomController(CONSTANT_SCALE.MANUAL_FACTOR * p.zoomSpeed);
}

// ---------------------------------------------------------------------------------------------- persistence

export const PREFERENCES_STORAGE_KEY = "eda-studio.preferences.v1";

const MODIFIERS: readonly ScrollModifier[] = ["none", "ctrl", "shift", "alt"];
const isModifier = (v: unknown): v is ScrollModifier => typeof v === "string" && (MODIFIERS as readonly string[]).includes(v);
const clampInt = (v: unknown, lo: number, hi: number, fallback: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : fallback);
const bool = (v: unknown, fallback: boolean) => (typeof v === "boolean" ? v : fallback);
const magnetic = (v: unknown, fallback: MagneticOption): MagneticOption => (v === "never" || v === "track-tool" || v === "always" ? v : fallback);

/** The magnetic settings the board editor's snapping reads, with `allLayers` (the toggle of `common.Control.magneticSnapToggle`, Shift+S, kept in the studio state). */
export function toPcbMagnetic(p: Pick<Preferences, "magneticPads" | "magneticTracks" | "magneticGraphics">, allLayers: boolean): PcbMagnetic {
  return { pads: p.magneticPads, tracks: p.magneticTracks, graphics: p.magneticGraphics, allLayers };
}

/** Whatever was stored, made into valid preferences: unknown or malformed fields fall back to their defaults, a clashing wheel assignment to the defaults. */
export function sanitizePreferences(raw: unknown): Preferences {
  const o = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_PREFERENCES;
  const p: Preferences = {
    scrollModifierZoom: isModifier(o.scrollModifierZoom) ? o.scrollModifierZoom : d.scrollModifierZoom,
    scrollModifierPanH: isModifier(o.scrollModifierPanH) ? o.scrollModifierPanH : d.scrollModifierPanH,
    scrollModifierPanV: isModifier(o.scrollModifierPanV) ? o.scrollModifierPanV : d.scrollModifierPanV,
    reverseScrollZoom: bool(o.reverseScrollZoom, d.reverseScrollZoom),
    reverseScrollPanH: bool(o.reverseScrollPanH, d.reverseScrollPanH),
    autoPan: bool(o.autoPan, d.autoPan),
    autoPanAcceleration: clampInt(o.autoPanAcceleration, 1, 10, d.autoPanAcceleration),
    zoomAcceleration: bool(o.zoomAcceleration, d.zoomAcceleration),
    zoomSpeedAuto: bool(o.zoomSpeedAuto, d.zoomSpeedAuto),
    zoomSpeed: clampInt(o.zoomSpeed, 1, 10, d.zoomSpeed),
    magneticPads: magnetic(o.magneticPads, d.magneticPads),
    magneticTracks: magnetic(o.magneticTracks, d.magneticTracks),
    magneticGraphics: bool(o.magneticGraphics, d.magneticGraphics),
  };
  return scrollModSetValid(p) ? p : { ...p, ...MOUSE_SCROLL_DEFAULTS };
}

/** The slice of `Storage` used, so tests can pass a plain object. */
export interface PreferencesStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** The saved preferences, or the defaults when nothing usable is saved or storage is unavailable (private window, blocked site data). */
export function loadPreferences(storage: PreferencesStorage | null | undefined): Preferences {
  try {
    const text = storage?.getItem(PREFERENCES_STORAGE_KEY);
    return text ? sanitizePreferences(JSON.parse(text)) : DEFAULT_PREFERENCES;
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function savePreferences(storage: PreferencesStorage | null | undefined, prefs: Preferences): void {
  try {
    storage?.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    /* storage unavailable: the preferences still apply for this session */
  }
}
