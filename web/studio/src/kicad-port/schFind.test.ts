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
