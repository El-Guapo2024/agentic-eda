import { test } from "node:test";
import assert from "node:assert/strict";
import { ensureUniqueLibId, ensureUniqueName, footprintNameError, joinLibName, libraryGroups, pastedFootprintName, splitLibName, symbolLibIdError } from "./libraryNames";

test("splitLibName: only the first colon separates the nickname", () => {
  assert.deepEqual(splitLibName("Device:R"), { lib: "Device", item: "R" });
  assert.deepEqual(splitLibName("Untitled"), { lib: "", item: "Untitled" });
  assert.deepEqual(splitLibName("A:B:C"), { lib: "A", item: "B:C" });
  assert.equal(joinLibName("Device", "R"), "Device:R");
  assert.equal(joinLibName("", "R"), "R");
});

test("symbol and footprint names reject exactly the characters KiCad does", () => {
  assert.equal(symbolLibIdError("eda:My Part 1/2"), null, "a space and a slash are fine in a symbol name");
  assert.match(symbolLibIdError("eda:a:b") ?? "", /":"/);
  assert.match(symbolLibIdError("eda:a<b") ?? "", /"<"/);
  assert.match(symbolLibIdError("eda:") ?? "", /must have a name/);
  assert.match(symbolLibIdError("a\\b:c") ?? "", /nickname/);
  assert.equal(footprintNameError("R_0603"), null);
  assert.match(footprintNameError("a/b") ?? "", /"\/"/, "a footprint name is stricter: no slash");
  assert.match(footprintNameError("50%") ?? "", /"%"/);
  assert.match(footprintNameError("a\tb") ?? "", /a tab/);
  assert.match(footprintNameError("") ?? "", /must have a name/);
});

test("ensureUniqueName keeps a free name and counts _1, _2 from the original name (ensureUniqueName / DuplicateFootprint)", () => {
  assert.equal(ensureUniqueName("R", []), "R");
  assert.equal(ensureUniqueName("R", ["R"]), "R_1");
  assert.equal(ensureUniqueName("R", ["R", "R_1", "R_2"]), "R_3");
  assert.equal(ensureUniqueName("R", ["R", "R_2"]), "R_1", "the first free suffix");
});

test("ensureUniqueLibId only competes with the ids of the same library", () => {
  assert.equal(ensureUniqueLibId("eda:R", ["eda:R", "Device:R_1"]), "eda:R_1");
  assert.equal(ensureUniqueLibId("eda:R", ["Device:R"]), "eda:R");
  assert.equal(ensureUniqueLibId("R", ["R", "R_1"]), "R_2", "a bare name is its own library");
});

test("pastedFootprintName appends _copy until free (PasteFootprint)", () => {
  assert.equal(pastedFootprintName("R", []), "R");
  assert.equal(pastedFootprintName("R", ["R"]), "R_copy");
  assert.equal(pastedFootprintName("R", ["R", "R_copy"]), "R_copy_copy");
});

test("libraryGroups groups by nickname, bare names under the project library, sorted case-insensitively", () => {
  const items = ["Device:R", "device:Z", "Connector:USB", "Untitled", "Device:C", "Untitled_1"].map((name) => ({ name }));
  const g = libraryGroups(items);
  assert.deepEqual(
    g.map((x) => x.lib),
    ["Connector", "Device", "device", "eda"]
  );
  assert.deepEqual(
    g.find((x) => x.lib === "Device")!.items.map((i) => i.name),
    ["Device:C", "Device:R"]
  );
  assert.deepEqual(
    g.find((x) => x.lib === "eda")!.items.map((i) => i.name),
    ["Untitled", "Untitled_1"]
  );
});
