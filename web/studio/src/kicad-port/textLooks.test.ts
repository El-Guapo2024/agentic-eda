import { test } from "node:test";
import assert from "node:assert/strict";
import { chosenFont, fontChoices, linkChoices, oneStep, textAttrsCmd, validHyperlink, DEFAULT_FONT, KICAD_FONT } from "./textLooks";

test("fontChoices: Default Font and KiCad Font first, the families sorted, a missing font appended as not found", () => {
  const { choices, selected } = fontChoices(["Noto Sans", "DejaVu Sans", "Noto Sans"], null);
  assert.deepEqual(choices, [DEFAULT_FONT, KICAD_FONT, "DejaVu Sans", "Noto Sans"]);
  assert.equal(selected, DEFAULT_FONT);
  assert.equal(fontChoices(["DejaVu Sans"], "KiCad Font").selected, KICAD_FONT);
  const missing = fontChoices(["DejaVu Sans"], "Comic Sans");
  assert.equal(missing.selected, "Comic Sans <not found>");
  assert.equal(missing.choices.at(-1), "Comic Sans <not found>");
});

test("chosenFont: the default font is none, a font not found changes nothing", () => {
  assert.equal(chosenFont(DEFAULT_FONT), "");
  assert.equal(chosenFont("DejaVu Sans"), "DejaVu Sans");
  assert.equal(chosenFont("Comic Sans <not found>"), undefined);
});

test("validHyperlink: ValidateHyperlink takes a page or a URL with a scheme", () => {
  for (const ok of ["", "#2", "https://kicad.org", "file:///x.pdf"]) assert.ok(validHyperlink(ok), ok);
  for (const bad of ["kicad.org", "1x:y", ":y"]) assert.ok(!validHyperlink(bad), bad);
});

test("linkChoices: a page per sheet, the root as <root sheet>, then the URL starts", () => {
  const c = linkChoices([
    { page: "1", name: "", depth: 0 },
    { page: "2", name: "Power", depth: 1 },
  ]);
  assert.deepEqual(
    c.map((x) => [x.value, x.label]),
    [
      ["#1", "Page 1 (<root sheet>)"],
      ["#2", "Page 2 (Power)"],
      ["---", "---"],
      ["file://", "file://..."],
      ["http://", "http://..."],
      ["https://", "https://..."],
    ]
  );
});

test("textAttrsCmd: only what changed, a label's bold left to its own verb, one step for a dialog's OK", () => {
  const red = { r: 255, g: 0, b: 0, a: 255 };
  assert.equal(textAttrsCmd("txt_a", { font: null }, { font: "", color: null, href: "", bold: false, italic: false, excludeFromSim: false }, true), null);
  assert.deepEqual(textAttrsCmd("txt_a", {}, { font: "KiCad Font", color: red, href: " #2 ", bold: true, excludeFromSim: true }, true), {
    op: "sch_edit",
    verb: "set_text_attrs",
    ids: ["txt_a"],
    font: "KiCad Font",
    color: red,
    href: "#2",
    exclude_from_sim: true,
    bold: true,
  });
  // back to the default colour and font
  assert.deepEqual(textAttrsCmd("lbl_a", { font: "KiCad Font", color: red }, { font: "", color: null, bold: true }, false), { op: "sch_edit", verb: "set_text_attrs", ids: ["lbl_a"], font: "", color: { r: 0, g: 0, b: 0, a: 0 } });
  const a = { op: "sch_edit", verb: "edit_text", id: "txt_a", text: "x" } as never;
  assert.equal(oneStep([null, null]), null);
  assert.equal(oneStep([a, null]), a);
  assert.deepEqual(oneStep([a, a]), { op: "batch", cmds: [a, a] });
});
