import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Cmd } from "../api/types";
import { boardTextBoxOf } from "./pcbTextBox";
import { pcbEdit, pcbFriendlyName, pcbGrid, pcbItemOf } from "./pcbProperties";

const textBox = boardTextBoxOf({
  id: "tbx_a",
  layer: "F.SilkS",
  text: "hello",
  start: { x: 10_000, y: 10_000 },
  end: { x: 30_000, y: 20_000 },
  margin_left_um: 500,
  margin_top_um: 600,
  margin_right_um: 700,
  margin_bottom_um: 800,
  stroke_width_um: 100,
  size_um: 1_000,
  thickness_um: 150,
  halign: "left",
  valign: "top",
});

const board = (): BoardState =>
  ({
    name: "t",
    dir: "/t",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [],
    rules: [],
    routing: null,
    drawings: { shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, text_boxes: [textBox] },
    checks: [],
    activity: [],
    job: "idle",
    locked: [],
  }) as unknown as BoardState;

const names = (b: BoardState, ids: string[]): Array<[string, string[]]> => pcbGrid(b, ids, "mm").groups.map((g) => [g.caption, g.rows.map((r) => r.name)]);
const edit = (b: BoardState, ids: string[], name: string, value: string | number | boolean): Cmd[] => {
  const plan = pcbEdit(b, ids, name, value, "mm");
  assert.equal(plan.ok, true, plan.ok ? "" : plan.error);
  return plan.ok ? plan.cmds : [];
};
/** The box a one-command edit sends. */
const sent = (cmds: Cmd[]) => {
  assert.equal(cmds.length, 1);
  const c = cmds[0]! as Extract<Cmd, { op: "pcb_item" }>;
  assert.equal(c.verb, "edit_text_box");
  return (c as Extract<typeof c, { verb: "edit_text_box" }>).text_box;
};

test("a text box is a PCB_TEXTBOX, named Text Box", () => {
  const item = pcbItemOf(board(), "tbx_a")!;
  assert.equal(item.type, "PCB_TEXTBOX");
  assert.equal(pcbFriendlyName(item), "Text Box");
});

test("a text box has the rows of PCB_TEXTBOX_DESC: position, lock and layer, then Text Properties, Border Properties and Margins", () => {
  const groups = names(board(), ["tbx_a"]);
  assert.deepEqual(groups.map((g) => g[0]), ["Basic Properties", "Text Properties", "Border Properties", "Margins"]);
  assert.deepEqual(groups[0]![1], ["Position X", "Position Y", "Locked", "Layer"]);
  assert.deepEqual(groups[1]![1], ["Text", "Thickness", "Italic", "Bold", "Mirrored", "Knockout", "Width", "Height", "Horizontal Justification", "Vertical Justification", "Orientation"]);
  assert.deepEqual(groups[2]![1], ["Border", "Border Style", "Border Width"]);
  assert.deepEqual(groups[3]![1], ["Margin Left", "Margin Top", "Margin Right", "Margin Bottom"]);
});

test("every edit of a text box is one edit_text_box of the whole box", () => {
  const b = board();
  assert.equal(sent(edit(b, ["tbx_a"], "Text", "bye")).text, "bye");
  assert.equal(sent(edit(b, ["tbx_a"], "Layer", "F.Fab")).layer, "F.Fab");
  assert.equal(sent(edit(b, ["tbx_a"], "Knockout", true)).knockout, true);
  assert.equal(sent(edit(b, ["tbx_a"], "Border", false)).border, false);
  assert.equal(sent(edit(b, ["tbx_a"], "Border Style", "dash")).line_style, "dash");
  assert.equal(sent(edit(b, ["tbx_a"], "Border Width", 250)).stroke_width_um, 250);
  assert.equal(sent(edit(b, ["tbx_a"], "Margin Right", 1_200)).margin_right_um, 1_200);
  assert.equal(sent(edit(b, ["tbx_a"], "Margin Left", 1_200)).margin_left_um, 1_200);
  assert.equal(sent(edit(b, ["tbx_a"], "Horizontal Justification", "right")).halign, "right");
  assert.equal(sent(edit(b, ["tbx_a"], "Vertical Justification", "bottom")).valign, "bottom");
  assert.equal(sent(edit(b, ["tbx_a"], "Italic", true)).italic, true);
  // an edit that changes nothing sends nothing
  assert.deepEqual(edit(b, ["tbx_a"], "Text", "hello"), []);
});

test("the pen: a thickness is the user's, one wider than a sixth of the glyph is clamped, 0 is the automatic one, and bold moves the automatic one", () => {
  const b = board();
  const own = sent(edit(b, ["tbx_a"], "Thickness", 120));
  assert.deepEqual([own.thickness_um], [120]);
  assert.equal(sent(edit(b, ["tbx_a"], "Thickness", 600)).thickness_um, 167, "a sixth of 1 mm");
  assert.equal(sent(edit(b, ["tbx_a"], "Thickness", 0)).thickness_um, 0);
  const auto = board();
  auto.drawings!.text_boxes = [{ ...textBox, thickness_auto: true, thickness: 125 }];
  assert.equal(sent(edit(auto, ["tbx_a"], "Bold", true)).thickness_um, 0, "an automatic pen stays automatic");
});

test("a size sets the glyph, the orientation turns the box about its position and a position moves it", () => {
  const b = board();
  assert.equal(sent(edit(b, ["tbx_a"], "Height", 1_500)).size_um, 1_500);
  const w = sent(edit(b, ["tbx_a"], "Width", 800));
  assert.equal(w.width_um, 800);
  const turned = sent(edit(b, ["tbx_a"], "Orientation", 90));
  assert.equal(turned.angle, 90_000);
  assert.deepEqual(turned.start, { x: 10_000, y: 10_000 });
  assert.deepEqual(turned.end, { x: 20_000, y: -10_000 });
  assert.deepEqual(edit(b, ["tbx_a"], "Position X", 11_000), [{ op: "move_items", ids: ["tbx_a"], dx: 1_000, dy: 0 }]);
  assert.deepEqual(edit(b, ["tbx_a"], "Locked", true), [{ op: "set_locked", ids: ["tbx_a"], locked: true }]);
});
