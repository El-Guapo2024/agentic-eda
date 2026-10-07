import { test } from "node:test";
import assert from "node:assert/strict";
import { convertCmds, type ConvertSource } from "./schConvertText";
import { fnv1aHex, itemId, labelId } from "./schIds";
import { escapeNetname, isBusGroup, unescapeString, validNetName } from "./schNetName";
import type { SchGraphic } from "../api/schEditTypes";

const measure = (text: string, size: number) => text.length * size * 0.6;

test("FNV-1a-64 matches the published test vectors the backend's ids are built on", () => {
  assert.equal(fnv1aHex(""), "cbf29ce484222325");
  assert.equal(fnv1aHex("a"), "af63dc4c8601ec8c");
  assert.equal(fnv1aHex("foobar"), "85944171f73967e8");
});

test("an item id is the prefix and the first 12 hex digits, with _2, _3 on a collision", () => {
  const id = labelId("VCC", [1000, 2000]);
  assert.ok(/^lbl_[0-9a-f]{12}$/.test(id), id);
  assert.equal(itemId("lbl", "VCC|1000,2000", new Set([id])), `${id}_2`);
  assert.equal(itemId("lbl", "VCC|1000,2000", new Set([id, `${id}_2`])), `${id}_3`);
});

test("netname escaping: a slash becomes {slash} and back; line breaks are dropped", () => {
  assert.equal(escapeNetname("A/B"), "A{slash}B");
  assert.equal(escapeNetname("A\nB"), "AB");
  assert.equal(unescapeString("A{slash}B"), "A/B");
  assert.equal(unescapeString("{dblquote}x{lt}"), '"x<');
  assert.equal(unescapeString("~{RESET}"), "~{RESET}", "overbar markup is left alone");
  assert.equal(unescapeString("ab"), "ab");
});

test("a valid net name has no spaces unless it is a bus group; an empty one is <empty>", () => {
  assert.equal(validNetName("my net"), "my_net");
  assert.equal(validNetName("a\tb"), "a_b");
  assert.equal(validNetName("a/b c"), "a{slash}b_c");
  assert.equal(validNetName("USB{DP DM}"), "USB{DP DM}");
  assert.equal(validNetName(""), "<empty>");
  assert.equal(isBusGroup("{A B}"), true);
  assert.equal(isBusGroup("D~{1}"), false);
});

const label = (over: Partial<Extract<ConvertSource, { kind: "label" }>> = {}): ConvertSource => ({ kind: "label", id: "lbl_old", net: "VCC", at: [10_000, 20_000], scope: "local", shape: null, spin: "right", ...over });
const text = (over: Partial<Extract<ConvertSource, { kind: "text" }>> = {}): ConvertSource => ({ kind: "text", id: "txt_old", content: "hello world", at: [5_000, 6_000], angleDeg: 0, sizeUm: 1_270, ...over });

test("a local label becomes a global label carrying its text and the constructor's default shape", () => {
  const out = convertCmds([label()], "global_label", { measure });
  assert.deepEqual(out.cmds, [
    { op: "delete_label", id: "lbl_old" },
    { op: "add_label", net: "VCC", at: { x: 10_000, y: 20_000 }, kind: { scope: "global", shape: "input" } },
  ]);
  assert.deepEqual(out.predictedIds, [labelId("VCC", [10_000, 20_000])]);
});

test("a global label keeps its shape going to a hierarchical label, and drops it going local", () => {
  const g = label({ scope: "global", shape: "bidirectional" });
  assert.deepEqual(convertCmds([g], "hier_label", { measure }).cmds[1], { op: "add_label", net: "VCC", at: { x: 10_000, y: 20_000 }, kind: { scope: "hierarchical", shape: "bidirectional" } });
  assert.deepEqual(convertCmds([g], "label", { measure }).cmds[1], { op: "add_label", net: "VCC", at: { x: 10_000, y: 20_000 }, kind: { scope: "local" } });
});

test("items already of the target type are skipped", () => {
  assert.deepEqual(convertCmds([label()], "label", { measure }).cmds, []);
  assert.deepEqual(convertCmds([text()], "text", { measure }).cmds, []);
});

test("a text becomes a label: spaces turn into underscores, the shape is passive", () => {
  const out = convertCmds([text()], "global_label", { measure });
  assert.deepEqual(out.cmds[1], { op: "add_label", net: "hello_world", at: { x: 5_000, y: 6_000 }, kind: { scope: "global", shape: "passive" } });
});

test("a label becomes a text: unescaped, the label's spin gives the text angle, default size", () => {
  const out = convertCmds([label({ net: "A{slash}B", spin: "up" })], "text", { measure });
  assert.deepEqual(out.cmds, [
    { op: "delete_label", id: "lbl_old" },
    { op: "add_sch_text", content: "A/B", at: { x: 10_000, y: 20_000 }, angle_millideg: 90_000, size_um: 1_270 },
  ]);
});

test("a text becomes a directive label whose Netclass is the text; a label's fields do not exist, so it starts empty", () => {
  const fromText = convertCmds([text({ content: "HV" })], "directive_label", { measure }).cmds[1];
  assert.deepEqual(fromText, { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "directive", at: { x: 5_000, y: 6_000 }, orientation: 0, shape: "round", pin_length_um: 2_540, netclass: "HV", component_class: "" } } });
  const fromLabel = convertCmds([label({ spin: "left" })], "directive_label", { measure }).cmds[1];
  assert.deepEqual(fromLabel, { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "directive", at: { x: 10_000, y: 20_000 }, orientation: 180_000, shape: "round", pin_length_um: 2_540, netclass: "", component_class: "" } } });
});

test("a text becomes a text box around its extent with the default margin", () => {
  const out = convertCmds([text({ content: "abc" })], "text_box", { measure });
  const add = out.cmds[1] as { op: "sch_edit"; verb: "add_graphic"; graphic: { shape: { type: "text_box"; start: { x: number; y: number }; end: { x: number; y: number }; text: string; size_um: number } } };
  assert.equal(add.graphic.shape.type, "text_box");
  assert.equal(add.graphic.shape.text, "abc");
  const w = add.graphic.shape.end.x - add.graphic.shape.start.x;
  // the text's width (3 chars * 0.6 * 1.27 mm) plus a margin on each side plus the slop
  assert.ok(Math.abs(w - (3 * 1_270 * 0.6 + 2 * 953 + Math.round(953 / 20))) <= 2, `${w}`);
});

test("a text box becomes a label at the middle of its left edge, with its text", () => {
  const g: SchGraphic = { id: "tbox_old", shape: { type: "text_box", start: { x: 0, y: 0 }, end: { x: 10_000, y: 4_000 }, text: "net a", size_um: 1_270, h_align: "left", v_align: "top" } };
  const out = convertCmds([{ kind: "text_box", id: "tbox_old", graphic: g }], "label", { measure });
  assert.deepEqual(out.cmds, [
    { op: "sch_edit", verb: "delete_graphic", id: "tbox_old" },
    { op: "add_label", net: "net_a", at: { x: 0, y: 2_000 }, kind: { scope: "local" } },
  ]);
});

test("a right-justified text box puts the label at its right edge", () => {
  const g: SchGraphic = { id: "t", shape: { type: "text_box", start: { x: 0, y: 0 }, end: { x: 10_000, y: 4_000 }, text: "x", size_um: 1_270, h_align: "right", v_align: "top" } };
  const out = convertCmds([{ kind: "text_box", id: "t", graphic: g }], "label", { measure });
  assert.deepEqual(out.cmds[1], { op: "add_label", net: "x", at: { x: 10_000, y: 2_000 }, kind: { scope: "local" } });
});

test("a directive label has no text: it becomes <empty>", () => {
  const g: SchGraphic = { id: "d", shape: { type: "directive", at: { x: 1, y: 2 }, orientation: 0, shape: "round", pin_length_um: 2_540 } };
  const out = convertCmds([{ kind: "directive", id: "d", graphic: g }], "label", { measure });
  assert.deepEqual(out.cmds[1], { op: "add_label", net: "<empty>", at: { x: 1, y: 2 }, kind: { scope: "local" } });
});

test("the old items are all deleted before any new one is added", () => {
  const out = convertCmds([label({ id: "a" }), label({ id: "b", at: [0, 0] })], "global_label", { measure });
  assert.deepEqual(out.cmds.map((c) => c.op), ["delete_label", "delete_label", "add_label", "add_label"]);
});
