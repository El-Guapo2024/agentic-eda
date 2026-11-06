import { test } from "node:test";
import assert from "node:assert/strict";
import { loadErrorDetails, loadErrorSummary, mergeLoadErrors } from "./libraryLoadErrors";

test("an answer adds the libraries it read with errors; a newer message replaces an older one; name order", () => {
  const known = [{ lib: "Mini", error: "old" }];
  const merged = mergeLoadErrors(known, [{ lib: "Device", error: "d" }, { lib: "Mini", error: "new" }]);
  assert.deepEqual(merged, [{ lib: "Device", error: "d" }, { lib: "Mini", error: "new" }]);
  assert.deepEqual(mergeLoadErrors(merged, []), merged, "an answer about other libraries changes nothing");
});

test("the warning line names one library, or counts several", () => {
  assert.equal(loadErrorSummary([]), "");
  assert.equal(loadErrorSummary([{ lib: "Mini", error: "x" }]), "Library 'Mini' loaded with errors");
  assert.equal(loadErrorSummary([{ lib: "A", error: "x" }, { lib: "B", error: "y" }]), "2 libraries loaded with errors");
  assert.equal(loadErrorDetails([{ lib: "Mini", error: "Unable to read file 'b.kicad_sym'" }]), "Library 'Mini' loaded with errors:\n\nUnable to read file 'b.kicad_sym'");
});
