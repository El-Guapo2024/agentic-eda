import { test } from "node:test";
import assert from "node:assert/strict";
import { loadAngleSnapMode, saveAngleSnapMode } from "./pcbParityState";

test("the board's line mode is kept between sessions, DIRECT when nothing is stored (m_AngleSnapMode, round 6 A6-5)", () => {
  const store = new Map<string, string>();
  const storage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
  assert.equal(loadAngleSnapMode(storage), "direct");
  saveAngleSnapMode(storage, "45");
  assert.equal(loadAngleSnapMode(storage), "45");
  store.set("eda.pcb.angleSnapMode", "bogus");
  assert.equal(loadAngleSnapMode(storage), "direct");
  assert.equal(loadAngleSnapMode(null), "direct");
  assert.equal(loadAngleSnapMode({ getItem: () => { throw new Error("blocked"); } }), "direct");
});
