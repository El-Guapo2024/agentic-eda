import { test } from "node:test";
import assert from "node:assert/strict";
import { drawZoneStarted, escapeKeepsDrawingTool, undoDropsDrawing, withoutLastCorner } from "./drawingCancel";

test("Escape with a line started keeps the line tool; with nothing started it leaves (DRAWING_TOOL::drawShape)", () => {
  assert.equal(escapeKeepsDrawingTool("pcb", "line", { kind: "shape" }), true);
  assert.equal(escapeKeepsDrawingTool("pcb", "zone", { kind: "zone" }), true);
  assert.equal(escapeKeepsDrawingTool("pcb", "line", null), false, "nothing started: the tool is left");
  assert.equal(escapeKeepsDrawingTool("pcb", "route", { kind: "route" }), false, "the router has its own cancel");
  assert.equal(escapeKeepsDrawingTool("schematic", "line", { kind: "shape" }), false);
});

test("Undo or Delete with a zone or polygon started removes the last corner (DrawZone); Undo in the measure tool undoes the board", () => {
  const zone = { kind: "zone", pts: [[0, 0], [1, 0], [1, 1]] as [number, number][] };
  assert.equal(drawZoneStarted("pcb", "zone", zone), true);
  assert.equal(drawZoneStarted("pcb", "polygon", { kind: "shape", shapeKind: "polygon", pts: [[0, 0]] }), true);
  assert.equal(drawZoneStarted("pcb", "rect", { kind: "shape", shapeKind: "rect", pts: [[0, 0]] }), false, "drawShape drops a rectangle instead");
  assert.equal(drawZoneStarted("pcb", "zone", { kind: "zone", pts: [] }), false, "not started: Delete and Undo are the editor's");
  assert.deepEqual(withoutLastCorner(zone)?.pts, [[0, 0], [1, 0]]);
  assert.equal(withoutLastCorner({ kind: "zone", pts: [[0, 0]] }), null, "no corner left: cleanup()");
  assert.equal(undoDropsDrawing("pcb", "zone", zone), false);
  assert.equal(undoDropsDrawing("pcb", "line", { kind: "shape", shapeKind: "segment", pts: [[0, 0]] }), true);
  assert.equal(undoDropsDrawing("pcb", "measure", { kind: "measure" }), false, "MeasureTool handles only IsCancelInteractive()");
});
