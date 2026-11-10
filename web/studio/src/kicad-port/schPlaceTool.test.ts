import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd } from "../api/types";
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
  newPowerKind,
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
  type CarriedSymbol,
  type Carry,
  nextSymbolAfter,
  placeSymbolCmd,
  applyCarriedCmds,
  renameInCmd,
  settledSymbol,
  symbolOrientation,
  describeCarried,
  type CarriedItem,
} from "./schPlaceTool";

const label = (over: Partial<CarriedLabel> = {}): CarriedLabel => ({ kind: "label", scope: "global", text: "DATA0", shape: "output", spin: "right", sizeNm: null, bold: false, italic: false, autoRotate: true, ...over });
const text = (over: Partial<CarriedText> = {}): CarriedText => ({ kind: "text", content: "Note 1", sizeNm: 1_270_000, vertical: false, h: "left", v: "bottom", ...over });
const directive = (over: Partial<CarriedDirective> = {}): CarriedDirective => ({ kind: "directive", netclass: "Power", componentClass: "", shape: "round", orientation: 0, pinLengthNm: 2_540_000, ...over });
const power = (over: Partial<CarriedPower> = {}): CarriedPower => ({ kind: "power", libId: "power:GND", net: "GND", rotMillideg: 0, ...over });

test("the placing tools are the seven whose item rides the cursor, each armed by its own action", () => {
  assert.deepEqual(Object.keys(CARRY_TOOL_ACTIONS).sort(), ["sch_directive", "sch_label_global", "sch_label_hier", "sch_label_local", "sch_place_symbol", "sch_power", "sch_text"]);
  assert.equal(CARRY_TOOL_ACTIONS.sch_place_symbol, "eeschema.InteractiveDrawing.placeSymbol");
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

test("Preferences' Power Symbols (new_power_symbols): Global or Local converts the placed power symbol in the click's step, Default sends nothing more", () => {
  assert.deepEqual([newPowerKind(0), newPowerKind(1), newPowerKind(2)], [undefined, "global", "local"]);
  const add = { op: "add_power_symbol", lib_id: "power:GND", at: { x: 9, y: 10 }, rot_millideg: 0, net: "GND", pin: "" };
  assert.deepEqual(placeCmd(power({ powerKind: "local" }), [9, 10]), { op: "batch", cmds: [add, { op: "sch_edit", verb: "set_new_power_kind", lib_id: "power:GND", local: true }] });
  assert.deepEqual(placeCmd(power({ powerKind: "global" }), [9, 10]), { op: "batch", cmds: [add, { op: "sch_edit", verb: "set_new_power_kind", lib_id: "power:GND", local: false }] });
  assert.deepEqual(placeCmd(power(), [9, 10]), add);
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
  // a power symbol in the hand: its Properties and Field Properties act on it (`PlaceSymbol` hands them to the edit tool); no Footprint field, no increment, no Change Symbol
  assert.deepEqual(k("eeschema.InteractiveEdit.properties", false, "sch_power"), { do: "properties" });
  assert.deepEqual(k("eeschema.InteractiveEdit.editValue", false, "sch_power"), { do: "edit_field", name: "Value" });
  assert.deepEqual(k("eeschema.EditorControl.setDNP", false, "sch_power"), { do: "attribute", key: "dnp" });
  for (const a of ["eeschema.InteractiveEdit.editFootprint", "eeschema.InteractiveEdit.toggleDeMorgan", "eeschema.Interactive.incrementPrimary", "eeschema.InteractiveEdit.changeSymbol", "eeschema.EditorControl.editWithSymbolEditor"]) assert.deepEqual(k(a, false, "sch_power"), { do: "refuse" }, a);
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

// ------------------------------------------------------------------------------------------------------------- Place Symbol (A), UI-LEFT S1b

const symbol = (over: Partial<CarriedSymbol> = {}): CarriedSymbol => ({
  kind: "symbol",
  libId: "Amplifier_Operational:LM358",
  ref: "U3",
  prefix: "U",
  value: "LM358",
  footprint: "",
  unit: 1,
  unitCount: 3,
  rotDeg: 0,
  mirror: null,
  turns: [],
  placeAllUnits: false,
  oneOnly: false,
  ...over,
});

test("a carried symbol keeps its R / Shift+R / X / Y as the placed unit's own turns, and the click places it with them in one command", () => {
  let s: CarriedItem = symbol();
  s = rotateCarried(s, true);
  s = mirrorCarried(s, true);
  s = rotateCarried(s, false);
  assert.deepEqual((s as CarriedSymbol).turns, ["rot_ccw", "mirror_v", "rot_cw"]);
  const cmd = placeCmd(s, [1_270_000, 2_540_000], { annotate: { algo: "first_free", start_number: 0 } });
  assert.equal(cmd.op, "batch");
  const cmds = (cmd as { cmds: Array<Record<string, unknown>> }).cmds;
  assert.deepEqual(cmds[0], { op: "add_symbol", id: "U3", lib_id: "Amplifier_Operational:LM358", at: { x: 1_270_000, y: 2_540_000 }, rot_millideg: 0, value: "LM358", footprint: "", unit: 1, annotate: { algo: "first_free", start_number: 0 } });
  // the click makes it in the orientation the turns left it in, so Autoplace Fields places its fields there (`AutoplaceFields( screen, AUTOPLACE_AUTO )`):
  // a quarter turn counter-clockwise, mirrored top to bottom, a quarter back is `Matrix::orientation`'s (0, mirrored)
  assert.deepEqual(cmds.slice(1), [{ op: "mirror_symbol", id: "U3", unit: 1 }]);
  // the preview has the turns as the placed unit's own `sch_move`s, `<reference>#<unit>` (a reference's other units on the sheet do not turn with it): the fields
  // where the turns leave them while it rides
  assert.deepEqual((previewCmd(s) as { cmds: unknown[] }).cmds.slice(1), [
    { op: "sch_move", ids: ["U3#1"], vertices: {}, verb: "rotate", ccw: true },
    { op: "sch_move", ids: ["U3#1"], vertices: {}, verb: "mirror", vertical: true },
    { op: "sch_move", ids: ["U3#1"], vertices: {}, verb: "rotate", ccw: false },
  ]);
  // with no turn it is the one `add_symbol`; a copy of a placed unit starts in that unit's orientation (`new SCH_SYMBOL( *symbol )`)
  assert.equal(placeSymbolCmd(symbol(), [0, 0]).op, "add_symbol");
  const copy = placeSymbolCmd(symbol({ rotDeg: 90, mirror: "y", unit: 2 }), [0, 0]) as { cmds: Array<Record<string, unknown>> };
  assert.deepEqual([copy.cmds[0]!.rot_millideg, copy.cmds[1]], [90_000, { op: "mirror_symbol", id: "U3", unit: 2 }]);
  // the preview is the same command at the far point
  assert.deepEqual((previewCmd(symbol()) as { at: unknown }).at, { x: PREVIEW_AT[0], y: PREVIEW_AT[1] });
});

test("after a click: Place all units steps to the next unit not placed, Place repeated copies numbers a copy anew, a handed symbol ends the tool", () => {
  const placed = new Set<number>();
  const ctx = { occupied: (u: number) => placed.has(u), nextRef: () => "U4" };
  // "Place all units" alone: B and C follow A under its reference, and the tool holds nothing after the last
  let s: CarriedSymbol | null = symbol({ placeAllUnits: true });
  const seq: string[] = [];
  while (s) {
    seq.push(`${s.ref}${s.unit}`);
    placed.add(s.unit);
    s = nextSymbolAfter(s, false, ctx);
  }
  assert.deepEqual(seq, ["U31", "U32", "U33"]);
  // with "Place repeated copies" too: after the last unit, unit 1 of a new reference (`annotate()`)
  placed.clear();
  placed.add(1).add(2).add(3);
  assert.deepEqual(nextSymbolAfter(symbol({ placeAllUnits: true, unit: 3 }), true, ctx), symbol({ placeAllUnits: true, unit: 1, ref: "U4" }));
  // "Place repeated copies" alone: the same unit again, unit 1 numbered anew, any other unit under the same reference (`if( currentReference.GetUnit() == 1 ) annotate();`)
  assert.deepEqual(nextSymbolAfter(symbol(), true, ctx), symbol({ ref: "U4" }));
  assert.deepEqual(nextSymbolAfter(symbol({ unit: 2 }), true, ctx), symbol({ unit: 2 }));
  // neither: nothing rides (the next click opens the chooser); a symbol an action handed the tool: nothing (`placeOneOnly`)
  assert.equal(nextSymbolAfter(symbol(), false, ctx), null);
  assert.equal(nextSymbolAfter(symbol({ oneOnly: true, placeAllUnits: true }), true, ctx), null);
  // through the tool's own rule
  const carry: Carry = { tool: "sch_place_symbol", item: symbol({ placeAllUnits: true }), queue: [], keep: false };
  placed.clear();
  placed.add(1);
  assert.equal((afterPlaced(carry, ctx)?.item as CarriedSymbol).unit, 2);
  assert.equal(afterPlaced(carry), null, "no units to look at: nothing");
});

test("the keys while a symbol rides are PlaceSymbol's: A places it, R / X / Y turn it, Esc and Undo drop it, the symbol edits and other tools are refused", () => {
  const k = (action: string, activates = false) => carryKey("sch_place_symbol", action, activates);
  assert.deepEqual(k("eeschema.InteractiveDrawing.placeSymbol", true), { do: "place" });
  assert.deepEqual(k("eeschema.InteractiveEdit.rotateCCW"), { do: "rotate", ccw: true });
  assert.deepEqual(k("eeschema.InteractiveEdit.mirrorV"), { do: "mirror", vertical: true });
  assert.deepEqual(k("common.Interactive.cancel"), { do: "drop" });
  assert.deepEqual(k("common.Interactive.undo"), { do: "drop" });
  assert.deepEqual(k("common.Interactive.delete"), { do: "drop" });
  assert.deepEqual(k("eeschema.InteractiveDrawing.placeLabel", true), { do: "busy", message: "Press <ESC> to cancel symbol creation." });
  for (const a of ["eeschema.InteractiveEdit.changeSymbol", "eeschema.EditorControl.editWithSymbolEditor", "common.Interactive.duplicate", "eeschema.Interactive.increment"]) assert.deepEqual(k(a), { do: "refuse" }, a);
  // `PlaceSymbol` hands Properties, U / V / F, Autoplace Fields, Cycle Body Style and the attribute toggles to the edit tool, which acts on the symbol in the hand
  assert.deepEqual(k("eeschema.InteractiveEdit.properties"), { do: "properties" });
  assert.deepEqual(["editReference", "editValue", "editFootprint"].map((a) => k(`eeschema.InteractiveEdit.${a}`)), [
    { do: "edit_field", name: "Reference" },
    { do: "edit_field", name: "Value" },
    { do: "edit_field", name: "Footprint" },
  ]);
  assert.deepEqual(k("eeschema.InteractiveEdit.autoplaceFields"), { do: "autoplace" });
  assert.deepEqual(k("eeschema.InteractiveEdit.toggleDeMorgan"), { do: "body_style" });
  assert.deepEqual(["setDNP", "setExcludeFromBOM", "setExcludeFromBoard", "setExcludeFromSimulation"].map((a) => k(`eeschema.EditorControl.${a}`)), [
    { do: "attribute", key: "dnp" },
    { do: "attribute", key: "exclude_from_bom" },
    { do: "attribute", key: "exclude_from_board" },
    { do: "attribute", key: "exclude_from_sim" },
  ]);
  // the right-button menu's Symbol Unit choice (`ID_POPUP_SCH_SELECT_UNIT`)
  assert.deepEqual(k("studio.SymbolUnit.select"), { do: "unit" });
  assert.deepEqual(k("common.Control.zoomIn"), { do: "pass" });
  // armed with nothing in the hand, Place Symbol lets Undo through (it checks Undo only with a symbol in hand)
  assert.equal(armedKey("sch_place_symbol", "common.Interactive.undo"), "pass");
  assert.deepEqual(describeCarried(symbol({ turns: ["rot_ccw"] })), { kind: "symbol", libId: "Amplifier_Operational:LM358", ref: "U3", unit: 1, turns: ["rot_ccw"], placeAllUnits: false, oneOnly: false, value: "LM358", footprint: "", bodyStyle: 1, attrs: {}, edits: 0 });
});

test("the turns of a carried symbol fold into KiCad's orientation as `Matrix::then` composes them and `GetOrientation` reads them back", () => {
  const o = (rotDeg: number, mirror: "x" | "y" | null, turns: CarriedSymbol["turns"]) => symbolOrientation({ rotDeg, mirror, turns });
  assert.deepEqual(o(0, null, []), { rotMillideg: 0, mirror: null });
  assert.deepEqual(o(0, null, ["rot_ccw"]), { rotMillideg: 90_000, mirror: null });
  assert.deepEqual(o(0, null, ["rot_ccw", "rot_ccw", "rot_ccw"]), { rotMillideg: 270_000, mirror: null });
  assert.deepEqual(o(0, null, ["rot_cw"]), { rotMillideg: 270_000, mirror: null });
  assert.deepEqual(o(0, null, ["rot_ccw", "rot_cw"]), { rotMillideg: 0, mirror: null });
  // X (Mirror Vertically, `SYM_MIRROR_X`) is the IR's `mirror_y`, "x" here; Y the IR's `mirrored`, "y"
  assert.deepEqual(o(0, null, ["mirror_v"]), { rotMillideg: 0, mirror: "x" });
  assert.deepEqual(o(0, null, ["mirror_h"]), { rotMillideg: 0, mirror: "y" });
  // two mirrors across one axis undo each other; across both they are half a turn
  assert.deepEqual(o(0, null, ["mirror_h", "mirror_h"]), { rotMillideg: 0, mirror: null });
  assert.deepEqual(o(0, null, ["mirror_h", "mirror_v"]), { rotMillideg: 180_000, mirror: null });
  // a copy made turned keeps its orientation: 90 + mirrored, turned back a quarter
  assert.deepEqual(o(90, "y", ["rot_cw"]), symbolOrientation({ rotDeg: 0, mirror: null, turns: ["rot_ccw", "mirror_h", "rot_cw"] }));
  // 90 + mirrored is the matrix of 270 + mirror_y, which `GetOrientation` lists first
  assert.deepEqual(o(90, "y", []), { rotMillideg: 270_000, mirror: "x" });
  // Autoplace Fields while it rides folds the turns in
  const settled = settledSymbol(symbol({ turns: ["rot_ccw", "mirror_v"] }));
  assert.deepEqual([settled.rotDeg, settled.mirror, settled.turns], [90, "x", []]);
});

test("what a carried symbol was given while it rode -- body style, attributes, the dialogs' commands -- goes into the click's one command after it is made", () => {
  const s = symbol({ bodyStyle: 2, attrs: { dnp: true }, edits: [{ op: "sch_edit", verb: "edit_field", id: "fld:U3:Value", bold: true } as unknown as Cmd] });
  const cmds = (placeCmd(s, [0, 0]) as { cmds: Array<Record<string, unknown>> }).cmds;
  assert.deepEqual(cmds.slice(1), [
    { op: "sch_edit", verb: "set_body_style", ids: ["U3"], style: 2 },
    { op: "set_symbol_attrs", ids: ["U3"], dnp: true },
    { op: "sch_edit", verb: "edit_field", id: "fld:U3:Value", bold: true },
  ]);
  // the preview runs them too: the server draws the symbol as the click will make it
  assert.equal((previewCmd(s) as { cmds: unknown[] }).cmds.length, 4);
});

test("a dialog's OK on the carried symbol changes what it is made with and keeps the rest, renamed with it", () => {
  const s = symbol({ edits: [{ op: "sch_edit", verb: "set_field_looks", ids: ["fld:U3:Value"], font: "KiCad Font" } as unknown as Cmd] });
  const next = applyCarriedCmds(s, [
    { op: "edit_symbol_fields", id: "U3", value: "TL072", footprint: "Package_SO:SOIC-8", datasheet: "" } as unknown as Cmd,
    { op: "set_symbol_attrs", ids: ["U3"], exclude_from_bom: true } as Cmd,
    { op: "sch_edit", verb: "set_body_style", ids: ["U3"], style: 2 } as unknown as Cmd,
    { op: "sch_edit", verb: "set_symbol_unit", id: "U3", unit: 1, new_unit: 2 } as unknown as Cmd,
    { op: "sch_edit", verb: "edit_field", id: "fld:U3:Reference", visible: false } as unknown as Cmd,
    { op: "rename_symbol", id: "U3", new_id: "U9" } as Cmd,
  ]) as CarriedSymbol;
  assert.deepEqual([next.ref, next.value, next.footprint, next.unit, next.bodyStyle, next.attrs], ["U9", "TL072", "Package_SO:SOIC-8", 2, 2, { exclude_from_bom: true }]);
  // the commands kept name the new reference: the field ids, the symbol's own id
  assert.deepEqual(next.edits, [
    { op: "sch_edit", verb: "set_field_looks", ids: ["fld:U9:Value"], font: "KiCad Font" },
    { op: "edit_symbol_fields", id: "U9", datasheet: "" },
    { op: "sch_edit", verb: "edit_field", id: "fld:U9:Reference", visible: false },
  ]);
  // Field Properties' text of the Value: the symbol is made with it; the edit keeps only what else it changes, and goes when that is nothing
  const v = applyCarriedCmds(s, [{ op: "sch_edit", verb: "edit_field", id: "fld:U3:Value", text: "OPA2134" } as unknown as Cmd]) as CarriedSymbol;
  assert.deepEqual([v.value, v.edits?.length], ["OPA2134", 1]);
  const vb = applyCarriedCmds(s, [{ op: "sch_edit", verb: "edit_field", id: "fld:U3:Value", text: "OPA2134", italic: true } as unknown as Cmd]) as CarriedSymbol;
  assert.deepEqual(vb.edits?.[1], { op: "sch_edit", verb: "edit_field", id: "fld:U3:Value", italic: true });
  // a power symbol's Value is its net; the rest of its Properties run by the id the server numbers it with
  const power = { kind: "power" as const, libId: "power:GND", net: "GND", rotMillideg: 0 };
  const p = applyCarriedCmds(power, [{ op: "sch_edit", verb: "edit_field", id: "fld:#PWR03:Value", text: "AGND" } as unknown as Cmd, { op: "sch_edit", verb: "set_power_props", id: "#PWR03", dnp: true } as unknown as Cmd]);
  assert.deepEqual([(p as typeof power).net, p.edits?.length], ["AGND", 1]);
  const placed = placeCmd(p, [0, 0], { powerId: "#PWR05" }) as { cmds: Array<Record<string, unknown>> };
  assert.deepEqual(placed.cmds[1], { op: "sch_edit", verb: "set_power_props", id: "#PWR05", dnp: true });
});

test("renameInCmd follows a reference through ids, unit keys, field and pin ids, and leaves other names alone", () => {
  const c = { op: "batch", cmds: [{ op: "x", id: "U3", ids: ["U3#2", "fld:U3:Value", "fld:U3#2:Value", "pin:U3:4", "U30", "fld:U30:Value"] }] } as unknown as Cmd;
  assert.deepEqual(renameInCmd(c, "U3", "U7"), { op: "batch", cmds: [{ op: "x", id: "U7", ids: ["U7#2", "fld:U7:Value", "fld:U7#2:Value", "pin:U7:4", "U30", "fld:U30:Value"] }] });
});
