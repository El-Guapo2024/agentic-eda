import { test } from "node:test";
import assert from "node:assert/strict";
import { PickerHost, type PickerSession } from "./pickerHost";

const hit = (x: number, y: number, item: string | null = null) => ({ point: { x, y }, item: () => item });

test("a click with no session running is not the picker's", () => {
  const host = new PickerHost();
  assert.equal(host.click(hit(1, 2)), false);
  assert.equal(host.cancel(), false);
});

test("a point session gets the snapped point; returning false ends it and finalizes without cancelling", () => {
  const host = new PickerHost();
  const log: string[] = [];
  host.start({
    kind: "point",
    prompt: "Select reference point...",
    onPoint: (at) => {
      log.push(`point ${at.x},${at.y}`);
      return false;
    },
    onCancel: () => log.push("cancel"),
    onFinalize: () => log.push("finalize"),
  });
  assert.equal(host.session()?.prompt, "Select reference point...");
  assert.equal(host.click(hit(10, 20)), true);
  assert.deepEqual(log, ["point 10,20", "finalize"]);
  assert.equal(host.session(), null);
});

test("returning true keeps the session going (getNext)", () => {
  const host = new PickerHost();
  let n = 0;
  host.start({ kind: "point", prompt: "p", onPoint: () => ++n < 3 });
  host.click(hit(0, 0));
  host.click(hit(0, 0));
  assert.notEqual(host.session(), null);
  host.click(hit(0, 0));
  assert.equal(host.session(), null);
  assert.equal(n, 3);
});

test("an item session ignores a click that found nothing and gives the id of one that did", () => {
  const host = new PickerHost();
  const got: string[] = [];
  host.start({
    kind: "item",
    prompt: "Select reference item...",
    onItem: (id) => {
      got.push(id);
      return false;
    },
  });
  assert.equal(host.click(hit(0, 0, null)), true, "the click is still the picker's");
  assert.deepEqual(got, []);
  assert.notEqual(host.session(), null, "still looking for an item");
  host.click(hit(0, 0, "U1"));
  assert.deepEqual(got, ["U1"]);
  assert.equal(host.session(), null);
});

test("an item handler that rejects the item (returns true) keeps looking", () => {
  const host = new PickerHost();
  const got: string[] = [];
  host.start({
    kind: "item",
    prompt: "p",
    onItem: (id) => {
      got.push(id);
      return id !== "U2";
    },
  });
  host.click(hit(0, 0, "R1"));
  assert.notEqual(host.session(), null);
  host.click(hit(0, 0, "U2"));
  assert.equal(host.session(), null);
  assert.deepEqual(got, ["R1", "U2"]);
});

test("Escape runs the cancel handler, then the finalize handler", () => {
  const host = new PickerHost();
  const log: string[] = [];
  host.start({ kind: "point", prompt: "p", onCancel: () => log.push("cancel"), onFinalize: () => log.push("finalize") });
  assert.equal(host.cancel(), true);
  assert.deepEqual(log, ["cancel", "finalize"]);
  assert.equal(host.session(), null);
  assert.equal(host.cancel(), false);
});

test("a click handler may hand back the next session (InteractiveOffset's two clicks)", () => {
  const host = new PickerHost();
  const log: string[] = [];
  const second: PickerSession = {
    kind: "point",
    prompt: "second",
    onPoint: (at) => {
      log.push(`second ${at.x}`);
      return false;
    },
  };
  host.start({
    kind: "point",
    prompt: "first",
    onPoint: (at) => {
      log.push(`first ${at.x}`);
      return second;
    },
    onFinalize: () => log.push("first done"),
  });
  host.click(hit(1, 0));
  assert.equal(host.session()?.prompt, "second");
  host.click(hit(2, 0));
  assert.equal(host.session(), null);
  assert.deepEqual(log, ["first 1", "first done", "second 2"]);
});

test("a cancel handler may start a session again (backing out to the first stage), and learns whether Escape or another tool ended it", () => {
  const host = new PickerHost();
  const first: PickerSession = { kind: "point", prompt: "first" };
  const seen: boolean[] = [];
  const second: PickerSession = {
    kind: "point",
    prompt: "second",
    onCancel: (activated) => {
      seen.push(activated);
      if (!activated) host.start(first);
    },
  };
  host.start(second);
  assert.equal(host.cancel(), true);
  assert.equal(host.session(), first);
  host.start(second);
  assert.equal(host.cancel(true), true);
  assert.equal(host.session(), null, "another tool took over: no backing out");
  assert.deepEqual(seen, [false, true]);
});

test("starting a session while one runs cancels the running one", () => {
  const host = new PickerHost();
  const log: string[] = [];
  host.start({ kind: "point", prompt: "a", onCancel: (activated) => log.push(`a cancelled ${activated}`), onFinalize: () => log.push("a done") });
  host.start({ kind: "item", prompt: "b" });
  assert.deepEqual(log, ["a cancelled true", "a done"]);
  assert.equal(host.session()?.prompt, "b");
});

test("subscribers hear about every change of the running session", () => {
  const host = new PickerHost();
  let n = 0;
  const off = host.subscribe(() => n++);
  host.start({ kind: "point", prompt: "p", onPoint: () => false });
  host.click(hit(0, 0));
  assert.equal(n, 2);
  off();
  host.start({ kind: "point", prompt: "q" });
  assert.equal(n, 2);
});
