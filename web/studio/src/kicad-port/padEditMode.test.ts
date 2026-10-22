import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibraryFootprint, LibraryPad } from "../api/types";
import { editPadCommand, enterContrast, exitContrast, inPadEditMode, padEditLayer, padEditMenu, padToolsBlocked } from "./padEditMode";

const pad = (id: string, layers: string[] = [], kind: LibraryPad["kind"] = "smd"): LibraryPad =>
  ({ id, number: id, at: { x: 0, y: 0 }, offset: { x: 0, y: 0 }, size: [1000, 600], shape: "custom", kind, drill: null, drill_slot: null, rot: 0, roundrect_ratio: null, trapezoid_delta: null, chamfer_ratio: null, chamfer_corners: { top_left: false, top_right: false, bottom_left: false, bottom_right: false }, layers, clearance_override: null, thermal_gap_override: null, thermal_spoke_width_override: null }) as LibraryPad;

const fp = (pads: LibraryPad[], editing: string | null = null): LibraryFootprint => ({ name: "Lib:Row", pads, graphics: [], texts: [], fields: [], courtyard: null, anchor: { x: 0, y: 0 }, published: false, pad_edit: editing ? { pad: editing } : null }) as unknown as LibraryFootprint;

test("the selection menu offers Edit Pad as Graphic Shapes for one pad and Finish Pad Edit while a pad is edited", () => {
  const f = fp([pad("a"), pad("b")]);
  assert.deepEqual(padEditMenu(f, new Set(["a"])), { explode: true, recombine: false });
  assert.deepEqual(padEditMenu(f, new Set(["a", "b"])), { explode: false, recombine: false }, "Count( 1 )");
  assert.deepEqual(padEditMenu(f, new Set()), { explode: false, recombine: false });
  assert.deepEqual(padEditMenu(f, new Set(["not-a-pad"])), { explode: false, recombine: false }, "OnlyTypes( PCB_PAD_T )");
  const editing = fp([pad("a"), pad("b")], "a");
  assert.deepEqual(padEditMenu(editing, new Set(["a"])), { explode: false, recombine: true });
  assert.deepEqual(padEditMenu(editing, new Set()), { explode: false, recombine: true });
  assert.deepEqual(padEditMenu(null, new Set(["a"])), { explode: false, recombine: false });
});

test("EditPad recombines whichever action asked while a pad is edited, and explodes the one selected pad otherwise", () => {
  assert.deepEqual(editPadCommand("Lib:Row", fp([pad("a")], "a"), new Set()), { op: "recombine_pad", footprint: "Lib:Row" });
  assert.deepEqual(editPadCommand("Lib:Row", fp([pad("a"), pad("b")]), new Set(["b"])), { op: "explode_pad", footprint: "Lib:Row", id: "b" });
  assert.equal(editPadCommand("Lib:Row", fp([pad("a"), pad("b")]), new Set(["a", "b"])), null);
  assert.equal(editPadCommand("Lib:Row", fp([pad("a")]), new Set()), null);
  assert.equal(editPadCommand("Lib:Row", null, new Set(["a"])), null);
});

test("the pad edit mode is the footprint's, and it shuts the Pad Table and Renumber Pads", () => {
  assert.equal(inPadEditMode(fp([pad("a")], "a")), true);
  assert.equal(inPadEditMode(fp([pad("a")])), false);
  assert.equal(inPadEditMode(null), false);
  assert.equal(padToolsBlocked(fp([pad("a")], "a")), true);
  assert.equal(padToolsBlocked(fp([pad("a")])), false);
});

test("the layer of the edit is F.Cu, else B.Cu, else the pad's first layer", () => {
  assert.equal(padEditLayer(pad("a", ["F.Cu", "F.Paste", "F.Mask"])), "F.Cu");
  assert.equal(padEditLayer(pad("a", ["B.Cu", "B.Paste", "B.Mask"])), "B.Cu");
  assert.equal(padEditLayer(pad("a", [], "through_hole")), "F.Cu", "a through-hole pad's default layers hold F.Cu");
  assert.equal(padEditLayer(pad("a", ["*.Cu", "*.Mask"], "through_hole")), "F.Cu");
  assert.equal(padEditLayer(pad("a", ["F.Mask", "F.Paste"])), "F.Paste", "the layer lists put paste before mask");
  assert.equal(padEditLayer(pad("a", ["B.Mask", "Dwgs.User"])), "B.Mask");
});

test("the high contrast mode is dimmed for the edit and put back after it", () => {
  assert.deepEqual(enterContrast(false), { highContrast: true, remembered: false });
  assert.deepEqual(enterContrast(true), { highContrast: true, remembered: true });
  assert.equal(exitContrast(false, true), false, "it was normal before");
  assert.equal(exitContrast(true, true), true, "it was dimmed already");
  assert.equal(exitContrast(null, true), true, "the mode never began here");
});
