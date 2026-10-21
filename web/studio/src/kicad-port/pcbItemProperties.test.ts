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

// ------------------------------------------------------------------------------------------------------------------------- reference images

const refImage = { id: "img_a", layer: "Cmts.User", x: 50_000, y: 40_000, w: 12_000, h: 8_000, scale_ppm: 1_000_000, px: [120, 80] as [number, number], ppi: 254, hash: "h" };
const withImage = (): BoardState => {
  const b = board();
  b.drawings!.images = [refImage];
  return b;
};
const sentImage = (cmds: Cmd[]) => {
  assert.equal(cmds.length, 1);
  const c = cmds[0]! as Extract<Cmd, { op: "pcb_item" }>;
  assert.equal(c.verb, "edit_ref_image");
  return c as Extract<typeof c, { verb: "edit_ref_image" }>;
};

test("a reference image is a PCB_REFERENCE_IMAGE with the rows of PCB_REFERENCE_IMAGE_DESC: position, lock, Associated Layer, then Image Properties", () => {
  const item = pcbItemOf(withImage(), "img_a")!;
  assert.equal(item.type, "PCB_REFERENCE_IMAGE");
  assert.equal(pcbFriendlyName(item), "Reference Image");
  const groups = names(withImage(), ["img_a"]);
  assert.deepEqual(groups.map((g) => g[0]), ["Basic Properties", "Image Properties"]);
  assert.deepEqual(groups[0]![1], ["Position X", "Position Y", "Locked", "Associated Layer"]);
  assert.deepEqual(groups[1]![1], ["Scale", "Width", "Height"]);
});

test("the scale, the size and the layer of an image are one edit_ref_image that keeps its centre; a size is a scale about the centre", () => {
  const b = withImage();
  const half = sentImage(edit(b, ["img_a"], "Scale", 0.5));
  assert.deepEqual([half.id, half.layer, half.at, half.scale_ppm], ["img_a", "Cmts.User", { x: 50_000, y: 40_000 }, 500_000]);
  // 120 px at 254 ppi are 12 mm: 6 mm wide is half the scale, 16 mm high (80 px are 8 mm) is twice
  assert.equal(sentImage(edit(b, ["img_a"], "Width", 6_000)).scale_ppm, 500_000);
  assert.equal(sentImage(edit(b, ["img_a"], "Height", 16_000)).scale_ppm, 2_000_000);
  assert.equal(sentImage(edit(b, ["img_a"], "Associated Layer", "Dwgs.User")).layer, "Dwgs.User");
  assert.deepEqual(edit(b, ["img_a"], "Scale", 1), [], "the same scale sends nothing");
  assert.deepEqual(edit(b, ["img_a"], "Position X", 51_000), [{ op: "move_items", ids: ["img_a"], dx: 1_000, dy: 0 }]);
  // a scale that is not a number above zero is refused
  const refused = pcbEdit(b, ["img_a"], "Scale", 0, "mm");
  assert.equal(refused.ok, false);
});

// ------------------------------------------------------------------------------------------------------------------------------- points

test("a point is a PCB_POINT with the rows of PCB_POINT_DESC: position, lock, layer and a Size; its lock is not writeable", () => {
  const b = board();
  b.drawings!.points = [{ id: "pnt_a", layer: "F.SilkS", x: 10_000, y: 20_000, size: 1_000 }];
  const item = pcbItemOf(b, "pnt_a")!;
  assert.equal(item.type, "PCB_POINT");
  assert.equal(pcbFriendlyName(item), "Point");
  const grid = pcbGrid(b, ["pnt_a"], "mm");
  const rows = grid.groups.flatMap((g) => g.rows);
  assert.deepEqual(rows.map((r) => r.name), ["Position X", "Position Y", "Locked", "Layer", "Size"]);
  assert.equal(rows.find((r) => r.name === "Locked")?.writable, false, "the file form of a point has no lock");
  const sentPoint = (cmds: Cmd[]) => {
    assert.equal(cmds.length, 1);
    const c = cmds[0]! as Extract<Cmd, { op: "pcb_item" }>;
    assert.equal(c.verb, "edit_point");
    return c as Extract<typeof c, { verb: "edit_point" }>;
  };
  const size = sentPoint(edit(b, ["pnt_a"], "Size", 2_500));
  assert.deepEqual([size.id, size.layer, size.at, size.size_um], ["pnt_a", "F.SilkS", { x: 10_000, y: 20_000 }, 2_500]);
  assert.equal(sentPoint(edit(b, ["pnt_a"], "Layer", "F.Fab")).layer, "F.Fab");
  assert.deepEqual(edit(b, ["pnt_a"], "Size", 1_000), [], "the same size sends nothing");
  assert.deepEqual(edit(b, ["pnt_a"], "Position Y", 21_000), [{ op: "move_items", ids: ["pnt_a"], dx: 0, dy: 1_000 }]);
  assert.equal(pcbEdit(b, ["pnt_a"], "Size", 0, "mm").ok, false, "a mark of no size is refused");
});

// ----------------------------------------------------------------------------------------------------------------------------- barcodes

test("a barcode is a PCB_BARCODE with the rows of PCB_BARCODE_DESC; Error Correction is for the QR kinds (Micro QR has no High) and the margins for a knockout; every row is one edit_barcode", () => {
  const b = board();
  b.drawings!.barcodes = [{ id: "bar_a", layer: "F.SilkS", x: 10_000, y: 20_000, angle: 0, w: 10_000, h: 10_000, text: "HELLO", text_height: 1_500, kind: "qr", ecc: "L", show_text: true, knockout: false, margin_x: 0, margin_y: 0 }];
  const item = pcbItemOf(b, "bar_a")!;
  assert.equal(item.type, "PCB_BARCODE");
  assert.equal(pcbFriendlyName(item), "Barcode");
  const rows = (): string[] => pcbGrid(b, ["bar_a"], "mm").groups.flatMap((g) => g.rows).map((r) => r.name);
  assert.deepEqual(rows(), ["Position X", "Position Y", "Locked", "Layer", "Text", "Show Text", "Text Size", "Width", "Height", "Orientation", "Barcode Type", "Error Correction", "Knockout"]);
  const sentBarcode = (cmds: Cmd[]) => {
    assert.equal(cmds.length, 1);
    const c = cmds[0]! as Extract<Cmd, { op: "pcb_item" }>;
    assert.equal(c.verb, "edit_barcode");
    return c as Extract<typeof c, { verb: "edit_barcode" }>;
  };
  const text = sentBarcode(edit(b, ["bar_a"], "Text", "WORLD"));
  assert.deepEqual([text.id, text.barcode.text, text.barcode.kind, text.barcode.width_um, text.barcode.at], ["bar_a", "WORLD", "qr", 10_000, { x: 10_000, y: 20_000 }]);
  assert.equal(sentBarcode(edit(b, ["bar_a"], "Width", 12_000)).barcode.width_um, 12_000);
  assert.equal(sentBarcode(edit(b, ["bar_a"], "Orientation", 90)).barcode.angle, 90_000);
  assert.equal(sentBarcode(edit(b, ["bar_a"], "Orientation", -90)).barcode.angle, 270_000, "a turn is 0 to 360");
  assert.equal(sentBarcode(edit(b, ["bar_a"], "Show Text", false)).barcode.show_text, false);
  assert.equal(sentBarcode(edit(b, ["bar_a"], "Layer", "F.Fab")).barcode.layer, "F.Fab");
  assert.deepEqual(edit(b, ["bar_a"], "Text", "HELLO"), [], "the same text sends nothing");
  assert.deepEqual(edit(b, ["bar_a"], "Position X", 11_000), [{ op: "move_items", ids: ["bar_a"], dx: 1_000, dy: 0 }]);
  assert.equal(pcbEdit(b, ["bar_a"], "Width", 0, "mm").ok, false, "a symbol of no size is refused");
  // the kinds: Micro QR has no High, so a High level goes to Q with the kind; Code 128 has no level at all
  b.drawings!.barcodes![0]!.ecc = "H";
  const micro = sentBarcode(edit(b, ["bar_a"], "Barcode Type", "microqr"));
  assert.deepEqual([micro.barcode.kind, micro.barcode.ecc], ["microqr", "Q"]);
  assert.deepEqual(pcbGrid(b, ["bar_a"], "mm").groups.flatMap((g) => g.rows).find((r) => r.name === "Error Correction")?.choices?.map((c) => c.value), ["L", "M", "Q", "H"]);
  b.drawings!.barcodes![0]!.kind = "microqr";
  b.drawings!.barcodes![0]!.ecc = "Q";
  assert.deepEqual(pcbGrid(b, ["bar_a"], "mm").groups.flatMap((g) => g.rows).find((r) => r.name === "Error Correction")?.choices?.map((c) => c.value), ["L", "M", "Q"]);
  b.drawings!.barcodes![0]!.kind = "code128";
  assert.ok(!rows().includes("Error Correction"), "a one-dimensional code has no error correction");
  // a knockout has margins
  assert.ok(!rows().includes("Margin X"));
  b.drawings!.barcodes![0]!.knockout = true;
  assert.deepEqual(rows().slice(-3), ["Knockout", "Margin X", "Margin Y"]);
  assert.equal(sentBarcode(edit(b, ["bar_a"], "Margin X", 800)).barcode.margin_x_um, 800);
});
