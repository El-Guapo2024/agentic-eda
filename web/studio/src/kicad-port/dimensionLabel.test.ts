import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdDimension, TextGraphicsDefaults } from "../api/types";
import { autoThicknessToggled, boldToggled, fontPicked, labelLooksOf, labelLooksOfCmd, thicknessTyped, widthTyped, withLabelLooks } from "./dimensionLabel";
import { fpDimensionHeightOffGrid, fpDimensionOnLayer, fpDimensionWithLineWidth, fpNewDimension, FP_DIMENSION_STYLE, type FpDimDraw } from "./fpDrawTools";
import { dimensionTextSpec } from "./textSpecs";
import { FACTORY_TEXT_GRAPHICS } from "./layerDefaults";

const MM = 1_000_000;

const cmd = (over: Partial<CmdDimension> = {}): CmdDimension => ({
  ...fpNewDimension("aligned", "F.Fab", [0, 0]),
  end: { x: 20 * MM, y: 0 },
  ...over,
});

test("a label without looks of its own is square, upright, centred, in the stroke font and mirrored as its layer's side says", () => {
  const l = labelLooksOfCmd(cmd());
  assert.deepEqual(l, { widthNm: l.heightNm, heightNm: l.heightNm, thicknessNm: l.thicknessNm, autoThickness: false, face: null, bold: false, italic: false, mirrored: false, hJustify: "center", keepUpright: false });
  assert.equal(labelLooksOfCmd(cmd({ layer: "B.Fab" })).mirrored, true, "DrawDimension's SetMirrored( IsBackLayer( layer ) )");
  // no pen of its own is the automatic pen (UI-LEFT-3 P3-11): KiCad 25ec2335's dialog shows `GetPenSizeForNormal( GetTextWidth() )`
  const auto = labelLooksOf({ layer: "B.SilkS", text_size_nm: MM, text_thickness_nm: null, text_width_nm: 2 * MM });
  assert.deepEqual([auto.autoThickness, auto.thicknessNm], [true, 250_000]);
});

test("the looks go into the dimension's text_effects, each left out at its default", () => {
  const base = cmd({ layer: "B.Fab" });
  const l = labelLooksOfCmd(base);
  assert.equal(withLabelLooks(base, l).text_effects, undefined, "nothing changed: no overlay");
  const out = withLabelLooks(base, { ...l, widthNm: 2 * MM, face: "DejaVu Sans", bold: true, italic: true, mirrored: false, hJustify: "left", keepUpright: true });
  assert.deepEqual(out.text_effects, { width_nm: 2 * MM, face: "DejaVu Sans", bold: true, italic: true, mirrored: false, h_justify: "left", keep_upright: true });
  // the mirror is kept only when it is not the layer's: going to F.Fab with the mirror on keeps it (an attribute, which a layer change leaves)
  const moved = withLabelLooks({ ...base, layer: "F.Fab" }, l);
  assert.deepEqual(moved.text_effects, { mirrored: true });
  assert.deepEqual(labelLooksOfCmd(moved), { ...l }, "and reads back the same");
});

test("the automatic pen: kept while the button is checked, frozen at GetPenSizeForNormal( width ) when it is unchecked (KiCad 25ec2335, UI-LEFT-3 P3-11)", () => {
  const base = { ...cmd(), text_thickness_nm: undefined };
  const l = labelLooksOfCmd(base);
  assert.equal(l.autoThickness, true);
  assert.equal(withLabelLooks(base, l).text_thickness_nm, undefined);
  assert.equal(withLabelLooks(base, { ...l, heightNm: 2 * MM }).text_thickness_nm, undefined, "a new size keeps it automatic");
  // `onTextSize`: the shown pen follows the width; Bold leaves an automatic pen alone
  const wide = widthTyped(l, 4 * MM);
  assert.equal(wide.thicknessNm, 500_000);
  assert.deepEqual(boldToggled(wide, true), { ...wide, bold: true });
  // `onAutoTextThickness`: unchecked, the pen shown is the label's own; checked again, none of its own
  const own = autoThicknessToggled(wide, false);
  assert.equal(withLabelLooks(base, own).text_thickness_nm, 500_000);
  assert.equal(withLabelLooks({ ...base, text_thickness_nm: 300_000 }, autoThicknessToggled({ ...own, thicknessNm: 300_000 }, true)).text_thickness_nm, undefined);
  // drawn with `GetEffectiveTextPenWidth`: an eighth of the width, a fifth when bold, at most a quarter of the smaller side
  const pen = (bold: boolean) => dimensionTextSpec({ text: "20", text_at: [0, 0], computed_text_angle: 0, text_size_nm: MM, text_thickness_nm: null, text_width_nm: 1.6 * MM, bold }).spec.pen;
  assert.deepEqual([pen(false), pen(true)], [200_000, 250_000]);
});

test("the dialog's Bold button, a thickness typed in and the font follow DIALOG_DIMENSION_PROPERTIES", () => {
  const l = { ...labelLooksOfCmd(cmd()), widthNm: 2 * MM, heightNm: MM, thicknessNm: 150_000 };
  // `onBoldToggle`: the pen of the smaller size
  assert.equal(boldToggled(l, true).thicknessNm, 200_000);
  assert.equal(boldToggled(l, false).thicknessNm, 125_000);
  // `onThickness`: Bold follows the pen it is nearer
  assert.equal(thicknessTyped(l, 190_000).bold, true);
  assert.equal(thicknessTyped(l, 130_000).bold, false);
  // `onFontSelected`: back to the stroke font sets Bold from the pen; a face keeps the flag
  assert.equal(fontPicked({ ...l, face: "DejaVu Sans", bold: true }, null).bold, false);
  assert.deepEqual(fontPicked(l, "DejaVu Sans"), { ...l, face: "DejaVu Sans" });
});

test("the label is drawn with its width, justification, mirror, italic, bold and face", () => {
  const { spec, face } = dimensionTextSpec({ text: "20", text_at: [0, 0], computed_text_angle: 0, text_size_nm: MM, text_thickness_nm: 150_000, text_width_nm: 2 * MM, face: "DejaVu Sans", bold: true, italic: true, mirrored: true, h_justify: "right" });
  assert.deepEqual([spec.size, spec.halign, spec.mirror, spec.italic, spec.bold, face], [[2 * MM, MM], "right", true, true, true, "DejaVu Sans"]);
  const plain = dimensionTextSpec({ text: "20", text_at: [0, 0], computed_text_angle: 0, text_size_nm: MM, text_thickness_nm: null });
  assert.deepEqual([plain.spec.size, plain.spec.halign, plain.spec.mirror, plain.face], [[MM, MM], "center", false, null], "an older backend: square, centred");
});

test("DrawDimension gives a new label its layer's width, italic and mirror, and a layer change its size, pen, italic and line again but not its mirror", () => {
  const defaults: TextGraphicsDefaults = { ...FACTORY_TEXT_GRAPHICS, fab: { ...FACTORY_TEXT_GRAPHICS.fab, text_width_nm: 1.2 * MM, italic: true } };
  const style = { ...FP_DIMENSION_STYLE, textDefaults: defaults };
  const d = fpNewDimension("aligned", "F.Fab", [0, 0], style);
  assert.deepEqual(d.text_effects, { width_nm: 1.2 * MM, italic: true });
  const back = fpNewDimension("aligned", "B.SilkS", [0, 0], style);
  assert.equal(back.text_effects, undefined, "a back layer's label is mirrored by the layer");
  // `layerChanged`: SetLayer, SetTextSize, SetTextThickness, SetItalic, SetLineThickness -- not SetMirrored
  const moved = fpDimensionOnLayer(back, "F.Fab", style);
  assert.deepEqual([moved.layer, moved.text_size_nm, moved.text_thickness_nm, moved.stroke_width], ["F.Fab", defaults.fab.text_height_nm, defaults.fab.text_thickness_nm, defaults.fab.line_width_nm]);
  assert.deepEqual(moved.text_effects, { width_nm: 1.2 * MM, italic: true, mirrored: true }, "still mirrored on F.Fab");
  // the label's other looks ride through
  const styled = fpDimensionOnLayer({ ...d, text_effects: { ...d.text_effects, bold: true, h_justify: "left" } }, "B.Fab", style);
  assert.deepEqual(styled.text_effects, { bold: true, h_justify: "left", width_nm: 1.2 * MM, italic: true, mirrored: false });
});

test("the height of a slanted aligned dimension is off the grid, an orthogonal one's or a level one's on it; the width keys set the line", () => {
  const at = (start: [number, number], end: [number, number], kind: "aligned" | "orthogonal", step: "end" | "height"): FpDimDraw => ({
    kind,
    step,
    dim: { ...fpNewDimension(kind, "F.Fab", start), end: { x: end[0], y: end[1] } },
  });
  assert.equal(fpDimensionHeightOffGrid(at([0, 0], [MM, MM], "aligned", "height")), true);
  assert.equal(fpDimensionHeightOffGrid(at([0, 0], [MM, 0], "aligned", "height")), false, "cardinal");
  assert.equal(fpDimensionHeightOffGrid(at([0, 0], [MM, MM], "orthogonal", "height")), false);
  assert.equal(fpDimensionHeightOffGrid(at([0, 0], [MM, MM], "aligned", "end")), false, "only at SET_HEIGHT");
  assert.equal(fpDimensionHeightOffGrid(null), false);
  assert.equal(fpDimensionWithLineWidth(at([0, 0], [MM, MM], "aligned", "end"), 250_000).dim.stroke_width, 250_000);
});
