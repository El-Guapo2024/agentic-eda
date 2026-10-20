import { test } from "node:test";
import assert from "node:assert/strict";
import type { Block, BlockLibrary } from "../api/designBlockClient";
import { blockContextMenu, blockId, cleanFields, composeBlockGroups, findBlock, groupNameFor, linkProblem, nameProblem, parseBlockId, placeProblem, pushHistory } from "./designBlocks";
import { visibleRows } from "./libChooser";

const block = (name: string, extra: Partial<Block> = {}): Block => ({ name, description: "", keywords: "", fields: [], schematic: true, board: false, schematic_path: `/lib/${name}.kicad_sch`, board_path: null, ...extra });
const lib = (name: string, blocks: Block[], extra: Partial<BlockLibrary> = {}): BlockLibrary => ({ name, description: "", scope: "project", path: `/blocks/${name}.kicad_blocks`, writable: true, blocks, ...extra });

const LIBS = [
  lib("Sensors", [block("Temp 10", { keywords: "thermistor ntc" }), block("Temp 2", { description: "Digital temperature" })]),
  lib("Power", [block("Buck5V", { keywords: "regulator buck 5V", description: "A 5 V buck converter" }), block("LDO3V3", { keywords: "regulator ldo" })], { description: "Regulators" }),
  lib("Empty", []),
];

test("the tree lists the libraries by name and their blocks naturally, with the recently used first", () => {
  const groups = composeBlockGroups({ libraries: LIBS, query: "", history: ["Power:Buck5V", "Gone:Block"], pinned: new Set() });
  assert.deepEqual(groups.map((g) => g.lib), ["-- Recently Used --", "Empty", "Power", "Sensors"]);
  assert.deepEqual(groups[0]?.items.map((i) => i.id), ["Power:Buck5V"], "a block that is gone from its library leaves the history");
  assert.deepEqual(groups[3]?.items.map((i) => i.name), ["Temp 2", "Temp 10"], "natural order");
  assert.equal(groups[2]?.description, "Regulators");
  assert.equal(groups[1]?.count, 0);
});

test("a pinned library goes first among the libraries", () => {
  const groups = composeBlockGroups({ libraries: LIBS, query: "", history: [], pinned: new Set(["Sensors"]) });
  assert.deepEqual(groups.map((g) => g.lib), ["Sensors", "Empty", "Power"]);
  assert.equal(groups[0]?.pinned, true);
});

test("a search scores the name, the library, the keywords and the description and hides what has no score", () => {
  const groups = composeBlockGroups({ libraries: LIBS, query: "regulator", history: ["Sensors:Temp 2"], pinned: new Set() });
  assert.deepEqual(groups.map((g) => g.lib), ["Power"], "the recent block does not match, the other libraries have nothing");
  assert.deepEqual(groups[0]?.items.map((i) => i.name).sort(), ["Buck5V", "LDO3V3"]);
  const byName = composeBlockGroups({ libraries: LIBS, query: "buck", history: [], pinned: new Set() });
  assert.deepEqual(byName[0]?.items.map((i) => i.name), ["Buck5V"], "only the block with the word");
  const two = composeBlockGroups({ libraries: LIBS, query: "temp digital", history: [], pinned: new Set() });
  assert.deepEqual(two[0]?.items.map((i) => i.name), ["Temp 2"], "every word has to score");
  const lib = composeBlockGroups({ libraries: LIBS, query: "sensors", history: [], pinned: new Set() });
  assert.equal(lib[0]?.items.length, 2, "the nickname is a term");
  assert.deepEqual(composeBlockGroups({ libraries: LIBS, query: "zzz", history: [], pinned: new Set() }), []);
});

test("the rows are a library row and its blocks while it is open", () => {
  const groups = composeBlockGroups({ libraries: LIBS, query: "", history: [], pinned: new Set() });
  const rows = visibleRows(groups, { groups: new Set(["Power"]), items: new Set(), searching: false }, "footprint");
  assert.deepEqual(rows.map((r) => (r.kind === "group" ? `G ${r.group.lib}` : r.kind === "item" ? `I ${r.item.id}` : "?")), ["G Empty", "G Power", "I Power:Buck5V", "I Power:LDO3V3", "G Sensors"]);
});

test("ids, history, names and links follow KiCad's rules", () => {
  assert.equal(blockId("Power", "Buck5V"), "Power:Buck5V");
  assert.deepEqual(parseBlockId("Power:Buck5V"), { lib: "Power", name: "Buck5V" });
  assert.deepEqual(parseBlockId("Power:"), { lib: "Power", name: "" });
  assert.deepEqual(parseBlockId(null), { lib: "", name: "" });
  assert.equal(findBlock(LIBS, "Power:Buck5V")?.name, "Buck5V");
  assert.equal(findBlock(LIBS, "Power:"), undefined);
  assert.equal(findBlock(LIBS, "Power:Nope"), undefined);
  // seven (`while( size() >= 8 ) pop_back()`), newest first, no duplicate
  let h: string[] = [];
  for (let i = 0; i < 10; i++) h = pushHistory(h, `L:B${i}`);
  assert.equal(h.length, 7);
  assert.equal(h[0], "L:B9");
  assert.deepEqual(pushHistory(["a:1", "a:2", "a:3"], "a:2"), ["a:2", "a:1", "a:3"]);
  // the group's name; repeated copies count up
  assert.equal(groupNameFor("Buck", false, 1), "Buck");
  assert.equal(groupNameFor("Buck", true, 3), "Buck3");
  // what cannot be placed
  assert.equal(placeProblem(block("A", { schematic: false }), "schematic"), "Design block has no schematic to place.");
  assert.equal(placeProblem(block("A", { board: false }), "pcb"), "Design block has no layout to place.");
  assert.equal(placeProblem(block("A", { board: true }), "pcb"), null);
  assert.equal(placeProblem(undefined, "pcb", "Power:LDO"), "Could not find design block Power:LDO.");
  // names
  assert.equal(nameProblem("Good name-1"), null);
  assert.equal(nameProblem("a/b"), "Illegal character '/' in name 'a/b'.");
  assert.equal(nameProblem("a:b"), "Illegal character ':' in name 'a:b'.");
  assert.equal(nameProblem(""), "Design block does not have a valid library ID.");
  assert.ok(nameProblem("v1.2")?.startsWith("Illegal character '.'"));
  // fields
  assert.deepEqual(cleanFields([{ name: "  Part \n Number ", value: "x" }, { name: "A", value: "" }]), { fields: [{ name: "Part Number", value: "x" }, { name: "A", value: "" }] });
  assert.deepEqual(cleanFields([{ name: "A", value: "1" }, { name: " A ", value: "2" }]), { problem: "Duplicate fields are not allowed." });
  // the library link of Group Properties
  assert.equal(linkProblem(""), null);
  assert.equal(linkProblem("Power:Buck"), null);
  assert.equal(linkProblem("Power"), "Invalid library link: 'Power'");
  assert.equal(linkProblem(":x"), "Invalid library link: ':x'");
});

const labels = (items: { id?: string; label: string; separator?: boolean }[]) => items.map((i) => (i.separator ? "-" : i.id));

test("the menu of a design block row has KiCad's entries in the order of their priorities, with a separator only between groups", () => {
  assert.deepEqual(labels(blockContextMenu("schematic", { kind: "item" }, true)), [
    "newLibrary", "-", "place", "-", "properties", "saveSheet", "saveSelection", "updateSheet", "updateSelection", "delete", "-", "hide",
  ]);
  // nothing selected in the editor: the two selection entries are not offered
  assert.deepEqual(labels(blockContextMenu("pcb", { kind: "item" }, false)), ["newLibrary", "-", "place", "-", "properties", "saveSheet", "updateSheet", "delete", "-", "hide"]);
});

test("the menu of a library row pins or unpins, saves into the library, and has no block entries", () => {
  assert.deepEqual(labels(blockContextMenu("schematic", { kind: "library", pinned: false }, false)), ["pin", "newLibrary", "-", "saveSheet", "-", "hide"]);
  assert.deepEqual(labels(blockContextMenu("schematic", { kind: "library", pinned: true }, true)), ["unpin", "newLibrary", "-", "saveSheet", "saveSelection", "-", "hide"]);
  // the Recently Used group is no library to pin
  assert.deepEqual(labels(blockContextMenu("schematic", { kind: "library", pseudo: true }, false)), ["newLibrary", "-", "saveSheet", "-", "hide"]);
});

test("the empty tree still offers New Library and Hide Library Tree", () => {
  assert.deepEqual(labels(blockContextMenu("pcb", { kind: "none" }, true)), ["newLibrary", "-", "hide"]);
});

test("the sheet and board entries are named for their editor", () => {
  const find = (editor: "schematic" | "pcb", id: string) => blockContextMenu(editor, { kind: "item" }, true).find((i) => i.id === id)?.label;
  assert.equal(find("schematic", "saveSheet"), "Save Current Sheet as Design Block...");
  assert.equal(find("pcb", "saveSheet"), "Save Board as Design Block...");
  assert.equal(find("schematic", "updateSheet"), "Update Design Block from Current Sheet");
  assert.equal(find("pcb", "updateSheet"), "Update Design Block from Board");
});
