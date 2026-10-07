import { test } from "node:test";
import assert from "node:assert/strict";
import { libIdIsValid, libLinkChanges, libLinkRows, orphanCandidates } from "./libLinks";

const syms = [
  { id: "R2", lib_id: "Device:R" },
  { id: "C1", lib_id: "Device:C" },
  { id: "R1", lib_id: "Device:R" },
  { id: "R1", lib_id: "Device:R" },
  { id: "U1", lib_id: "eda:U1" },
  { id: "J1", lib_id: null },
];

test("symbols group by library id, each reference once, sorted; a group no library has is an orphan", () => {
  const rows = libLinkRows(syms, new Set(["Device:R", "Device:C"]));
  assert.deepEqual(
    rows.map((r) => [r.libId, r.refs.join(","), r.orphan]),
    [
      ["", "J1", true],
      ["Device:C", "C1", false],
      ["Device:R", "R1,R2", false],
      ["eda:U1", "U1", true],
    ]
  );
});

test("a library id needs a nickname and an item, and only one colon", () => {
  assert.equal(libIdIsValid("Device:R"), true);
  for (const bad of ["R", ":R", "Device:", "a:b:c", ""]) assert.equal(libIdIsValid(bad), false, bad);
});

test("Map Orphans offers the symbols that share the orphan's item name", () => {
  const rows = libLinkRows([{ id: "U1", lib_id: "Old:LM358" }, { id: "R1", lib_id: "Device:R" }], new Set(["Device:R"]));
  const found = orphanCandidates(rows, ["Device:R", "Amplifier_Operational:LM358", "Other:LM358", "Device:C"]);
  assert.deepEqual([...found.entries()], [["Old:LM358", ["Amplifier_Operational:LM358", "Other:LM358"]]]);
});

test("only a filled-in id that is not the current one is a change", () => {
  const rows = libLinkRows(syms, new Set(["Device:R", "Device:C"]));
  assert.deepEqual(libLinkChanges(rows, { "Device:R": " Device:R_Small ", "Device:C": "Device:C", "": "", "eda:U1": "" }), [["Device:R", "Device:R_Small"]]);
});
