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
  /** `SmoothDraggedSegments()` -- "Smooth dragged segments": a dragged segment or corner snaps to line up with the segments around it. */
  smoothDragged: boolean;
  /** `GetOptimizeEntireDraggedTrack()` -- "Optimize entire track being dragged": a dragged track is run through the optimizer where the drag changed it (`DRAGGER::optimizeAndUpdateDraggedLine`). */
  optimizeEntireDragged: boolean;
  /** `GetAutoPosture()` -- "Use mouse path to set track posture": the elbow of the head follows how the mouse has moved (`MOUSE_TRAIL_TRACER`). */
  autoPosture: boolean;
  /** `GetSnapToTracks()`: not a row of the dialog but the magnetic setting for tracks (`TOOL_BASE::checkSnap`) -- a route ends on the same-net track under the cursor. Set by the canvas ([`withTrackSnapping`]); left out, the router's own default (on). */
  snapToTracks?: boolean;
}

/** `ROUTING_SETTINGS`' constructor (pns_routing_settings.cpp): Walk around, medium effort, shove vias, remove loops, smart pads, fix all segments, smooth dragged segments, auto posture; no optimizing of the entire dragged track. */
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
  smoothDragged: true,
  optimizeEntireDragged: false,
  autoPosture: true,
};

/** The `settings` object of the router's start requests (`settings_of` in crates/cli/src/route_api.rs). */
export function routerSettingsToWire(s: RouterSettings): Record<string, string | boolean> {
  return {
    ...(s.snapToTracks === undefined ? {} : { snap_to_tracks: s.snapToTracks }),
    mode: s.mode,
    optimizer_effort: s.optimizerEffort,
    shove_vias: s.shoveVias,
    jump_over_obstacles: s.jumpOverObstacles,
    remove_loops: s.removeLoops,
    smart_pads: s.smartPads,
    allow_drc_violations: s.allowDrcViolations,
    free_angle_mode: s.freeAngleMode,
    fix_all_segments: s.fixAllSegments,
    smooth_dragged_segments: s.smoothDragged,
    optimize_entire_dragged_track: s.optimizeEntireDragged,
    auto_posture: s.autoPosture,
  };
}

/** `TOOL_BASE::checkSnap`: the router snaps a route's end onto a same-net track only when the magnetic setting for tracks allows it (its default captures the cursor in the track tool). The canvas's own snapping (`PcbSnap.routePoint`) has already put the cursor on the track; this tells the router to take it as the end. */
export function withTrackSnapping(s: RouterSettings, magneticTracks: "never" | "track-tool" | "always"): RouterSettings {
  return { ...s, snapToTracks: magneticTracks !== "never" };
}

/** The options `DIALOG_PNS_SETTINGS::onModeChange` switches with the mode: the two under Highlight collisions, the two under Shove. */
export type ModeOption = "freeAngleMode" | "allowDrcViolations" | "shoveVias" | "jumpOverObstacles";

/** `onModeChange`: `m_freeAngleMode` and `m_violateDrc` are enabled with Highlight collisions, `m_shoveVias` and `m_backPressure` ("Jump over obstacles") with Shove. */
export function modeOptionEnabled(option: ModeOption, mode: RouteMode): boolean {
  return option === "freeAngleMode" || option === "allowDrcViolations" ? mode === "mark_obstacles" : mode === "shove";
}
