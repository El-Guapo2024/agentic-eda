import { test } from "node:test";
import assert from "node:assert/strict";
import { IPC4761_CONFIGS, viaConfiguration, withPreset } from "./viaProtection";
import { IPC4761_PRESETS } from "./globalEditTracksVias";

test("getViaConfiguration: a via's tenting, covering, plugging, filling and capping read as the IPC-4761 preset they are, else custom (VIA_PROTECTION_UI_MIXIN)", () => {
  assert.equal(viaConfiguration(undefined), "from_board", "a via of KiCad's defaults takes everything from the board");
  assert.equal(viaConfiguration({ front_tenting: true, back_tenting: false, front_covering: false, back_covering: false, front_plugging: false, back_plugging: false, filling: false, capping: false }), "ia");
  assert.equal(viaConfiguration({ front_tenting: true, back_tenting: true, front_covering: false, back_covering: false, front_plugging: true, back_plugging: true, filling: false, capping: false }), "ivb");
  // tented on the front only, the rest from the board: no preset
  assert.equal(viaConfiguration({ front_tenting: true, back_tenting: false }), "custom");
});

test("setViaConfiguration: each preset sets what m_IPC4761Presets says, and reads back as itself", () => {
  assert.equal(IPC4761_CONFIGS.length, IPC4761_PRESETS.length, "one configuration for each preset the dialog lists");
  for (const [preset] of IPC4761_CONFIGS) assert.equal(viaConfiguration(withPreset({ front_tenting: true }, preset)), preset);
  const vii = withPreset({ backdrill: { size: 450_000, start: "B.Cu", end: "In2.Cu" } }, "vii");
  assert.deepEqual(vii, { backdrill: { size: 450_000, start: "B.Cu", end: "In2.Cu" }, front_tenting: false, back_tenting: false, front_plugging: false, back_plugging: false, front_covering: false, back_covering: false, filling: true, capping: true }, "the drills stay");
  assert.deepEqual(withPreset({ front_tenting: true }, "from_board"), {});
  assert.deepEqual(withPreset({ front_tenting: true }, "custom"), { front_tenting: true }, "CUSTOM changes nothing");
});
