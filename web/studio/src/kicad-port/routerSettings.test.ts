import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_ROUTER_SETTINGS, modeOptionEnabled, routerSettingsToWire, UNSUPPORTED_ROUTER_OPTIONS, type RouterSettings } from "./routerSettings";

test("the defaults are ROUTING_SETTINGS' constructor", () => {
  // pns_routing_settings.cpp: m_routingMode = RM_Walkaround, m_optimizerEffort = OE_MEDIUM, m_removeLoops = true,
  // m_smartPads = true, m_shoveVias = true, m_jumpOverObstacles = false, m_allowDRCViolations = false,
  // m_freeAngleMode = false, m_fixAllSegments = true.
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
  });
});

test("every setting goes to the backend under the name crates/cli/src/route_api.rs reads", () => {
  const s: RouterSettings = { mode: "shove", optimizerEffort: "full", shoveVias: false, jumpOverObstacles: true, removeLoops: false, smartPads: false, allowDrcViolations: true, freeAngleMode: true, fixAllSegments: false };
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
  });
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

test("the rows the router cannot honour say why", () => {
  for (const reason of Object.values(UNSUPPORTED_ROUTER_OPTIONS)) assert.match(reason, /^Not implemented: /);
});
