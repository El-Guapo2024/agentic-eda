import { test } from "node:test";
import assert from "node:assert/strict";
import { droppedFilesOf, droppedLeft, droppedSaid, nextDropped, queueDropped } from "./sheetDrop";

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

test("the sheets that were not dropped are named with the file that places them, and a drop of several files says each is placed in turn", () => {
  assert.equal(droppedSaid({ root: "main.kicad_sch", missing: [], left_out: [] }), null);
  assert.equal(droppedSaid({}), null);
  assert.equal(
    droppedSaid({ root: "main.kicad_sch", missing: [{ file: "gone.kicad_sch", placed_by: "main.kicad_sch" }] }),
    "Not in the drop: gone.kicad_sch (placed by main.kicad_sch). Its sheet comes in empty."
  );
  assert.equal(
    droppedSaid({ root: "main.kicad_sch", missing: [{ file: "a.kicad_sch", placed_by: "main.kicad_sch" }, { file: "deep.kicad_sch", placed_by: "adc.kicad_sch" }], paths: ["/t/main.kicad_sch", "/t/other.kicad_sch"] }),
    "Not in the drop: a.kicad_sch (placed by main.kicad_sch), deep.kicad_sch (placed by adc.kicad_sch). Their sheets come in empty. 2 schematics dropped: each is placed in turn."
  );
  assert.equal(droppedSaid({ root: "a.kicad_sch", paths: ["/t/a.kicad_sch"] }), null, "one file: nothing to say");
});

test("the files of a drop wait in turn (DoWithAcceptedFiles): the first is taken, then the next", () => {
  queueDropped(["/t/a.kicad_sch", "/t/b.kicad_sch"]);
  assert.equal(droppedLeft(), 2);
  assert.equal(nextDropped(), "/t/a.kicad_sch");
  assert.equal(nextDropped(), "/t/b.kicad_sch");
  assert.equal(nextDropped(), null);
});
