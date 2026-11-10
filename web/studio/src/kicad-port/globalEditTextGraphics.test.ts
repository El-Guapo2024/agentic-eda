import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import {
  actionChangesSomething,
  autoThickness,
  fontChoices,
  initialTextGraphicsFilter,
  layerClassRows,
  matchCount,
  matchingTextAndGraphics,
  nextTriState,
  NOTHING_IN_TEXT_SCOPE,
  textAndGraphicsCmd,
  textGraphicsLayers,
  textSizeMessage,
  UNCHANGED_ACTION,
  type TextGraphicsFilter,
  type TextGraphicsMatch,
  type TextGraphicsScope,
} from "./globalEditTextGraphics";

function board(groups: { id: string; member_ids: string[] }[] = []): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [],
    rules: [],
    routing: null,
    drawings: {
      shapes: [
        { id: "s1", layer: "F.SilkS" },
        { id: "s2", layer: "Edge.Cuts" },
        { id: "s3", layer: "User.1" },
      ],
      texts: [
        { id: "t1", layer: "F.SilkS" },
        { id: "t2", layer: "B.Fab" },
      ],
      dimensions: [{ id: "d1", layer: "Dwgs.User" }],
      groups,
      dimension_settings: {},
    },
    checks: [],
    activity: [],
    job: "idle",
  } as unknown as BoardState;
}

const BOARD_ONLY: TextGraphicsScope = { ...NOTHING_IN_TEXT_SCOPE, graphics: true, text: true, dimensions: true };
const ALL = BOARD_ONLY;
const OFF: Omit<TextGraphicsFilter, "layerOn" | "layer" | "selectedOnly"> = { referenceOn: false, reference: "", footprintOn: false, footprint: "" };
const FILTER: TextGraphicsFilter = { layerOn: false, layer: "F.SilkS", selectedOnly: false, ...OFF };
const flat = (r: TextGraphicsMatch) => [...r.fieldIds, ...r.fpTextIds, ...r.fpShapeIds, ...r.shapeIds, ...r.textIds, ...r.dimensionIds];
const none = (): TextGraphicsMatch => ({ shapeIds: [], textIds: [], dimensionIds: [], fieldIds: [], fpTextIds: [], fpShapeIds: [], fpItemTextIds: [], fpItemShapeIds: [], fpDimensionIds: [] });

test("the layer list is the copper, the technical layers, and any other layer an item is on", () => {
  const layers = textGraphicsLayers(board());
  assert.deepEqual(layers.slice(0, 2), ["F.Cu", "B.Cu"]);
  assert.ok(layers.includes("F.SilkS") && layers.includes("Edge.Cuts") && layers.includes("B.Fab") && layers.includes("Dwgs.User"));
  assert.equal(layers[layers.length - 1], "User.1", "a user layer the board uses is listed after the rest");
  assert.equal(new Set(layers).size, layers.length, "each once");
  assert.deepEqual(textGraphicsLayers(null).slice(0, 3), ["F.Adhes", "B.Adhes", "F.Paste"]);
});

test("the scope picks the kinds; with nothing in scope nothing matches", () => {
  const b = board();
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, FILTER, new Set())), ["s1", "s2", "s3", "t1", "t2", "d1"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, NOTHING_IN_TEXT_SCOPE, FILTER, new Set())), []);
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, text: true }, FILTER, new Set()), { ...none(), textIds: ["t1", "t2"] });
  assert.deepEqual(flat(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, dimensions: true }, FILTER, new Set())), ["d1"]);
});

test("the layer filter takes the items on one layer, whatever their kind", () => {
  const b = board();
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, { ...FILTER, layerOn: true, layer: "F.SilkS" }, new Set())), ["s1", "t1"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, { ...FILTER, layerOn: true, layer: "Dwgs.User" }, new Set())), ["d1"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, { ...FILTER, layerOn: true, layer: "User.1" }, new Set())), ["s3"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, { ...FILTER, layerOn: true, layer: "F.Cu" }, new Set())), []);
});

test("selected items only takes the selection and the members of a selected group, nested ones too", () => {
  const b = board([
    { id: "inner", member_ids: ["t2", "d1"] },
    { id: "outer", member_ids: ["inner", "s1"] },
  ]);
  const f = { ...FILTER, selectedOnly: true };
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, f, new Set(["s2", "t1"]))), ["s2", "t1"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, f, new Set())), []);
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, f, new Set(["inner"]))), ["t2", "d1"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, f, new Set(["outer"]))), ["s1", "t2", "d1"]);
  // both filters at once
  assert.deepEqual(flat(matchingTextAndGraphics(b, ALL, { ...f, layerOn: true, layer: "F.SilkS" }, new Set(["outer", "t1"]))), ["s1", "t1"]);
});

test("a board with no drawings matches nothing", () => {
  assert.deepEqual(matchingTextAndGraphics(null, ALL, FILTER, new Set()), none());
  assert.deepEqual(matchingTextAndGraphics({ ...board(), drawings: null } as unknown as BoardState, ALL, FILTER, new Set()), none());
});

test("the dialog opens with every filter off, the layer on the active layer when the list has it", () => {
  const b = board();
  assert.deepEqual(initialTextGraphicsFilter(b, "B.Fab"), { layerOn: false, layer: "B.Fab", selectedOnly: false, ...OFF });
  // the two parent filters keep their text for the session, their boxes off (`ChangeValue`)
  assert.deepEqual(initialTextGraphicsFilter(b, null, { reference: "R*", footprint: "Lib:*" }), { layerOn: false, layer: "F.Cu", selectedOnly: false, referenceOn: false, reference: "R*", footprintOn: false, footprint: "Lib:*" });
  assert.equal(initialTextGraphicsFilter(b, "Nope").layer, "F.Cu");
  assert.equal(initialTextGraphicsFilter(b, null).layer, "F.Cu");
});

test("text boxes and table cells are texts, barcodes graphics (round 3 D-9: `PCB_TEXTBOX_T`, `RunOnChildren`, `PCB_BARCODE_T`)", () => {
  const b = board();
  const dr = b.drawings as unknown as Record<string, unknown>;
  dr.text_boxes = [{ id: "x1", layer: "F.SilkS" }];
  dr.barcodes = [{ id: "q1", layer: "F.SilkS" }];
  dr.tables = [{ id: "tb", layer: "F.Fab", cells: [{ row: 0, col: 0 }, { row: 0, col: 1 }] }];
  const off: TextGraphicsFilter = { layerOn: false, layer: "", selectedOnly: false, ...OFF };
  const all = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, graphics: true, text: true }, off, new Set());
  assert.deepEqual(all.shapeIds, ["s1", "s2", "s3", "q1"]);
  assert.deepEqual(all.textIds, ["t1", "t2", "x1", "tb:0:0", "tb:0:1"]);
  // a cell is on its table's layer, and selected only as itself
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, text: true }, { ...off, layerOn: true, layer: "F.Fab" }, new Set()).textIds, ["tb:0:0", "tb:0:1"]);
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, text: true }, { ...off, selectedOnly: true }, new Set(["tb", "tb:0:1"])).textIds, ["tb:0:1"]);
});

// ------------------------------------------------------------------------------------------------ the footprints' items (UI-LEFT P12)

function field(ref: string, name: string, layer: string, text = name === "Reference" ? ref : "x") {
  return { id: `${ref}:${name}`, name, text, layer };
}

/** Two footprints with fields, texts and graphics, and one that is not placed. */
function boardWithFootprints(): BoardState {
  const b = board([{ id: "grp", member_ids: ["R2"] }]);
  (b as unknown as { parts: unknown[] }).parts = [
    {
      ref: "R1",
      placed: true,
      footprint: "Resistor_SMD:R_0603",
      fields: [field("R1", "Reference", "F.SilkS"), field("R1", "Value", "F.Fab"), field("R1", "MPN", "F.Fab"), field("R1", "Ref2", "F.Fab", "${REFERENCE}")],
      texts: [
        { id: "R1~t0", text: "R1", layer: "F.Fab", role: "reference" },
        { id: "R1~t1", text: "note", layer: "F.SilkS" },
      ],
      graphics: [{ kind: "segment", layer: "F.SilkS" }, { kind: "rect", layer: "F.Fab" }],
    },
    {
      ref: "R2",
      placed: true,
      footprint: "Resistor_SMD:R_0805",
      fields: [field("R2", "Reference", "B.SilkS"), field("R2", "Value", "B.Fab")],
      texts: [{ id: "R2~t0", text: "10k", layer: "B.Fab", role: "value" }],
      graphics: [{ kind: "segment", layer: "B.SilkS" }],
    },
    { ref: "C1", placed: false, footprint: "Capacitor_SMD:C_0603", fields: [field("C1", "Reference", "F.SilkS")] },
  ];
  return b;
}

const FP_ALL: TextGraphicsScope = { ...NOTHING_IN_TEXT_SCOPE, references: true, values: true, otherFields: true, footprintGraphics: true, footprintTexts: true };

test("each footprint scope box picks its items, footprint by footprint, before the board's", () => {
  const b = boardWithFootprints();
  const m = matchingTextAndGraphics(b, { ...FP_ALL, graphics: true }, FILTER, new Set());
  assert.deepEqual(m.fieldIds, ["R1:Reference", "R1:Value", "R1:MPN", "R1:Ref2", "R2:Reference", "R2:Value"], "a footprint that is not placed is not on the board");
  assert.deepEqual(m.fpTextIds, ["R1~t0", "R1~t1", "R2~t0"]);
  assert.deepEqual(m.fpShapeIds, ["R1~g0", "R1~g1", "R2~g0"]);
  assert.deepEqual(m.shapeIds, ["s1", "s2", "s3"]);
  assert.equal(matchCount(m), 15);
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, values: true }, FILTER, new Set()).fieldIds, ["R1:Value", "R2:Value"]);
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintGraphics: true }, FILTER, new Set()).fpShapeIds, ["R1~g0", "R1~g1", "R2~g0"]);
  assert.equal(matchCount(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintDimensions: true }, FILTER, new Set())), 0, "these footprints have no dimensions");
});

/** R1 with a hidden text, a courtyard graphic, a text box, a table, a barcode and a dimension of its own (UI-LEFT-2 PP23). */
function boardWithFootprintItems(): BoardState {
  const b = boardWithFootprints();
  const r1 = (b.parts as unknown as Record<string, unknown>[])[0]!;
  r1.hidden_texts = [{ id: "R1~t2", layer: "F.Fab", role: "reference" }];
  r1.courtyard_graphics = [{ id: "R1~c0", layer: "F.CrtYd" }];
  r1.items = {
    text_boxes: [{ id: "R1~tbx_1", layer: "F.SilkS" }],
    tables: [{ id: "R1~tbl_1", layer: "F.Fab", cells: [{ row: 0, col: 0 }, { row: 0, col: 1 }] }],
    barcodes: [{ id: "R1~bar_1", layer: "F.SilkS" }],
    dimensions: [{ id: "R1~dim_1", layer: "F.Fab" }],
  };
  return b;
}

test("the footprints' hidden texts, courtyard graphics and own items are in their scopes, and go in the command by their kinds", () => {
  const b = boardWithFootprintItems();
  const texts = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintTexts: true }, FILTER, new Set());
  assert.deepEqual(texts.fpTextIds, ["R1~t0", "R1~t1", "R1~t2", "R2~t0"], "a hidden text too");
  assert.deepEqual(texts.fpItemTextIds, ["R1~tbx_1", "R1~tbl_1:0:0", "R1~tbl_1:0:1"]);
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, references: true }, FILTER, new Set()).fpTextIds, ["R1~t0", "R1~t2"], "a hidden ${REFERENCE} text goes with Reference designators");
  const graphics = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintGraphics: true }, FILTER, new Set());
  assert.deepEqual(graphics.fpShapeIds, ["R1~g0", "R1~g1", "R1~c0", "R2~g0"], "the courtyard's graphics");
  assert.deepEqual(graphics.fpItemShapeIds, ["R1~bar_1"]);
  const dims = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintDimensions: true }, FILTER, new Set());
  assert.deepEqual(dims.fpDimensionIds, ["R1~dim_1"]);
  // filters: the layer, the parent, selected only (a cell as itself; the others through their footprint)
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintGraphics: true }, { ...FILTER, layerOn: true, layer: "F.CrtYd" }, new Set()).fpShapeIds, ["R1~c0"]);
  assert.equal(matchCount(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintDimensions: true }, { ...FILTER, referenceOn: true, reference: "R2" }, new Set())), 0);
  const selR1 = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, footprintTexts: true, footprintDimensions: true }, { ...FILTER, selectedOnly: true }, new Set(["R1"]));
  assert.deepEqual([selR1.fpItemTextIds, selR1.fpDimensionIds], [["R1~tbx_1"], ["R1~dim_1"]]);
  const cmd = textAndGraphicsCmd({ ...none(), fpItemTextIds: ["R1~tbx_1"], fpItemShapeIds: ["R1~bar_1"], fpDimensionIds: ["R1~dim_1"], fpShapeIds: ["R1~c0"], textIds: ["t1"] }, true, UNCHANGED_ACTION);
  assert.deepEqual([cmd.shape_ids, cmd.text_ids, cmd.dimension_ids, cmd.footprint.shape_ids], [["R1~bar_1"], ["R1~tbx_1", "t1"], ["R1~dim_1"], ["R1~c0"]]);
  assert.ok(textGraphicsLayers(b).includes("F.CrtYd"));
});

test("without Other footprint fields or Footprint text items, a ${REFERENCE} or ${VALUE} one goes with its Reference or Value box", () => {
  const b = boardWithFootprints();
  const refs = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, references: true }, FILTER, new Set());
  assert.deepEqual(refs.fieldIds, ["R1:Reference", "R1:Ref2", "R2:Reference"], "the user field holding ${REFERENCE}");
  assert.deepEqual(refs.fpTextIds, ["R1~t0"], "the ${REFERENCE} text of the fab layer");
  const values = matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, values: true }, FILTER, new Set());
  assert.deepEqual(values.fpTextIds, ["R2~t0"]);
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, otherFields: true }, FILTER, new Set()).fieldIds, ["R1:MPN", "R1:Ref2"]);
});

test("the parent filters match the footprint's reference and library link with wildcards, case-blind; a board item has no parent and passes", () => {
  const b = boardWithFootprints();
  const scope = { ...FP_ALL, graphics: true };
  const byRef = matchingTextAndGraphics(b, scope, { ...FILTER, referenceOn: true, reference: "r2" }, new Set());
  assert.deepEqual(flat(byRef), ["R2:Reference", "R2:Value", "R2~t0", "R2~g0", "s1", "s2", "s3"]);
  const byLink = matchingTextAndGraphics(b, scope, { ...FILTER, footprintOn: true, footprint: "*0603" }, new Set());
  assert.deepEqual(byLink.fieldIds, ["R1:Reference", "R1:Value", "R1:MPN", "R1:Ref2"]);
  assert.deepEqual(byLink.shapeIds, ["s1", "s2", "s3"]);
  // a box ticked with an empty text filters nothing; a text with its box off filters nothing either
  assert.equal(matchCount(matchingTextAndGraphics(b, scope, { ...FILTER, referenceOn: true, reference: "" }, new Set())), matchCount(matchingTextAndGraphics(b, scope, FILTER, new Set())));
  assert.equal(matchCount(matchingTextAndGraphics(b, scope, { ...FILTER, reference: "R2" }, new Set())), matchCount(matchingTextAndGraphics(b, scope, FILTER, new Set())));
  // with the layer filter: the footprints' items are on their board layers
  assert.deepEqual(flat(matchingTextAndGraphics(b, scope, { ...FILTER, layerOn: true, layer: "B.SilkS" }, new Set())), ["R2:Reference", "R2~g0"]);
});

test("selected items only takes an item selected itself, through its footprint, or through a group above the footprint", () => {
  const b = boardWithFootprints();
  const sel = { ...FILTER, selectedOnly: true };
  assert.deepEqual(flat(matchingTextAndGraphics(b, FP_ALL, sel, new Set(["R1:MPN", "R1~g1"]))), ["R1:MPN", "R1~g1"]);
  assert.deepEqual(matchingTextAndGraphics(b, FP_ALL, sel, new Set(["R1"])).fieldIds, ["R1:Reference", "R1:Value", "R1:MPN", "R1:Ref2"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, FP_ALL, sel, new Set(["grp"]))), ["R2:Reference", "R2:Value", "R2~t0", "R2~g0"]);
  assert.deepEqual(flat(matchingTextAndGraphics(b, FP_ALL, sel, new Set())), []);
});

test("the layer list holds the layers the footprints' items are on", () => {
  const layers = textGraphicsLayers(boardWithFootprints());
  for (const l of ["F.SilkS", "B.SilkS", "F.Fab", "B.Fab"]) assert.ok(layers.includes(l), l);
  assert.equal(new Set(layers).size, layers.length);
});

test("the action rows: three-state boxes, the font list, auto thickness and the size check", () => {
  assert.deepEqual([nextTriState(null), nextTriState(false), nextTriState(true)], [false, true, null]);
  assert.deepEqual(fontChoices(["Arial", "KiCad Font", "DejaVu Sans"]), ["-- leave unchanged --", "KiCad Font", "Arial", "DejaVu Sans"]);
  assert.equal(autoThickness(1_000_000, 1_500_000, null), 125_000, "GetPenSizeForNormal of the smaller size");
  assert.equal(autoThickness(1_000_000, 1_500_000, true), 200_000, "GetPenSizeForBold");
  assert.equal(autoThickness(null, 1_000_000, false), null, "(auto) while a size is left unchanged");
  assert.equal(textSizeMessage(500, null), "Text width must be at least 0.001 mm.");
  assert.equal(textSizeMessage(null, 300_000_000), "Text height must be less than 250 mm.");
  assert.equal(textSizeMessage(1_000_000, null), null);
  assert.equal(actionChangesSomething(UNCHANGED_ACTION), false);
  assert.equal(actionChangesSomething({ ...UNCHANGED_ACTION, keepUpright: false }), true, "a box set unchecked is a change");
});

test("the command carries the footprints' ids and only the rows that are set", () => {
  const m = { ...none(), fieldIds: ["R1:Reference"], fpTextIds: ["R1~t1"], fpShapeIds: ["R1~g0"], textIds: ["t1"] };
  const cmd = textAndGraphicsCmd(m, false, { ...UNCHANGED_ACTION, textHeight: 1_200_000, bold: true, visible: false, font: "Arial", centerOnFootprint: true, textThickness: 150_000 });
  assert.deepEqual(cmd, {
    op: "edit_text_and_graphics",
    shape_ids: [],
    text_ids: ["t1"],
    dimension_ids: [],
    footprint: { field_ids: ["R1:Reference"], text_ids: ["R1~t1"], shape_ids: ["R1~g0"] },
    layer: null,
    line_width: null,
    text_thickness: 150_000,
    rows: { height: 1_200_000, bold: true, font: "Arial", visible: false, center_on_footprint: true },
  });
  // auto thickness sends no thickness; layer defaults send none of the rows
  assert.equal((textAndGraphicsCmd(m, false, { ...UNCHANGED_ACTION, autoThickness: true, textThickness: 150_000 }) as { text_thickness: number | null }).text_thickness, null);
  assert.deepEqual(textAndGraphicsCmd(m, true, { ...UNCHANGED_ACTION, bold: true }), { op: "edit_text_and_graphics", shape_ids: [], text_ids: ["t1"], dimension_ids: [], footprint: { field_ids: ["R1:Reference"], text_ids: ["R1~t1"], shape_ids: ["R1~g0"] }, layer_defaults: true });
});

test("the layer-class grid has KiCad's six rows, the edges and courtyards a line thickness only", () => {
  const c = { line_width_nm: 1, text_width_nm: 2, text_height_nm: 3, text_thickness_nm: 4, italic: true, upright: false };
  const rows = layerClassRows({ silk: c, copper: c, fab: c, others: c, edge_cuts_line_width_nm: 50, courtyard_line_width_nm: 60 });
  assert.deepEqual(rows.map((r) => r.name), ["Silk Layers", "Copper Layers", "Edge Cuts", "Courtyards", "Fab Layers", "Other Layers"]);
  assert.deepEqual([rows[2]!.line, rows[2]!.textWidth, rows[3]!.line, rows[3]!.italic], [50, null, 60, null]);
  assert.deepEqual(layerClassRows(undefined), []);
});
