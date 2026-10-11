import { test } from "node:test";
import assert from "node:assert/strict";
import {
  appendRow,
  baseName,
  deleteRows,
  emptyRow,
  fixNickname,
  fixVariableName,
  flagOffers,
  guessType,
  isVariableName,
  libraryExtension,
  moveRow,
  nicknameForFile,
  pathSubstitutions,
  sameRows,
  sameVariables,
  setCell,
  setFlag,
  statusTitle,
  uniqueLibraryName,
  unwritable,
  validateVariables,
  variablesUsed,
  verifyRows,
  type TableRow,
} from "./libraryTable";

const row = (nickname: string, uri: string, extra: Partial<TableRow> = {}): TableRow => ({ ...emptyRow(), nickname, uri, type: "KiCad", ...extra });

test("a nickname is the file's name; a footprint library that is not a .pretty keeps its whole name (browseLibrariesHandler, AddLibrary)", () => {
  assert.equal(nicknameForFile("symbol", "My Parts.kicad_sym"), "My Parts");
  assert.equal(nicknameForFile("symbol", "Device.kicad_sym"), "Device");
  assert.equal(nicknameForFile("footprint", "Snap.pretty"), "Snap");
  assert.equal(nicknameForFile("footprint", "plain_folder"), "plain_folder");
  assert.equal(nicknameForFile("footprint", "folder.v2"), "folder.v2", "not a .pretty: the whole name");
  assert.equal(nicknameForFile("symbol", "a:b.kicad_sym"), "a_b", "LIB_ID::FixIllegalChars");
  assert.equal(fixNickname("x\\y:z\tq"), "x_y_z_q");
  assert.equal(fixNickname("Plain.name (v2)"), "Plain.name (v2)");
  assert.equal(libraryExtension("symbol"), ".kicad_sym");
  assert.equal(libraryExtension("footprint"), ".pretty");
  assert.equal(baseName("/a/b/Snap.pretty/"), "Snap.pretty");
  assert.equal(baseName("C:\\libs\\x.kicad_sym"), "x.kicad_sym");
});

test("New Library names itself New_Library, then New_Library_0, _1 (GetUniqueLibraryName)", () => {
  assert.equal(uniqueLibraryName([]), "New_Library");
  assert.equal(uniqueLibraryName(["New_Library"]), "New_Library_0");
  assert.equal(uniqueLibraryName(["New_Library", "New_Library_0", "New_Library_2"]), "New_Library_1");
});

test("setting a path gives the row the format it says, a library table file is a chained table (SetValue)", () => {
  assert.equal(guessType("symbol", "${KIPRJMOD}/libs/Mine.kicad_sym"), "KiCad");
  assert.equal(guessType("footprint", "/x/Mine.pretty"), "KiCad");
  assert.equal(guessType("symbol", "/x/Mine.pretty"), null);
  assert.equal(guessType("footprint", "/x/fp-lib-table"), "Table");
  assert.equal(guessType("symbol", "/x/sym-lib-table"), "Table");
  const checked: TableRow = { ...row("A", "/old"), status: "error", error: "gone", path: "/old" };
  const moved = setCell(checked, "uri", "/new/Mine.kicad_sym", "symbol");
  assert.equal(moved.uri, "/new/Mine.kicad_sym");
  assert.equal(moved.type, "KiCad");
  assert.deepEqual([moved.status, moved.error, moved.path], [undefined, undefined, undefined], "what was found out about the old path is gone");
  assert.equal(setCell(emptyRow(), "uri", "/x/no_extension", "symbol").type, "", "no guess, the format stays what it was");
  const named = setCell(checked, "nickname", "B", "symbol");
  assert.equal(named.status, "error", "a new nickname does not change what the path says");
  assert.equal(setCell({ ...checked, type: "KiCad" }, "type", "Legacy", "symbol").status, undefined);
});

test("rows are appended, deleted and moved the way the buttons do", () => {
  const rows = [row("A", "/a"), row("B", "/b"), row("C", "/c")];
  assert.equal(appendRow(rows).length, 4);
  assert.deepEqual(appendRow(rows)[3], emptyRow());
  assert.deepEqual(deleteRows(rows, [0, 2]).map((r) => r.nickname), ["B"]);
  assert.deepEqual(deleteRows(rows, []).map((r) => r.nickname), ["A", "B", "C"]);
  assert.deepEqual(moveRow(rows, 1, -1).rows.map((r) => r.nickname), ["B", "A", "C"]);
  assert.equal(moveRow(rows, 1, -1).at, 0);
  assert.deepEqual(moveRow(rows, 1, 1).rows.map((r) => r.nickname), ["A", "C", "B"]);
  assert.deepEqual(moveRow(rows, 0, -1), { rows, at: 0 }, "the first row does not move up");
  assert.deepEqual(moveRow(rows, 2, 1), { rows, at: 2 }, "nor the last down");
});

test("Activate and Deactivate Selected, Set and Unset Visible Flag, and what the menu offers for a selection", () => {
  const rows = [row("A", "/a"), row("B", "/b", { enabled: false }), row("C", "/c", { visible: false })];
  assert.deepEqual(setFlag(rows, [0, 1], "enabled", true).map((r) => r.enabled), [true, true, true]);
  assert.deepEqual(setFlag(rows, [0], "enabled", false).map((r) => r.enabled), [false, false, true]);
  assert.deepEqual(setFlag(rows, [0, 1, 2], "visible", true).map((r) => r.visible), [true, true, true]);
  assert.deepEqual(flagOffers(rows, [0]), { activate: false, deactivate: true, show: false, hide: true });
  assert.deepEqual(flagOffers(rows, [0, 1]), { activate: true, deactivate: true, show: false, hide: true });
  assert.deepEqual(flagOffers(rows, [2]), { activate: false, deactivate: true, show: true, hide: false });
});

test("OK drops a row with no path, trims, and stops at a missing or illegal nickname or a duplicate (VerifyTable)", () => {
  const ok = verifyRows([row(" A ", " /a "), row("", ""), row("B", "/b")], true);
  assert.ok("rows" in ok);
  assert.deepEqual(ok.rows.map((r) => [r.nickname, r.uri]), [["A", "/a"], ["B", "/b"]]);
  const none = verifyRows([row("A", "/a"), row("", "/b")], true);
  assert.deepEqual("problem" in none && none.problem, { row: 1, column: "nickname", message: "Library must have a nickname." });
  const illegal = verifyRows([row("A:B", "/a")], true);
  assert.equal("problem" in illegal && illegal.problem.message, "Illegal character ':' in nickname 'A:B'.");
  const dup = verifyRows([row("A", "/a"), row("Z", ""), row("B", "/b"), row("A", "/c")], true);
  assert.deepEqual("problem" in dup && dup.problem, { row: 3, column: "nickname", message: "Multiple libraries cannot share the same nickname ('A')." });
  // a table without a Show column keeps no hidden flag
  const hidden = row("A", "/a", { visible: false });
  assert.equal("rows" in verifyRows([hidden], false) && (verifyRows([hidden], false) as { rows: TableRow[] }).rows[0]?.visible, true);
  assert.equal((verifyRows([hidden], true) as { rows: TableRow[] }).rows[0]?.visible, false);
});

test("what the table format cannot write is refused where it is typed", () => {
  assert.equal(unwritable([row("A", "/a")]), null);
  assert.deepEqual(unwritable([row("A", "/a"), row("B", "/b", { descr: 'say "hi"' })]), {
    row: 1,
    column: "descr",
    message: "A description cannot hold a double quote or a line break: KiCad's library table has no way to write one.",
  });
  assert.equal(unwritable([row("A", "/a\n")])?.column, "uri");
});

test("two tables are the same when no field the file holds differs", () => {
  const a = [row("A", "/a"), row("B", "/b", { descr: "x" })];
  assert.equal(sameRows(a, [row("A", "/a"), row("B", "/b", { descr: "x" })]), true);
  assert.equal(sameRows(a, [row("A", "/a")]), false);
  assert.equal(sameRows(a, [row("A", "/a"), row("B", "/b", { descr: "y" })]), false);
  assert.equal(sameRows(a, [row("A", "/a"), row("B", "/b", { descr: "x", enabled: false })]), false);
  assert.equal(sameRows(a, [row("A", "/a"), { ...row("B", "/b", { descr: "x" }), status: "error", error: "?" }]), true, "what the server found out is not part of the table");
});

test("the status column says why a library cannot be read, or that a row chains a table", () => {
  assert.equal(statusTitle({ ...row("A", "/a"), status: "error", error: "Library '/a' not found." }), "Library '/a' not found.");
  assert.equal(statusTitle({ ...row("KiCad", "/t"), type: "Table", status: "table" }), "Open library table");
  assert.equal(statusTitle(row("A", "/a")), "");
});

test("variables are found in both spellings, each once, in order", () => {
  assert.deepEqual(variablesUsed("${A}/x/${B}/$(C)/${A}"), ["A", "B", "C"]);
  assert.deepEqual(variablesUsed("/plain/path"), []);
  assert.deepEqual(variablesUsed("${unclosed"), []);
});

test("the path substitutions list what the tables use, KIPRJMOD and the folder variable of the kind, each with its value, sorted", () => {
  const values = [
    { name: "KIPRJMOD", value: "/work/board" },
    { name: "KICAD10_SYMBOL_DIR", value: "/K/symbols" },
    { name: "TEAM", value: "/shared" },
  ];
  assert.deepEqual(pathSubstitutions(["${TEAM}/x.kicad_sym", "${OLD_DIR}/y"], "symbol", 10, values), [
    { name: "KICAD10_SYMBOL_DIR", value: "/K/symbols" },
    { name: "KIPRJMOD", value: "/work/board" },
    { name: "OLD_DIR", value: "" },
    { name: "TEAM", value: "/shared" },
  ]);
  assert.deepEqual(pathSubstitutions([], "footprint", 10, []).map((v) => v.name), ["KICAD10_FOOTPRINT_DIR", "KIPRJMOD"]);
});

test("a variable's name is capitals, digits and underscores, typing a lower-case letter gives the capital (ENV_VAR_NAME_VALIDATOR)", () => {
  assert.equal(fixVariableName("my_libs-2 v3"), "MY_LIBS2V3");
  assert.equal(fixVariableName("9lives"), "LIVES");
  assert.equal(fixVariableName("A9"), "A9");
  assert.ok(isVariableName("KICAD10_SYMBOL_DIR") && isVariableName("_X"));
  assert.ok(!isVariableName("lower") && !isVariableName("9X") && !isVariableName("") && !isVariableName("A B"));
});

test("Configure Paths skips an empty row and refuses a name without a path, a path without a name, KIPRJMOD and a repeated name", () => {
  const ok = validateVariables([
    { name: " TEAM_LIBS ", value: " /srv/libs " },
    { name: "", value: "" },
    { name: "OTHER", value: "/o" },
  ]);
  assert.deepEqual("variables" in ok && ok.variables, [
    { name: "TEAM_LIBS", value: "/srv/libs" },
    { name: "OTHER", value: "/o" },
  ]);
  const problem = (rows: { name: string; value: string }[]) => {
    const r = validateVariables(rows);
    return "problem" in r ? r.problem : null;
  };
  assert.deepEqual(problem([{ name: "", value: "/x" }]), { row: 0, column: "name", message: "Environment variable name cannot be empty." });
  assert.deepEqual(problem([{ name: "A", value: "" }]), { row: 0, column: "value", message: "Environment variable path cannot be empty." });
  assert.equal(problem([{ name: "KIPRJMOD", value: "/x" }])?.message, "The name KIPRJMOD is reserved, and cannot be used.");
  assert.equal(problem([{ name: "a b", value: "/x" }])?.message, "'a b' is not a variable name: capital letters, digits (not first) and underscores only.");
  assert.deepEqual(problem([{ name: "A", value: "/x" }, { name: "A", value: "/y" }]), { row: 1, column: "name", message: "Two variables are named A." });
  assert.equal(problem([{ name: "A", value: 'a"b' }])?.column, "value");
  assert.ok(sameVariables([{ name: "A", value: "1" }], [{ name: "A", value: "1" }]));
  assert.ok(!sameVariables([{ name: "A", value: "1" }], [{ name: "A", value: "2" }]));
});
