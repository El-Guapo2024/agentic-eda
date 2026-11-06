import { test } from "node:test";
import assert from "node:assert/strict";
import { linePositions, layoutKicadText, STROKE, type KicadFont, type OutlineFace } from "./kicadText";
import { measureStrokeText } from "../components/text/strokeFont";
import { schTextLines } from "./schTextItem";
import { textOrigin } from "./schText";
import { deviceTextDrawSpec, labelTextBoxWidth, labelTextOffset, schDrawPen, schFieldDrawSpec, schLabelDrawSpec, schOwnPen, schTextBoxDrawPos, schTextBoxDrawSpec, schTextDrawSpec } from "./schTextSpecs";

const MIL50 = 1_270_000;
const MM = 1_000_000;

/** Within `tol` nm. */
const near = (a: number, b: number, tol: number, what: string) => assert.ok(Math.abs(a - b) <= tol, `${what}: ${a} vs ${b}`);

test("a free text: the shared layout puts each line where schTextLines' port of getLinePositions does (eeschema's 100 nm rounding aside)", () => {
  const cases = [
    { content: "Hello", h: "left", v: "bottom", vertical: false },
    { content: "AB\nCDE", h: "center", v: "center", vertical: false },
    { content: "x_{1}", h: "right", v: "top", vertical: true },
  ] as const;
  for (const c of cases) {
    const t = { content: c.content, at: [10 * MM, 20 * MM] as [number, number], angle: c.vertical ? 90 : 0, size_nm: MIL50, h: c.h, v: c.v, vertical: c.vertical };
    // the same pen both ways (schTextLines takes GetPenSizeForNormal)
    const spec = { ...schTextDrawSpec(t), pen: Math.round(MIL50 / 8) };
    const ours = linePositions(spec, STROKE);
    const theirs = schTextLines(t, measureStrokeText);
    assert.equal(ours.length, theirs.length);
    ours.forEach((l, i) => {
      const glyphs = Array.from(l.text).length;
      near(l.x - t.at[0], theirs[i]!.x, 100 + 50 * glyphs, `${c.content} line ${i} x`);
      near(l.y - t.at[1], theirs[i]!.y, 200 * (i + 1), `${c.content} line ${i} y`);
    });
    assert.equal(spec.angleDeg, c.vertical ? 90 : 0);
  }
});

test("SCH_PAINTER::draw( SCH_FIELD ): centred at its box's centre -- the anchor for a centred field (as textOrigin places it), off it for a left / bottom one", () => {
  const at: [number, number] = [5 * MM, 5 * MM];
  const centred = schFieldDrawSpec("R12", { at, h: "center", v: "center", vertical: false, size_nm: MIL50 });
  assert.deepEqual([centred.pos, centred.halign, centred.valign], [at, "center", "center"]);
  const pen = schDrawPen(MIL50, false);
  assert.equal(pen, 152_400, "the schematic's default line width, 6 mil (getTextThickness)");
  const [dx, dy] = textOrigin(measureStrokeText("R12", MIL50), MIL50, pen, "center", "center");
  const line = linePositions(centred, STROKE)[0]!;
  near(line.x - at[0], dx, 200, "x");
  near(line.y - at[1], dy, 200, "y");
  // left and bottom: GetTextBox (its own pen, an eighth of the size: 158800 nm) is the advance less INTER_CHAR and 1.5 pens each side wide, 1.17 x (size + 3 pens) high
  const own = schOwnPen(MIL50, false);
  assert.equal(own, 158_800, "KiROUND( 12700 / 8 ) in eeschema's units");
  const lb = schFieldDrawSpec("R12", { at, h: "left", v: "bottom", vertical: false, size_nm: MIL50 });
  const w = measureStrokeText("R12", MIL50) - 0.2 * MIL50 + 2 * Math.round(1.5 * own);
  const ext = MIL50 + 2 * Math.round(1.5 * own);
  const textH = ext + Math.round(ext * 0.17);
  near(lb.pos[0], at[0] + w / 2, 300, "centre x");
  near(lb.pos[1], at[1] - textH / 2 + Math.round(ext * 0.17), 300, "centre y");
  // a field at 90 degrees: the box turned a quarter about the anchor
  const up = schFieldDrawSpec("R12", { at, h: "left", v: "bottom", vertical: true, size_nm: MIL50 });
  near(up.pos[0], at[0] + (lb.pos[1] - at[1]), 100, "turned x");
  near(up.pos[1], at[1] - (lb.pos[0] - at[0]), 100, "turned y");
  assert.equal(up.angleDeg, 90);
});

test("a symbol's text (LAYER_DEVICE): at the box's centre vertically, its left edge for a left one", () => {
  const at: [number, number] = [0, 0];
  const c = deviceTextDrawSpec("IN", { at, h: "center", v: "center", vertical: false, size_nm: MIL50 });
  assert.deepEqual([c.pos, c.halign, c.valign], [at, "center", "center"]);
  const l = deviceTextDrawSpec("IN", { at, h: "left", v: "top", vertical: false, size_nm: MIL50 });
  assert.equal(l.pos[0], 0);
  assert.equal(l.halign, "left");
  assert.equal(l.valign, "center");
  assert.ok(l.pos[1] > 0, "a top-justified text's box hangs below its anchor");
});

test("labels: SetSpinStyle's justification and GetSchematicTextOffset in eeschema's units", () => {
  const at: [number, number] = [0, 0];
  // a local label: GetTextOffset (0.15 of 12700) + GetPenWidth (KiROUND( 12700 / 8 )) off the wire, bottom-justified
  assert.deepEqual(labelTextOffset({ scope: "local", shape: null }, "right"), [0, -349_300]);
  assert.deepEqual(labelTextOffset({ scope: "local", shape: null }, "up"), [-349_300, 0]);
  const local = schLabelDrawSpec({ net: "A", at, scope: "local" }, "left");
  assert.deepEqual([local.halign, local.valign, local.angleDeg], ["right", "bottom", 0]);
  // a global input: the box expansion (KiROUND( 0.375 * 12700 ) = 4763) and three quarters of the height (9525), 0.0715 of it down (908)
  assert.deepEqual(labelTextOffset({ scope: "global", shape: "input" }, "left"), [-1_428_800, 90_800]);
  assert.deepEqual(labelTextOffset({ scope: "global", shape: "passive" }, "right"), [476_300, 90_800]);
  assert.deepEqual(labelTextOffset({ scope: "global", shape: "output" }, "up"), [90_800, -476_300]);
  // a hierarchical label: GetTextOffset + GetTextWidth (the glyph width, not the text's advance)
  assert.deepEqual(labelTextOffset({ scope: "hierarchical", shape: "input" }, "left"), [-1_460_500, 0]);
  assert.deepEqual(labelTextOffset({ scope: "hierarchical", shape: "input" }, "bottom"), [0, 1_460_500]);
  const glob = schLabelDrawSpec({ net: "CLK", at, scope: "global", shape: "input" }, "bottom");
  assert.deepEqual([glob.halign, glob.valign, glob.angleDeg], ["right", "center", 90]);
});

/** A face of 1000 units to the em with every glyph half an em wide and no ink. */
const half: OutlineFace = { family: "t", unitsPerEm: 1000, ascender: 800, descender: -200, height: 1200, fakeBold: false, fakeItalic: false, advance: (t) => Array.from(t).length / 2, ink: () => null };
const outline: KicadFont = { kind: "outline", face: half };

test("a global label's flag is as long as its text's box in its face (SCH_GLOBALLABEL::CreateGraphicShape: GetTextBox().GetWidth())", () => {
  const stroke = labelTextBoxWidth({ net: "DATA", size_nm: MIL50 });
  near(stroke, measureStrokeText("DATA", MIL50) - 0.2 * MIL50 + 2 * Math.round(1.5 * 158_800), 400, "the stroke font's box");
  // an outline face: the advance (1.4 x the size to the em), no pen round it
  near(labelTextBoxWidth({ net: "DATA", size_nm: MIL50 }, outline), 2 * 1.4 * MIL50, 100, "the outline face's box");
  // its text: no stroke fudges, so a left-justified line starts on the text's position
  const spec = schLabelDrawSpec({ net: "DATA", at: [0, 0], scope: "global", shape: "passive" }, "right");
  assert.equal(linePositions(spec, outline)[0]!.x, spec.pos[0]);
});

test("SCH_TEXTBOX::GetDrawPos and the text broken to the column (GetShownText's LinebreakText)", () => {
  const box = { start: [0, 0] as const, end: [10 * MM, 5 * MM] as const, margins: [MM, MM, MM, MM] as const, angleDeg: 0 };
  assert.deepEqual(schTextBoxDrawPos({ ...box, h: "left", v: "top" }), [MM, MM]);
  assert.deepEqual(schTextBoxDrawPos({ ...box, h: "center", v: "center" }), [5 * MM, 2.5 * MM]);
  assert.deepEqual(schTextBoxDrawPos({ ...box, h: "right", v: "bottom" }), [9 * MM, 4 * MM]);
  // running up the box: the horizontal justification picks along the height, the vertical one across it
  assert.deepEqual(schTextBoxDrawPos({ ...box, angleDeg: 90, h: "left", v: "top" }), [MM, 4 * MM]);
  const spec = schTextBoxDrawSpec({ ...box, text: "one two three four five six", size_nm: MIL50, h: "left", v: "top" });
  assert.ok(spec.text.split("\n").length > 1, spec.text);
  for (const l of layoutKicadText(spec).lines) assert.ok(l.extent <= 8 * MM, `${l.text}: ${l.extent}`);
});
