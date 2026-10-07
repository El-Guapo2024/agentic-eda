import { test } from "node:test";
import assert from "node:assert/strict";
import { RouteSelectedLoop, type QueueOutcome } from "./routeSelectedLoop";

const run = (log: string[], name: string, outcome: QueueOutcome) => async () => {
  log.push(name);
  return outcome;
};

test("a connection that completes by itself lets the loop go straight on to the next", async () => {
  const log: string[] = [];
  const loop = new RouteSelectedLoop([run(log, "a", "done"), run(log, "b", "done")], () => log.push("end"));
  await loop.advance();
  assert.deepEqual(log, ["a", "b", "end"]);
  assert.ok(!loop.active);
});

test("a live connection holds the loop until advance is called again", async () => {
  const log: string[] = [];
  const loop = new RouteSelectedLoop([run(log, "a", "live"), run(log, "b", "done"), run(log, "c", "live"), run(log, "d", "done")], () => log.push("end"));
  await loop.advance();
  assert.deepEqual(log, ["a"]);
  assert.ok(loop.active);
  assert.equal(loop.remaining, 3);
  await loop.advance(); // a finished (or was skipped with cancelCurrentItem)
  assert.deepEqual(log, ["a", "b", "c"]);
  await loop.advance();
  assert.deepEqual(log, ["a", "b", "c", "d", "end"]);
});

test("cancel drops the rest of the loop and never reports the end", async () => {
  const log: string[] = [];
  const loop = new RouteSelectedLoop([run(log, "a", "live"), run(log, "b", "done")], () => log.push("end"));
  await loop.advance();
  loop.cancel();
  await loop.advance();
  assert.deepEqual(log, ["a"]);
  assert.ok(!loop.active);
});

test("cancelling while a connection is still starting stops the loop after it", async () => {
  const log: string[] = [];
  let release: () => void = () => {};
  const slow = () =>
    new Promise<QueueOutcome>((resolve) => {
      log.push("slow");
      release = () => resolve("done");
    });
  const loop = new RouteSelectedLoop([slow, run(log, "b", "done")], () => log.push("end"));
  const started = loop.advance();
  loop.cancel();
  release();
  await started;
  assert.deepEqual(log, ["slow"]);
});

test("an empty loop ends at once", async () => {
  const log: string[] = [];
  await new RouteSelectedLoop([], () => log.push("end")).advance();
  assert.deepEqual(log, ["end"]);
});
