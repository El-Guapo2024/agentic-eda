import { test } from "node:test";
import assert from "node:assert/strict";
import { alternatesByName, alternatesError, alternatesPaneOpen, newAlternate, ALTERNATE_COLUMNS, ALTERNATE_NAME_REQUIRED, PIN_SHAPE_OPTIONS, PIN_TYPE_OPTIONS } from "./symPinAlternates";

test("the grid's columns and the new row are the dialog's", () => {
  assert.deepEqual(ALTERNATE_COLUMNS, ["Alternate Pin Name", "Electrical Type", "Graphic Style"]);
  // the row a press of the add button makes: no name, the pin's type and style
  assert.deepEqual(newAlternate({ electrical_type: "tri_state", shape: "inverted_clock" }), { name: "", electrical_type: "tri_state", shape: "inverted_clock" });
});

test("OK refuses a row without a name, with the dialog's words", () => {
  assert.equal(alternatesError([]), null);
  assert.equal(alternatesError([{ name: "SPI_CLK", electrical_type: "output", shape: "line" }]), null);
  assert.equal(alternatesError([{ name: "SPI_CLK", electrical_type: "output", shape: "line" }, newAlternate({ electrical_type: "input", shape: "line" })]), ALTERNATE_NAME_REQUIRED);
  assert.equal(ALTERNATE_NAME_REQUIRED, "Alternate pin definitions must have a name.");
});

test("a name is kept once, the last definition standing, in the order of the names", () => {
  const rows = [
    { name: "UART_TX", electrical_type: "output", shape: "line" },
    { name: "ADC1", electrical_type: "input", shape: "line" },
    { name: "UART_TX", electrical_type: "bidirectional", shape: "inverted" },
  ] as const;
  assert.deepEqual(alternatesByName(rows), [
    { name: "ADC1", electrical_type: "input", shape: "line" },
    { name: "UART_TX", electrical_type: "bidirectional", shape: "inverted" },
  ]);
});

test("the pane starts open when the pin has alternates or the dialog was left open the last time", () => {
  assert.equal(alternatesPaneOpen(0, false), false);
  assert.equal(alternatesPaneOpen(2, false), true);
  assert.equal(alternatesPaneOpen(0, true), true);
});

test("the type and style choices are the ones a file can say, in the dialog's order", () => {
  assert.deepEqual(
    PIN_TYPE_OPTIONS.map((o) => o.value),
    ["input", "output", "bidirectional", "tri_state", "passive", "free", "unspecified", "power_in", "power_out", "open_collector", "open_emitter", "no_connect"]
  );
  assert.deepEqual(
    PIN_SHAPE_OPTIONS.map((o) => o.value),
    ["line", "inverted", "clock", "inverted_clock", "input_low", "clock_low", "output_low", "edge_clock_high", "non_logic"]
  );
});
