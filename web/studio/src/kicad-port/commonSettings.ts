// Preferences > Common (`PANEL_COMMON_SETTINGS`, common/dialogs/panel_common_settings.cpp and its _base at KiCad 8303b2ad) and the `COMMON_SETTINGS` it edits
// (common/settings/common_settings.cpp). The studio keeps the rows a browser page can honour and shows the others greyed out with the reason as their tooltip:
//
//   honoured   Icon theme: Light / Dark / Automatic (`appearance.icon_theme`, default AUTO: the icon set follows the system's light or dark mode); Toolbar icon size:
//              Small 16 / Normal 24 / Large 32 (`appearance.toolbar_icon_size`, default 24); "Use alternating row colors in tables" (`appearance.grid_striping`,
//              default off); "High-contrast mode dimming factor:" (`appearance.hicontrast_dimming_factor`, default 0.8, in percent on the page: the board's High
//              Contrast Mode mixes the inactive layers `1 - factor` of the way back to the background, `PCB_RENDER_SETTINGS::LoadColors`).
//   greyed     the Rendering Engine (the studio draws with the browser's canvas), Antialiasing (the browser's canvas always antialiases), the Helper Applications
//              (a page cannot start programs), Show icons in menus (the studio's menus are drawn without icons), Show scrollbars in editors (its canvases have
//              none), Focus follows mouse between schematic and PCB editors (the editors are tabs of one window), the hotkey popup indicator (`HOTKEY_CYCLE_POPUP`
//              is not ported), Disable custom cursors (the canvases use the browser's cursors), Warp mouse to anchor of moved object (a page cannot move the
//              pointer), First hotkey selects tool (the studio's hotkeys always act at once, KiCad's default), Session (no project manager re-opens files), and
//              Project Backup (the studio keeps every edit in the board's own `.history`, the undo history, not KiCad's backup archives).
//   not shown  as KiCad hides them: the canvas scale (`m_AllowManualCanvasScale` is off by default), "Apply icon scaling to fonts" (`m_scaleFonts->Show( false )`)
//              and the App theme (Windows only).
//
// Pure: state/commonOptions.ts keeps them in the browser's local storage with the other per-person options.

/** `ICON_THEME`. */
export type IconTheme = "light" | "dark" | "auto";

/** The `COMMON_SETTINGS` the studio honours. */
export interface CommonSettings {
  /** `appearance.icon_theme`, default AUTO. */
  iconTheme: IconTheme;
  /** `appearance.toolbar_icon_size`: 16, 24 (default) or 32 on the page (the param takes 16 .. 64). */
  toolbarIconSize: number;
  /** `appearance.grid_striping`: every other table row in another colour. Default off. */
  gridStriping: boolean;
  /** `appearance.hicontrast_dimming_factor`, 0 .. 1 (default 0.8). */
  hicontrastDimmingFactor: number;
}

export const DEFAULT_COMMON_SETTINGS: CommonSettings = { iconTheme: "auto", toolbarIconSize: 24, gridStriping: false, hicontrastDimmingFactor: 0.8 };

/** The page's three toolbar icon sizes (`m_rbIconSizeSmall` 16, `m_rbIconSizeNormal` 24, `m_rbIconSizeLarge` 32). */
export const TOOLBAR_ICON_SIZES: ReadonlyArray<{ size: number; label: string; tip: string }> = [
  { size: 16, label: "Small", tip: "Use compact icons in the toolbars" },
  { size: 24, label: "Normal", tip: "Use the default KiCad icon size in the toolbars" },
  { size: 32, label: "Large", tip: "Use larger icons in the toolbars" },
];

/** `PARAM<int>( "appearance.toolbar_icon_size", .., 24, 16, 64 )`: out of range or not a number is the default. */
export function sanitizeToolbarIconSize(v: unknown): number {
  return typeof v === "number" && Number.isFinite(v) && v >= 16 && v <= 64 ? Math.round(v) : DEFAULT_COMMON_SETTINGS.toolbarIconSize;
}

/**
 * The dimming factor as typed on the page (`TransferDataFromWindow`: `double dimmingPercent = 80; m_highContrastCtrl->GetValue().ToDouble( &dimmingPercent );
 * hicontrast_dimming_factor = dimmingPercent / 100.0f`): a text that is not a number keeps 80 %. KiCad does not clamp it; a factor outside 0 .. 1 makes the
 * painter's mix go past the background, so the studio keeps it inside (`PCB_RENDER_SETTINGS::LoadColors` takes `1.0f - factor`).
 */
export function dimmingFactorFromPercentText(text: string): number {
  const t = text.trim();
  const n = t === "" ? NaN : Number(t);
  const pct = Number.isFinite(n) ? n : 80;
  return Math.min(1, Math.max(0, pct / 100));
}

/** `applySettingsToPanel`: `wxString::Format( "%.0f", hicontrast_dimming_factor * 100.0f )`. */
export function dimmingPercentText(factor: number): string {
  return (factor * 100).toFixed(0);
}

/** `PCB_RENDER_SETTINGS::m_hiContrastFactor`: `1.0f - hicontrast_dimming_factor`. */
export function hiContrastFactorOf(s: Pick<CommonSettings, "hicontrastDimmingFactor">): number {
  return 1 - s.hicontrastDimmingFactor;
}

/** The icon set to draw (`ICON_THEME::AUTO` follows the system: `KIPLATFORM::UI::IsDarkTheme`). */
export function iconSchemeOf(theme: IconTheme, systemScheme: "light" | "dark"): "light" | "dark" {
  return theme === "auto" ? systemScheme : theme;
}

/** What a previous session kept, made valid: anything missing or malformed is its default. */
export function sanitizeCommonSettings(raw: unknown): CommonSettings {
  const o = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_COMMON_SETTINGS;
  return {
    iconTheme: o.iconTheme === "light" || o.iconTheme === "dark" || o.iconTheme === "auto" ? o.iconTheme : d.iconTheme,
    toolbarIconSize: sanitizeToolbarIconSize(o.toolbarIconSize),
    gridStriping: typeof o.gridStriping === "boolean" ? o.gridStriping : d.gridStriping,
    hicontrastDimmingFactor: typeof o.hicontrastDimmingFactor === "number" && Number.isFinite(o.hicontrastDimmingFactor) ? Math.min(1, Math.max(0, o.hicontrastDimmingFactor)) : d.hicontrastDimmingFactor,
  };
}

/** The greyed rows' reasons (the page's tooltips). */
export const COMMON_ROW_NOT_OFFERED = {
  rendering: "Not offered here: the studio draws with the browser's canvas, not KiCad's OpenGL or Cairo engines.",
  antialiasing: "Not offered here: the browser's canvas always antialiases.",
  helpers: "Not offered here: a web page cannot start programs on your computer; files open in the browser.",
  iconsInMenus: "Not offered here: the studio's menus are drawn without icons.",
  scrollbars: "Not offered here: the studio's canvases have no scrollbars.",
  focusFollows: "Not offered here: the editors are tabs of one browser window.",
  hotkeyFeedback: "Not ported: the studio has no hotkey popup indicator (KiCad's HOTKEY_CYCLE_POPUP).",
  customCursors: "Not offered here: the canvases use the browser's own cursors.",
  warpMouse: "Not offered here: a web page cannot move the pointer.",
  immediateActions: "Not ported: the studio's hotkeys always act at once (KiCad's default).",
  session: "Not offered here: the studio has no project manager to re-open files.",
  backup: "Not offered here: the studio keeps every edit in the board's own .history (its undo history), not KiCad's backup archives.",
} as const;
