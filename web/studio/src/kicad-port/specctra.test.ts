import { test } from "node:test";
import assert from "node:assert/strict";
import { describeExport, describeImport, dsnFileName, SESSION_ACCEPT, specctraImportCmd } from "./specctra";

test("the DSN is named after the board, with KiCad's extension", () => {
  assert.equal(dsnFileName("opamp filter"), "opamp filter.dsn");
  assert.equal(dsnFileName("a/b"), "a_b.dsn");
  assert.equal(dsnFileName(""), "board.dsn");
  assert.equal(SESSION_ACCEPT, ".ses");
});

test("an export says BOARD exported OK, a warning of FromBOARD, or the server's refusal", () => {
  assert.deepEqual(describeExport({ ok: true, message: "BOARD exported OK." }, "x.dsn"), { message: "BOARD exported OK. Saved x.dsn.", kind: "info" });
  const warned = describeExport({ ok: true, message: "", warnings: ["Board outline is malformed. Run DRC for a full analysis."] }, "x.dsn");
  assert.equal(warned.kind, "error");
  assert.match(warned.message, /Board outline is malformed/);
  assert.deepEqual(describeExport({ ok: false, message: "Unable to export, please fix and try again: Multiple footprints have the reference designator 'R1'." }, "x.dsn").kind, "error");
});

test("a session file is the specctra command, and its answer KiCad's words", () => {
  assert.deepEqual(specctraImportCmd({ name: "b.ses", text: "(session b)" }), { op: "specctra", session: "(session b)", name: "b.ses" });
  assert.match(describeImport({ ok: true, message: "" }).message, /^Session file imported and merged OK\./);
  // D4 (r2-editors-exports): KiCad's "Fix problem and try again" over the error.
  assert.deepEqual(describeImport({ ok: false, message: "Session file uses invalid layer id 'In3.Cu'." }), { message: "Fix problem and try again\nSession file uses invalid layer id 'In3.Cu'.", kind: "error" });
});
