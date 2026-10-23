import { test } from "node:test";
import assert from "node:assert/strict";
import { lastPlaced, rememberPlaced, resetLastPlaced } from "./schLastPlaced";

test("the tool starts with centre and centre text, no Auto and no spin chosen, and remembers what it was last given", () => {
  resetLastPlaced();
  assert.deepEqual(lastPlaced(), { spin: "right", bold: false, italic: false, shape: "input", autoRotate: false, textH: "center", textV: "center", textVertical: false });
  rememberPlaced({ autoRotate: true, shape: "output" });
  rememberPlaced({ textH: "left" });
  assert.equal(lastPlaced().autoRotate, true);
  assert.equal(lastPlaced().shape, "output");
  assert.equal(lastPlaced().textH, "left");
  assert.equal(lastPlaced().textV, "center", "what was not said is kept");
  resetLastPlaced();
  assert.equal(lastPlaced().autoRotate, false);
});
