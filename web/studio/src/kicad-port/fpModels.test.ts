import { test } from "node:test";
import assert from "node:assert/strict";
import type { Part } from "../api/types";
import { boardModelsCmds, cleanPath, libraryModels, rowOfCmd, rowsOfLibrary, evaluateExpression, evaluateField, formatOffset, isNumberText, formatRotation, formatScale, modelCmd, movedRows, opacityOf, plainRow, readNumber, readOffset, readRotation, rotationFromString, rowsOf, sameRows, showRotation, sliderOf, statusIcon, stepOffset, stepRotation, stepScale } from "./fpModels";

const part = (models: Part["models"]): Part => ({ ref: "U1", value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: null, models }) as Part;
const entry = (name: string, over: Record<string, unknown> = {}) => ({ name, offset: [0, 0, 0] as [number, number, number], scale: [1, 1, 1] as [number, number, number], rotate: [0, 0, 0] as [number, number, number], opacity: 1, show: true, ...over });

test("a part's models become rows with the file name as the path, and a row becomes the command's model", () => {
  const rows = rowsOf([entry("a.step", { offset: [1, 2, 3], opacity: 0.5, show: false })]);
  assert.deepEqual(rows, [{ path: "a.step", offset: [1, 2, 3], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 0.5, show: false }]);
  assert.deepEqual(modelCmd(rows[0]!), { path: "a.step", offset: [1, 2, 3], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 0.5, show: false });
  assert.deepEqual(rowsOf(undefined), []);
  assert.deepEqual(plainRow("x.wrl"), { path: "x.wrl", offset: [0, 0, 0], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 1, show: true });
  // a copy: changing the row does not change the part
  rows[0]!.offset[0] = 9;
  assert.equal(part([entry("a.step", { offset: [1, 2, 3] })]).models![0]!.offset[0], 1);
});

test("OK sends nothing for a list that is the footprint's, and the whole list when it is not; a row with no name is not sent", () => {
  const p = part([entry("a.step"), entry("b.step", { opacity: 0.25 })]);
  assert.deepEqual(boardModelsCmds(p, rowsOf(p.models)), []);
  const rows = rowsOf(p.models);
  rows[1]!.show = false;
  const cmds = boardModelsCmds(p, rows);
  assert.equal(cmds.length, 1);
  assert.equal(cmds[0]!.op, "set_board_models");
  assert.deepEqual((cmds[0] as { models: unknown[] }).models.map((m) => (m as { show: boolean }).show), [true, false]);
  // a blank row (Add) that stays blank is no change; a name with a line break in it is cleaned
  assert.deepEqual(boardModelsCmds(p, [...rowsOf(p.models), plainRow("")]), []);
  const named = boardModelsCmds(p, [...rowsOf(p.models), plainRow("c\n.step")]);
  assert.deepEqual((named[0] as { models: Array<{ path: string }> }).models.map((m) => m.path), ["a.step", "b.step", "c.step"]);
  // every model taken off is the empty list, which is a change
  const none = boardModelsCmds(p, []);
  assert.deepEqual(none, [{ op: "set_board_models", part: "U1", models: [] }]);
  assert.deepEqual(boardModelsCmds(part(undefined), []), []);
  assert.equal(cleanPath("a\r\n\tb"), "ab");
});

test("an expression in a field is worked out as NUMERIC_EVALUATOR does, and text that is not one is left", () => {
  for (const [text, want] of [["1+2", 3], ["2*3.5", 7], ["(1+2)*3", 9], ["-4/2", -2], [" 1.5 ", 1.5], ["1e1+1", 11], ["-(2+1)", -3], ["2*-3", -6], [".5*4", 2]] as const) assert.equal(evaluateExpression(text), want, text);
  for (const bad of ["", "abc", "1+", "(1", "1 2", "1/0x"]) assert.equal(evaluateExpression(bad), null, bad);
  assert.equal(evaluateExpression("1/0"), null, "infinity is no number");
  assert.equal(evaluateField("1+2"), "3");
  assert.equal(evaluateField("12.5°"), "12.5°", "a character that is not ASCII leaves the text alone");
  assert.equal(readNumber("2*45", 0), 90);
  assert.equal(readNumber("45°", 0), 45);
  assert.equal(readNumber("nope", 7), 7);
});

test("a field that is half typed in is not a number yet", () => {
  for (const ok of ["1", "-2.5", "1+2", "90°", "45 deg", "1.5 mm", "0.05in", '2"', "50 mils"]) assert.ok(isNumberText(ok), ok);
  for (const bad of ["", "-", "1+", "(", "abc", "mm"]) assert.ok(!isNumberText(bad), bad);
});

test("the fields show what the panel shows: four decimals, two and the degree sign, six and the unit", () => {
  assert.equal(formatScale(1), "1.0000");
  assert.equal(formatScale(0.12345), "0.1235");
  assert.equal(formatRotation(90), "90.00°");
  assert.equal(formatRotation(-0.004), "0.00°", "no negative zero");
  assert.equal(formatOffset(1.5, "mm"), "1.500000 mm");
  assert.equal(formatOffset(25.4, "in"), "1.000000 in");
  assert.equal(formatOffset(0.0254 * 50, "mil"), "50.000000 mils");
  assert.equal(formatOffset(-0, "mm"), "0.000000 mm");
});

test("the rotation field shows the angle counter-clockwise where the file keeps it clockwise", () => {
  assert.equal(showRotation(90), "-90.00°");
  assert.equal(showRotation(-30), "30.00°");
  assert.equal(showRotation(0), "0.00°");
  assert.equal(readRotation("-90.00°"), 90);
  assert.equal(readRotation("30"), -30);
  assert.equal(readRotation("0"), 0);
  assert.ok(Object.is(readRotation("0"), 0), "not negative zero");
  assert.equal(readRotation("45+45"), -90);
});

test("a typed rotation is brought into range by KiCad's rule, which keeps 200 as 20", () => {
  // `rotationFromString`: n = round( rotation / 180 ), rotation -= 180 n
  for (const [text, want] of [["90", 90], ["180", 180], ["181", 1], ["200", 20], ["270", -90], ["360", 0], ["540", 0], ["-181", -1], ["-200", -20], ["-270", 90], ["-360", 0]] as const) assert.equal(rotationFromString(text), want, text);
});

test("an offset is read in the editor's units, or with its own unit after it, and kept to the six decimals the field shows", () => {
  assert.equal(readOffset("1.5", "mm"), 1.5);
  assert.equal(readOffset("1.5 mm", "mil"), 1.5);
  assert.equal(readOffset("1", "in"), 25.4);
  assert.equal(readOffset("50", "mil"), 1.27);
  assert.equal(readOffset("50 mils", "mm"), 1.27);
  assert.equal(readOffset("0.05in", "mm"), 1.27);
  assert.equal(readOffset('2"', "mm"), 50.8);
  assert.equal(readOffset("1+0.5 mm", "in"), 1.5);
  assert.equal(readOffset("0.0625", "in"), 1.5875);
  assert.equal(readOffset("junk", "mm"), 0);
});

test("the spin buttons and the wheel step the fields as the panel does", () => {
  assert.equal(stepScale(1, 1, false), 1.1);
  assert.equal(stepScale(1, -1, true), 0.98);
  assert.equal(stepScale(10000, 1, false), 10000, "kept at MAX_SCALE");
  assert.equal(stepScale(0.00011, -1, false), 1 / 10000, "and at its inverse");
  assert.equal(stepRotation(10, 1, false), 100);
  assert.equal(stepRotation(10, -1, true), 9);
  assert.equal(stepOffset(0, 1, false, "mm"), 0.5);
  assert.equal(stepOffset(0, -1, true, "mm"), -0.1);
  assert.equal(stepOffset(0, 1, false, "mil"), 0.635, "25 mils");
  assert.equal(stepOffset(0, 1, true, "in"), 0.127, "5 mils");
  assert.equal(stepOffset(999.9, 1, false, "mm"), 1000);
  assert.equal(stepOffset(-999.9, -1, false, "mm"), -1000);
});

test("the opacity slider runs 0 to 100 and the opacity 0 to 1", () => {
  assert.equal(sliderOf(0.65), 65);
  assert.equal(opacityOf(65), 0.65);
  assert.equal(opacityOf(sliderOf(1)), 1);
});

test("a status shows as a warning for no name, an error for the others and nothing for a model that is there", () => {
  assert.deepEqual((["ok", "empty", "illegal", "missing", "forbidden"] as const).map(statusIcon), ["", "warning", "error", "error", "error"]);
  assert.equal(statusIcon(undefined), "", "not asked yet");
});

test("a row moves to a position and the others keep their order", () => {
  assert.deepEqual(movedRows(["a", "b", "c", "d"], 3, 0), ["d", "a", "b", "c"]);
  assert.deepEqual(movedRows(["a", "b", "c", "d"], 0, 2), ["b", "c", "a", "d"]);
  assert.deepEqual(movedRows(["a", "b"], 1, 1), ["a", "b"]);
  assert.deepEqual(movedRows(["a", "b"], 0, 5), ["a", "b"], "a position the list has none at moves nothing");
  assert.ok(sameRows([plainRow("a")], [plainRow("a")]) && !sameRows([plainRow("a")], [plainRow("b")]) && !sameRows([plainRow("a")], []));
});

test("a library footprint's models are rows with KiCad's defaults where the JSON leaves a key out, and an older entry is its one path", () => {
  assert.deepEqual(rowOfCmd({ path: "a.step" }), plainRow("a.step"));
  assert.deepEqual(rowOfCmd({ path: "a.step", offset: [1, 2, 3], opacity: 0.5, show: false }), { path: "a.step", offset: [1, 2, 3], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 0.5, show: false });
  assert.deepEqual(rowsOfLibrary({ models3d: [{ path: "a.step" }, { path: "b.step", scale: [2, 2, 2] }], model: "a.step" }).map((r) => [r.path, r.scale[0]]), [["a.step", 1], ["b.step", 2]]);
  assert.deepEqual(rowsOfLibrary({ model: "old.step" }), [plainRow("old.step")]);
  assert.deepEqual(rowsOfLibrary({ models3d: [], model: "old.step" }), [plainRow("old.step")]);
  assert.deepEqual(rowsOfLibrary({ model: null }), []);
  assert.deepEqual(rowsOfLibrary({}), []);
});

test("the Footprint Editor's OK sends the whole list, without the blank rows, and the first path as the footprint's model", () => {
  const sent = libraryModels([plainRow("a\n.step"), plainRow(""), { ...plainRow("b.step"), opacity: 0.5 }]);
  assert.equal(sent.model, "a.step");
  assert.deepEqual(sent.models3d.map((m) => m.path), ["a.step", "b.step"]);
  assert.equal(sent.models3d[1]!.opacity, 0.5);
  assert.deepEqual(libraryModels([]), { model: null, models3d: [] });
  assert.deepEqual(libraryModels([plainRow("  ")]), { model: null, models3d: [] });
});
