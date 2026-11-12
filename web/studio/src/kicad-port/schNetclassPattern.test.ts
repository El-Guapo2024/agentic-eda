import { test } from "node:test";
import assert from "node:assert/strict";
import { netNamePattern } from "./schNetclassPattern";

test("a bus vector offers PREFIX*, a bus group PREFIX.*, a labelled net its name and an unlabelled one nothing", () => {
  assert.equal(netNamePattern("D[0..7]", true), "D*");
  assert.equal(netNamePattern("MEM{A D}", true), "MEM.*");
  assert.equal(netNamePattern("/CLK", false), "/CLK");
  assert.equal(netNamePattern("Net-(R1-Pad1)", false), null);
  assert.equal(netNamePattern("unconnected-(U1-NC-Pad3)", false), null);
});
