import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbol } from "../api/types";
import {
  footprintExists,
  footprintSaveTarget,
  isTableLibrary,
  libraryOfId,
  libraryRowLabel,
  modifiedLabel,
  MUST_SAVE_ELSEWHERE,
  nameInLibrary,
  notWritableMessage,
  readOnlySaveAsMessage,
  REVERT_DETAIL,
  revertCommands,
  revertFootprintCommand,
  revertQuestion,
  saveAllPlan,
  saveAsLibraryRows,
  saveLibraryAsDefaultName,
  saveLibraryAsTitle,
  symbolSaveAsCheck,
  symbolSaveLibrary,
  symbolTarget,
  TABLE_OPTIONS,
  takenInLibrary,
} from "./librarySave";

const sym = (lib_id: string, extra: Partial<LibrarySymbol> = {}): LibrarySymbol => ({ lib_id, graphics: [], pins: [], ...extra }) as unknown as LibrarySymbol;

test("KiCad's words, word for word", () => {
  assert.equal(notWritableMessage("Device"), "Symbol library 'Device' is not writable.");
  assert.equal(MUST_SAVE_ELSEWHERE, "You must save to a different location.");
  assert.equal(readOnlySaveAsMessage("Device", "R2"), "Library 'Device' is read-only. Choose a different library to save the symbol 'R2' to.");
  assert.equal(revertQuestion("R"), "Revert 'R' to last version saved?");
  assert.equal(REVERT_DETAIL, "Your current changes will be permanently lost.");
  assert.equal(saveLibraryAsTitle("Mine"), "Save Library 'Mine' As...");
  assert.deepEqual(
    TABLE_OPTIONS.map((o) => o.label),
    ["Add new project library table entry", "Update existing library table entry"]
  );
  assert.equal(modifiedLabel("R", true), "R *");
  assert.equal(modifiedLabel("R", false), "R");
});

test("the project library is the design's own; every other library has a file", () => {
  assert.equal(libraryOfId("Device:R"), "Device");
  assert.equal(libraryOfId("R_0603"), "eda", "a bare footprint name is the project library's");
  assert.equal(isTableLibrary("eda"), false);
  assert.equal(isTableLibrary(""), false);
  assert.equal(isTableLibrary("Device"), true);
  assert.equal(saveLibraryAsDefaultName("symbol", "Mine"), "Mine.kicad_sym");
  assert.equal(saveLibraryAsDefaultName("footprint", "Mine"), "Mine.pretty");
});

test("Save works on the tree's library while the tree is shown, else the open symbol's (SYMBOL_EDIT_FRAME::Save)", () => {
  assert.equal(symbolSaveLibrary(true, [], ["Device:R"], "Mine:X"), "Device", "a symbol selected in the tree");
  assert.equal(symbolSaveLibrary(true, ["Mine"], [], "Device:R"), "Mine", "a library row");
  assert.equal(symbolSaveLibrary(true, [], [], "Mine:X"), "Mine", "nothing selected: saveCurrentSymbol");
  assert.equal(symbolSaveLibrary(false, ["Mine"], ["Device:R"], "Other:X"), "Other", "the tree hidden: the open symbol");
  assert.equal(symbolSaveLibrary(true, [], [], null), null);
  assert.equal(symbolSaveLibrary(true, [], ["eda:New"], null), "eda");
});

test("Revert and Save Library As target the tree's selection, else the open symbol (GetTargetLibId)", () => {
  assert.deepEqual(symbolTarget(["Device:R"], ["Mine"], "Mine:X"), { kind: "symbol", id: "Device:R" });
  assert.deepEqual(symbolTarget([], ["Mine"], "Device:R"), { kind: "library", lib: "Mine" });
  assert.deepEqual(symbolTarget([], [], "Device:R"), { kind: "symbol", id: "Device:R" });
  assert.deepEqual(symbolTarget([], [], null), { kind: "none" });
});

test("the Footprint Editor saves only when the tree's target is the loaded footprint", () => {
  assert.equal(footprintSaveTarget(null, [], "Mine:R"), "Mine:R");
  assert.equal(footprintSaveTarget("Mine:R", [], "Mine:R"), "Mine:R");
  assert.equal(footprintSaveTarget("Mine:C", [], "Mine:R"), null, "another footprint selected: Save does nothing");
  assert.equal(footprintSaveTarget(null, ["Mine"], "Mine:R"), null, "a library row selected");
  assert.equal(footprintSaveTarget(null, [], null), null);
});

test("Revert puts the file's symbols back and, for a library, drops the ones the file never had", () => {
  const saved = { "Mine:Kid": sym("Kid", { extends: "Mine:Root" }), "Mine:Root": sym("Root"), "Mine:New": null };
  const lib = revertCommands(saved, true);
  assert.deepEqual(
    lib.map((c) => (c.op === "put_library_symbol" ? `put ${c.symbol.lib_id}` : c.op === "delete_library_symbol" ? `delete ${c.lib_id}` : c.op)),
    ["put Mine:Root", "put Mine:Kid", "delete Mine:New"],
    "the parent before what derives from it; the never-saved symbol goes"
  );
  assert.ok(lib.every((c) => c.op !== "put_library_symbol" || c.overwrite === true));
  assert.deepEqual(revertCommands({ "Mine:New": null }, false), [], "a single symbol never saved has nothing to go back to");
  const fp = revertFootprintCommand("Mine:R", { name: "R" } as never);
  assert.equal(fp.op, "put_library_footprint");
  assert.equal(fp.op === "put_library_footprint" && fp.footprint.name, "Mine:R");
});

test("Save All saves the writable modified libraries and asks about a read-only one only when it is the only one", () => {
  const w = new Map([
    ["A", true],
    ["B", false],
    ["C", true],
  ]);
  assert.deepEqual(saveAllPlan(["A", "B", "C", "eda"], w), { save: ["A", "C"], readOnly: ["B"], askAboutReadOnly: false });
  assert.deepEqual(saveAllPlan(["B"], w), { save: [], readOnly: ["B"], askAboutReadOnly: true });
  assert.deepEqual(saveAllPlan([], w), { save: [], readOnly: [], askAboutReadOnly: false });
});

test("Save As lists every library of the tables, the pinned ones first, with the project library", () => {
  const libs = [
    { name: "Device", description: "Resistors", writable: false },
    { name: "Mine", description: "", writable: true },
    { name: "Hidden", description: "", writable: true, hidden: true },
  ];
  const rows = saveAsLibraryRows(libs, new Set(["Mine"]), ["Fresh"]);
  assert.deepEqual(
    rows.map((r) => r.name),
    ["Mine", "eda", "Device", "Fresh"]
  );
  assert.equal(rows.find((r) => r.name === "Device")!.writable, false, "a read-only library is listed (the validator refuses it)");
  assert.equal(libraryRowLabel(rows[0]!), "☆ Mine");
  assert.equal(libraryRowLabel(rows[2]!), "Device");
});

test("a name is taken in a library by the design's copy or by the library's file, in any case", () => {
  const taken = takenInLibrary("Device", ["R", "C"], ["eda:X", "Device:Q"]);
  assert.ok(nameInLibrary("Device", "r", taken), "SymbolNameInUse ignores case");
  assert.ok(nameInLibrary("Device", "Q", taken));
  assert.ok(!nameInLibrary("Device", "L", taken));
  assert.ok(nameInLibrary("eda", "x", taken));
  assert.deepEqual([...takenInLibrary("eda", ["R"], ["eda:X"])], ["eda:X"], "the project library has no file");
  assert.ok(footprintExists("Mine", "R1", ["R1"], []));
  assert.ok(footprintExists("Mine", "R2", [], ["Mine:R2"]));
  assert.ok(footprintExists("eda", "R3", [], ["R3"]), "the project library's footprints are bare names");
  assert.ok(!footprintExists("eda", "R1", ["R1"], []));
});

test("Save Symbol As refuses, in KiCad's order: no library, no name, a read-only library", () => {
  const rows = [
    { name: "eda", writable: true },
    { name: "Device", writable: false },
    { name: "Mine", writable: true },
  ];
  assert.deepEqual(symbolSaveAsCheck("", "R", rows), { error: "A library must be specified." });
  assert.deepEqual(symbolSaveAsCheck("Mine", "   ", rows), { error: "Symbol must have a name." });
  assert.deepEqual(symbolSaveAsCheck("Device", " R 2 ", rows), { error: "Library 'Device' is read-only. Choose a different library to save the symbol 'R_2' to." });
  assert.deepEqual(symbolSaveAsCheck("Mine", " R 2 ", rows), { name: "R_2", libId: "Mine:R_2" }, "the name trimmed, its blanks made underscores");
  assert.deepEqual(symbolSaveAsCheck("New", "X", rows), { name: "X", libId: "New:X" }, "a library the list does not know yet (made a moment ago) is not refused");
});

import {
  deleteBaseMessage,
  deleteFootprintQuestion,
  deleteModifiedQuestion,
  footprintDeletedStatus,
  footprintLibraryReadOnly,
  newFootprintLibrary,
  newFootprintReadOnlyWarning,
  readOnlyFootprintBanner,
  SAVE_AS_EDITABLE_COPY,
  symbolLibraryNotFound,
  uniqueNewFootprintName,
} from "./librarySave";

test("Delete, Rename and New Footprint say what KiCad says (UI-LEFT-2 LL1, LL2)", () => {
  assert.equal(symbolLibraryNotFound("Mine"), "Symbol library 'Mine' not found.");
  assert.equal(deleteModifiedQuestion("R"), "The symbol 'R' has been modified.\nDo you want to remove it from the library?");
  assert.equal(deleteBaseMessage("Zeta", ["Alpha", "Beta"]), "Deleting a base symbol will delete all symbols derived from it.\n\nZeta (base)\nAlpha\nBeta\n");
  assert.equal(deleteFootprintQuestion("R_0603", "Mine"), "Delete footprint 'R_0603' from library 'Mine'?");
  assert.equal(footprintDeletedStatus("R_0603", "Mine"), "Footprint 'R_0603' deleted from library 'Mine'");
  assert.equal(footprintLibraryReadOnly("Mine"), "Library 'Mine' is read only.");
  assert.equal(newFootprintReadOnlyWarning("Mine"), "The footprint could not be added to the selected library ('Mine'). This library is read-only.");
  assert.equal(readOnlyFootprintBanner("Mine"), "Editing footprint from read-only library Mine.");
  assert.equal(SAVE_AS_EDITABLE_COPY, "Save as editable copy");
});

test("New Footprint goes into the target library under Untitled made unique there", () => {
  // `GetTargetFPID()`: the tree's footprint, else its library row, else the loaded footprint
  assert.equal(newFootprintLibrary("Fp:Chip", ["Other"], "Mine:X"), "Fp");
  assert.equal(newFootprintLibrary(null, ["Other"], "Mine:X"), "Other");
  assert.equal(newFootprintLibrary(null, [], "Mine:X"), "Mine");
  assert.equal(newFootprintLibrary(null, [], null), "");
  const taken = new Set(["Untitled", "Untitled_1"]);
  assert.equal(uniqueNewFootprintName((n) => taken.has(n)), "Untitled_2");
  assert.equal(uniqueNewFootprintName(() => false), "Untitled");
});
