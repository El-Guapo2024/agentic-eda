import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_COMMON_SETTINGS, dimmingFactorFromPercentText, dimmingPercentText, hiContrastFactorOf, iconSchemeOf, sanitizeCommonSettings, sanitizeToolbarIconSize } from "./commonSettings";
import { DIALOG_STATE_STORAGE_KEYS, STUDIO_STORAGE_PREFIXES, studioStorageKeys } from "./maintenance";
import { edgeCutsDimFactor, hiContrastFactor, setHiContrastFactor, HI_CONTRAST_FACTOR } from "./color4d";

test("COMMON_SETTINGS' defaults: icon theme Automatic, 24 px toolbar icons, no striping, 80 % dimming", () => {
  assert.deepEqual(DEFAULT_COMMON_SETTINGS, { iconTheme: "auto", toolbarIconSize: 24, gridStriping: false, hicontrastDimmingFactor: 0.8 });
  assert.deepEqual(sanitizeCommonSettings(null), DEFAULT_COMMON_SETTINGS);
  assert.deepEqual(sanitizeCommonSettings({ iconTheme: "dark", toolbarIconSize: 32, gridStriping: true, hicontrastDimmingFactor: 0.5 }), { iconTheme: "dark", toolbarIconSize: 32, gridStriping: true, hicontrastDimmingFactor: 0.5 });
  assert.deepEqual(sanitizeCommonSettings({ iconTheme: "purple", toolbarIconSize: 8, gridStriping: "yes", hicontrastDimmingFactor: 4 }), { ...DEFAULT_COMMON_SETTINGS, hicontrastDimmingFactor: 1 });
});

test("toolbar_icon_size is a PARAM<int> of 16 .. 64", () => {
  assert.equal(sanitizeToolbarIconSize(16), 16);
  assert.equal(sanitizeToolbarIconSize(64), 64);
  assert.equal(sanitizeToolbarIconSize(65), 24);
  assert.equal(sanitizeToolbarIconSize("32"), 24);
});

test("the dimming factor is typed in percent: '%.0f' shown, ToDouble read, 80 when the text is no number", () => {
  assert.equal(dimmingPercentText(0.8), "80");
  assert.equal(dimmingPercentText(0.555), "56");
  assert.equal(dimmingFactorFromPercentText("60"), 0.6);
  assert.equal(dimmingFactorFromPercentText(" 25.5 "), 0.255);
  assert.equal(dimmingFactorFromPercentText("abc"), 0.8);
  assert.equal(dimmingFactorFromPercentText(""), 0.8);
  assert.equal(dimmingFactorFromPercentText("150"), 1);
  assert.ok(Math.abs(hiContrastFactorOf({ hicontrastDimmingFactor: 0.8 }) - 0.2) < 1e-12, "m_hiContrastFactor = 1 - factor");
});

test("the painters' factor follows the setting; Edge.Cuts keeps at least 0.3", () => {
  setHiContrastFactor(hiContrastFactorOf({ hicontrastDimmingFactor: 0.5 }));
  assert.equal(hiContrastFactor(), 0.5);
  assert.equal(edgeCutsDimFactor(), 0.5);
  setHiContrastFactor(hiContrastFactorOf({ hicontrastDimmingFactor: 0.95 }));
  assert.ok(Math.abs(hiContrastFactor() - 0.05) < 1e-12);
  assert.equal(edgeCutsDimFactor(), 0.3);
  setHiContrastFactor(HI_CONTRAST_FACTOR);
});

test("the icon set: Light and Dark as chosen, Automatic the system's", () => {
  assert.equal(iconSchemeOf("light", "dark"), "light");
  assert.equal(iconSchemeOf("dark", "light"), "dark");
  assert.equal(iconSchemeOf("auto", "dark"), "dark");
  assert.equal(iconSchemeOf("auto", "light"), "light");
});

test("Maintenance: the studio's keys are found under its prefixes, read before any is removed", () => {
  const keys = ["eda-studio.preferences.v1", "other.app", "eda.drcDialog", "eda-studio.color-themes.v1", "edax"];
  const storage = { length: keys.length, key: (i: number) => keys[i] ?? null };
  assert.deepEqual(studioStorageKeys(storage, STUDIO_STORAGE_PREFIXES), ["eda-studio.preferences.v1", "eda.drcDialog", "eda-studio.color-themes.v1"]);
  assert.deepEqual(studioStorageKeys(null, STUDIO_STORAGE_PREFIXES), []);
  assert.ok(DIALOG_STATE_STORAGE_KEYS.every((k) => STUDIO_STORAGE_PREFIXES.some((p) => k.startsWith(p))));
});
