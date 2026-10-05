import { test } from "node:test";
import assert from "node:assert/strict";
import { STALE_NOTICE, isStale, revisionAfterOwnEdit, revisionOf, statusNotes, type CheckStatus } from "./checkRevision";

test("a report is current while the board is on the revision it was computed on", () => {
  assert.equal(isStale("r1", "r1"), false);
});

test("a report is out of date once the board's revision has moved", () => {
  assert.equal(isStale("r1", "r2"), true);
});

test("nothing is out of date before the page knows the board's revision", () => {
  assert.equal(isStale("r1", null), false);
  assert.equal(isStale(null, null), false);
});

test("a report of unknown revision is out of date as soon as the board's is known", () => {
  assert.equal(isStale(null, "r1"), true);
});

test("the server's stamp wins over the version the page saw when it asked", () => {
  assert.equal(revisionOf({ revision: "r2" }, "r1"), "r2");
  assert.equal(revisionOf({ revision: null }, "r1"), "r1", "an older server does not stamp: the page's own guess");
  assert.equal(revisionOf({}, "r1"), "r1");
  assert.equal(revisionOf(null, "r1"), "r1");
  assert.equal(revisionOf({}, null), null);
});

test("an edit made while the check runs leaves its report out of date when it lands", () => {
  // The page asks at r1; the board moves to r2 during the run; the report says r1.
  const report = revisionOf({ revision: "r1" }, "r1");
  assert.equal(isStale(report, "r2"), true);
  // Run again: the new report carries r2.
  assert.equal(isStale(revisionOf({ revision: "r2" }, "r2"), "r2"), false);
});

test("a run started right after an edit is not called out of date because the poll is a step behind", () => {
  // The page last polled r1; the board is already at r2 (the edit just landed), so the run reads r2.
  const report = revisionOf({ revision: "r2" }, "r1");
  assert.equal(report, "r2");
  assert.equal(isStale(report, "r2"), false, "current once the poll catches up");
});

test("an edit that does not change a report keeps a current report current and an old one old", () => {
  assert.equal(revisionAfterOwnEdit("r1", "r1", "r2"), "r2", "current before, so current after the exclusion's own edit");
  assert.equal(isStale(revisionAfterOwnEdit("r1", "r1", "r2"), "r2"), false);
  assert.equal(revisionAfterOwnEdit("r1", "r2", "r3"), "r1", "already out of date: the exclusion does not refresh it");
  assert.equal(isStale(revisionAfterOwnEdit("r1", "r2", "r3"), "r3"), true);
  assert.equal(revisionAfterOwnEdit("r1", null, "r2"), "r2", "board revision unknown before: treated as current");
  assert.equal(revisionAfterOwnEdit("r1", "r1", null), null, "revision lost after: out of date");
});

test("the notice names what to do", () => {
  assert.match(STALE_NOTICE, /design changed since this check/);
  assert.match(STALE_NOTICE, /rerun/);
});

const quiet: CheckStatus = { tab: "pcb", drcRunning: false, ercRunning: false, drcStale: false, ercStale: false };

test("the status bar says nothing while no check is running and the reports are current", () => {
  assert.deepEqual(statusNotes(quiet), []);
});

test("the status bar shows a running check on any tab, so a closed dialog still has a sign of it", () => {
  assert.deepEqual(statusNotes({ ...quiet, drcRunning: true }), ["DRC running…"]);
  assert.deepEqual(statusNotes({ ...quiet, tab: "3d", ercRunning: true }), ["ERC running…"]);
});

test("an out-of-date report is called out only on the tab its markers are on", () => {
  assert.deepEqual(statusNotes({ ...quiet, drcStale: true, ercStale: true }), ["DRC out of date"]);
  assert.deepEqual(statusNotes({ ...quiet, tab: "schematic", drcStale: true, ercStale: true }), ["ERC out of date"]);
  assert.deepEqual(statusNotes({ ...quiet, tab: "footprint", drcStale: true, ercStale: true }), []);
});

test("a run in progress wins over out of date: its result is on the way", () => {
  assert.deepEqual(statusNotes({ ...quiet, drcRunning: true, drcStale: true }), ["DRC running…"]);
});
