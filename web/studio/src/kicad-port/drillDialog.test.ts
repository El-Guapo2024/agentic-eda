import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_DRILL_FORM, drillEnables, drillFormAtOpening, drillPrecision, drillRequest, drillSettingsCommand, MAP_FORMATS, ZEROS } from "./drillDialog";

test("Generate Drill Files starts as dialog_gendrill_base.cpp does, the Origin and folder from the board", () => {
  const f = drillFormAtOpening(DEFAULT_DRILL_FORM, { use_aux_origin: true, options: { outputdirectory: "fab/" } });
  assert.deepEqual([f.format, f.unitsMm, f.zeros, f.mapFormat, f.generateMap, f.mergePthNpth, f.useAuxOrigin, f.outputDirectory], ["excellon", true, "decimal", "gerberx2", false, false, true, "fab/"]);
  assert.deepEqual(MAP_FORMATS.map((m) => m.label), ["Postscript", "Gerber X2", "DXF", "SVG", "PDF"]);
  assert.equal(ZEROS[0]?.label, "Decimal format (recommended)");
});

test("the format enables its controls and the precision is the writer's", () => {
  assert.deepEqual(drillEnables(DEFAULT_DRILL_FORM), { excellon: true, tenting: false });
  assert.deepEqual(drillEnables({ ...DEFAULT_DRILL_FORM, format: "gerber" }), { excellon: false, tenting: true });
  assert.deepEqual(drillPrecision(DEFAULT_DRILL_FORM, 6), { text: "3:3", enabled: false }, "decimal format: no precision");
  assert.deepEqual(drillPrecision({ ...DEFAULT_DRILL_FORM, unitsMm: false, zeros: "keep" }, 6), { text: "2:4", enabled: true });
  assert.deepEqual(drillPrecision({ ...DEFAULT_DRILL_FORM, format: "gerber" }, 5), { text: "4.5", enabled: true });
});

test("Generate sends every control and stores the Origin and the folder only when they changed", () => {
  const body = drillRequest({ ...DEFAULT_DRILL_FORM, generateMap: true, mapFormat: "pdf", mirrorY: true });
  assert.deepEqual([body.format, body.generate_map, body.map_format, body.mirror_y, body.units_mm, body.zeros], ["excellon", true, "pdf", true, true, "decimal"]);
  const board = { use_aux_origin: false, options: { outputdirectory: "" } };
  assert.equal(drillSettingsCommand(drillFormAtOpening(DEFAULT_DRILL_FORM, board), board), null);
  assert.deepEqual(drillSettingsCommand({ ...DEFAULT_DRILL_FORM, outputDirectory: "out\\drill" }, board), { op: "set_plot_settings", options: { outputdirectory: "out/drill" } });
  assert.deepEqual(drillSettingsCommand({ ...DEFAULT_DRILL_FORM, useAuxOrigin: true }, board), { op: "set_plot_settings", use_aux_origin: true });
});
