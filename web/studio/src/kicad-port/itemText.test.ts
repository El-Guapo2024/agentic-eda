import { test } from "node:test";
import assert from "node:assert/strict";
import type { Schematic } from "../api/types";
import { selectionAsText, datasheetTarget, schSelectionTexts } from "./itemText";

test("selectionAsText trims, drops empties, joins with newline", () => {
  assert.equal(selectionAsText(["  hello ", "", null, undefined, "world\n"]), "hello\nworld");
  assert.equal(selectionAsText([]), "");
});

test("datasheetTarget: empty and '~' mean none; http(s)/file launch; www gets https; else unresolvable", () => {
  assert.deepEqual(datasheetTarget(""), { kind: "none" });
  assert.deepEqual(datasheetTarget(" ~ "), { kind: "none" });
  assert.deepEqual(datasheetTarget(null), { kind: "none" });
  assert.deepEqual(datasheetTarget("https://a.b/c.pdf"), { kind: "url", url: "https://a.b/c.pdf" });
  assert.deepEqual(datasheetTarget("www.ti.com/x"), { kind: "url", url: "https://www.ti.com/x" });
  assert.deepEqual(datasheetTarget("ds/part.pdf"), { kind: "unresolvable", text: "ds/part.pdf" });
});

// A sheet with one of each kind that has a text: a label, a free text, a sheet with a pin, a symbol with a field shown with its name and a pin, and a text box.
function sheet(): Schematic {
  return {
    lib_symbols: { "Dev:R": { graphics: [], pins: [{ number: "1", name: "A", electrical_type: "passive", shape: "line", at: [0, 3.81], angle_deg: 270, length_mm: 2.54, unit: 1, body_style: 1, hidden: false }, { number: "2", name: "~", electrical_type: "passive", shape: "line", at: [0, -3.81], angle_deg: 90, length_mm: 2.54, unit: 1, body_style: 1, hidden: false }], pin_name_offset: 0 } },
    symbols: [
      {
        id: "R1",
        lib_id: "Dev:R",
        at: [50_000, 50_000],
        rot: 0,
        mirror: null,
        unit: 1,
        body_style: 1,
        value: "10k",
        mpn: null,
        package: null,
        footprint: null,
        datasheet: null,
        pins: [],
        fields: [{ id: "fld:R1:Value", name: "Value", text: "10k", at: [0, 0], vertical: false, h: "left", v: "center", visible: true, name_shown: true }],
      },
    ],
    power_symbols: [],
    wires: [],
    no_connects: [],
    labels: [{ id: "lbl_a", net: "  CLK  ", at: [0, 0], scope: "local", shape: null }],
    texts: [{ id: "txt_a", content: "note\nover two lines", at: [0, 0], angle: 0, size_um: 1270 }],
    title_block: null,
    bus_entries: [],
    sheets: [{ id: "sheet_a", name: "Sub", file: "sub.kicad_sch", at: [100_000, 100_000], size: [20_000, 10_000], pins: [{ id: "shpin_a", name: "IN", shape: "input", at: [100_000, 105_000] }] }],
    graphics: [{ id: "tb_a", shape: { type: "text_box", start: [0, 0], end: [10, 10], text: " boxed ", size_um: 1270 } }, { id: "shp_a", shape: { type: "rectangle", start: [0, 0], end: [1, 1] } }],
    sheet_path: [],
  } as unknown as Schematic;
}

test("Copy as Text on the schematic: a label, a text, a sheet pin, a shown field, a text box and a pin each give their text, in selection order", () => {
  const s = sheet();
  assert.deepEqual(schSelectionTexts(s, ["lbl_a"]), ["  CLK  "]);
  assert.deepEqual(schSelectionTexts(s, ["txt_a"]), ["note\nover two lines"]);
  assert.deepEqual(schSelectionTexts(s, ["shpin_a"]), ["IN"]);
  assert.deepEqual(schSelectionTexts(s, ["fld:R1:Value"]), ["Value: 10k"], "a field whose name is shown gives the name too");
  assert.deepEqual(schSelectionTexts(s, ["tb_a"]), [" boxed "]);
  assert.deepEqual(schSelectionTexts(s, ["pin:R1:1"]), ["A"]);
  // a pin named "~" has no name to give, and what has no text gives nothing
  assert.deepEqual(schSelectionTexts(s, ["pin:R1:2"]), [""]);
  assert.deepEqual(schSelectionTexts(s, ["R1", "shp_a", "sheet_a", "gone"]), []);
  // what goes on the clipboard: each text trimmed, the empty ones dropped, one per line in the order selected
  assert.equal(selectionAsText(schSelectionTexts(s, ["txt_a", "lbl_a", "pin:R1:2", "R1", "shpin_a"])), "note\nover two lines\nCLK\nIN");
});
