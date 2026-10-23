import { test } from "node:test";
import assert from "node:assert/strict";
import type { EmbeddedFileInfo, EmbeddedListing } from "../api/embeddedTypes";
import { addCmd, addDecision, collectedMessage, fontsCmd, fullNameOf, INHERITED_MESSAGE, MAX_FILE_BYTES, NESTED_QUESTION, plainName, removeCmd, removePlan, rowsOf, sizeText } from "./embeddedFiles";

const info = (name: string, used_by: string[] = []): EmbeddedFileInfo => ({ name, type: "model", type_label: "3D model", link: `kicad-embed://${name}`, size: 10, stored: 8, checksum: "AB", status: "ok", message: "", used_by });
const listing = (files: EmbeddedFileInfo[], inherited: EmbeddedFileInfo[] = []): EmbeddedListing => ({ owner: { kind: "board" }, fonts: false, files, inherited });

test("the grid is one list sorted by name, an own file winning a name the footprints also hold", () => {
  const rows = rowsOf(listing([info("b.step"), info("a.pdf")], [info("c.wrl"), info("b.step")]));
  assert.deepEqual(rows.map((r) => [r.name, r.inherited]), [["a.pdf", false], ["b.step", false], ["c.wrl", true]]);
  assert.deepEqual(rowsOf(null), []);
});

test("a file is filed under its file name, a link names it too", () => {
  assert.equal(fullNameOf("C:\\models\\part.step"), "part.step");
  assert.equal(fullNameOf("/home/x/part.step"), "part.step");
  assert.equal(plainName("kicad-embed://part.step"), "part.step");
  assert.equal(plainName("part.step"), "part.step");
});

test("Add asks the conflict question for a name taken and refuses what cannot be embedded", () => {
  const l = listing([info("part.step")], [info("fp.step")]);
  assert.equal(addDecision(l, "new.step", 10).kind, "add");
  const c = addDecision(l, "/x/part.step", 10);
  assert.equal(c.kind, "conflict");
  assert.match((c as { message: string }).message, /A file named 'part.step' is already embedded/);
  assert.equal(addDecision(l, "fp.step", 10).kind, "add", "a file only footprints hold is the board's to add");
  assert.equal(addDecision(l, "big.step", MAX_FILE_BYTES + 1).kind, "refuse");
  assert.deepEqual(addCmd({ kind: "board" }, "/x/part.step", "QUJD"), { op: "embedded", verb: "add", owner: { kind: "board" }, name: "part.step", data: "QUJD" });
  assert.equal((addCmd({ kind: "board" }, "a", "", true) as { replace?: boolean }).replace, true);
});

test("Remove refuses an inherited file and asks about the footprints when one lists the file", () => {
  const rows = rowsOf(listing([info("used.step", ["U1"]), info("lone.pdf")], [info("fp.wrl", ["U2"])]));
  assert.deepEqual(removePlan({ kind: "board" }, rows, ["fp.wrl"]), { error: INHERITED_MESSAGE });
  assert.deepEqual(removePlan({ kind: "board" }, rows, ["lone.pdf"]), { names: ["lone.pdf"], askNested: false });
  assert.deepEqual(removePlan({ kind: "board" }, rows, ["kicad-embed://used.step", "lone.pdf"]), { names: ["used.step", "lone.pdf"], askNested: true });
  assert.deepEqual(removePlan({ kind: "footprint", reference: "U1" }, rows, ["used.step"]), { names: ["used.step"], askNested: false });
  assert.ok("error" in removePlan({ kind: "board" }, rows, []));
  assert.match(NESTED_QUESTION, /Delete from footprints as well\?/);
  assert.deepEqual(removeCmd({ kind: "board" }, ["a"], true), { op: "embedded", verb: "remove", owner: { kind: "board" }, names: ["a"], nested: true });
  assert.deepEqual(fontsCmd({ kind: "schematic" }, true), { op: "embedded", verb: "set_fonts", owner: { kind: "schematic" }, embed: true });
});

test("the words of the info bar and the size column", () => {
  assert.equal(collectedMessage(3), "3 3D model(s) successfully embedded.");
  assert.equal(sizeText(null), "");
  assert.equal(sizeText(512), "512 B");
  assert.equal(sizeText(2048), "2.0 KiB");
  assert.equal(sizeText(3 * 1024 * 1024), "3.0 MiB");
});
