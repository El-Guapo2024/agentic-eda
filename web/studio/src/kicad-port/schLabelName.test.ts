import { test } from "node:test";
import assert from "node:assert/strict";
import { findWire, labelNameFromWire, placeLabelCmd } from "./schLabelName";

const wire = (pts: [number, number][], extra: { bus?: boolean; label_driver?: string } = {}) => ({ pts, bus: extra.bus ?? false, ...(extra.label_driver !== undefined ? { label_driver: extra.label_driver } : {}) });

test("r2 C-L5: a local or global label put down on a wire a label names takes that name (createNewLabel / findWireLabelDriverName)", () => {
  const wires = [wire([[0, 0], [10_000_000, 0]], { label_driver: "SDA" }), wire([[0, 5_000_000], [10_000_000, 5_000_000]])];
  assert.equal(labelNameFromWire(wires, [2_540_000, 0], "local"), "SDA");
  assert.equal(labelNameFromWire(wires, [2_540_000, 0], "global"), "SDA");
  assert.equal(labelNameFromWire(wires, [2_540_000, 0], "hierarchical"), null, "a hierarchical label always asks");
  assert.equal(labelNameFromWire(wires, [2_540_000, 5_000_000], "local"), null, "a wire no label drives: the dialog asks");
  assert.equal(labelNameFromWire(wires, [2_540_000, 2_540_000], "local"), null, "off every wire");
  assert.equal(findWire([wire([[0, 0], [10, 0]], { bus: true })], [5, 0]), null, "a bus is not a wire");
});

test("the label's place_label command carries the dialog's choices", () => {
  assert.deepEqual(placeLabelCmd({ at: [1, 2], scope: "global", text: "SDA", shape: "input", spin: "", plainSpin: "left", sizeNm: 1_270_000, bold: true, italic: false, autoRotate: true }), {
    op: "sch_edit",
    verb: "place_label",
    net: "SDA",
    at: { x: 1, y: 2 },
    kind: { scope: "global", shape: "input" },
    spin: "left",
    bold: true,
    auto_rotate: true,
  });
});
