import { test } from "node:test";
import assert from "node:assert/strict";
import {
  addLayerNameRow,
  addTextItemRow,
  changeLayerNameRow,
  DEFAULT_FP_TEXT_ITEMS,
  deleteTextItemRows,
  enabledUserLayers,
  fieldDefaultsItems,
  isFactoryNewFootprint,
  layerNamesOf,
  loadFieldDefaults,
  loadLayerNames,
  newFootprintTexts,
  PLAIN_TEXT_LOOK,
  nextAvailableLayer,
  resetFieldDefaults,
  sanitizeFpTextItems,
  sanitizeUserLayerNames,
} from "./fpFieldDefaults";
import { FACTORY_TEXT_GRAPHICS } from "./layerDefaults";

test("Footprint Defaults: the two field rows and the text items of KiCad's default list (REF** on F.SilkS, an empty Value and ${REFERENCE} on F.Fab)", () => {
  const page = loadFieldDefaults(DEFAULT_FP_TEXT_ITEMS);
  assert.deepEqual(page.fields, [
    { text: "REF**", visible: true, layer: "F.SilkS" },
    { text: "", visible: true, layer: "F.Fab" },
  ]);
  assert.deepEqual(page.texts, [{ text: "${REFERENCE}", layer: "F.Fab" }]);
  assert.deepEqual(fieldDefaultsItems(page), DEFAULT_FP_TEXT_ITEMS);
  assert.deepEqual(resetFieldDefaults(), page);
});

test("loadFPSettings with fewer than two items: the missing field row is AppendRows' blank one on F.SilkS", () => {
  const page = loadFieldDefaults([{ text: "U**", visible: false, layer: "F.Fab" }]);
  assert.deepEqual(page.fields[1], { text: "", visible: true, layer: "F.SilkS" });
  assert.deepEqual(page.texts, []);
});

test("Add takes the previous row's layer (F.SilkS for the first); Delete drops the selected rows; the text items come back shown", () => {
  let page = loadFieldDefaults([DEFAULT_FP_TEXT_ITEMS[0]!, DEFAULT_FP_TEXT_ITEMS[1]!]);
  page = addTextItemRow(page);
  assert.deepEqual(page.texts, [{ text: "", layer: "F.SilkS" }]);
  page = { ...page, texts: [{ text: "rev", layer: "B.Fab" }] };
  page = addTextItemRow(page);
  assert.deepEqual(page.texts[1], { text: "", layer: "B.Fab" });
  page = deleteTextItemRows(page, new Set([0]));
  assert.deepEqual(page.texts, [{ text: "", layer: "B.Fab" }]);
  assert.equal(fieldDefaultsItems(page)[2]!.visible, true);
});

test("the stored list is read as the param's reader does: a layer that is no layer is F.SilkS; anything but a list is KiCad's defaults", () => {
  assert.equal(sanitizeFpTextItems(null), null);
  assert.deepEqual(sanitizeFpTextItems([{ text: "X", visible: false, layer: "Nope" }]), [{ text: "X", visible: false, layer: "F.SilkS" }]);
});

test("CreateNewFootprint: the Reference half a text height above the origin, each next text a whole height on, the layer class's size; an empty field is the name", () => {
  const t = newFootprintTexts(DEFAULT_FP_TEXT_ITEMS, null, "Untitled");
  assert.deepEqual([t.reference.text, t.reference.layout.at.y, t.reference.layout.layer, t.reference.layout.size, t.reference.layout.thickness], ["REF**", -500_000, "F.SilkS", [1_000_000, 1_000_000], 100_000]);
  assert.deepEqual([t.value.text, t.value.layout.at.y, t.value.layout.layer, t.value.layout.thickness], ["Untitled", 1_000_000, "F.Fab", 150_000]);
  assert.deepEqual(t.texts.map((x) => [x.content, x.at.y, x.layer]), [["${REFERENCE}", 2_500_000, "F.Fab"]], "crates/model fp_edit.rs new_footprint_texts: (0, 2.5 mm)");
  assert.ok(isFactoryNewFootprint(DEFAULT_FP_TEXT_ITEMS, null, "Untitled"));
  assert.ok(isFactoryNewFootprint(DEFAULT_FP_TEXT_ITEMS, FACTORY_TEXT_GRAPHICS, "Untitled"));
  // a copper Reference (1.5 mm text) and two texts: the steps follow each layer's height
  const items = [{ text: "U**", visible: false, layer: "F.Cu" }, { text: "", visible: true, layer: "F.Fab" }, { text: "a", visible: true, layer: "F.SilkS" }, { text: "b", visible: true, layer: "User.1" }];
  const c = newFootprintTexts(items, null, "X");
  assert.deepEqual([c.reference.text, c.reference.layout.at.y, c.reference.layout.visible], ["U**", -750_000, false]);
  assert.equal(c.value.layout.at.y, 750_000 + 500_000);
  assert.deepEqual(c.texts.map((x) => x.at.y), [2_750_000, 4_250_000]);
  assert.ok(!isFactoryNewFootprint(items, null, "X"));
  // the Graphics Defaults' sizes
  const big = { ...FACTORY_TEXT_GRAPHICS, silk: { ...FACTORY_TEXT_GRAPHICS.silk, text_height_nm: 2_000_000, text_width_nm: 2_000_000 } };
  assert.equal(newFootprintTexts(DEFAULT_FP_TEXT_ITEMS, big, "X").reference.layout.at.y, -1_000_000);
  assert.ok(!isFactoryNewFootprint(DEFAULT_FP_TEXT_ITEMS, big, "X"));
});

test("UI-LEFT-3 F3-6: CreateNewFootprint's other texts take their layer class's width, italic and keep-upright (`RunOnChildren`: SetTextSize, SetItalic, SetKeepUpright)", () => {
  const plain = newFootprintTexts(DEFAULT_FP_TEXT_ITEMS, null, "X");
  assert.deepEqual(plain.looks, [PLAIN_TEXT_LOOK], "KiCad's defaults: the text the backend makes as it is");
  const fab = { ...FACTORY_TEXT_GRAPHICS.fab, italic: true, upright: false, text_width_nm: 800_000 };
  const t = newFootprintTexts(DEFAULT_FP_TEXT_ITEMS, { ...FACTORY_TEXT_GRAPHICS, fab }, "X");
  assert.deepEqual([t.looks[0]!.italic, t.looks[0]!.keep_upright, t.looks[0]!.width_nm], [true, false, 800_000]);
  assert.deepEqual([t.value.layout.italic, t.value.layout.keep_upright], [true, false], "the fields as before");
  assert.ok(!isFactoryNewFootprint(DEFAULT_FP_TEXT_ITEMS, { ...FACTORY_TEXT_GRAPHICS, fab }, "X"));
});

test("User Layer Names: rows in the map's order, Add on the first free User.n, a layer taken twice moved to the next free one, a name taken twice an error", () => {
  const rows = loadLayerNames({ "User.10": "Ten", "User.2": "Two", "Dwgs.User": "Notes", "F.Cu": "no" });
  assert.deepEqual(rows.map((r) => r.layer), ["Dwgs.User", "User.10", "User.2"], "std::map's byte order; a copper layer is not a user layer");
  const added = addLayerNameRow(rows);
  assert.deepEqual(added[3], { layer: "User.1", name: "" });
  assert.equal(nextAvailableLayer(added), "User.3");
  // `getNextAvailableLayer` over the rows as they are now (the edited row holds the taken layer): User.1, which the row had, is free again
  const moved = changeLayerNameRow(added, 3, { layer: "User.2" });
  assert.equal(moved.rows[3]!.layer, "User.1");
  const named = changeLayerNameRow(moved.rows, 3, { name: "Two" });
  assert.equal(named.error, "Layer name Two already in use.");
  assert.deepEqual(layerNamesOf([...named.rows, { layer: "User.4", name: "" }]), { "Dwgs.User": "Notes", "User.10": "Ten", "User.2": "Two", "User.1": "Two" }, "an empty name is dropped; the error does not stop OK");
  assert.deepEqual(sanitizeUserLayerNames({ "User.5": "Five", "F.SilkS": "x", "User.6": "" }), { "User.5": "Five" });
});

test("the Footprint Editor enables the first user layers of the count and every named one", () => {
  assert.deepEqual(enabledUserLayers(4, {}), [1, 2, 3, 4]);
  assert.deepEqual(enabledUserLayers(0, { "User.9": "Nine", "Dwgs.User": "x" }), [9]);
  assert.deepEqual(enabledUserLayers(2, { "User.2": "Two", "User.7": "Seven" }), [1, 2, 7]);
});
