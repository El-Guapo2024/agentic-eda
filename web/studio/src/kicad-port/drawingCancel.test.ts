import { test } from "node:test";
import assert from "node:assert/strict";
import { escapeKeepsDrawingTool } from "./drawingCancel";

test("Escape with a line started keeps the line tool; with nothing started it leaves (DRAWING_TOOL::drawShape)", () => {
  assert.equal(escapeKeepsDrawingTool("pcb", "line", { kind: "shape" }), true);
  assert.equal(escapeKeepsDrawingTool("pcb", "zone", { kind: "zone" }), true);
  assert.equal(escapeKeepsDrawingTool("pcb", "line", null), false, "nothing started: the tool is left");
  assert.equal(escapeKeepsDrawingTool("pcb", "route", { kind: "route" }), false, "the router has its own cancel");
  assert.equal(escapeKeepsDrawingTool("schematic", "line", { kind: "shape" }), false);
});
