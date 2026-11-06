import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_SCH_LOOK, labelBoxExpansionNm, pinDecoSizeNm, pinTextClearanceNm, schFormattingLook, schLookOf, setSchFormattingLook, textOffsetNm, withDefaultSchLook } from "./schFormattingLook";
import { globalLabelOutline, globalLabelTextPlacement, hierLabelTextPlacement, localLabelTextPlacement } from "../components/schematic/labelShape";
import { pinTextPlacements, PIN_TEXT_CLEARANCE_NM } from "../components/schematic/pinText";
import { sheetPinBodyBox } from "./schSheetPinItems";
import fixture from "./schFormatting.kicadcli.json";

const H = 1_270_000;

test("KiCad's defaults: an older backend's setup, or none, lays text out as eeschema/default_values.h says", () => {
  assert.deepEqual(schLookOf(undefined), DEFAULT_SCH_LOOK);
  assert.deepEqual(schLookOf({ text_offset_ratio: 0.3 }), { ...DEFAULT_SCH_LOOK, textOffsetRatio: 0.3 });
  // `GetTextOffset` and `GetLabelBoxExpansion` at 50 mil: 0.15 and 0.375 of the size; the pin text's gap 4 + 4 mils
  assert.deepEqual([textOffsetNm(H, DEFAULT_SCH_LOOK), labelBoxExpansionNm(H, DEFAULT_SCH_LOOK), pinTextClearanceNm(DEFAULT_SCH_LOOK)], [190_500, 476_250, PIN_TEXT_CLEARANCE_NM]);
});

test("Schematic Setup's Label offset and Global label margin ratios move a label's text and a global label's flag as KiCad's GetTextOffset and GetLabelBoxExpansion do", () => {
  setSchFormattingLook({ text_offset_ratio: 0.5, label_size_ratio: 0.6 });
  try {
    // a local label: `dist = GetTextOffset() + GetPenWidth()` above its wire
    assert.deepEqual(localLabelTextPlacement("right", [0, 0], H).pos, [0, -(635_000 + 158_800)]);
    // a hierarchical label: `GetTextOffset() + GetTextWidth()` from its anchor
    const hier = hierLabelTextPlacement("A", "right", [0, 0], H);
    const hierDefault = withDefaultSchLook(() => hierLabelTextPlacement("A", "right", [0, 0], H));
    assert.equal(hier.pos[0] - hierDefault.pos[0], 635_000 - 190_500);
    // a global input label: `horiz = GetLabelBoxExpansion() + 3/4 of the size` (the flag's point), and its flag the expansion longer at each end
    assert.equal(globalLabelTextPlacement("input", "right", [0, 0], H).pos[0], 762_000 + 952_500);
    const len = (pts: [number, number][]) => Math.max(...pts.map((p) => p[0])) - Math.min(...pts.map((p) => p[0]));
    assert.equal(len(globalLabelOutline("A", "passive", "right", [0, 0], H)) - withDefaultSchLook(() => len(globalLabelOutline("A", "passive", "right", [0, 0], H))), 2 * (762_000 - 476_250));
    // a pin's texts: `MilsToIU( KiROUND( 24 * 0.5 ) )` + 4 mils off its line
    assert.equal(pinTextClearanceNm(), 16 * 25_400);
    const p = pinTextPlacements({ name: "IN", number: "3" }, [0, 0], [2_540_000, 0], { namesHidden: false, numbersHidden: false, nameOffsetNm: 0 });
    assert.equal(p.name!.at[1], -(16 * 25_400 + H / 2 + 152_400));
    // a sheet pin's box: its margin is the text offset
    assert.equal(sheetPinBodyBox([0, 0], "left", 0, H).maxX - withDefaultSchLook(() => sheetPinBodyBox([0, 0], "left", 0, H).maxX), 635_000 - 190_500);
  } finally {
    setSchFormattingLook(undefined);
  }
  assert.deepEqual(schFormattingLook(), DEFAULT_SCH_LOOK);
});

test("Pin symbol size is the decorations' radius; at 0 it is half the pin's text size (`externalPinDecoSize`)", () => {
  assert.equal(pinDecoSizeNm(H, schLookOf({ pin_symbol_size_nm: 1_016_000 })), 1_016_000);
  assert.equal(pinDecoSizeNm(H, schLookOf({ pin_symbol_size_nm: 0 })), 635_000);
  assert.equal(pinDecoSizeNm(2_000_000, schLookOf({ pin_symbol_size_nm: 0 })), 1_000_000);
});

test("the label offsets and the global label's flag land where kicad-cli plots them at the same ratios (schFormatting.kicadcli.json)", () => {
  const nm = (mm: number) => Math.round(mm * 1e6);
  const near = (a: number, b: number) => Math.abs(a - b) <= 100; // one schematic unit
  setSchFormattingLook({ text_offset_ratio: fixture.text_offset_ratio, label_size_ratio: fixture.label_size_ratio });
  try {
    const at = (p: number[]): [number, number] => [nm(p[0]!), nm(p[1]!)];
    const local = localLabelTextPlacement("right", at(fixture.local.at), H).pos;
    assert.ok(near(local[0], nm(fixture.local.text_at[0]!)) && near(local[1], nm(fixture.local.text_at[1]!)), `local text at ${local}`);
    // the text of a centred global label: kicad-cli writes its x as the anchor
    assert.ok(near(globalLabelTextPlacement("passive", "right", at(fixture.global.at), H).pos[0], nm(fixture.global.text_x)));
    const outline = globalLabelOutline(fixture.global.text, "passive", "right", at(fixture.global.at), H);
    fixture.global.outline.forEach((p, i) => assert.ok(near(outline[i]![0], nm(p[0]!)) && near(outline[i]![1], nm(p[1]!)), `outline point ${i}: ${outline[i]} vs ${p}`));
  } finally {
    setSchFormattingLook(undefined);
  }
});
