import { test } from "node:test";
import assert from "node:assert/strict";
import { CLI_EXPORTER_ACTIONS, cliExporterReason } from "./cliExporters";

const noIdf = { engine: "kicad-cli 10.99.0", unavailable: { idf: "This kicad-cli (10.99.0) has no IDF exporter; KiCad 10.99 nightly or later has one." } };

test("cliExporterReason: an exporter the kicad-cli lacks is refused with the server's reason", () => {
  assert.equal(cliExporterReason("pcbnew.EditorControl.exportIDF", noIdf), noIdf.unavailable.idf);
  assert.equal(cliExporterReason("pcbnew.EditorControl.exportGenCAD", noIdf), null, "GenCAD is there");
});

test("cliExporterReason: nothing is refused before the server answers, or for another action", () => {
  assert.equal(cliExporterReason("pcbnew.EditorControl.exportIDF", null), null);
  assert.equal(cliExporterReason("pcbnew.EditorControl.exportSTEP", { engine: null, unavailable: { step: "no" } }), null);
});

test("CLI_EXPORTER_ACTIONS: IDFv3 and GenCAD, by their kicad-cli commands", () => {
  assert.deepEqual(CLI_EXPORTER_ACTIONS, { "pcbnew.EditorControl.exportIDF": "idf", "pcbnew.EditorControl.exportGenCAD": "gencad" });
});
