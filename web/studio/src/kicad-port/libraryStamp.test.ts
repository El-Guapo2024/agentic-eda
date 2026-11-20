import { test } from "node:test";
import assert from "node:assert/strict";
import { libraryStampWatch } from "./libraryStamp";

/** A page's kept library answers, as api/libraryIndexClient.ts and libraryChooserClient.ts keep them: asked once per key until they are dropped. */
function keptAnswers() {
  const kept = new Map<string, string[]>();
  let asks = 0;
  let server: Record<string, string[]> = { "symbol/index": ["Device"] };
  return {
    ask(key: string): string[] {
      const hit = kept.get(key);
      if (hit) return hit;
      asks++;
      const answer = server[key] ?? [];
      kept.set(key, answer);
      return answer;
    },
    drop: () => kept.clear(),
    setServer: (s: Record<string, string[]>) => (server = s),
    asks: () => asks,
  };
}

test("the first stamp is the page's; the same stamp or none drops nothing; another one drops the kept answers once", () => {
  const page = keptAnswers();
  let dropped = 0;
  const watch = libraryStampWatch(() => {
    dropped++;
    page.drop();
  });
  assert.equal(watch.note("a.0"), false, "the first poll");
  assert.deepEqual(page.ask("symbol/index"), ["Device"]);
  // an agent writes the project's sym-lib-table: the server lists the library at once, the page's kept answer does not
  page.setServer({ "symbol/index": ["Device", "dog_power_supply"] });
  assert.deepEqual(page.ask("symbol/index"), ["Device"], "kept");
  assert.equal(watch.note("a.0"), false, "the same stamp");
  assert.equal(watch.note(null), false, "a server that says none");
  assert.equal(dropped, 0);
  // the next poll carries the moved stamp
  assert.equal(watch.note("a.1"), true);
  assert.equal(dropped, 1);
  assert.deepEqual(page.ask("symbol/index"), ["Device", "dog_power_supply"], "the next look asks the server");
  assert.equal(page.asks(), 2);
  assert.equal(watch.note("a.1"), false, "and once only");
  assert.equal(dropped, 1);
});

test("a server restarted or another project opened is another stamp; reset makes the next one the page's again", () => {
  let dropped = 0;
  const watch = libraryStampWatch(() => dropped++);
  watch.note("a.3");
  assert.equal(watch.note("b.0"), true, "another epoch");
  watch.reset();
  assert.equal(watch.note("c.0"), false);
  assert.equal(dropped, 1);
});
