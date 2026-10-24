// The Interactive Router Settings (pcbnew/dialogs/dialog_pns_settings.cpp) and the
// `PNS::ROUTING_SETTINGS` (pcbnew/router/pns_routing_settings.h) they edit -- the part of
// them this app's router (crates/pns, driven through crates/cli/src/route_api.rs) reads.
//
// Pure: no React, no DOM, no network. `components/RouterSettingsDialog.tsx` is the dialog,
// `api/client.ts` sends `routerSettingsToWire(...)` as the `settings` object of
// `POST /api/route/{start,drag_start}` and `POST /api/route/settings`.
import type { RouteMode } from "../api/types";

/** `DIRECTION_45::CORNER_MODE` (`GetCornerMode()`, the Track Corner Mode actions): mitered or rounded (an arc), 45 or 90 degrees. */
export type CornerMode = "mitered_45" | "rounded_45" | "mitered_90" | "rounded_90";

/** `ROUTER_TOOL::handlePnSCornerModeChange` for `SwitchRoundingToNext`: 45, arc 45, 90, arc 90, and round again. */
export function nextCornerMode(m: CornerMode): CornerMode {
  return m === "mitered_45" ? "rounded_45" : m === "rounded_45" ? "mitered_90" : m === "mitered_90" ? "rounded_90" : "mitered_45";
}

/** The Track Corner Mode submenu's check items (`cornerMode45Cond` ...): "45", "arc45", "90", "arc90". */
export function cornerModeMenuKey(m: CornerMode): "45" | "arc45" | "90" | "arc90" {
  return m === "mitered_45" ? "45" : m === "rounded_45" ? "arc45" : m === "mitered_90" ? "90" : "arc90";
}

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
  /** `GetCornerMode()`: not a row of the dialog but the Track Corner Mode actions (`pcbnew.InteractiveRouter.SwitchRounding*`). Left out, mitered 45. */
  cornerMode?: CornerMode;
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

/**
 * `ROUTING_SETTINGS` is a `NESTED_SETTINGS` of pcbnew.json (`tools.pns`, `PNS_TOOL_BASE::Reset`: `std::make_unique<ROUTING_SETTINGS>( settings,
 * "tools.pns" )`): the dialog's choices and the Track Corner Mode survive a restart. What is saved is every field but `snapToTracks` (the
 * magnetic setting's, set by the canvas). Reading takes each field that has the right type and keeps the default for the rest. (findings p5 F3-5)
 */
export function parseRouterSettings(raw: string | null): RouterSettings {
  if (!raw) return DEFAULT_ROUTER_SETTINGS;
  let j: Record<string, unknown>;
  try {
    j = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return DEFAULT_ROUTER_SETTINGS;
  }
  if (!j || typeof j !== "object") return DEFAULT_ROUTER_SETTINGS;
  const d = DEFAULT_ROUTER_SETTINGS;
  const bool = (k: keyof RouterSettings): boolean => (typeof j[k] === "boolean" ? (j[k] as boolean) : (d[k] as boolean));
  const modes: RouteMode[] = ["mark_obstacles", "walkaround", "shove"];
  const efforts: OptimizerEffort[] = ["low", "medium", "full"];
  const corners: CornerMode[] = ["mitered_45", "rounded_45", "mitered_90", "rounded_90"];
  const out: RouterSettings = {
    mode: modes.includes(j.mode as RouteMode) ? (j.mode as RouteMode) : d.mode,
    optimizerEffort: efforts.includes(j.optimizerEffort as OptimizerEffort) ? (j.optimizerEffort as OptimizerEffort) : d.optimizerEffort,
    shoveVias: bool("shoveVias"),
    jumpOverObstacles: bool("jumpOverObstacles"),
    removeLoops: bool("removeLoops"),
    smartPads: bool("smartPads"),
    allowDrcViolations: bool("allowDrcViolations"),
    freeAngleMode: bool("freeAngleMode"),
    fixAllSegments: bool("fixAllSegments"),
    smoothDragged: bool("smoothDragged"),
    optimizeEntireDragged: bool("optimizeEntireDragged"),
    autoPosture: bool("autoPosture"),
  };
  if (corners.includes(j.cornerMode as CornerMode)) out.cornerMode = j.cornerMode as CornerMode;
  return out;
}

/** What [`parseRouterSettings`] reads back: the settings without `snapToTracks`. */
export function serializeRouterSettings(s: RouterSettings): string {
  const { snapToTracks: _snap, ...kept } = s;
  return JSON.stringify(kept);
}

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
    corner_mode: s.cornerMode ?? "mitered_45",
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
