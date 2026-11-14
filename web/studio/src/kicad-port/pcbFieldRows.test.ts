import { test } from "node:test";
import assert from "node:assert/strict";
import type { FieldInfo, Part } from "../api/types";
import { addFieldCmds, fieldKeyEditable, labelOutcome, newBlankFieldName, removeFieldCmds, renameFieldCmds } from "./pcbFieldRows";

const field = (name: string, text = "", custom = false): FieldInfo => ({ id: `R1:${name}`, name, text, x: 0, y: 0, angle: 0, w: 1000000, h: 1000000, thickness: 150000, layer: "F.Fab", visible: false, halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false, lx: 0, ly: 0, langle: 0, custom });
const part = (ref: string, fields: FieldInfo[]): Part => ({ ref, placed: true, side: "top", rot: 0, at: [0, 0], fields }) as unknown as Part;

test("Add Field: Field%d, the first no selected footprint has, empty and hidden, on each footprint with its user fields kept", () => {
  const a = part("R1", [field("Reference", "R1"), field("Value", "10k"), field("Datasheet"), field("Field0", "x")]);
  const b = part("R2", [field("Reference", "R2"), field("Value", "10k"), field("Field1")]);
  assert.equal(newBlankFieldName([a, b]), "Field2");
  const cmds = addFieldCmds([a, b], "Field2") as Array<{ part: string; fields: { name: string; text: string; layout: { visible: boolean } }[] }>;
  assert.deepEqual(cmds.map((c) => [c.part, c.fields.map((f) => f.name)]), [["R1", ["Field0", "Field2"]], ["R2", ["Field1", "Field2"]]], "the symbol's Datasheet stays out of the footprint's list");
  assert.deepEqual([cmds[0]!.fields[1]!.text, cmds[0]!.fields[1]!.layout.visible], ["", false]);
});

test("a user field's name can be edited and the field removed; a mandatory one cannot; renames keep the text", () => {
  const a = part("R1", [field("Reference", "R1"), field("Value", "10k"), field("MPN", "X-1")]);
  assert.equal(fieldKeyEditable(a, "MPN"), true);
  assert.equal(fieldKeyEditable(a, "Value"), false);
  assert.equal(fieldKeyEditable(a, "Nope"), false);
  assert.deepEqual(removeFieldCmds([a], "Value"), []);
  assert.deepEqual((removeFieldCmds([a], "MPN")[0] as { fields: unknown[] }).fields, []);
  assert.deepEqual((renameFieldCmds([a], "MPN", "Part Number")[0] as { fields: { name: string; text: string }[] }).fields.map((f) => [f.name, f.text]), [["Part Number", "X-1"]]);
});

test("a name typed: the new row left blank or given a used name is removed, an old one keeps its label", () => {
  const inUse = (n: string) => ["Reference", "Field0"].includes(n);
  assert.equal(labelOutcome("", true, inUse), "remove");
  assert.equal(labelOutcome("Field0", true, inUse), "remove");
  assert.equal(labelOutcome("MPN", true, inUse), "rename");
  assert.equal(labelOutcome("Reference", false, inUse), null);
  assert.equal(labelOutcome("MPN", false, inUse), "rename");
});
