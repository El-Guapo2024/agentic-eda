import { test } from "node:test";
import assert from "node:assert/strict";
import {
  afterPlaced,
  armedKey,
  carryKey,
  CARRY_TOOL_ACTIONS,
  incrementCarried,
  isCarryTool,
  labelsOfEntry,
  mirrorCarried,
  mirrorPole,
  mirrorSpin,
  placeCmd,
  poleOfSpin,
  previewCmd,
  PREVIEW_AT,
  rotateCarried,
  rotateSpin,
  scopeOfTool,
  spinOfPole,
  type CarriedDirective,
  type CarriedLabel,
  type CarriedPower,
  type CarriedText,
  type Carry,
} from "./schPlaceTool";

const label = (over: Partial<CarriedLabel> = {}): CarriedLabel => ({ kind: "label", scope: "global", text: "DATA0", shape: "output", spin: "right", sizeNm: null, bold: false, italic: false, autoRotate: true, ...over });
const text = (over: Partial<CarriedText> = {}): CarriedText => ({ kind: "text", content: "Note 1", sizeNm: 1_270_000, vertical: false, h: "left", v: "bottom", ...over });
const directive = (over: Partial<CarriedDirective> = {}): CarriedDirective => ({ kind: "directive", netclass: "Power", componentClass: "", shape: "round", orientation: 0, pinLengthNm: 2_540_000, ...over });
const power = (over: Partial<CarriedPower> = {}): CarriedPower => ({ kind: "power", libId: "power:GND", net: "GND", rotMillideg: 0, ...over });

test("the placing tools are the six whose item rides the cursor, each armed by its own action", () => {
  assert.deepEqual(Object.keys(CARRY_TOOL_ACTIONS).sort(), ["sch_directive", "sch_label_global", "sch_label_hier", "sch_label_local", "sch_power", "sch_text"]);
  assert.equal(CARRY_TOOL_ACTIONS.sch_directive, "eeschema.InteractiveDrawing.placeClassLabel");
  assert.ok(isCarryTool("sch_power") && !isCarryTool("sch_no_connect") && !isCarryTool("wire") && !isCarryTool("sch_textbox"));
  assert.deepEqual([scopeOfTool("sch_label_local"), scopeOfTool("sch_label_global"), scopeOfTool("sch_label_hier"), scopeOfTool("sch_text")], ["local", "global", "hierarchical", null]);
});

test("SPIN_STYLE turns right, up, left, bottom counter-clockwise and mirrors across one axis", () => {
  assert.deepEqual(["right", "up", "left", "bottom"].map((s) => rotateSpin(s as never, true)), ["up", "left", "bottom", "right"]);
  assert.deepEqual(["right", "up", "left", "bottom"].map((s) => rotateSpin(s as never, false)), ["bottom", "right", "up", "left"]);
  assert.deepEqual(["right", "up", "left", "bottom"].map((s) => mirrorSpin(s as never, true)), ["left", "up", "right", "bottom"]);
  assert.deepEqual(["right", "up", "left", "bottom"].map((s) => mirrorSpin(s as never, false)), ["right", "bottom", "left", "up"]);
});

test("R, Shift+R, X and Y on a carried label turn its spin as SCH_EDIT_TOOL's single-item branch does", () => {
  assert.equal((rotateCarried(label(), true) as CarriedLabel).spin, "up", "R: counter-clockwise");
  assert.equal((rotateCarried(label(), false) as CarriedLabel).spin, "bottom", "Shift+R: clockwise");
  // Mirror Vertically (top and bottom trade places) is `MirrorSpinStyle( false )`: up and bottom; Mirror Horizontally right and left
  assert.equal((mirrorCarried(label({ spin: "up" }), true) as CarriedLabel).spin, "bottom");
  assert.equal((mirrorCarried(label({ spin: "up" }), false) as CarriedLabel).spin, "up");
  assert.equal((mirrorCarried(label(), false) as CarriedLabel).spin, "left");
  // "From the wire" turns from the right spin a label nothing is under has
  assert.equal((rotateCarried(label({ spin: "" }), true) as CarriedLabel).spin, "up");
  // the text and everything else stay
  const turned = rotateCarried(label({ bold: true }), true) as CarriedLabel;
  assert.equal(turned.text, "DATA0");
  assert.equal(turned.bold, true);
});

test("a directive label's pole turns a quarter and mirrors across the other axis (its pole is drawn across its text)", () => {
  assert.equal((rotateCarried(directive(), true) as CarriedDirective).orientation, 90_000);
  assert.equal((rotateCarried(directive({ orientation: 270_000 }), true) as CarriedDirective).orientation, 0);
  assert.equal((rotateCarried(directive(), false) as CarriedDirective).orientation, 270_000);
  // `mirror_orientation( o, vertical )`
  assert.equal((mirrorCarried(directive({ orientation: 0 }), true) as CarriedDirective).orientation, 180_000);
  assert.equal((mirrorCarried(directive({ orientation: 0 }), false) as CarriedDirective).orientation, 0);
  assert.equal((mirrorCarried(directive({ orientation: 90_000 }), false) as CarriedDirective).orientation, 270_000);
  assert.deepEqual([mirrorPole(180_000, true), mirrorPole(270_000, false), mirrorPole(90_000, true)], [0, 90_000, 90_000]);
});

test("a carried text turns along and up the sheet with its justification flipped as SCH_TEXT::Rotate90 flips it", () => {
  // counter-clockwise from along the sheet: up it, the anchor on the same end
  const up = rotateCarried(text(), true) as CarriedText;
  assert.deepEqual([up.vertical, up.h], [true, "left"]);
  // again: along the sheet with the other end on the anchor
  const back = rotateCarried(up, true) as CarriedText;
  assert.deepEqual([back.vertical, back.h], [false, "right"]);
  // clockwise from along the sheet flips the justification as it starts to run up
  const cw = rotateCarried(text(), false) as CarriedText;
  assert.deepEqual([cw.vertical, cw.h], [true, "right"]);
  // four turns either way come back
  let t: CarriedText = text({ h: "center" });
  for (let i = 0; i < 4; i++) t = rotateCarried(t, true) as CarriedText;
  assert.deepEqual([t.vertical, t.h], [false, "center"]);
  // a mirror flips it along the text only: X on a text along the sheet, Y on one up it
  assert.equal((mirrorCarried(text(), false) as CarriedText).h, "right");
  assert.equal((mirrorCarried(text(), true) as CarriedText).h, "left");
  assert.equal((mirrorCarried(text({ vertical: true }), true) as CarriedText).h, "right");
});

test("a carried power symbol turns a quarter about its anchor; a mirror across the horizontal axis is half a turn, across the vertical none", () => {
  assert.equal((rotateCarried(power(), true) as CarriedPower).rotMillideg, 90_000);
  assert.equal((rotateCarried(power({ rotMillideg: 270_000 }), true) as CarriedPower).rotMillideg, 0);
  assert.equal((rotateCarried(power(), false) as CarriedPower).rotMillideg, 270_000);
  assert.equal((mirrorCarried(power({ rotMillideg: 90_000 }), true) as CarriedPower).rotMillideg, 270_000);
  assert.equal((mirrorCarried(power({ rotMillideg: 90_000 }), false) as CarriedPower).rotMillideg, 90_000);
});

test("Increment steps a carried label's or text's text; a directive label and a power symbol are not incrementable", () => {
  assert.equal((incrementCarried(label(), 1, 0) as CarriedLabel).text, "DATA1");
  assert.equal((incrementCarried(label({ text: "D9" }), 1, 0) as CarriedLabel).text, "D10");
  assert.equal((incrementCarried(label({ text: "A1" }), 1, 1) as CarriedLabel).text, "B1", "the secondary part");
  assert.equal(incrementCarried(label({ text: "D0" }), -1, 0), null, "below zero: nothing");
  assert.equal((incrementCarried(text({ content: "R3" }), -1, 0) as CarriedText).content, "R2");
  assert.equal(incrementCarried(directive(), 1, 0), null);
  assert.equal(incrementCarried(power(), 1, 0), null);
});

test("the placing click sends the label, text, directive and power verbs the dialogs always sent", () => {
  assert.deepEqual(placeCmd(label({ spin: "up" }), [1_000, 2_000]), { op: "sch_edit", verb: "place_label", net: "DATA0", at: { x: 1_000, y: 2_000 }, kind: { scope: "global", shape: "output" }, spin: "up", auto_rotate: true });
  // a local label has no shape and no Auto; "From the wire" is the spin read off the wire where it lands
  assert.deepEqual(placeCmd(label({ scope: "local", spin: "" }), [0, 0], { plainSpin: "left" }), { op: "sch_edit", verb: "place_label", net: "DATA0", at: { x: 0, y: 0 }, kind: { scope: "local" }, spin: "left" });
  assert.deepEqual(placeCmd(text({ vertical: true }), [5, 6]), { op: "sch_edit", verb: "place_text", content: "Note 1", at: { x: 5, y: 6 }, angle: 90_000, size_nm: 1_270_000, h: "left", v: "bottom" });
  assert.deepEqual(placeCmd(directive({ orientation: 180_000 }), [7, 8]), {
    op: "sch_edit",
    verb: "add_graphic",
    graphic: { shape: { type: "directive", at: { x: 7, y: 8 }, orientation: 180_000, shape: "round", pin_length_nm: 2_540_000, netclass: "Power", component_class: "" } },
  });
  const annotate = { algo: "first_free" as const, start_number: 0 };
  assert.deepEqual(placeCmd(power({ rotMillideg: 90_000 }), [9, 10], { annotate }), { op: "add_power_symbol", lib_id: "power:GND", at: { x: 9, y: 10 }, rot_millideg: 90_000, net: "GND", pin: "", annotate });
});

test("the preview is the same command off the sheet, without the click's AutoRotateItem", () => {
  const p = previewCmd(label()) as { at: { x: number; y: number }; auto_rotate?: boolean };
  assert.deepEqual(p.at, { x: PREVIEW_AT[0], y: PREVIEW_AT[1] });
  assert.equal(p.auto_rotate, undefined);
  assert.equal((placeCmd(label(), [0, 0]) as { auto_rotate?: boolean }).auto_rotate, true);
});

test("after a click the tool holds the next label of the entry, the same power symbol with Place repeated copies, else nothing", () => {
  const a = label({ text: "A0" });
  const b = label({ text: "A1" });
  const c = label({ text: "A2" });
  const first: Carry = { tool: "sch_label_global", item: a, queue: [b, c], keep: false };
  const second = afterPlaced(first)!;
  assert.deepEqual([second.item, second.queue], [b, [c]]);
  const third = afterPlaced(second)!;
  assert.deepEqual([third.item, third.queue], [c, []]);
  assert.equal(afterPlaced(third), null, "the last of the entry: the next click makes a new one");
  const kept: Carry = { tool: "sch_power", item: power(), queue: [], keep: true };
  assert.equal(afterPlaced(kept), kept);
  assert.equal(afterPlaced({ ...kept, keep: false }), null);
});

test("Multiple label input makes a label of each non-empty line, trimmed; one line is the text as typed", () => {
  const base = label();
  assert.deepEqual(labelsOfEntry(base, "SDA\n  SCL \n\nINT\r", true).map((l) => l.text), ["SDA", "SCL", "INT"]);
  assert.deepEqual(labelsOfEntry(base, " A B ", false).map((l) => l.text), [" A B "]);
  assert.deepEqual(labelsOfEntry(base, "   ", false), []);
});

test("while an item rides, the tool's loop answers the keys before anyone else", () => {
  const k = (action: string, activates = false, tool: Carry["tool"] = "sch_label_local") => carryKey(tool, action, activates);
  assert.deepEqual(k("common.Interactive.cancel"), { do: "drop" });
  assert.deepEqual(k("common.Interactive.undo"), { do: "drop" });
  assert.deepEqual(k("common.Interactive.delete"), { do: "drop" });
  // the tool's own hotkey is a click
  assert.deepEqual(k("eeschema.InteractiveDrawing.placeLabel", true), { do: "place" });
  assert.deepEqual(k("eeschema.InteractiveDrawing.placePowerSymbol", true, "sch_power"), { do: "place" });
  // another tool is refused with KiCad's message; the move tool quietly
  assert.deepEqual(k("eeschema.InteractiveDrawingLineWireBus.drawWires", true), { do: "busy", message: "Press <ESC> to cancel item creation." });
  assert.deepEqual(k("eeschema.InteractiveDrawing.placeLabel", true, "sch_power"), { do: "busy", message: "Press <ESC> to cancel symbol creation." });
  assert.deepEqual(k("eeschema.InteractiveMove.move", true), { do: "refuse" });
  assert.deepEqual(k("eeschema.InteractiveEdit.rotateCCW"), { do: "rotate", ccw: true });
  assert.deepEqual(k("eeschema.InteractiveEdit.rotateCW"), { do: "rotate", ccw: false });
  assert.deepEqual(k("eeschema.InteractiveEdit.mirrorV"), { do: "mirror", vertical: true });
  assert.deepEqual(k("eeschema.InteractiveEdit.mirrorH", false, "sch_power"), { do: "mirror", vertical: false });
  assert.deepEqual(k("eeschema.InteractiveEdit.properties"), { do: "properties" });
  assert.deepEqual(k("eeschema.Interactive.incrementPrimary"), { do: "increment", delta: 1, index: 0 });
  assert.deepEqual(k("eeschema.Interactive.decrementSecondary"), { do: "increment", delta: -1, index: 1 });
  // Duplicate, Repeat, Paste, Redo and the Change To conversions ring the bell
  for (const a of ["common.Interactive.duplicate", "eeschema.InteractiveEdit.repeatDrawItem", "common.Interactive.paste", "common.Interactive.redo", "eeschema.InteractiveEdit.toGLabel", "eeschema.InteractiveEdit.toCLabel"]) assert.deepEqual(k(a), { do: "refuse" }, a);
  // a symbol in the hand: no dialog of its own, no increment, no Change Symbol
  for (const a of ["eeschema.InteractiveEdit.properties", "eeschema.InteractiveEdit.editValue", "eeschema.Interactive.incrementPrimary", "eeschema.InteractiveEdit.changeSymbol", "eeschema.EditorControl.editWithSymbolEditor"]) assert.deepEqual(k(a, false, "sch_power"), { do: "refuse" }, a);
  // zoom, pan, the grid: as always
  assert.deepEqual(k("common.Control.zoomIn"), { do: "pass" });
  assert.deepEqual(k("common.Control.gridNext", false, "sch_power"), { do: "pass" });
});

test("a directive label starts on the label tools' last spin and leaves its pole as their last", () => {
  assert.deepEqual(["right", "up", "left", "bottom", ""].map((s) => poleOfSpin(s as never)), [0, 90_000, 180_000, 270_000, 0]);
  assert.deepEqual([0, 90_000, 180_000, 270_000, 360_000].map(spinOfPole), ["right", "up", "left", "bottom", "right"]);
});

test("armed with nothing in the hand, Undo leaves TwoClickPlace's tools (undoing nothing) and is Place Power's to pass on", () => {
  assert.equal(armedKey("sch_label_local", "common.Interactive.undo"), "leave");
  assert.equal(armedKey("sch_text", "common.Interactive.undo"), "leave");
  assert.equal(armedKey("sch_directive", "common.Interactive.undo"), "leave");
  assert.equal(armedKey("sch_power", "common.Interactive.undo"), "pass");
  assert.equal(armedKey("sch_label_global", "common.Interactive.redo"), "pass");
});
