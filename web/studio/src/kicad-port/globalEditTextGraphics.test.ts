import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { initialTextGraphicsFilter, matchingTextAndGraphics, NOTHING_IN_TEXT_SCOPE, textGraphicsLayers, type TextGraphicsFilter, type TextGraphicsScope } from "./globalEditTextGraphics";

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

const ALL: TextGraphicsScope = { graphics: true, text: true, dimensions: true };
const FILTER: TextGraphicsFilter = { layerOn: false, layer: "F.SilkS", selectedOnly: false };
const flat = (r: { shapeIds: string[]; textIds: string[]; dimensionIds: string[] }) => [...r.shapeIds, ...r.textIds, ...r.dimensionIds];

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
  assert.deepEqual(matchingTextAndGraphics(b, { ...NOTHING_IN_TEXT_SCOPE, text: true }, FILTER, new Set()), { shapeIds: [], textIds: ["t1", "t2"], dimensionIds: [] });
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
  assert.deepEqual(matchingTextAndGraphics(null, ALL, FILTER, new Set()), { shapeIds: [], textIds: [], dimensionIds: [] });
  assert.deepEqual(matchingTextAndGraphics({ ...board(), drawings: null } as unknown as BoardState, ALL, FILTER, new Set()), { shapeIds: [], textIds: [], dimensionIds: [] });
});

test("the dialog opens with every filter off, the layer on the active layer when the list has it", () => {
  const b = board();
  assert.deepEqual(initialTextGraphicsFilter(b, "B.Fab"), { layerOn: false, layer: "B.Fab", selectedOnly: false });
  assert.equal(initialTextGraphicsFilter(b, "Nope").layer, "F.Cu");
  assert.equal(initialTextGraphicsFilter(b, null).layer, "F.Cu");
});

test("text boxes and table cells are texts, barcodes graphics (round 3 D-9: `PCB_TEXTBOX_T`, `RunOnChildren`, `PCB_BARCODE_T`)", () => {
  const b = board();
  const dr = b.drawings as unknown as Record<string, unknown>;
  dr.text_boxes = [{ id: "x1", layer: "F.SilkS" }];
  dr.barcodes = [{ id: "q1", layer: "F.SilkS" }];
  dr.tables = [{ id: "tb", layer: "F.Fab", cells: [{ row: 0, col: 0 }, { row: 0, col: 1 }] }];
  const off: TextGraphicsFilter = { layerOn: false, layer: "", selectedOnly: false };
  const all = matchingTextAndGraphics(b, { graphics: true, text: true, dimensions: false }, off, new Set());
  assert.deepEqual(all.shapeIds, ["s1", "s2", "s3", "q1"]);
  assert.deepEqual(all.textIds, ["t1", "t2", "x1", "tb:0:0", "tb:0:1"]);
  // a cell is on its table's layer, and selected only as itself
  assert.deepEqual(matchingTextAndGraphics(b, { graphics: false, text: true, dimensions: false }, { layerOn: true, layer: "F.Fab", selectedOnly: false }, new Set()).textIds, ["tb:0:0", "tb:0:1"]);
  assert.deepEqual(matchingTextAndGraphics(b, { graphics: false, text: true, dimensions: false }, { ...off, selectedOnly: true }, new Set(["tb", "tb:0:1"])).textIds, ["tb:0:1"]);
});
