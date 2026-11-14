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

test("a library text box's text is drawn on the placed symbol where its turned and mirrored copy has it, with the symbol's variables (`SCH_PAINTER::draw( SCH_SYMBOL )`)", () => {
  const box = { kind: "text_box", index: 0, text: "${REFERENCE}\nIN", start: [-4.445, 4.445], end: [0.635, 1.905], angle_deg: 0, size_mm: 1.27, bold: true, h_align: "left", v_align: "top", margin_mm: 0.508, unit: 0, body_style: 0 };
  const border = { kind: "rectangle", start: [-4.445, 4.445], end: [0.635, 1.905], stroke_width: 0.254, fill: "none", unit: 0, body_style: 0 };
  const lib = { "Test:TB": { graphics: [border, box], pins: [] } } as unknown as LibSymbols;
  const placed = (rot: number, mirror: "x" | "y" | null, shown?: Record<string, string>) => {
    const s = { id: "U1", lib_id: "Test:TB", at: [10_000_000, 20_000_000], rot, mirror, unit: 1, body_style: 1, pins: [], ...(shown ? { text_box_shown: shown } : {}) } as unknown as SchematicSymbol;
    const g = resolveLibSymbol(s, lib)!.graphics;
    assert.deepEqual(g.map((x) => x.kind), ["rectangle", "text_box"], "the border is the rectangle, the text the text box");
    const t = g[1]!;
    if (t.kind !== "text_box") throw new Error("a text box");
    return t.shape;
  };
  const upright = placed(0, null, { "0": "U1\nIN" });
  assert.deepEqual([upright.start, upright.end, upright.text, upright.angle, upright.h_align, upright.v_align, upright.margin_nm, upright.size_nm, upright.bold], [{ x: 5_555_000, y: 15_555_000 }, { x: 10_635_000, y: 18_095_000 }, "U1\nIN", 0, "left", "top", 508_000, 1_270_000, true]);
  assert.equal(placed(0, null).text, "${REFERENCE}\nIN", "no shown text: the stored one");
  // mirrored left to right: the box on the other side, its text right justified; turned a quarter: the box turned, its text running up it
  const mirrored = placed(0, "y");
  assert.deepEqual([mirrored.start.x, mirrored.end.x, mirrored.angle, mirrored.h_align], [14_445_000, 9_365_000, 0, "right"]);
  const turned = placed(90, null);
  assert.deepEqual([turned.start, turned.angle, turned.h_align], [{ x: 5_555_000, y: 24_445_000 }, 90_000, "left"]);
});

test("a library text is drawn on the placed symbol turned, justified, in its face and colour, with the symbol's variables (`SCH_PAINTER::draw( SCH_SYMBOL )`, UI-LEFT-3 S3-5)", () => {
  const text = { kind: "text", content: "${REFERENCE}", at: [1.27, 2.54], angle_deg: 0, size_mm: 1.27, unit: 0, body_style: 0, h_align: "left", v_align: "top", bold: true, font: "DejaVu Sans", text_color: { r: 10, g: 20, b: 30, a: 1 }, thickness_mm: 0.3, hidden: true };
  const lib = { "Test:T": { graphics: [text], pins: [] } } as unknown as LibSymbols;
  const at = (rot: number, mirror: "x" | "y" | null, shown?: Record<string, string>) => {
    const s = { id: "U7", lib_id: "Test:T", at: [10_000_000, 20_000_000], rot, mirror, unit: 1, body_style: 1, pins: [], ...(shown ? { text_shown: shown } : {}) } as unknown as SchematicSymbol;
    const g = resolveLibSymbol(s, lib)!.graphics[0]!;
    if (g.kind !== "text") throw new Error("a text");
    return g;
  };
  const upright = at(0, null, { "${REFERENCE}": "U7" });
  assert.deepEqual([upright.content, upright.at, upright.angleDeg, upright.h, upright.v], ["U7", [11_270_000, 17_460_000], 0, "left", "top"], "the shown text at its anchor, Y down");
  assert.deepEqual([upright.bold, upright.font, upright.color, upright.thicknessNm, upright.hidden, upright.recordId], [true, "DejaVu Sans", { r: 10, g: 20, b: 30, a: 1 }, 300_000, true, "U7/text/0"]);
  const turned = at(90, null);
  assert.deepEqual([turned.content, turned.at, turned.angleDeg, turned.h], ["${REFERENCE}", [7_460_000, 18_730_000], 90, "left"], "a quarter turn: up the sheet, its anchor turned with the body");
  const mirrored = at(0, "y");
  assert.deepEqual([mirrored.at, mirrored.angleDeg, mirrored.h, mirrored.v], [[8_730_000, 17_460_000], 0, "right", "top"], "mirrored left to right: the anchor mirrored, the justification flipped");
});
