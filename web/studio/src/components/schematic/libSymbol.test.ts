import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, LibSymbols, SchematicSymbol } from "../../api/types";
import { resolveLibSymbol, withAlternate } from "./libSymbol";

const pin = (over: Partial<LibPin> = {}): LibPin => ({ number: "1", name: "PA0", electrical_type: "bidirectional", shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 0, hidden: false, alternates: [{ name: "USART1_TX", electrical_type: "output", shape: "clock" }], ...over });

test("a placed pin that took an alternate is drawn named, typed and shaped by it (`SCH_PIN::GetShownName` / `GetType` / `GetShape` with `m_alt`)", () => {
  assert.deepEqual(withAlternate(pin(), "USART1_TX"), { ...pin(), name: "USART1_TX", electrical_type: "output", shape: "clock" });
  // none, its own name, or one the library pin has not: the pin as it is (`SCH_PIN::SetAlt`)
  for (const alt of [null, "", "PA0", "NOPE"]) assert.deepEqual(withAlternate(pin(), alt), pin(), String(alt));
  const lib: LibSymbols = { "Test:Mcu": { graphics: [], pins: [pin()] } } as unknown as LibSymbols;
  const s = { id: "U1", lib_id: "Test:Mcu", at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, pins: [{ number: "1", name: "PA0", kind: "signal", alt: "USART1_TX" }] } as unknown as SchematicSymbol;
  const r = resolveLibSymbol(s, lib)!;
  assert.deepEqual([r.pins[0]!.pin.name, r.pins[0]!.pin.shape, r.pins[0]!.pin.alternates?.length], ["USART1_TX", "clock", 1], "the drawn pin keeps its alternates for the icon");
});
