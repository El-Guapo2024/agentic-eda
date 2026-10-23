import { test } from "node:test";
import assert from "node:assert/strict";
import { cornerModeMenuKey, DEFAULT_ROUTER_SETTINGS, modeOptionEnabled, nextCornerMode, routerSettingsToWire, withTrackSnapping, type CornerMode, type RouterSettings } from "./routerSettings";

test("the defaults are ROUTING_SETTINGS' constructor", () => {
  // pns_routing_settings.cpp: m_routingMode = RM_Walkaround, m_optimizerEffort = OE_MEDIUM, m_removeLoops = true,
  // m_smartPads = true, m_shoveVias = true, m_jumpOverObstacles = false, m_allowDRCViolations = false,
  // m_freeAngleMode = false, m_fixAllSegments = true, m_autoPosture = true, m_optimizeEntireDraggedTrack = false.
  assert.deepEqual(DEFAULT_ROUTER_SETTINGS, {
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
  });
});

test("every setting goes to the backend under the name crates/cli/src/route_api.rs reads", () => {
  const s: RouterSettings = { mode: "shove", optimizerEffort: "full", shoveVias: false, jumpOverObstacles: true, removeLoops: false, smartPads: false, allowDrcViolations: true, freeAngleMode: true, fixAllSegments: false, smoothDragged: false, optimizeEntireDragged: true, autoPosture: false, cornerMode: "rounded_90" };
  assert.deepEqual(routerSettingsToWire(s), {
    mode: "shove",
    optimizer_effort: "full",
    shove_vias: false,
    jump_over_obstacles: true,
    remove_loops: false,
    smart_pads: false,
    allow_drc_violations: true,
    free_angle_mode: true,
    fix_all_segments: false,
    smooth_dragged_segments: false,
    optimize_entire_dragged_track: true,
    auto_posture: false,
    corner_mode: "rounded_90",
  });
  // The default settings are the backend's defaults, so a start request with nothing changed behaves as it always did.
  assert.deepEqual(routerSettingsToWire(DEFAULT_ROUTER_SETTINGS), {
    mode: "walkaround",
    optimizer_effort: "medium",
    shove_vias: true,
    jump_over_obstacles: false,
    remove_loops: true,
    smart_pads: true,
    allow_drc_violations: false,
    free_angle_mode: false,
    fix_all_segments: true,
    smooth_dragged_segments: true,
    optimize_entire_dragged_track: false,
    auto_posture: true,
    corner_mode: "mitered_45",
  });
});

test("Track Corner Mode: the next mode goes 45, arc 45, 90, arc 90 and round, and each is its check item of the menu", () => {
  const order: CornerMode[] = ["mitered_45", "rounded_45", "mitered_90", "rounded_90"];
  order.forEach((m, i) => assert.equal(nextCornerMode(m), order[(i + 1) % 4]));
  assert.deepEqual(order.map(cornerModeMenuKey), ["45", "arc45", "90", "arc90"]);
});

test("onModeChange: free angle and DRC violations belong to Highlight collisions, shove vias and jump over obstacles to Shove", () => {
  for (const option of ["freeAngleMode", "allowDrcViolations"] as const) {
    assert.equal(modeOptionEnabled(option, "mark_obstacles"), true, option);
    assert.equal(modeOptionEnabled(option, "shove"), false, option);
    assert.equal(modeOptionEnabled(option, "walkaround"), false, option);
  }
  for (const option of ["shoveVias", "jumpOverObstacles"] as const) {
    assert.equal(modeOptionEnabled(option, "shove"), true, option);
    assert.equal(modeOptionEnabled(option, "mark_obstacles"), false, option);
    assert.equal(modeOptionEnabled(option, "walkaround"), false, option);
  }
});

test("the magnetic setting for tracks goes to the router as snap_to_tracks, and only when it was given", () => {
  assert.equal("snap_to_tracks" in routerSettingsToWire(DEFAULT_ROUTER_SETTINGS), false, "left out, the router's own default (on) stands");
  assert.equal(routerSettingsToWire(withTrackSnapping(DEFAULT_ROUTER_SETTINGS, "track-tool")).snap_to_tracks, true);
  assert.equal(routerSettingsToWire(withTrackSnapping(DEFAULT_ROUTER_SETTINGS, "always")).snap_to_tracks, true);
  assert.equal(routerSettingsToWire(withTrackSnapping(DEFAULT_ROUTER_SETTINGS, "never")).snap_to_tracks, false);
});
