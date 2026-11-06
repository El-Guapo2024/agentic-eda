import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, LibSymbol, Schematic, SchematicSymbol } from "../api/types";
import { parseSelectUnitArg, PIN_FUNCTION_ACTION, PLACE_UNIT_ACTION, pinFunctionMenu, pinFunctionsOf, SELECT_UNIT_ACTION, selectUnit, setPinFunctionCmd, symbolUnitMenu } from "./schSymbolMenus";

const libPin = (number: string, name: string, unit: number, alternates?: LibPin["alternates"]): LibPin => ({ number, name, electrical_type: "bidirectional", shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit, body_style: 0, hidden: false, ...(alternates ? { alternates } : {}) });
const quad: LibSymbol = {
  graphics: [],
  pins: [
    libPin("1", "PA0", 1, [
      { name: "TIM2_CH1", electrical_type: "input", shape: "inverted" },
      { name: "USART1_TX", electrical_type: "output", shape: "line" },
    ]),
    libPin("2", "PA1", 1),
    libPin("3", "B", 2),
    libPin("4", "C", 3),
  ],
  unit_count: 3,
  unit_names: { "3": "Power" },
};
const placed = (unit: number, alt?: Record<string, string>): SchematicSymbol =>
  ({ id: "U1", lib_id: "Test:Quad", at: [0, 0], rot: 0, mirror: null, unit, body_style: 1, value: "Q", mpn: null, package: null, footprint: null, datasheet: null, pins: quad.pins.filter((p) => p.unit === unit).map((p) => ({ number: p.number, name: p.name, kind: "signal" as const, alt: alt?.[p.number] ?? null })) }) as SchematicSymbol;
const sheet = (symbols: SchematicSymbol[]): Schematic => ({ symbols, lib_symbols: { "Test:Quad": quad }, sheet_path: [] }) as unknown as Schematic;

test("Symbol Unit lists every unit by its display name, says which are placed, checks the symbol's own, and offers to place the others", () => {
  const menu = symbolUnitMenu(sheet([placed(1), placed(2)]), "U1", 2);
  assert.ok(menu && menu.type === "submenu" && menu.label === "Symbol Unit");
  if (menu?.type !== "submenu") return;
  assert.deepEqual(
    menu.items.map((n) => (n.type === "item" ? [n.action, n.arg, n.label, n.checked ?? null] : n.type)),
    [
      [SELECT_UNIT_ACTION, "2:1", "A (already placed)", false],
      [SELECT_UNIT_ACTION, "2:2", "B (already placed)", true],
      [SELECT_UNIT_ACTION, "2:3", "Power", false],
      "separator",
      [PLACE_UNIT_ACTION, 3, "Place unit Power", null],
    ]
  );
  assert.deepEqual(parseSelectUnitArg("2:3"), { unit: 2, newUnit: 3 });
  assert.equal(parseSelectUnitArg(3), null);
  // `SingleMultiUnitSymbol`: a symbol of one unit has no such menu
  const single = { ...sheet([placed(1)]), lib_symbols: { "Test:Quad": { ...quad, unit_count: 1, pins: [libPin("1", "A", 1)] } } } as unknown as Schematic;
  assert.equal(symbolUnitMenu(single, "U1"), null);
});

test("SelectUnit: a free unit changes at once, one placed on the sheet asks Swap or Duplicate first, the own unit does nothing", () => {
  const sch = sheet([placed(1), placed(2)]);
  assert.deepEqual(selectUnit(sch, "U1", 1, 3, ""), { do: "cmd", cmd: { op: "sch_edit", verb: "set_symbol_unit", id: "U1", unit: 1, new_unit: 3 } });
  assert.deepEqual(selectUnit(sch, "U1", 1, 2, "/MCU"), { do: "ask", message: "Symbol unit 'B' is already placed (on sheet '/MCU')", swapLabel: "Swap 'B' and 'A'", duplicateLabel: "Duplicate 'B'", newUnit: 2 });
  assert.equal((selectUnit(sch, "U1", 2, 1, "") as { message: string }).message, "Symbol unit 'A' is already placed (on sheet 'Root')");
  assert.deepEqual(selectUnit(sch, "U1", 2, 2, ""), { do: "nothing" });
  // `if( aUnit > unitCount ) aUnit = unitCount`
  assert.deepEqual(selectUnit(sch, "U1", 1, 9, ""), { do: "cmd", cmd: { op: "sch_edit", verb: "set_symbol_unit", id: "U1", unit: 1, new_unit: 3 } });
});

test("Pin Function: the pin's own name and its alternates, the one it took checked; choosing sets one pin of one unit, and the checked one again goes back", () => {
  const sch = sheet([placed(1, { "1": "USART1_TX" })]);
  const f = pinFunctionsOf(sch, "pin:U1:1");
  assert.deepEqual(f, { ref: "U1", unit: 1, number: "1", base: "PA0", alternates: ["TIM2_CH1", "USART1_TX"], alt: "USART1_TX" });
  const menu = pinFunctionMenu(f!);
  assert.equal(menu.type === "submenu" && menu.label, "Pin Function");
  if (menu.type !== "submenu") return;
  assert.deepEqual(menu.items.map((n) => (n.type === "item" ? [n.action, n.label, n.checked] : n.type)), [
    [PIN_FUNCTION_ACTION, "PA0", false],
    [PIN_FUNCTION_ACTION, "TIM2_CH1", false],
    [PIN_FUNCTION_ACTION, "USART1_TX", true],
  ]);
  assert.deepEqual(setPinFunctionCmd(f!, "TIM2_CH1"), { op: "sch_edit", verb: "set_pin_alternates", id: "U1", unit: 1, alternates: { "1": "TIM2_CH1" } });
  // the base name, and the checked alternate again (`aFunction == aPin->GetName()`): no alternate
  assert.deepEqual(setPinFunctionCmd(f!, "PA0"), { op: "sch_edit", verb: "set_pin_alternates", id: "U1", unit: 1, alternates: { "1": "" } });
  assert.deepEqual(setPinFunctionCmd(f!, "USART1_TX"), { op: "sch_edit", verb: "set_pin_alternates", id: "U1", unit: 1, alternates: { "1": "" } });
  const plain = pinFunctionsOf(sheet([placed(1)]), "pin:U1:1")!;
  assert.equal(setPinFunctionCmd(plain, "PA0"), null, "nothing changes");
  // `SingleMultiFunctionPin`: a pin with no alternates has no menu
  assert.equal(pinFunctionsOf(sch, "pin:U1:2"), null);
  assert.equal(pinFunctionsOf(sheet([placed(2)]), "pin:U1#2:3"), null);
});
