import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, FieldInfo, Pad, Part } from "../api/types";
import { attrsOf, checkFieldRow, checkPadValues, drawAngle, editAttrsCmd, editFieldCmd, editPadCmd, fieldAsText, fieldById, fieldId, layoutOf, localAngleOf, newFieldLayout, newFieldName, nthOfPad, padEditOf, padEditWith, parseFieldId, userFieldsOf } from "./fpFields";
import { subItemIds } from "./pcbItems";

const field = (over: Partial<FieldInfo> = {}): FieldInfo => ({
  id: "U1:Reference",
  name: "Reference",
  text: "U1",
  x: 30_000,
  y: 16_900,
  angle: 0,
  w: 1000,
  h: 1200,
  thickness: 150,
  layer: "F.SilkS",
  visible: true,
  halign: 0,
  valign: 0,
  mirror: false,
  bold: false,
  italic: false,
  upright: true,
  knockout: false,
  lx: 0,
  ly: -3100,
  langle: 0,
  custom: false,
  ...over,
});

const pad = (num: string, over: Partial<Pad> = {}): Pad => ({ num, net: null, x: 0, y: 0, w: 600, h: 600, round: false, th: false, ...over });

const part = (over: Partial<Part> = {}): Part =>
  ({ ref: "U1", value: "MCU", package: null, mpn: null, footprint: null, block: null, placed: true, size: null, at: [30_000, 20_000], rot: 0, side: "top", pads: [pad("1"), pad("2"), pad("2", { y: 500 })], fields: [field(), field({ id: "U1:Value", name: "Value", text: "MCU" }), field({ id: "U1:Vendor", name: "Vendor", text: "ACME", visible: false })], ...over }) as Part;

const board = (p: Part = part()): BoardState => ({ parts: [p], layers: ["F.Cu", "B.Cu"] }) as unknown as BoardState;

test("a field id is the footprint's reference, a colon and the field's name; the first colon splits it", () => {
  assert.equal(fieldId("U1", "Reference"), "U1:Reference");
  assert.deepEqual(parseFieldId("U1:Reference"), { ref: "U1", name: "Reference" });
  assert.deepEqual(parseFieldId("R1#2:Vendor:code"), { ref: "R1#2", name: "Vendor:code" });
  for (const bad of ["U1", ":x", "U1:", ""]) assert.equal(parseFieldId(bad), null, bad);
});

test("the board finds a field by its id, only on a placed footprint that has it", () => {
  const b = board();
  assert.equal(fieldById(b, "U1:Vendor")?.field.text, "ACME");
  assert.equal(fieldById(b, "U1:Nope"), null);
  assert.equal(fieldById(board(part({ placed: false })), "U1:Vendor")?.field.name, undefined);
});

test("the pads and the fields of the placed footprints stay selectable after a refresh, a hidden field too", () => {
  assert.deepEqual(subItemIds(board()), ["U1.1", "U1.2", "U1.2#2", "U1:Reference", "U1:Value", "U1:Vendor"]);
  assert.deepEqual(subItemIds(board(part({ placed: false }))), [], "an unplaced footprint has none");
  assert.deepEqual(subItemIds(board(part({ fields: undefined, pads: undefined }))), []);
});

test("the layout the verbs take is the field's own frame, size as [width, height]", () => {
  const l = layoutOf(field({ halign: -1, bold: true, langle: 90_000, lx: 100, ly: -200 }));
  assert.deepEqual(l.at, { x: 100, y: -200 });
  assert.deepEqual([l.angle, l.size, l.thickness, l.layer, l.visible, l.halign, l.bold, l.keep_upright], [90_000, [1000, 1200], 150, "F.SilkS", true, -1, true, true]);
  const cmd = editFieldCmd(part(), field(), { thickness: 200 }, "text");
  assert.deepEqual(cmd, { op: "edit_board_field", part: "U1", name: "Reference", layout: { ...layoutOf(field()), thickness: 200 }, text: "text" });
});

test("the user fields are everything after the Reference and the Value, in order", () => {
  assert.deepEqual(userFieldsOf(part()).map((f) => [f.name, f.text, f.layout.visible]), [["Vendor", "ACME", false]]);
});

test("the attributes the verb takes are the footprint's, a missing set being all off", () => {
  assert.deepEqual(attrsOf(part()), { kind: "unspecified", board_only: false, exclude_from_pos_files: false, exclude_from_bom: false, dnp: false, allow_missing_courtyard: false });
  const p = part({ attrs: { kind: "smd", board_only: true, exclude_from_pos_files: false, exclude_from_bom: true, dnp: true, allow_missing_courtyard: false, custom: true } });
  assert.deepEqual(editAttrsCmd(p, { dnp: false }), { op: "edit_board_footprint", part: "U1", attrs: { kind: "smd", board_only: true, exclude_from_pos_files: false, exclude_from_bom: true, dnp: false, allow_missing_courtyard: false } });
});

test("a field's angle in the footprint's frame: the board angle is sense * langle - rot, a bottom footprint turning the other way", () => {
  // top side, turned 90 degrees clockwise (rot 90): text that reads at 0 on the board is 90 degrees in the frame
  assert.equal(localAngleOf({ side: "top", rot: 90 }, 0), 90_000);
  assert.equal(localAngleOf({ side: "top", rot: 0 }, 45_000), 45_000);
  assert.equal(localAngleOf({ side: "top", rot: 0 }, -90_000), 270_000, "negative angles are brought into range");
  assert.equal(localAngleOf({ side: "bottom", rot: 0 }, 45_000), 315_000);
  assert.equal(localAngleOf({ side: "bottom", rot: 90 }, 0), 270_000);
});

test("footprint text that keeps upright is drawn within ]-90, 90] degrees; text that does not is drawn as it is", () => {
  assert.equal(drawAngle(0, true), 0);
  assert.equal(drawAngle(90_000, true), 90_000, "a quarter turn is upright");
  assert.equal(drawAngle(180_000, true), 0);
  assert.equal(drawAngle(270_000, true), 90_000, "-90 is not within ]-90, 90]: a half turn brings it back to 90");
  assert.equal(drawAngle(200_000, true), 20_000);
  assert.equal(drawAngle(-10_000, true), -10_000);
  assert.equal(drawAngle(200_000, false), 200_000);
  assert.equal(drawAngle(-90_000, false), 270_000);
});

test("a field as the board text the canvas already draws: justification by sign, angle as drawn", () => {
  const t = fieldAsText(field({ halign: 1, angle: 200_000, mirror: true }));
  assert.deepEqual([t.content, t.x, t.y, t.size, t.stroke_width, t.justify, t.mirror, t.angle], ["U1", 30_000, 16_900, 1200, 150, "right", true, 20_000]);
  assert.equal(fieldAsText(field({ halign: -1 })).justify, "left");
});

test("the pad that shares a number is told apart by which of them it is", () => {
  const p = part();
  assert.deepEqual(p.pads!.map((q) => nthOfPad(p, q)), [1, 1, 2]);
});

test("a pad's edit is the stored one with the change laid over it; a key set to undefined goes, a hole is round or oblong", () => {
  const p = part();
  const q = p.pads![0]!;
  assert.deepEqual(padEditOf(p, q), { number: "1", nth: 1 });
  const stored = { ...q, edit: { number: "1", nth: 1, size: [700, 700] as [number, number], solder_paste_margin: 250, drill: 300 } };
  assert.deepEqual(padEditWith(p, stored, { solder_mask_margin: 40 }), { number: "1", nth: 1, size: [700, 700], solder_paste_margin: 250, drill: 300, solder_mask_margin: 40 });
  assert.deepEqual(padEditWith(p, stored, { solder_paste_margin: undefined }), { number: "1", nth: 1, size: [700, 700], drill: 300 }, "the override goes: the library's value is back");
  assert.deepEqual(padEditWith(p, stored, { drill_slot: [300, 600] }), { number: "1", nth: 1, size: [700, 700], solder_paste_margin: 250, drill_slot: [300, 600] }, "an oblong hole replaces the round one");
  assert.deepEqual(padEditWith(p, stored, { drill: 350 }), { number: "1", nth: 1, size: [700, 700], solder_paste_margin: 250, drill: 350 });
  const second = p.pads![2]!;
  assert.deepEqual(editPadCmd(p, second, { shape: "oval" }), { op: "edit_board_pad", part: "U1", edit: { number: "2", nth: 2, shape: "oval" } });
});

test("the field grid's rows are checked the way the dialog's Validate does", () => {
  const fmt = (um: number) => `${um / 1000} mm`;
  assert.equal(checkFieldRow("Vendor", { size: [1000, 1000], thickness: 150 }, fmt), null);
  assert.equal(checkFieldRow("  ", { size: [1000, 1000], thickness: 150 }, fmt), "Fields must have a name.");
  assert.equal(checkFieldRow("A", { size: [0, 1000], thickness: 150 }, fmt), "Text width must be at least 0.001 mm.");
  assert.equal(checkFieldRow("A", { size: [1000, 300_000], thickness: 150 }, fmt), "Text height must be at most 250 mm.");
  assert.match(checkFieldRow("A", { size: [1000, 1000], thickness: 400 }, fmt)!, /thickness is too large for the text size/);
  assert.equal(checkFieldRow("A", { size: [1000, 1000], thickness: 250 }, fmt), null, "a quarter of the size is the most");
});

test("a new field starts hidden at the origin on the fabrication layer of the footprint's side", () => {
  assert.deepEqual([newFieldLayout(part()).layer, newFieldLayout(part()).visible, newFieldLayout(part()).mirror], ["F.Fab", false, false]);
  assert.deepEqual([newFieldLayout(part({ side: "bottom" })).layer, newFieldLayout(part({ side: "bottom" })).mirror], ["B.Fab", true]);
  assert.equal(newFieldName([], 4), "Field4");
  assert.equal(newFieldName(["Field4", "Field5"], 4), "Field6");
});

test("pad values: sizes, holes and ratios are refused with the dialog's words", () => {
  const fmt = (um: number) => `${um / 1000} mm`;
  const ok = { kind: "through_hole", shape: "circle", size: [1600, 1600] as [number, number], drill: 800 };
  assert.equal(checkPadValues(ok, fmt), null);
  assert.equal(checkPadValues({ ...ok, size: [0, 0] }, fmt), "Error: (Pad must have a positive size)");
  assert.equal(checkPadValues({ ...ok, drill: 0 }, fmt), "Error: Through hole pad has no hole.");
  assert.match(checkPadValues({ ...ok, drill: 1600 }, fmt)!, /PTH pad hole leaves no copper/);
  assert.equal(checkPadValues({ ...ok, kind: "non_plated_hole", drill: 1600 }, fmt), null, "a non-plated hole may be as big as its pad");
  assert.equal(checkPadValues({ kind: "smd", shape: "rect", size: [600, 400], drill: null }, fmt), null, "an SMD pad has no hole");
  assert.match(checkPadValues({ kind: "through_hole", shape: "oval", size: [1500, 900], slot: [900, 1000] }, fmt)!, /leaves no copper/);
  assert.equal(checkPadValues({ kind: "smd", shape: "round_rect", size: [600, 400], ratio: 0.7 }, fmt), "Corner radius ratio must be between 0 and 50%.");
  assert.equal(checkPadValues({ kind: "smd", shape: "rect", size: [600, 400], pasteRatio: -0.8 }, fmt), "Solder paste ratio must be between -50% and 100%.");
});
