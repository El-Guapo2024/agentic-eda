// The Interactive Router Settings (pcbnew/dialogs/dialog_pns_settings.cpp) and the
// `PNS::ROUTING_SETTINGS` (pcbnew/router/pns_routing_settings.h) they edit -- the part of
// them this app's router (crates/pns, driven through crates/cli/src/route_api.rs) reads.
//
// Pure: no React, no DOM, no network. `components/RouterSettingsDialog.tsx` is the dialog,
// `api/client.ts` sends `routerSettingsToWire(...)` as the `settings` object of
// `POST /api/route/{start,drag_start}` and `POST /api/route/settings`.
import type { RouteMode } from "../api/types";

/** `PNS_OPTIMIZATION_EFFORT`: `OE_LOW`, `OE_MEDIUM`, `OE_FULL`. KiCad keeps it in the settings file (`tools.pns.effort`) and has no row for it in the dialog. */
export type OptimizerEffort = "low" | "medium" | "full";

export interface RouterSettings {
  /** `Mode()`: Highlight collisions (`mark_obstacles`), Shove, Walk around. */
  mode: RouteMode;
  /** `OptimizerEffort()`. */
  optimizerEffort: OptimizerEffort;
  /** `ShoveVias()` -- "Shove vias". */
  shoveVias: boolean;
  /** `JumpOverObstacles()` -- "Jump over obstacles". */
  jumpOverObstacles: boolean;
  /** `RemoveLoops()` -- "Remove redundant tracks". */
  removeLoops: boolean;
  /** `SmartPads()` -- "Optimize pad connections". */
  smartPads: boolean;
  /** `GetAllowDRCViolationsSetting()` -- "Allow DRC violations" (Highlight collisions only). */
  allowDrcViolations: boolean;
  /** `GetFreeAngleMode()` -- "Free angle mode" (Highlight collisions only). */
  freeAngleMode: boolean;
  /** `GetFixAllSegments()` -- "Fix all segments on click". */
  fixAllSegments: boolean;
}

/** `ROUTING_SETTINGS`' constructor (pns_routing_settings.cpp): Walk around, medium effort, shove vias, remove loops, smart pads, fix all segments. */
export const DEFAULT_ROUTER_SETTINGS: RouterSettings = {
  mode: "walkaround",
  optimizerEffort: "medium",
  shoveVias: true,
  jumpOverObstacles: false,
  removeLoops: true,
  smartPads: true,
  allowDrcViolations: false,
  freeAngleMode: false,
  fixAllSegments: true,
};

/** The `settings` object of the router's start requests (`settings_of` in crates/cli/src/route_api.rs). */
export function routerSettingsToWire(s: RouterSettings): Record<string, string | boolean> {
  return {
    mode: s.mode,
    optimizer_effort: s.optimizerEffort,
    shove_vias: s.shoveVias,
    jump_over_obstacles: s.jumpOverObstacles,
    remove_loops: s.removeLoops,
    smart_pads: s.smartPads,
    allow_drc_violations: s.allowDrcViolations,
    free_angle_mode: s.freeAngleMode,
    fix_all_segments: s.fixAllSegments,
  };
}

/** The options `DIALOG_PNS_SETTINGS::onModeChange` switches with the mode: the two under Highlight collisions, the two under Shove. */
export type ModeOption = "freeAngleMode" | "allowDrcViolations" | "shoveVias" | "jumpOverObstacles";

/** `onModeChange`: `m_freeAngleMode` and `m_violateDrc` are enabled with Highlight collisions, `m_shoveVias` and `m_backPressure` ("Jump over obstacles") with Shove. */
export function modeOptionEnabled(option: ModeOption, mode: RouteMode): boolean {
  return option === "freeAngleMode" || option === "allowDrcViolations" ? mode === "mark_obstacles" : mode === "shove";
}

/**
 * The rows of the dialog that KiCad has and this router cannot honour, with the reason each is shown disabled
 * (the same convention the Board Setup and zone dialogs use for a field with nothing behind it).
 */
export const UNSUPPORTED_ROUTER_OPTIONS = {
  smoothDragged: "Not implemented: the corner drag moves a vertex and does not snap or merge segments (DRAGGER::dragCorner45 and the segment slide are not ported).",
  optimizeEntireDraggedTrack: "Not implemented: a drag is not optimized afterwards (DRAGGER::optimizeAndUpdateDraggedLine is not ported).",
  autoPosture: "Not implemented: the posture follows the last segment and the / key. The router is asked one cursor position at a time, with no mouse trail to read it from (MOUSE_TRAIL_TRACER is not ported).",
} as const;
