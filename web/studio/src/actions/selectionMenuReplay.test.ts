import { test } from "node:test";
import assert from "node:assert/strict";
import { askingHandler, askingRegistry, newForcedSelection, SelectionMenuPending, type ActionHandler } from "./selectionMenuReplay";

/** A `requestSelection` of the board: the forced items while a replay runs, else -- inside an action -- the menu asks (its choice handed to `menu`). */
function requester(forced: ReturnType<typeof newForcedSelection>, menu: { choose: ((ids: string[]) => void) | null }) {
  return (): string[] => {
    if (forced.ids) return [...forced.ids];
    if (forced.depth === 0) return ["best"];
    throw new SelectionMenuPending((choose) => (menu.choose = choose));
  };
}

test("an action that asks stops, and the menu's choice replays it with the items chosen (RequestSelection -> doSelectionMenu)", () => {
  const forced = newForcedSelection();
  const menu: { choose: ((ids: string[]) => void) | null } = { choose: null };
  const request = requester(forced, menu);
  const ran: string[][] = [];
  const h = askingHandler(() => void ran.push(request()), forced);
  h();
  assert.deepEqual(ran, [], "nothing runs while the menu asks");
  assert.ok(menu.choose, "the menu opened");
  menu.choose!(["track-2"]);
  assert.deepEqual(ran, [["track-2"]], "the replay takes the chosen item");
  assert.equal(forced.ids, null, "the choice holds only for the replay");
  assert.equal(forced.depth, 0);
});

test("a dismissed menu runs nothing; outside an action the best candidate is taken", () => {
  const forced = newForcedSelection();
  const menu: { choose: ((ids: string[]) => void) | null } = { choose: null };
  const request = requester(forced, menu);
  let runs = 0;
  const h = askingHandler(() => {
    request();
    runs++;
  }, forced);
  h();
  menu.choose!([]);
  assert.equal(runs, 0, "an empty choice (Escape) does not replay");
  assert.deepEqual(request(), ["best"]);
});

test("an async action keeps the choice until it settles, and resolves when it asks", async () => {
  const forced = newForcedSelection();
  const menu: { choose: ((ids: string[]) => void) | null } = { choose: null };
  const request = requester(forced, menu);
  const seen: string[][] = [];
  const h: ActionHandler = askingHandler((async () => {
    await Promise.resolve();
    seen.push(request());
    await Promise.resolve();
    seen.push(request());
  }) as unknown as ActionHandler, forced);
  await (h() as unknown as Promise<void>);
  assert.ok(menu.choose);
  menu.choose!(["a", "b"]);
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(seen, [["a", "b"], ["a", "b"]]);
  assert.equal(forced.ids, null);
  assert.equal(forced.depth, 0);
});

test("other errors pass through; askingRegistry wraps every handler", () => {
  const forced = newForcedSelection();
  const m = new Map<string, ActionHandler>([["boom", () => { throw new Error("x"); }], ["ok", () => undefined]]);
  askingRegistry(m, forced);
  let thrown: unknown = null;
  try {
    m.get("boom")!();
  } catch (e) {
    thrown = e;
  }
  assert.ok(thrown instanceof Error && thrown.message === "x");
  assert.equal(forced.depth, 0);
  m.get("ok")!();
  assert.equal(forced.depth, 0);
});
