import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PAD_MASTER, PAD_CONNECTION_OPTIONS, defaultSpokeAngleDeg, importPadSettings, newPadFromMaster, settingsOf } from "./padSettings";
import type { LibraryPad } from "../api/types";

function pad(number: string, over: Partial<LibraryPad> = {}): LibraryPad {
  return {
    id: `pad_${number}`,
    number,
    at: { x: 100, y: 200 },
    offset: { x: 0, y: 0 },
    size: [1000, 600],
    shape: "rect",
    kind: "smd",
    drill: null,
    drill_slot: null,
    rot: 0,
    roundrect_ratio: null,
    trapezoid_delta: null,
    chamfer_ratio: null,
    chamfer_corners: { top_left: false, top_right: false, bottom_left: false, bottom_right: false },
    layers: ["F.Cu", "F.Paste", "F.Mask"],
    clearance_override: null,
    thermal_gap_override: null,
    thermal_spoke_width_override: null,
    ...over,
  };
}

test("the default pad is KiCad's master pad: 2.54 x 1.27 round rectangle, plated through-hole with a 0.8 hole", () => {
  assert.deepEqual(DEFAULT_PAD_MASTER.size, [2540, 1270]);
  assert.equal(DEFAULT_PAD_MASTER.shape, "round_rect");
  assert.equal(DEFAULT_PAD_MASTER.roundrect_ratio, 0.15);
  assert.equal(DEFAULT_PAD_MASTER.kind, "through_hole");
  assert.equal(DEFAULT_PAD_MASTER.drill, 800);
  assert.deepEqual(DEFAULT_PAD_MASTER.layers, ["*.Cu", "*.Mask"]);
});

test("importPadSettings copies the padstack, layers, type, orientation and overrides but never the number or position", () => {
  const master = settingsOf(pad("M", { shape: "oval", size: [2000, 1000], kind: "through_hole", drill: 700, rot: 90_000, layers: ["*.Cu", "*.Mask"], clearance_override: 250, thermal_gap_override: 300 }));
  const out = importPadSettings(pad("7"), master);
  assert.equal(out.number, "7");
  assert.deepEqual(out.at, { x: 100, y: 200 });
  assert.equal(out.id, "pad_7");
  assert.deepEqual([out.shape, out.size, out.kind, out.drill, out.rot], ["oval", [2000, 1000], "through_hole", 700, 90_000]);
  assert.deepEqual(out.layers, ["*.Cu", "*.Mask"]);
  assert.equal(out.clearance_override, 250);
  assert.equal(out.thermal_gap_override, 300);
});

test("importPadSettings: a circle master squares the size from x, an SMD master drops the hole, an NPTH master clears the number", () => {
  const circle = settingsOf(pad("M", { shape: "circle", size: [1600, 900], kind: "through_hole", drill: 700 }));
  assert.deepEqual(importPadSettings(pad("1"), circle).size, [1600, 1600]);

  const smd = settingsOf(pad("M", { kind: "smd", drill: 500 }));
  const onTht = importPadSettings(pad("2", { kind: "through_hole", drill: 900 }), smd);
  assert.equal(onTht.drill, null);
  assert.equal(onTht.drill_slot, null);

  const npth = settingsOf(pad("M", { kind: "non_plated_hole", drill: 1200 }));
  const numbered = importPadSettings(pad("4"), npth);
  assert.equal(numbered.number, "");
  assert.equal(numbered.drill, 1200);
});

test("newPadFromMaster: the master's settings with the given number and position", () => {
  const p = newPadFromMaster(DEFAULT_PAD_MASTER, "3", { x: 500, y: -500 });
  assert.equal(p.number, "3");
  assert.deepEqual(p.at, { x: 500, y: -500 });
  assert.deepEqual(p.size, [2540, 1270]);
  assert.equal(p.kind, "through_hole");
  assert.equal(p.id, undefined, "ids are the backend's to assign");
  const npth = newPadFromMaster({ ...DEFAULT_PAD_MASTER, kind: "non_plated_hole" }, "9", { x: 0, y: 0 });
  assert.equal(npth.number, "", "a pad that cannot have a number gets none");
});

test("Copy / Paste / Push Pad Properties carry the pad connection and the spoke angle (PAD::ImportSettingsFrom)", () => {
  const master = settingsOf(pad("M", { zone_connection: "Full", thermal_spoke_angle_mdeg: 30_000 }));
  const out = importPadSettings(pad("7", { zone_connection: "None", thermal_spoke_angle_mdeg: null }), master);
  assert.equal(out.zone_connection, "Full");
  assert.equal(out.thermal_spoke_angle_mdeg, 30_000);
  // a pad that sets nothing leaves the master's inherited
  assert.equal(importPadSettings(pad("8", { zone_connection: "Full" }), settingsOf(pad("M"))).zone_connection, null);
  assert.equal(DEFAULT_PAD_MASTER.zone_connection, null, "the default pad inherits its connection");
  assert.equal(DEFAULT_PAD_MASTER.thermal_spoke_angle_mdeg, null);
});

test("the pad connection choices are KiCad's four, and the default spoke angle follows the shape", () => {
  assert.deepEqual(
    PAD_CONNECTION_OPTIONS.map((o) => o.label),
    ["From parent footprint", "Solid", "Thermal relief", "None"],
  );
  assert.deepEqual(PAD_CONNECTION_OPTIONS.map((o) => o.value), ["", "Full", "Thermal", "None"]);
  assert.equal(defaultSpokeAngleDeg("rect"), 90);
  assert.equal(defaultSpokeAngleDeg("oval"), 90);
  assert.equal(defaultSpokeAngleDeg("round_rect"), 90);
  assert.equal(defaultSpokeAngleDeg("circle"), 45);
  assert.equal(defaultSpokeAngleDeg("trapezoid"), 45);
});
