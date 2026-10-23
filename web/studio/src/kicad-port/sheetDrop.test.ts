import { test } from "node:test";
import assert from "node:assert/strict";
import { droppedFilesOf, droppedSaid } from "./sheetDrop";

test("the action takes the files as a list, a wrapper or one file", () => {
  const a = { name: "a.kicad_sch", text: "(kicad_sch)" };
  const b = { name: "b.kicad_sch", text: "(kicad_sch)" };
  assert.deepEqual(droppedFilesOf({ files: [a, b] }), [a, b]);
  assert.deepEqual(droppedFilesOf([a, b]), [a, b]);
  assert.deepEqual(droppedFilesOf(a), [a]);
  assert.deepEqual(droppedFilesOf({ files: [a, { name: 3 }, null, { name: "c", text: 4 }] }), [a], "what is no file is left out");
  assert.deepEqual(droppedFilesOf(undefined), []);
  assert.deepEqual(droppedFilesOf("main.kicad_sch"), [], "a path is no file: a browser has none");
});

test("the sheets that were not dropped are named with the file that places them, and the files left out are said", () => {
  assert.equal(droppedSaid({ root: "main.kicad_sch", missing: [], left_out: [] }), null);
  assert.equal(droppedSaid({}), null);
  assert.equal(
    droppedSaid({ root: "main.kicad_sch", missing: [{ file: "gone.kicad_sch", placed_by: "main.kicad_sch" }] }),
    "Not in the drop: gone.kicad_sch (placed by main.kicad_sch). Its sheet comes in empty."
  );
  assert.equal(
    droppedSaid({ root: "main.kicad_sch", missing: [{ file: "a.kicad_sch", placed_by: "main.kicad_sch" }, { file: "deep.kicad_sch", placed_by: "adc.kicad_sch" }], left_out: ["other.kicad_sch"] }),
    "Not in the drop: a.kicad_sch (placed by main.kicad_sch), deep.kicad_sch (placed by adc.kicad_sch). Their sheets come in empty. Only main.kicad_sch is imported; other.kicad_sch is not placed by it (drop it on its own)."
  );
  assert.equal(droppedSaid({ root: "a.kicad_sch", left_out: ["b.kicad_sch", "c.kicad_sch"] }), "Only a.kicad_sch is imported; b.kicad_sch, c.kicad_sch are not placed by it (drop them on their own).");
});
