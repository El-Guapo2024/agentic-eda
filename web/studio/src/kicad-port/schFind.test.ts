import { test } from "node:test";
import assert from "node:assert/strict";
import type { FindMatch } from "../api/types";
import { defaultSearch, pickMatch, scopeFromSelection } from "./schFind";

const m = (key: string, x: number): FindMatch => ({ key, kind: "field", id: key, name: "Value", at: [x, 0], text: key });
const list = [m("a", 0), m("b", 10), m("c", 20)];

test("pickMatch: Find Next from a fresh cursor visits the first match", () => {
  const r = pickMatch(list, null, false);
  assert.equal(r.match?.key, "a");
  assert.equal(r.cursor, "a");
  assert.equal(r.endReached, false);
});

test("pickMatch: Find Next walks forward, then reports the end and the next call wraps", () => {
  let r = pickMatch(list, "a", false);
  assert.equal(r.match?.key, "b");
  r = pickMatch(list, "c", false);
  assert.equal(r.match, null);
  assert.equal(r.endReached, true);
  assert.equal(r.cursor, null, "cursor reset so the following Find wraps to the start");
  r = pickMatch(list, r.cursor, false);
  assert.equal(r.match?.key, "a");
});

test("pickMatch: Find Previous walks the list reversed", () => {
  let r = pickMatch(list, null, true);
  assert.equal(r.match?.key, "c");
  r = pickMatch(list, "c", true);
  assert.equal(r.match?.key, "b");
  r = pickMatch(list, "a", true);
  assert.equal(r.endReached, true);
});

test("pickMatch: a cursor whose match vanished (replaced) restarts from the beginning", () => {
  const r = pickMatch(list, "gone", false);
  assert.equal(r.match?.key, "a");
});

test("pickMatch: no matches at all is an end-reached with no match", () => {
  const r = pickMatch([], null, false);
  assert.equal(r.match, null);
  assert.equal(r.endReached, true);
});

test("defaultSearch matches EDA_SEARCH_DATA's constructor defaults; scope only when selected-only", () => {
  const d = defaultSearch();
  assert.equal(d.mode, "plain");
  assert.equal(d.match_case, false);
  assert.equal(d.search_hidden_fields, false);
  assert.equal(scopeFromSelection(new Set(["R1"]), false), undefined);
  assert.deepEqual(scopeFromSelection(new Set(["R1"]), true), ["R1"]);
});

// ---- over every sheet (`SCH_FIND_REPLACE_TOOL::FindNext` with `searchAllSheets`)
import { WRAP_CURSOR, endReachedMessage, matchId, searchesAllSheets } from "./schFind";

const on = (sheet: string[], rank: number, key: string, x = 0): FindMatch => ({ key, uid: `${sheet.join("/")}#${key}`, kind: "field", id: key, name: "Value", at: [x, 0], text: key, sheet, sheet_rank: rank });
// pages in order: the root (rank 0), sheet s1 (rank 1), sheet s2 (rank 2); each sheet's matches in x order
const sheets = [on([], 0, "a"), on([], 0, "b", 10), on(["s1"], 1, "c"), on(["s2"], 2, "d"), on(["s2"], 2, "e", 10)];

test("pickMatch over sheets: the cursor is the sheet and the key, so one key on two sheets is two matches", () => {
  const twin = [on([], 0, "same"), on(["s1"], 1, "same")];
  assert.equal(matchId(twin[1]!), "s1#same");
  const first = pickMatch(twin, null, false, 0);
  assert.equal(first.match?.sheet?.length, 0);
  const second = pickMatch(twin, first.cursor, false, 0);
  assert.deepEqual(second.match?.sheet, ["s1"]);
});

test("pickMatch over sheets: a Find with no cursor starts on the sheet in view and goes on to the later sheets", () => {
  const r = pickMatch(sheets, null, false, 1);
  assert.equal(r.match?.key, "c", "the sheets before it only come after a wrap");
  assert.equal(pickMatch(sheets, r.cursor, false, 1).match?.key, "d");
});

test("pickMatch over sheets: running out of sheets reports the end, and the next call wraps to the first sheet", () => {
  const end = pickMatch(sheets, matchId(sheets[4]!), false, 1);
  assert.equal(end.match, null);
  assert.equal(end.endReached, true);
  assert.equal(end.cursor, WRAP_CURSOR, "null would start on the sheet in view again");
  const wrapped = pickMatch(sheets, end.cursor, false, 1);
  assert.equal(wrapped.match?.key, "a", "the wrap starts at the first sheet of the whole schematic, the sheet in view included");
});

test("pickMatch over sheets: a Find with no cursor and nothing on or after the sheet in view is the end of the schematic", () => {
  const r = pickMatch(sheets.slice(0, 2), null, false, 2);
  assert.equal(r.match, null);
  assert.equal(r.cursor, WRAP_CURSOR);
});

test("pickMatch over sheets: Find Previous starts on the sheet in view and goes back through the earlier sheets", () => {
  const r = pickMatch(sheets, null, true, 1);
  assert.equal(r.match?.key, "c", "backward from the last match of the sheet in view");
  assert.equal(pickMatch(sheets, r.cursor, true, 1).match?.key, "b");
  assert.equal(pickMatch(sheets, matchId(sheets[0]!), true, 1).endReached, true);
});

test("a search goes over every sheet unless the current sheet only, or the selection, is asked for", () => {
  assert.equal(searchesAllSheets(false, false), true);
  assert.equal(searchesAllSheets(true, false), false);
  assert.equal(searchesAllSheets(false, true), false);
  assert.match(endReachedMessage(true), /^Reached end of schematic\. Find again/);
  assert.match(endReachedMessage(false), /^Reached end of sheet\. Find again/);
});
