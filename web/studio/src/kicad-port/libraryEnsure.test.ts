// Copying a library symbol into the project library before the editor opens it (libraryEnsure.ts): never a silent blank.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ensureInProject, symbolNotLoaded, UNKNOWN_SYMBOL, type EnsureDeps } from "./libraryEnsure";

/** Fake reads and puts: each call takes the next answer of its script (an Error is thrown). */
function deps(reads: Array<{ inProject: boolean } | Error>, puts: Array<{ ok: boolean; message?: string }> = []) {
  const log = { reads: 0, puts: 0 };
  const d: EnsureDeps<string> = {
    fetchAny: async () => {
      const r = reads[log.reads++] ?? new Error("no more answers");
      if (r instanceof Error) throw r;
      return { item: "Device:R", inProject: r.inProject };
    },
    put: async () => puts[log.puts++] ?? { ok: false, message: "no more answers" },
  };
  return { d, log };
}

test("an installed symbol is put into the project library; one already there is left alone; a bare name is the project's own", async () => {
  let f = deps([{ inProject: false }], [{ ok: true }]);
  assert.equal(await ensureInProject("Device:R", f.d, UNKNOWN_SYMBOL), null);
  assert.deepEqual(f.log, { reads: 1, puts: 1 });
  f = deps([{ inProject: true }]);
  assert.equal(await ensureInProject("Device:R", f.d, UNKNOWN_SYMBOL), null);
  assert.deepEqual(f.log, { reads: 1, puts: 0 });
  f = deps([]);
  assert.equal(await ensureInProject("R", f.d, UNKNOWN_SYMBOL), null);
  assert.equal(f.log.reads, 0);
});

test("only the server's \"no symbol with this lib_id\" allows a blank symbol: any other read error is retried once, then said", async () => {
  let f = deps([new Error("ops_unknown_symbol: no symbol with this lib_id in the project library or anything it loads")]);
  assert.equal(await ensureInProject("Lib:Nope", f.d, UNKNOWN_SYMBOL), null, "unknown everywhere: the open verb starts a blank one");
  f = deps([new Error("HTTP 500"), { inProject: false }], [{ ok: true }]);
  assert.equal(await ensureInProject("Device:R", f.d, UNKNOWN_SYMBOL), null, "a failed read is tried again");
  assert.deepEqual(f.log, { reads: 2, puts: 1 });
  f = deps([new Error("HTTP 500"), new Error("Failed to fetch")]);
  assert.equal(await ensureInProject("Device:R", f.d, UNKNOWN_SYMBOL), "Failed to fetch", "two failures: why, and nothing is opened");
});

test("a refused put is not ignored: tried once more, then its message is the reason", async () => {
  let f = deps([{ inProject: false }, { inProject: false }], [{ ok: false, message: "busy" }, { ok: true }]);
  assert.equal(await ensureInProject("Device:R", f.d, UNKNOWN_SYMBOL), null);
  assert.deepEqual(f.log, { reads: 2, puts: 2 });
  f = deps([{ inProject: false }, { inProject: false }], [{ ok: false, message: "ops_bad_symbol: Device:R" }, { ok: false, message: "ops_bad_symbol: Device:R" }]);
  const problem = await ensureInProject("Device:R", f.d, UNKNOWN_SYMBOL);
  assert.equal(problem, "ops_bad_symbol: Device:R");
  assert.equal(symbolNotLoaded("Device:R", problem!), "Error loading symbol R from library 'Device'.\nops_bad_symbol: Device:R", "LoadSymbol's words, the reason after them");
});
