import { test } from "node:test";
import assert from "node:assert/strict";
import { lockCmd, resolveLockMode, withoutLocked } from "./schLock";

const locked = new Set(["U2", "wire_b"]);

test("toggle locks everything unless any selected item is already locked", () => {
  assert.equal(resolveLockMode("toggle", ["U1", "R1"], locked), "lock");
  assert.equal(resolveLockMode("toggle", ["U1", "U2"], locked), "unlock");
  assert.equal(resolveLockMode("lock", ["U2"], locked), "lock");
  assert.equal(resolveLockMode("unlock", ["U1"], locked), "unlock");
});

test("the verb carries only the items whose state changes", () => {
  assert.deepEqual(lockCmd("lock", ["U1", "U2"], locked), { verb: "set_locked", ids: ["U1"], locked: true });
  assert.deepEqual(lockCmd("unlock", ["U1", "U2", "wire_b"], locked), { verb: "set_locked", ids: ["U2", "wire_b"], locked: false });
  assert.equal(lockCmd("lock", ["U2"], locked), null, "already locked: nothing to send");
  assert.equal(lockCmd("toggle", [], locked), null);
});

test("a toggle over a mixed selection unlocks the locked ones and leaves the rest alone", () => {
  assert.deepEqual(lockCmd("toggle", ["U1", "U2"], locked), { verb: "set_locked", ids: ["U2"], locked: false });
});

test("sheet pins inherit their parent's lock and are never locked themselves", () => {
  assert.equal(lockCmd("lock", ["shpin_abc"], locked), null);
  assert.deepEqual(lockCmd("lock", ["shpin_abc", "U1"], locked), { verb: "set_locked", ids: ["U1"], locked: true });
});

test("locked items are filtered out of an edit selection", () => {
  assert.deepEqual(withoutLocked(["U1", "U2", "wire_b", "R1"], locked), ["U1", "R1"]);
});
