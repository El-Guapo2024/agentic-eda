import { test } from "node:test";
import assert from "node:assert/strict";
import { FOOTPRINT_LIBRARY_CHANGED, SYMBOL_LIBRARY_CHANGED, WATCH_IDLE, watchStep, type WatchAnswer, type WatchState } from "./libraryWatch";

const at = (stamp: string | null, more: Partial<WatchAnswer> = {}): WatchAnswer => ({ path: "/libs/Mine.kicad_sym", stamp, own: false, steady: true, modified: false, dirty: false, ...more });

/** Runs the answers one after the other, as the page's ticks do; the actions taken. */
function run(answers: (WatchAnswer | null)[], id = "Mine:R", dialogOpen = false): { state: WatchState; actions: string[] } {
  let state = WATCH_IDLE;
  const actions: string[] = [];
  for (const a of answers) {
    const r = watchStep(state, id, a, dialogOpen);
    state = r.state;
    actions.push(r.action);
  }
  return { state, actions };
}

test("the questions are KiCad's words", () => {
  assert.equal(SYMBOL_LIBRARY_CHANGED, "The library containing the current symbol has changed.\nDo you want to reload the library?");
  assert.equal(FOOTPRINT_LIBRARY_CHANGED, "The library containing the current footprint has changed.\nDo you want to reload the footprint?");
});

test("a library changed on disk reloads an item that is not modified, after the debounce, once", () => {
  const { actions } = run([at("1"), at("1"), at("2"), at("2"), at("2")]);
  assert.deepEqual(actions, ["none", "none", "none", "reload", "none"]);
});

test("a modified item asks instead, whether it was modified before the change or its flag says so", () => {
  assert.deepEqual(run([at("1", { modified: true }), at("2", { modified: true }), at("2", { modified: true })]).actions, ["none", "none", "ask"]);
  // modified only against the new file (the change itself): not modified before it, so it reloads unasked
  assert.deepEqual(run([at("1"), at("2", { modified: true }), at("2", { modified: true })]).actions, ["none", "none", "reload"]);
  // an edit after the change: the flag says so
  assert.deepEqual(run([at("1"), at("2"), at("2", { dirty: true, modified: true })]).actions, ["none", "none", "ask"]);
});

test("the studio's own save, a file that is gone and an item with no file are no change to offer", () => {
  assert.deepEqual(run([at("1"), at("2", { own: true }), at("2"), at("2")]).actions, ["none", "none", "none", "none"]);
  assert.deepEqual(run([at("1"), at(null), at(null)]).actions, ["none", "none", "none"]);
  assert.deepEqual(run([{ path: null }, { path: null }]).actions, ["none", "none"]);
});

test("a time that keeps moving waits until it stands still, and an open dialog puts the reload off", () => {
  assert.deepEqual(run([at("1"), at("2"), at("3"), at("4"), at("4")]).actions, ["none", "none", "none", "none", "reload"]);
  assert.deepEqual(run([at("1"), at("2"), at("2"), at("2")], "Mine:R", true).actions, ["none", "none", "none", "none"]);
});

test("another item is watched afresh", () => {
  let state = run([at("1"), at("2")]).state;
  const r = watchStep(state, "Mine:C", at("2"), false);
  assert.equal(r.action, "none");
  state = r.state;
  assert.equal(state.stamp, "2");
  assert.equal(state.id, "Mine:C");
});
