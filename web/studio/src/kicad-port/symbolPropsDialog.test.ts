import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, LibSymbol, Schematic, SchematicSymbol, SchField } from "../api/types";
import { assignAlt, colorCell, colorOfCell, fieldLookCmds, fieldLookError, fontChoices, lookOf, pinAlternatesCmd, pinCell, pinRows, shapeOf, sortPinRows, typeOf, unitChangeError, unitChoices } from "./symbolPropsDialog";

const libPin = (number: string, name: string, unit: number, alternates?: LibPin["alternates"], type: LibPin["electrical_type"] = "bidirectional"): LibPin => ({ number, name, electrical_type: type, shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit, body_style: 0, hidden: false, ...(alternates ? { alternates } : {}) });
const mcu: LibSymbol = {
  graphics: [],
  pins: [libPin("10", "PB2", 2, [{ name: "SPI_MOSI", electrical_type: "output", shape: "clock" }]), libPin("2", "PA1", 1), libPin("1", "PA0", 1, [{ name: "USART1_TX", electrical_type: "output", shape: "line" }, { name: "ADC0", electrical_type: "input", shape: "inverted" }]), libPin("3", "VDD", 1, undefined, "power_in")],
  unit_count: 2,
};
const placed = (unit: number, alt: Record<string, string> = {}): SchematicSymbol =>
  ({ id: "U1", lib_id: "Test:Mcu", at: [0, 0], rot: 0, mirror: null, unit, body_style: 1, value: "MCU", mpn: null, package: null, footprint: null, datasheet: null, pins: mcu.pins.filter((p) => p.unit === unit).map((p) => ({ number: p.number, name: p.name, kind: "signal" as const, alt: alt[p.number] ?? null })) }) as SchematicSymbol;
const sheet = (symbols: SchematicSymbol[]): Schematic => ({ symbols, lib_symbols: { "Test:Mcu": mcu } }) as unknown as Schematic;

test("the Pin Functions tab lists every pin of every unit, by number, with the alternate each placed unit keeps", () => {
  const a = placed(1, { "1": "ADC0" });
  const sch = sheet([a, placed(2, { "10": "SPI_MOSI" })]);
  const rows = pinRows(sch, a);
  assert.deepEqual(rows.map((r) => r.number), ["1", "2", "3", "10"], "`PIN_NUMBERS::Compare`: 10 after 3");
  assert.deepEqual(rows.map((r) => pinCell(r, "alt")), ["ADC0", "", "", "SPI_MOSI"], "a pin with no alternates has an empty, read-only cell; another unit's pin keeps that unit's");
  assert.deepEqual([pinCell(rows[0]!, "type"), pinCell(rows[0]!, "shape"), pinCell(rows[3]!, "shape")], ["Input", "Inverted", "Clock"], "the alternate's type and style (`GetType` / `GetShape` with `m_alt`)");
  assert.deepEqual([typeOf(rows[2]!), shapeOf(rows[1]!)], ["power_in", "line"]);
  // `GetValue( COL_ALT_NAME )`: the base name when no alternate is assigned
  const plain = pinRows(sheet([placed(1)]), placed(1));
  assert.equal(pinCell(plain[0]!, "alt"), "PA0");
});

test("a column header sorts the pins (`compare`: the column's text, then the number), the base name assigned is no alternate, and OK sends what changed", () => {
  const sch = sheet([placed(1)]);
  const rows = pinRows(sch, placed(1));
  assert.deepEqual(sortPinRows(rows, "base", true).map((r) => r.base), ["PA0", "PA1", "PB2", "VDD"]);
  assert.deepEqual(sortPinRows(rows, "number", false).map((r) => r.number), ["10", "3", "2", "1"]);
  assert.deepEqual(sortPinRows(rows, "type", true).map((r) => r.number), ["1", "2", "10", "3"], "Bidirectional before Power input, the number breaking the tie");
  const edited = rows.map((r) => (r.number === "1" ? assignAlt(r, "USART1_TX") : r.number === "10" ? assignAlt(assignAlt(r, "SPI_MOSI"), "PB2") : r));
  assert.equal(edited.find((r) => r.number === "10")!.alt, "", "choosing the base name again clears it");
  assert.deepEqual(pinAlternatesCmd("U1", 1, rows, edited), { op: "sch_edit", verb: "set_pin_alternates", id: "U1", unit: 1, alternates: { "1": "USART1_TX" }, other_units: true });
  assert.equal(pinAlternatesCmd("U1", 1, rows, rows), null);
});

test("the Unit choice: every unit's display name, none for a symbol of one unit; a unit placed on the sheet is refused with the reason", () => {
  const sch = sheet([placed(1), placed(2)]);
  assert.deepEqual(unitChoices(sch, placed(1)), [{ unit: 1, label: "A" }, { unit: 2, label: "B" }]);
  assert.match(unitChangeError(sch, placed(1), 2) ?? "", /^Unit B of U1 is already placed on this sheet/);
  assert.equal(unitChangeError(sheet([placed(1)]), placed(1), 2), null);
  assert.equal(unitChangeError(sch, placed(1), 1), null);
  const single = { ...sch, lib_symbols: { "Test:Mcu": { ...mcu, unit_count: 1, pins: [libPin("1", "A", 1)] } } } as unknown as Schematic;
  assert.deepEqual(unitChoices(single, placed(1)), []);
});

test("the fields grid's look columns: what changed becomes edit_field and set_field_looks, the Font's first choices are KiCad's", () => {
  const f: SchField = { name: "Value", text: "10k", at: [0, 0], vertical: false, h: "left", v: "center", visible: true, id: "fld:U1:Value", size_nm: 1_270_000, bold: false, italic: false };
  const was = lookOf(f);
  assert.deepEqual(was, { nameShown: false, h: "left", v: "center", italic: false, bold: false, sizeMm: 1.27, font: "Default Font", color: "", allowAutoplace: true }, "no place in its own frame sent: no place cells");
  assert.deepEqual(fieldLookCmds(f.id!, was, was), []);
  assert.deepEqual(fieldLookCmds(f.id!, was, { ...was, h: "right", bold: true, sizeMm: 2 }), [{ op: "sch_edit", verb: "edit_field", id: "fld:U1:Value", h: "right", bold: true, size_nm: 2_000_000 }]);
  assert.deepEqual(fieldLookCmds(f.id!, was, { ...was, font: "DejaVu Sans", color: "#ff0000" }), [{ op: "sch_edit", verb: "set_field_looks", ids: ["fld:U1:Value"], font: "DejaVu Sans", color: { r: 255, g: 0, b: 0, a: 255 } }]);
  assert.deepEqual(fieldLookCmds(f.id!, { ...was, color: "#ff0000" }, was), [{ op: "sch_edit", verb: "set_field_looks", ids: ["fld:U1:Value"], color: { r: 0, g: 0, b: 0, a: 0 } }], "no colour: the layer's");
  assert.equal(colorCell({ r: 16, g: 32, b: 255, a: 255 }), "#1020ff");
  assert.equal(colorCell({ r: 0, g: 0, b: 0, a: 0 }), "");
  assert.deepEqual(colorOfCell("nope"), { r: 0, g: 0, b: 0, a: 0 });
  assert.deepEqual(fontChoices(["Zapf", "DejaVu Sans", "KiCad Font"]), ["Default Font", "KiCad Font", "DejaVu Sans", "Zapf"]);
  assert.match(fieldLookError("Value", { ...was, sizeMm: 0 }) ?? "", /between 0.01 and 1000 mm/);
  assert.equal(fieldLookError("Value", was), null);
});
