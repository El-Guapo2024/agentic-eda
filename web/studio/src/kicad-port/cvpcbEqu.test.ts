import { test } from "node:test";
import assert from "node:assert/strict";
import { addEquFiles, autoAssociateInput, changeFocus, cvpcbHotkey, equEntryFor, moveEquDown, moveEquUp, nextUnassigned, relativeTo, removeEqu, viewerCaption, viewerFootprint } from "./cvpcbEqu";

test("the viewer shows the picked footprint, else the selected symbol's (InitDisplay)", () => {
  assert.equal(viewerFootprint("Resistor_SMD:R_0603_1608Metric", "Old:Fp"), "Resistor_SMD:R_0603_1608Metric");
  assert.equal(viewerFootprint(null, "Old:Fp"), "Old:Fp");
  assert.equal(viewerFootprint("", null), "");
  assert.deepEqual(viewerCaption("Resistor_SMD:R_0603_1608Metric", "Resistor_SMD"), { title: "Footprint: Resistor_SMD:R_0603_1608Metric", status: "Lib: Resistor_SMD" });
  assert.deepEqual(viewerCaption("", undefined), { title: "", status: "" });
});

test("Add writes a file under ${KIPRJMOD}, climbing out of it when it must (OnAddFiles, MakeRelativeTo)", () => {
  const vars = [
    { name: "KIPRJMOD", value: "/work/board" },
    { name: "KICAD10_FOOTPRINT_DIR", value: "/usr/share/kicad/footprints" },
  ];
  assert.equal(relativeTo("/work/board/a/x.equ", "/work/board"), "a/x.equ");
  assert.equal(relativeTo("/usr/share/kicad/footprints/std.equ", "/work/board"), "../../usr/share/kicad/footprints/std.equ");
  assert.equal(relativeTo("x.equ", "/work"), null);
  assert.equal(equEntryFor("/work/board/mine.equ", vars), "${KIPRJMOD}/mine.equ");
  assert.equal(equEntryFor("/usr/share/kicad/footprints/std.equ", vars), "${KIPRJMOD}/../../usr/share/kicad/footprints/std.equ", "the project's folder takes every file on Unix");
  assert.equal(equEntryFor("/usr/share/kicad/footprints/std.equ", [{ name: "KIPRJMOD", value: "" }, vars[1]!]), "${KICAD10_FOOTPRINT_DIR}/std.equ");
  assert.equal(equEntryFor("C:\\x\\y.equ", vars), "C:/x/y.equ", "unix separators only");
  const added = addEquFiles(["${KIPRJMOD}/mine.equ"], ["/work/board/mine.equ", "/work/board/two.equ"], vars);
  assert.deepEqual(added.list, ["${KIPRJMOD}/mine.equ", "${KIPRJMOD}/two.equ"]);
  assert.deepEqual(added.errors, ["File '${KIPRJMOD}/mine.equ' already exists in list."]);
});

test("Move Up, Move Down and Remove act on the selection as the dialog's buttons do", () => {
  const list = ["a", "b", "c", "d"];
  assert.deepEqual(moveEquUp(list, [1, 3]), { list: ["b", "a", "d", "c"], selected: [0, 2] });
  assert.deepEqual(moveEquUp(list, [0, 2]).list, list, "the first one selected: nothing moves");
  assert.deepEqual(moveEquDown(list, [0, 2]), { list: ["b", "a", "d", "c"], selected: [1, 3] });
  assert.deepEqual(moveEquDown(list, [3]).list, list, "the last one selected: nothing moves");
  assert.deepEqual(moveEquDown(list, [1, 2]), { list: ["a", "d", "b", "c"], selected: [2, 3] });
  assert.deepEqual(removeEqu(list, [0, 2]), ["b", "d"]);
});

test("Automatically Assign Footprints sees the dialog's staged assignments", () => {
  const rows = [
    { ref: "R1", value: "10k", footprint: "", libId: "Device:R" },
    { ref: "R2", value: "10k", footprint: "Old:Fp", libId: "Device:R" },
  ];
  assert.deepEqual(autoAssociateInput(rows, { R1: "New:Fp", R2: "" }), [
    { ref: "R1", value: "10k", footprint: "New:Fp", lib_id: "Device:R" },
    { ref: "R2", value: "10k", footprint: "", lib_id: "Device:R" },
  ]);
});

test("CvPcb's hotkeys: Tab and Shift+Tab move between the lists, Enter assigns and moves to the next unassigned symbol, Delete, Ctrl+S (UI-LEFT-2 LL7)", () => {
  const k = (key: string, m: Partial<{ shiftKey: boolean; ctrlKey: boolean }> = {}) => cvpcbHotkey({ key, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...m });
  assert.equal(k("Tab"), "focusRight");
  assert.equal(k("Tab", { shiftKey: true }), "focusLeft");
  assert.equal(k("Enter"), "associate");
  assert.equal(k("Delete"), "deleteAssoc");
  assert.equal(k("s", { ctrlKey: true }), "save");
  assert.equal(k("a"), null);
  assert.equal(changeFocus("library", "right"), "component");
  assert.equal(changeFocus("component", "right"), "footprint");
  assert.equal(changeFocus("footprint", "right"), "library");
  assert.equal(changeFocus("library", "left"), "footprint");
  assert.equal(changeFocus("component", "left"), "library");
  assert.equal(changeFocus(null, "left"), null);
  const rows = [{ ref: "C1" }, { ref: "R1" }, { ref: "R2" }, { ref: "U1" }];
  const fp: Record<string, string> = { C1: "", R1: "R:0603", R2: "", U1: "" };
  assert.equal(nextUnassigned(rows, (r) => fp[r]!, ["R1"]), "R2");
  assert.equal(nextUnassigned(rows, (r) => fp[r]!, ["R2"]), "U1");
  assert.equal(nextUnassigned(rows, (r) => fp[r]!, ["U1"]), null, "none after the last: the selection stays");
  assert.equal(nextUnassigned(rows, (r) => fp[r]!, []), null, "nothing selected: nothing moves (newSel = UINT_MAX)");
});
