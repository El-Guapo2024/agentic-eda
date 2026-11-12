import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdShape, CmdText, LibraryFootprint, LibraryPad } from "../api/types";
import { fieldLayoutOf, fpCanMirror, fpEditTarget, fpFields, fpGroupCmd, fpHitTest, fpItemBounds, fpItemKind, fpItemPosition, fpLeaves, fpMirrorCmd, fpModificationPoint, fpMoveCmd, fpRotateCmd, fpRotationPivot, fpSelectionTarget, fpUngroupCmd } from "./fpItems";

const pad = (id: string, number: string, x: number, y: number): LibraryPad =>
  ({ id, number, at: { x, y }, size: [1_000_000, 600_000], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", drill: null, drill_slot: null, rot: 0, layers: ["F.Cu"], chamfer_corners: {} }) as unknown as LibraryPad;
const text = (id: string, content: string, x: number, y: number): CmdText => ({ id, content, at: { x, y }, angle: 0, layer: "F.Fab", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false });
const line = (id: string, x0: number, y0: number, x1: number, y1: number): CmdShape => ({ id, kind: "segment", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: x0, y: y0 }, end: { x: x1, y: y1 } });

function footprint(extra: Partial<LibraryFootprint> = {}): LibraryFootprint {
  return {
    name: "Lib:Part",
    description: "",
    keywords: "",
    attributes: { smd: true } as never,
    reference_visible: true,
    value_visible: true,
    model: null,
    pads: [pad("pad_1", "1", -2_000_000, 0), pad("pad_2", "2", 2_000_000, 0)],
    graphics: [line("fpg_1", -3_000_000, -1_000_000, 3_000_000, -1_000_000)],
    texts: [text("fpt_1", "${REFERENCE}", 0, 2_500_000)],
    fields: [{ name: "MPN", value: "X123", visible: true, layout: { at: { x: 0, y: 4_000_000 }, size: [1_000_000, 1_000_000], layer: "F.Fab" } }],
    courtyard: null,
    anchor: { x: 0, y: 0 },
    published: false,
    ...extra,
  } as LibraryFootprint;
}

test("the fields are KiCad's in KiCad's order, the Reference REF** and the Value the footprint's name at CreateNewFootprint's places when they have none", () => {
  const f = fpFields(footprint());
  assert.deepEqual(
    f.map((x) => [x.id, x.text, x.layer, x.x, x.y, x.custom]),
    [
      ["field:Reference", "REF**", "F.SilkS", 0, -500_000, false],
      ["field:Value", "Lib:Part", "F.Fab", 0, 1_000_000, false],
      ["field:MPN", "X123", "F.Fab", 0, 4_000_000, true],
    ]
  );
  // a Reference of its own text, hidden
  const g = fpFields(footprint({ reference: { at: { x: 1, y: 2 }, size: [800_000, 900_000], layer: "F.Fab", text: "U**" }, reference_visible: false }));
  assert.deepEqual([g[0]!.text, g[0]!.w, g[0]!.h, g[0]!.visible], ["U**", 800_000, 900_000, false]);
  assert.deepEqual(fieldLayoutOf(g[0]!).size, [800_000, 900_000]);
});

test("a click picks a pad first, then a field or a text, then a graphic; a hidden field is not picked", () => {
  const fp = footprint();
  assert.deepEqual(fpHitTest(fp, -2_000_000, 0, 50_000), { kind: "pad", id: "pad_1" });
  assert.deepEqual(fpHitTest(fp, 0, -500_000, 50_000), { kind: "field", id: "field:Reference" });
  assert.deepEqual(fpHitTest(fp, 0, 2_500_000, 50_000), { kind: "text", id: "fpt_1" });
  assert.deepEqual(fpHitTest(fp, 1_000_000, -1_000_000, 50_000), { kind: "graphic", id: "fpg_1" });
  assert.equal(fpHitTest(fp, 9_000_000, 9_000_000, 50_000), null);
  assert.equal(fpHitTest(footprint({ reference_visible: false }), 0, -500_000, 50_000)?.id, undefined);
});

test("a click on a member of a group selects the outermost group; a group stands for its members", () => {
  const fp = footprint({ items: { groups: [{ id: "grp_in", name: "", member_ids: ["pad_1", "fpg_1"] }, { id: "grp_out", name: "", member_ids: ["grp_in", "field:Reference"] }] } });
  assert.equal(fpSelectionTarget(fp, "pad_1"), "grp_out");
  assert.equal(fpSelectionTarget(fp, "pad_2"), "pad_2");
  // with a group entered (`EnterGroup`, UI-LEFT-2 FF8): its members one by one, a nested group as itself, nothing outside it
  assert.equal(fpSelectionTarget(fp, "pad_1", "grp_out"), "grp_in");
  assert.equal(fpSelectionTarget(fp, "field:Reference", "grp_out"), "field:Reference");
  assert.equal(fpSelectionTarget(fp, "pad_1", "grp_in"), "pad_1");
  assert.equal(fpSelectionTarget(fp, "pad_2", "grp_out"), null);
  assert.equal(fpItemKind(fp, "grp_in"), "group");
  assert.deepEqual(fpLeaves(fp, ["grp_out", "pad_2"]).sort(), ["fpg_1", "field:Reference", "pad_1", "pad_2"].sort());
});

test("Rotate: one item about its own position, a lone line too; several about the grid-snapped centre of the box of what is not text (updateModificationPoint)", () => {
  const fp = footprint();
  // R is rotateCcw: one quarter turn, -90 degrees on the wire (clockwise-positive)
  assert.deepEqual(fpRotateCmd(fp, "Lib:Part", ["pad_2"], 1), { op: "fp_item", verb: "rotate", footprint: "Lib:Part", ids: ["pad_2"], pivot: { x: 2_000_000, y: 0 }, angle_millideg: -90_000 });
  assert.deepEqual(fpItemPosition(fp, "fpg_1"), [-3_000_000, -1_000_000]);
  // the pads' box is centred on (0, 0); the Reference and the text are left out of it
  assert.deepEqual(fpModificationPoint(fp, ["pad_1", "pad_2", "field:Reference", "fpt_1"]), [0, 0]);
  const snap = (p: readonly [number, number]): [number, number] => [Math.round(p[0] / 250_000) * 250_000, Math.round(p[1] / 250_000) * 250_000];
  // the line runs from -3 to 3 mm at y -1, the pad covers y -0.3..0.3: the centre (0, -0.35) goes to the 0.25 mm grid
  assert.deepEqual(fpModificationPoint(fp, ["pad_2", "fpg_1"], snap), [0, -250_000]);
  // only texts and fields: their own box
  assert.ok(fpModificationPoint(fp, ["field:Reference", "fpt_1"]));
  // a lone rectangle turns about its centre (`usePcbShapeCenter`)
  const rect = footprint({ graphics: [{ id: "r", kind: "rect", layer: "F.Fab", stroke_width: 100_000, filled: false, start: { x: 0, y: 0 }, end: { x: 2_000_000, y: 4_000_000 } }] });
  assert.deepEqual(fpRotationPivot(rect, ["r"]), [1_000_000, 2_000_000]);
  assert.equal(fpRotateCmd(fp, "Lib:Part", [], 1), null);
});

test("Mirror acts on EDIT_TOOL::MirrorableItems about the modification point; a dimension alone is not mirrorable", () => {
  const fp = footprint({ display: { dimensions: [{ id: "dim_1", layer: "Cmts.User", kind: "aligned", start: [0, 0], end: [1, 0], lines: [], text: "", text_at: [0, 0], text_size_nm: 1, stroke_width: 1 } as never], zones: [], text_boxes: [], images: [], points: [], barcodes: [], tables: [] } });
  assert.equal(fpCanMirror(fp, ["dim_1"]), false);
  assert.equal(fpMirrorCmd(fp, "Lib:Part", ["dim_1"], "left_right"), null);
  assert.deepEqual(fpMirrorCmd(fp, "Lib:Part", ["pad_1", "pad_2"], "top_bottom"), { op: "fp_item", verb: "mirror", footprint: "Lib:Part", ids: ["pad_1", "pad_2"], pivot: { x: 0, y: 0 }, direction: "top_bottom" });
  assert.equal(fpCanMirror(fp, ["field:Value"]), true);
});

test("Move, Group and Ungroup are one fp_item verb each; a group needs two groupable items", () => {
  const fp = footprint({ display: { dimensions: [], zones: [], text_boxes: [], images: [], points: [{ id: "pnt_1", layer: "F.Fab", x: 0, y: 0, size: 1_000_000 }], barcodes: [], tables: [] }, items: { groups: [{ id: "grp_1", name: "", member_ids: ["pad_1", "pad_2"] }] } });
  assert.deepEqual(fpMoveCmd("Lib:Part", ["field:Reference"], 250_000, 0), { op: "fp_item", verb: "move", footprint: "Lib:Part", ids: ["field:Reference"], dx: 250_000, dy: 0 });
  assert.equal(fpMoveCmd("Lib:Part", ["pad_1"], 0, 0), null);
  // `IsGroupableType`: a point is not
  assert.equal(fpGroupCmd(fp, "Lib:Part", ["pnt_1", "pad_1"]), null);
  assert.deepEqual(fpGroupCmd(fp, "Lib:Part", ["fpg_1", "field:Value", "pnt_1"]), { op: "fp_item", verb: "group", footprint: "Lib:Part", ids: ["fpg_1", "field:Value"] });
  assert.deepEqual(fpUngroupCmd(fp, "Lib:Part", ["grp_1", "pad_1"]), { op: "fp_item", verb: "ungroup", footprint: "Lib:Part", ids: ["grp_1"] });
  assert.equal(fpUngroupCmd(fp, "Lib:Part", ["pad_1"]), null);
});

test("E / a double click opens OnEditItemRequest's dialog of the item; nothing selected is Footprint Properties, a point opens nothing", () => {
  const fp = footprint({ display: { dimensions: [], zones: [], text_boxes: [], images: [], points: [{ id: "pnt_1", layer: "F.Fab", x: 0, y: 0, size: 1_000_000 }], barcodes: [], tables: [] } });
  assert.deepEqual(fpEditTarget(fp, []), { dialog: "footprint" });
  assert.deepEqual(fpEditTarget(fp, ["pad_1"]), { dialog: "pad", id: "pad_1" });
  assert.deepEqual(fpEditTarget(fp, ["field:Value"]), { dialog: "field", id: "field:Value" });
  assert.deepEqual(fpEditTarget(fp, ["fpt_1"]), { dialog: "text", id: "fpt_1" });
  assert.deepEqual(fpEditTarget(fp, ["fpg_1"]), { dialog: "shape", id: "fpg_1" });
  assert.equal(fpEditTarget(fp, ["pnt_1"]), null);
  assert.equal(fpEditTarget(fp, ["pad_1", "pad_2"]), null);
});

test("a centred text's box is about its anchor, a top- or bottom-justified one's below or above it, as the painter draws them (UI-LEFT F5)", () => {
  // `${REFERENCE}` at (0, 2.5 mm), 1 mm high: a line 0.83 mm below the anchor is the line's, not the text's
  const fp = footprint({ texts: [text("fpt_r", "${REFERENCE}", 0, 2_500_000)], graphics: [line("fpg_l", 1_130_000, 3_330_000, 2_130_000, 3_330_000)] });
  const b = fpItemBounds(fp, "fpt_r")!;
  assert.deepEqual([b[1], b[3]], [2_000_000, 3_000_000]);
  assert.deepEqual(fpHitTest(fp, 1_130_000, 3_330_000, 80_000), { kind: "graphic", id: "fpg_l" });
  const top = fpItemBounds({ ...fp, kicad: { text_extras: [{ id: "fpt_r", valign: "top" }] } } as unknown as LibraryFootprint, "fpt_r")!;
  const bottom = fpItemBounds({ ...fp, kicad: { text_extras: [{ id: "fpt_r", valign: "bottom" }] } } as unknown as LibraryFootprint, "fpt_r")!;
  assert.deepEqual([top[1], top[3], bottom[1], bottom[3]], [2_360_000, 3_360_000, 1_640_000, 2_640_000]);
});

test("the Selection Filter pane gates a click (`itemPassesFilter`, UI-LEFT-2 FF5)", () => {
  const fp = footprint();
  const all = { lockedItems: false, footprints: true, text: true, tracks: true, vias: true, pads: true, graphics: true, zones: true, keepouts: true, dimensions: true, points: true, otherItems: true };
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, { layerVisible: {}, filter: all })?.id, "pad_1");
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, { layerVisible: {}, filter: { ...all, pads: false } }), null);
  assert.equal(fpHitTest(fp, 1_000_000, -1_000_000, 50_000, { layerVisible: {}, filter: { ...all, graphics: false } }), null);
  const locked = footprint({ kicad: { locked: ["fpg_1"] } } as never);
  assert.equal(fpHitTest(locked, 1_000_000, -1_000_000, 50_000, { layerVisible: {}, filter: all }), null, "a locked item needs Locked items");
  assert.equal(fpHitTest(locked, 1_000_000, -1_000_000, 50_000, { layerVisible: {}, filter: { ...all, lockedItems: true } })?.id, "fpg_1");
});
