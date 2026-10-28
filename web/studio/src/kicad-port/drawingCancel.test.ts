import { test } from "node:test";
import assert from "node:assert/strict";
import { drawShapeStarted, drawZoneStarted, escapeKeepsDrawingTool, redoRefused, routerEditKey, takeBackLastLine, undoDropsDrawing, withoutLastCorner } from "./drawingCancel";

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
  assert.equal(drawZoneStarted("pcb", "zone", null), false, "not started: Delete and Undo are the editor's");
  assert.deepEqual(withoutLastCorner(zone)?.pts, [[0, 0], [1, 0]]);
  assert.equal(undoDropsDrawing("pcb", "zone", zone), false);
  assert.equal(undoDropsDrawing("pcb", "line", { kind: "shape", shapeKind: "segment", pts: [[0, 0]] }), true);
  assert.equal(undoDropsDrawing("pcb", "measure", { kind: "measure" }), false, "MeasureTool handles only IsCancelInteractive()");
});

test("the last corner's removal keeps the outline started; only the press after it ends it (POLYGON_GEOM_MANAGER::DeleteLastCorner, round 4 A-4)", () => {
  const one = { kind: "zone", pts: [[0, 0]] as [number, number][] };
  const none = withoutLastCorner(one);
  assert.deepEqual(none, { kind: "zone", pts: [] }, "DeleteLastCorner answered the corner: still started");
  assert.equal(drawZoneStarted("pcb", "zone", none), true, "the next Delete is the tool's too");
  assert.equal(withoutLastCorner(none!), null, "nothing left to remove: cleanup()");
});

test("Delete with a shape started is drawShape's, not the editor's (round 4 A-2)", () => {
  assert.equal(drawShapeStarted("pcb", "rect", { kind: "shape", shapeKind: "rect", pts: [[0, 0]] }), true, "consumed by the tool");
  assert.equal(drawShapeStarted("pcb", "line", { kind: "shape", shapeKind: "segment", pts: [[0, 0]] }), true);
  assert.equal(drawShapeStarted("pcb", "arc", { kind: "shape", shapeKind: "arc", pts: [[0, 0]] }), false, "drawArc passes Delete on");
  assert.equal(drawShapeStarted("pcb", "rect", null), false);
  assert.deepEqual(takeBackLastLine({ kind: "shape", shapeKind: "rect", pts: [[0, 0]] }), { draw: null, undo: false }, "cleanup(): the rectangle goes");
});

test("Delete or Backspace in a chain of lines takes the last committed segment back and draws on from its start (DrawLine, round 4 A-1)", () => {
  const draw = { kind: "shape", shapeKind: "segment", pts: [[10, 10]] as [number, number][], committed: [[[0, 0], [10, 0]], [[10, 0], [10, 10]]] as [[number, number], [number, number]][] };
  const back = takeBackLastLine(draw);
  assert.equal(back.undo, true, "the Draw Line commit is undone");
  assert.deepEqual(back.draw?.pts, [[10, 0]]);
  assert.deepEqual(back.draw?.committed, [[[0, 0], [10, 0]]]);
  const again = takeBackLastLine(takeBackLastLine(back.draw!).draw!);
  assert.deepEqual(again, { draw: null, undo: false }, "no committed segment left: cleanup()");
});

test("Redo and the zone fills are refused while an item is being drawn (wxBell, round 4 A-3)", () => {
  for (const d of [{ kind: "shape", shapeKind: "segment", pts: [[0, 0]] }, { kind: "zone", pts: [[0, 0]] }, { kind: "dimension", pts: [[0, 0]] }, { kind: "table" }]) assert.equal(redoRefused("pcb", "x", d), true, d.kind);
  assert.equal(redoRefused("pcb", "measure", { kind: "measure" }), false, "the measure tool has no such branch");
  assert.equal(redoRefused("pcb", "zone", null), false, "nothing started");
});

test("a graphic polygon emptied by Delete is still started: the next Delete ends it, not the selection (round 5 A5-6)", () => {
  const emptied = withoutLastCorner({ kind: "shape", shapeKind: "polygon", pts: [[0, 0]] as [number, number][] });
  assert.deepEqual(emptied, { kind: "shape", shapeKind: "polygon", pts: [] });
  assert.equal(drawZoneStarted("pcb", "draw_polygon", emptied), true);
  assert.equal(withoutLastCorner(emptied!), null, "nothing left to remove: cleanup()");
});

test("Escape with a text carried drops the text and keeps the Text tool; the next one leaves (PlaceText, round 5 B5-2)", () => {
  assert.equal(escapeKeepsDrawingTool("pcb", "text", { kind: "text" }), true);
  assert.equal(escapeKeepsDrawingTool("pcb", "text", null), false);
});

test("undo_and_delete_while_routing_take_back_the_last_segment (R7-R1: UndoRedoBlock, performRouting's routerUndoLastSegment; R7-R5: E is ACT_CustomTrackWidth)", () => {
  for (const kind of ["route", "diffpair"]) {
    assert.equal(routerEditKey("pcb", { kind }, "undo"), "undoSegment");
    assert.equal(routerEditKey("pcb", { kind }, "delete"), "undoSegment");
    assert.equal(routerEditKey("pcb", { kind }, "redo"), "nothing", "UndoRedoBlock");
    assert.equal(routerEditKey("pcb", { kind }, "fill"), "nothing", "IsZoneFillAction: wxBell");
    assert.equal(routerEditKey("pcb", { kind }, "properties"), "customSize");
  }
  // performDragging: an undo is an escape; the fill actions ring the bell
  assert.equal(routerEditKey("pcb", { kind: "drag" }, "undo"), "cancel");
  assert.equal(routerEditKey("pcb", { kind: "drag" }, "redo"), "nothing");
  assert.equal(routerEditKey("pcb", { kind: "drag" }, "fill"), "nothing");
  assert.equal(routerEditKey("pcb", { kind: "drag" }, "delete"), "nothing");
  assert.equal(routerEditKey("pcb", null, "undo"), null, "no session: the editor's undo");
  assert.equal(routerEditKey("pcb", { kind: "shape" }, "undo"), null, "the drawing tools have their own");
  assert.equal(routerEditKey("schematic", { kind: "route" }, "undo"), null);
});
