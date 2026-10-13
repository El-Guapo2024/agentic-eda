import { test } from "node:test";
import assert from "node:assert/strict";
import { addedPart, insertedMessage, outlineCentre } from "./fpInsert";

test("the footprint goes to the middle of the outline, or the origin when the board has none", () => {
  assert.deepEqual(outlineCentre([[0, 0], [20_000, 0], [20_000, 10_000], [0, 10_000]]), { x: 10_000, y: 5_000 });
  assert.deepEqual(outlineCentre([[-4000, -2000], [6000, 8000]]), { x: 1000, y: 3000 });
  assert.deepEqual(outlineCentre([]), { x: 0, y: 0 });
  assert.deepEqual(outlineCentre(null), { x: 0, y: 0 });
});

test("the part a command added is the reference that was not there before", () => {
  assert.equal(addedPart([{ ref: "R1" }, { ref: "U1" }], [{ ref: "R1" }, { ref: "REF1" }, { ref: "U1" }]), "REF1");
  assert.equal(addedPart([{ ref: "R1" }], [{ ref: "R1" }]), null);
});

test("the toast names the part and where it went", () => {
  assert.equal(insertedMessage("Mine:Hole", "H1"), "Mine:Hole: inserted into the board as H1, at the centre of the board");
  assert.equal(insertedMessage("Mine:Hole", null), "Mine:Hole: inserted into the board");
});
