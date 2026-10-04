import { test } from "node:test";
import assert from "node:assert/strict";
import { updatePcbMessage } from "./updatePcb";

test("a board with every footprint placed reports up to date and the count", () => {
  const m = updatePcbMessage([
    { ref: "R1", placed: true },
    { ref: "R2", placed: true },
  ]);
  assert.match(m, /up to date with the schematic/);
  assert.match(m, /2 footprints, all placed\.$/);
});

test("footprints still to place are named, singular and plural, and a long list is cut", () => {
  assert.match(
    updatePcbMessage([
      { ref: "R1", placed: true },
      { ref: "C1", placed: false },
    ]),
    /2 footprints: 1 placed, 1 still to place \(C1\)/
  );
  const many = Array.from({ length: 12 }, (_, i) => ({ ref: `R${i + 1}`, placed: false }));
  const m = updatePcbMessage(many);
  assert.match(m, /12 footprints: 0 placed, 12 still to place \(R1, R2, R3, R4, R5, R6, R7, R8, \.\.\.\)/);
});

test("an empty board and a single footprint read naturally", () => {
  assert.match(updatePcbMessage([]), /It has no footprints yet\.$/);
  assert.match(updatePcbMessage([{ ref: "U1", placed: true }]), /1 footprint, all placed\.$/);
});
