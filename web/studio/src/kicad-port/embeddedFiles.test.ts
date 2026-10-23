import { test } from "node:test";
import assert from "node:assert/strict";
import type { EmbeddedFileInfo, EmbeddedListing } from "../api/embeddedTypes";
import { addCmd, addDecision, checksumOf, collectedMessage, CONFLICT_TITLE, exportNames, fontsCmd, fullNameOf, INHERITED_MESSAGE, MAX_FILE_BYTES, mmh3x64_128, NESTED_QUESTION, NESTED_TITLE, plainName, removeCmd, removePlan, rowsOf, sizeText } from "./embeddedFiles";

const info = (name: string, used_by: string[] = [], content_checksum: string | null = "AB"): EmbeddedFileInfo => ({ name, type: "model", type_label: "3D model", link: `kicad-embed://${name}`, size: 10, stored: 8, checksum: "AB", content_checksum, status: "ok", message: "", used_by });
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

const bytes = (s: string) => new TextEncoder().encode(s);
const STEP = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

test("the checksum is MMH3_HASH with KiCad's seed, as the server computes it", () => {
  assert.equal(mmh3x64_128(bytes("hello"), 0), "CBD8A7B341BD9B025B1E906A48AE1D19");
  assert.equal(mmh3x64_128(bytes("The quick brown fox jumps over the lazy dog"), 0), "E34BBC7BBC071B6C7A433CA9C49A9347");
  assert.equal(checksumOf(bytes(STEP)), "78BAD7207D5443162D6B499F698EE1BC");
  assert.equal(checksumOf(new Uint8Array(0)), "0A8AD5D826E19AE9B0771881583631B3");
});

test("AddEmbeddedFile: the same content is reused without a word, other content asks KiCad's question, an empty file fails", () => {
  const sum = checksumOf(bytes(STEP));
  const l = listing([info("part.step", [], sum)]);
  assert.deepEqual(addDecision(l, "/x/part.step", STEP.length, sum), { kind: "reuse" });
  const c = addDecision(l, "/x/part.step", 3, checksumOf(bytes("abc")));
  assert.deepEqual(c, {
    kind: "conflict",
    message: "A file named 'part.step' is already embedded, but the file on disk has different content.\n\nDo you want to replace the embedded file with the new version?",
  });
  assert.equal(CONFLICT_TITLE, "Embedded File Conflict");
  assert.deepEqual(addDecision(l, "empty.txt", 0, checksumOf(new Uint8Array(0))), { kind: "refuse", message: "Failed to add file 'empty.txt'." });
});

test("Remove takes the board's own files and refuses the inherited ones among them; the footprints question is KiCad's Confirmation", () => {
  const rows = rowsOf(listing([info("used.step", ["U1"]), info("lone.pdf")], [info("fp.wrl", ["U2"])]));
  assert.deepEqual(removePlan({ kind: "board" }, rows, ["fp.wrl", "lone.pdf"]), { names: ["lone.pdf"], askNested: false, error: INHERITED_MESSAGE });
  assert.equal(NESTED_TITLE, "Confirmation");
  assert.equal(NESTED_QUESTION, "Deleted embedded files are also referenced in some footprints.\nDelete from footprints as well?");
});

test("Export writes every file of the list, whatever is selected (onExportFiles)", () => {
  const rows = rowsOf(listing([info("b.step"), info("a.pdf")], [info("c.wrl")]));
  assert.deepEqual(exportNames(rows), ["a.pdf", "b.step", "c.wrl"]);
});
