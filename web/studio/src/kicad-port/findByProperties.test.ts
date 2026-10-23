import { test } from "node:test";
import assert from "node:assert/strict";
import { anyActive, buildRows, checkQuery, collectAllBoardItems, completionNames, exprFieldName, generateExpression, matchedText, normalizeQueryFieldAliases, queryUsesPairwiseSyntax, savedModes, selectMatchingFromQuery, selectMatchingFromRows, withRecentQuery, type FindRow, type MatchMode } from "./findByProperties";
import { pcbExprBoard } from "./pcbExprFixture";

const board = pcbExprBoard();
const rowsOf = (ids: string[], saved: ReadonlyMap<string, MatchMode> = new Map()) => buildRows(board, ids, "mm", saved);
const setMode = (rows: FindRow[], name: string, mode: MatchMode): void => {
  const row = rows.find((r) => r.name === name);
  assert.ok(row, `a row ${name}`);
  row.mode = mode;
};
const names = (rows: readonly FindRow[]): string[] => rows.map((r) => r.name);
const row = (rows: readonly FindRow[], name: string): FindRow => {
  const r = rows.find((x) => x.name === name);
  assert.ok(r, `a row ${name}`);
  return r;
};

// ----------------------------------------------------------------------------------------------------------------------------------- the rows

test("one track: its visible properties in the alphabetical order of their names, with Type, and no hidden ones", () => {
  const b = rowsOf(["trk_a"]);
  assert.equal(b.caption, "Track");
  assert.deepEqual(names(b.rows), ["End X", "End Y", "Layer", "Locked", "Net", "Start X", "Start Y", "Type", "Width"]);
  assert.equal(row(b.rows, "Type").display, "Track");
  assert.equal(row(b.rows, "Width").display, "0.2 mm");
  assert.equal(row(b.rows, "Width").raw, 200);
  assert.equal(row(b.rows, "Layer").display, "F.Cu");
  assert.equal(row(b.rows, "Net").display, "GND");
  assert.equal(row(b.rows, "Locked").display, "false");
  assert.equal(row(b.rows, "Start X").display, "10 mm");
  assert.ok(b.rows.every((r) => r.mode === "ignored" && !r.mixed));
  assert.equal(anyActive(b.rows), false);
  assert.deepEqual(b.types, ["PCB_TRACK"]);
});

test("a selection of tracks that agree on a value shows it, those that differ show <...> and cannot be matched", () => {
  const b = rowsOf(["trk_a", "trk_b", "trk_c"]);
  assert.equal(b.caption, "3 objects selected");
  assert.equal(row(b.rows, "Layer").mixed, true, "F.Cu, F.Cu and B.Cu");
  assert.equal(row(b.rows, "Layer").display, "<...>");
  assert.equal(row(b.rows, "Layer").raw, null);
  assert.equal(row(b.rows, "Width").mixed, true);
  const same = rowsOf(["trk_a", "trk_b"]);
  assert.equal(row(same.rows, "Layer").mixed, false);
  assert.equal(row(same.rows, "Layer").display, "F.Cu");
});

test("items of different classes: the properties they all have by name", () => {
  const b = rowsOf(["trk_a", "via_a"]);
  assert.deepEqual(names(b.rows), ["Locked", "Net", "Type"]);
  assert.deepEqual(b.types, ["PCB_TRACK", "PCB_VIA"]);
  assert.equal(row(b.rows, "Type").mixed, true);
});

test("footprints: their fields that the properties do not show are rows of their own, after the properties", () => {
  const b = rowsOf(["U1", "R1"]);
  const last = b.rows[b.rows.length - 1] as FindRow;
  assert.equal(last.name, "Supplier");
  assert.equal(last.prop, null);
  assert.equal(last.raw, "ACME");
  assert.equal(last.display, "ACME");
  assert.ok(!names(b.rows).includes("Reference") || row(b.rows, "Reference").prop !== null, "Reference is the property, not a field row");
  const mixed = rowsOf(["U1", "R2"]);
  assert.equal(row(mixed.rows, "Supplier").mixed, true);
  assert.equal(rowsOf(["U1", "trk_a"]).rows.some((r) => r.prop === null), false, "only a selection of footprints has field rows");
});

test("nothing selected, and an id the board lacks", () => {
  assert.deepEqual(rowsOf([]), { caption: "No items selected", rows: [], types: [] });
  assert.deepEqual(rowsOf(["nope"]).rows, []);
});

test("the Match modes of the last build are kept for the rows of the same name that are not mixed", () => {
  const first = rowsOf(["trk_a"]);
  setMode(first.rows, "Width", "matching");
  setMode(first.rows, "Net", "different");
  assert.equal(anyActive(first.rows), true);
  const saved = savedModes(first.rows);
  assert.deepEqual([...saved.entries()], [["Net", "different"], ["Width", "matching"]]);
  const next = rowsOf(["trk_b"], saved);
  assert.equal(row(next.rows, "Width").mode, "matching");
  assert.equal(row(next.rows, "Net").mode, "different");
  const mixed = rowsOf(["trk_a", "trk_b"], saved);
  assert.equal(row(mixed.rows, "Width").mode, "ignored", "a mixed row cannot be matched");
});

// ------------------------------------------------------------------------------------------------------------------------------- selecting

test("Select Matching from the rows: the class of the selection and each row that is Matching or Different", () => {
  const b = rowsOf(["trk_a"]);
  setMode(b.rows, "Width", "matching");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["trk_a"], "an arc is another class, so is not a match");
  setMode(b.rows, "Width", "ignored");
  setMode(b.rows, "Net", "different");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["trk_b", "trk_p", "trk_n"]);
  setMode(b.rows, "Net", "matching");
  setMode(b.rows, "Layer", "matching");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["trk_a"], "GND on F.Cu");
  setMode(b.rows, "Layer", "ignored");
  setMode(b.rows, "Net", "ignored");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["trk_a", "trk_b", "trk_c", "trk_p", "trk_n"], "no row: every item of the class");
});

test("Select Matching on a field of a footprint", () => {
  const b = rowsOf(["U1", "R1"]);
  setMode(b.rows, "Supplier", "matching");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["U1", "R1"]);
  setMode(b.rows, "Supplier", "different");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["R2"]);
});

test("a pad row: the pads like it", () => {
  const b = rowsOf(["U1.1"]);
  assert.equal(b.caption, "Pad");
  setMode(b.rows, "Pad Type", "matching");
  setMode(b.rows, "Net", "matching");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["U1.1", "R1.1"], "the SMD pads on GND");
});

test("everything the board has is searched: tracks, vias, footprints with pads and fields, shapes, texts, zones; groups are not", () => {
  const all = collectAllBoardItems(board).map((i) => i.id);
  for (const id of ["trk_a", "via_b", "U1", "U1.3", "U1:Supplier", "R2", "R2.1", "shp_rect", "txt_a", "zone_ra"]) assert.ok(all.includes(id), id);
  assert.equal(all.includes("grp_a"), false);
  assert.equal(all.includes("C9"), false, "a part not placed is not on the board");
  assert.equal(new Set(all).size, all.length);
});

test("the status line", () => {
  assert.equal(matchedText(0), "0 items matched");
  assert.equal(matchedText(5), "5 items matched");
});

// ------------------------------------------------------------------------------------------------------------------------ the generated query

test("Create from Selection Tab: the rows that are not Ignored, a length with its unit, a choice quoted, a flag as true or false", () => {
  const b = rowsOf(["trk_a"]);
  setMode(b.rows, "Width", "matching");
  setMode(b.rows, "Net", "matching");
  setMode(b.rows, "Layer", "different");
  setMode(b.rows, "Locked", "matching");
  setMode(b.rows, "Type", "matching");
  assert.equal(generateExpression(b.rows, board, ["trk_a"], "mm"), "A.Layer != 'F.Cu' && A.Locked == false && A.Net == 'GND' && A.Type == 'Track' && A.Width == 0.2mm");
  assert.equal(generateExpression(rowsOf(["trk_a"]).rows, board, ["trk_a"], "mm"), "", "nothing active: nothing to write");
});

test("the generated query selects what the rows select", () => {
  const b = rowsOf(["via_a"]);
  setMode(b.rows, "Via Type", "matching");
  setMode(b.rows, "Hole", "matching");
  const q = generateExpression(b.rows, board, ["via_a"], "mm");
  assert.equal(q, "A.Hole == 0.3mm && A.Via_Type == 'Through'");
  assert.deepEqual(selectMatchingFromRows(board, b.rows, b.types, "mm"), ["via_a"]);
  assert.deepEqual(selectMatchingFromQuery(q, b.rows, board, "mm").ids, ["via_a"]);
  // in mil
  const m = rowsOf(["trk_a"]);
  setMode(m.rows, "Width", "matching");
  assert.equal(generateExpression(m.rows, board, ["trk_a"], "mil"), "A.Width == 7.87mil");
});

test("a footprint field row is written A.getField('Name')", () => {
  const b = rowsOf(["U1"]);
  setMode(b.rows, "Supplier", "matching");
  assert.equal(generateExpression(b.rows, board, ["U1"], "mm"), "A.getField('Supplier') == 'ACME'");
  assert.deepEqual(selectMatchingFromQuery("A.getField('Supplier') == 'ACME'", b.rows, board, "mm").ids, ["U1", "R1"]);
});

// ------------------------------------------------------------------------------------------------------------------------------ the query tab

test("field aliases: A.Reference and a field's name become getField, outside quotes and whole words only", () => {
  const rows = rowsOf(["U1", "R1"]).rows;
  assert.equal(normalizeQueryFieldAliases("A.Reference == 'U1'", rows), "A.getField('Reference') == 'U1'");
  assert.equal(normalizeQueryFieldAliases("A.Supplier == 'A.Supplier'", rows), "A.getField('Supplier') == 'A.Supplier'");
  assert.equal(normalizeQueryFieldAliases("A.ReferenceX == 1", rows), "A.ReferenceX == 1");
  assert.equal(normalizeQueryFieldAliases("xA.Reference == 1", rows), "xA.Reference == 1");
  assert.equal(normalizeQueryFieldAliases("A.Datasheet == ''", rows), "A.getField('Datasheet') == ''");
  assert.equal(normalizeQueryFieldAliases("A.Width > 1mm", rows), "A.Width > 1mm");
});

test("B. is not supported, but a B inside a name or a string is", () => {
  assert.equal(queryUsesPairwiseSyntax("A.Width > B.Width"), true);
  assert.equal(queryUsesPairwiseSyntax("A.Net == 'B.Cu'"), false);
  assert.equal(queryUsesPairwiseSyntax("AB.isCoupledDiffPair()"), false);
  assert.equal(queryUsesPairwiseSyntax("A.Width > 1mm && (B.Net == A.Net)"), true);
});

test("Check Syntax: what the dialog's status line says", () => {
  const rows: FindRow[] = [];
  assert.equal(checkQuery("", rows, board, "mm").status, "Expression is empty");
  assert.equal(checkQuery("   ", rows, board, "mm").status, "Expression is empty");
  assert.equal(checkQuery("A.Width > 0.1mm", rows, board, "mm").status, "Syntax OK");
  assert.equal(checkQuery("A.Width >", rows, board, "mm").status, "Syntax error: Syntax error");
  assert.equal(checkQuery("A.Width == B.Width", rows, board, "mm").status, "B. expressions are not supported.");
  assert.equal(checkQuery("A.Width ?? 1", rows, board, "mm").status, "Syntax error: Unrecognized character '?'", "the text so far is whole: the bad character is the only error");
  assert.equal(checkQuery("A.Width == ?", rows, board, "mm").status, "Syntax error: Unrecognized character '?'\nSyntax error", "and here the text is cut short as well");
  // an unknown property is reported but the text still compiles, as in KiCad
  const unknown = checkQuery("A.Nope == 'x'", rows, board, "mm");
  assert.equal(unknown.ok, true);
  assert.deepEqual(unknown.errors, ["Unrecognized property 'Nope'"]);
});

test("Select Matching from a query: the ids, or null when it does not compile", () => {
  const rows: FindRow[] = [];
  assert.deepEqual(selectMatchingFromQuery("A.Type == 'Via' && A.Diameter >= 0.8mm", rows, board, "mm").ids, ["via_b", "via_c"]);
  assert.equal(selectMatchingFromQuery("A.Type ==", rows, board, "mm").ids, null);
  assert.deepEqual(selectMatchingFromQuery("A.Reference == 'R*'", rowsOf(["R1"]).rows, board, "mm").ids, ["R1", "R2"], "A.Reference is the field of a footprint (getField), so a pad is not one");
  assert.deepEqual(selectMatchingFromQuery("A.Type == 'Pad' && A.Library_Link == 'R_0603'", rowsOf(["R1"]).rows, board, "mm").ids, ["R1.1", "R2.1"], "a pad has its footprint's library link, a property");
});

test("completion after A.: the names of the properties and the fields, with _ for a blank, sorted", () => {
  const names = completionNames(rowsOf(["U1"]).rows);
  for (const n of ["Width", "Diameter", "Pad_Type", "Position_X", "Reference", "Datasheet", "Supplier", "Type", "Via_Type", "Keep_Out_Vias"]) assert.ok(names.includes(n), n);
  assert.deepEqual([...names].sort(), names);
  assert.equal(new Set(names).size, names.length);
  assert.equal(exprFieldName("Pad Type"), "Pad_Type");
});

test("recent queries: newest first, once each, ten", () => {
  let recent: string[] = [];
  for (let i = 0; i < 12_000; i++) recent = withRecentQuery(recent, `q${i}`);
  assert.equal(recent.length, 10);
  assert.equal(recent[0], "q11");
  recent = withRecentQuery(recent, "q5");
  assert.deepEqual(recent.slice(0, 2), ["q5", "q11"]);
  assert.equal(recent.length, 10);
  assert.deepEqual(withRecentQuery(recent, ""), recent);
});
