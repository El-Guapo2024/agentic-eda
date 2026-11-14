import { test } from "node:test";
import assert from "node:assert/strict";
import { exportFileOf, exportSize, initialExport3D, pixelSizeLabel, setResolutionUnit, setSizeUnit, setWidth, toggleLock } from "./export3dImage";

test("DIALOG_EXPORT_3D_IMAGE: the canvas' size, 300 px/in, the aspect locked; the units through pixels; OK's size", () => {
  let s = initialExport3D(800, 600);
  assert.equal(pixelSizeLabel(s), "800 × 600 pixels");
  s = setWidth(s, 1600);
  assert.deepEqual([s.width, s.height], [1600, 1200], "the height follows while locked");
  s = toggleLock(s);
  s = setWidth(s, 1000);
  assert.deepEqual([s.width, s.height], [1000, 1200], "unlocked: it does not");
  s = toggleLock(toggleLock(s));
  s = setSizeUnit(s, "in");
  assert.ok(Math.abs(s.width - 1000 / 300) < 1e-9 && Math.abs(s.height - 4) < 1e-9);
  assert.deepEqual(exportSize(s), { width: 1000, height: 1200 });
  s = setResolutionUnit(s, "px/mm");
  assert.ok(Math.abs(s.xRes - 300 / 25.4) < 1e-9);
  const p = setSizeUnit(initialExport3D(800, 600), "percent");
  assert.deepEqual([p.width, p.height], [100, 100]);
  assert.equal(pixelSizeLabel(p), "80000 × 60000 pixels", "KiCad's own label for percent (100 * the original size)");
  assert.deepEqual(exportSize(p), { width: 800, height: 600 });
  assert.deepEqual(exportFileOf("view"), { name: "view.png", format: "png" });
  assert.deepEqual(exportFileOf("view.JPG"), { name: "view.JPG", format: "jpeg" });
});
