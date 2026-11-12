import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { setDsExpressionEvaluator, buildDrawItems, buildFullText, convertLegacyVariableRefs, convertToNewOverbarNotation, defaultModel, effectiveModel, expandTextVars, incrementLabel, parseDrawingSheet, pngInfo, replaceAntiSlashSequence, textLines, type DsDrawItem, type DsDrawText, type DsPageContext } from "./dsDataModel";
import { DEFAULT_DRAWING_SHEET, EMPTY_DRAWING_SHEET } from "./dsDefaultSheet";

const A4: DsPageContext = {
  widthNm: 297_000_000,
  heightNm: 210_000_000,
  paperName: "A4",
  titleBlock: { title: "Blinky", date: "2026-11-06", rev: "B", company: "ACME", comments: ["first", "", "third"] },
  fileName: "/work/blinky/blinky.kicad_pcb",
  sheetName: "",
  sheetPath: "/",
  pageNumber: "1",
  sheetCount: 3,
  isFirstPage: true,
  kicadVersion: "KiCad E.D.A. eda studio",
};

const mm = (v: number) => Math.round(v * 1_000_000);
const texts = (items: DsDrawItem[]) => items.filter((i): i is DsDrawText => i.kind === "text");
const byKind = (items: DsDrawItem[]) => items.reduce<Record<string, number>>((acc, i) => ((acc[i.kind] = (acc[i.kind] ?? 0) + 1), acc), {});

test("KiCad's default sheet reads as drawing_sheet_default_description.cpp writes it", () => {
  const r = parseDrawingSheet(DEFAULT_DRAWING_SHEET, "default page");
  assert.equal(r.error, null);
  assert.equal(r.model.version, 20210606);
  assert.equal(r.model.items.length, 29, "29 items in the file");
  assert.equal(r.model.defaultLineWidth, 0.15);
  assert.deepEqual(r.model.defaultTextSize, { x: 1.5, y: 1.5 });
  assert.deepEqual([r.model.leftMargin, r.model.rightMargin, r.model.topMargin, r.model.bottomMargin], [10, 10, 10, 10]);
  const zones = r.model.items[3]!;
  assert.equal(zones.type, "text");
  assert.deepEqual(zones.pos, { x: 25, y: 1, anchor: "lt" });
  assert.equal(zones.repeat, 100);
  assert.deepEqual(zones.incr, { x: 50, y: 0 });
  const letters = r.model.items[7]!;
  assert.ok(letters.type === "text" && letters.hJustify === "center" && letters.vJustify === "center", "(justify center) centres both ways");
  const title = r.model.items.find((i) => i.type === "text" && i.textBase.startsWith("Title"))!;
  assert.ok(title.type === "text" && title.bold && title.italic && title.size.x === 2);
});

test("the default sheet on A4 landscape: the two frames, 6 numbers and 4 letters a side, the title block (BuildDrawItemsList)", () => {
  const items = buildDrawItems(defaultModel(), A4);
  assert.equal(items.length, 58);
  assert.deepEqual(byKind(items), { rect: 3, line: 22, text: 33 });
  // the outer frame on the margins, the inner one 2 mm inside (`(repeat 2) (incrx 2) (incry 2)`)
  const rects = items.filter((i) => i.kind === "rect");
  assert.deepEqual(rects.map((r) => [r.start, r.end]), [
    [[mm(287 - 110), mm(200 - 34)], [mm(285), mm(198)]],
    [[mm(10), mm(10)], [mm(287), mm(200)]],
    [[mm(12), mm(12)], [mm(285), mm(198)]],
  ]);
  assert.equal(rects[0]!.kind === "rect" && rects[0]!.widthNm, 150_000);
  const t = texts(items);
  const top = t.filter((x) => x.pos[1] === mm(11));
  assert.deepEqual(top.map((x) => x.text), ["1", "2", "3", "4", "5", "6"], "IncrementLabel: '1' + 50 mm steps while inside the margins");
  assert.deepEqual(top.map((x) => x.pos[0]), [35, 85, 135, 185, 235, 285].map(mm));
  const left = t.filter((x) => x.pos[0] === mm(11));
  assert.deepEqual(left.map((x) => x.text), ["A", "B", "C", "D"]);
  const right = t.filter((x) => x.pos[0] === mm(286));
  assert.deepEqual(right.map((x) => x.text), ["A", "B", "C", "D"]);
  assert.equal(t.filter((x) => x.pos[1] === mm(199)).length, 6, "the numbers along the bottom too");
  // the title block's variables
  const shown = t.map((x) => x.text);
  for (const s of ["Title: Blinky", "Date: 2026-11-06", "Rev: B", "Size: A4", "Id: 1/3", "File: blinky.kicad_pcb", "Sheet: /", "ACME", "first", "", "third", "KiCad E.D.A. eda studio"]) assert.ok(shown.includes(s), `shows "${s}"`);
  const titleText = t.find((x) => x.text.startsWith("Title"))!;
  assert.deepEqual(titleText.pos, [mm(287 - 109), mm(200 - 10.7)]);
  assert.deepEqual(titleText.size, [mm(2), mm(2)]);
  assert.equal(titleText.penNm, mm(0.4), "bold: GetPenSizeForBold, a fifth of the size");
  assert.equal(t.find((x) => x.text === "Size: A4")!.penNm, mm(0.15), "the setup's text line width");
});

test("a bigger paper gets more zones; a portrait one turns them", () => {
  const a3 = buildDrawItems(defaultModel(), { ...A4, widthNm: 420_000_000, heightNm: 297_000_000, paperName: "A3" });
  assert.deepEqual(texts(a3).filter((x) => x.pos[1] === mm(11)).map((x) => x.text), ["1", "2", "3", "4", "5", "6", "7", "8"]);
  assert.deepEqual(texts(a3).filter((x) => x.pos[0] === mm(11)).map((x) => x.text), ["A", "B", "C", "D", "E", "F"]);
  const portrait = buildDrawItems(defaultModel(), { ...A4, widthNm: 210_000_000, heightNm: 297_000_000 });
  assert.equal(texts(portrait).filter((x) => x.pos[1] === mm(11)).length, 4);
  assert.equal(texts(portrait).filter((x) => x.pos[0] === mm(11)).length, 6);
});

test("KiCad's QA sheet (qa/data/cli/basic_test/custom_ds.kicad_wks): its own texts on top of the default frame", () => {
  const text = readFileSync("src/kicad/drawing_sheets/custom_ds.kicad_wks", "utf8");
  const r = parseDrawingSheet(text, "custom_ds.kicad_wks");
  assert.equal(r.error, null);
  assert.equal(r.model.version, 20220228);
  assert.equal(r.model.items.length, 31);
  const items = buildDrawItems(r.model, A4);
  assert.equal(items.length, 60);
  const custom = texts(items).filter((x) => x.text === "CUSTOM DRAWING SHEET");
  assert.equal(custom.length, 4);
  assert.deepEqual(custom[0]!.pos, [mm(287 - 79.789), mm(200 - 22.8422)]);
  assert.deepEqual(custom[0]!.size, [mm(4), mm(4)]);
  assert.equal(texts(items).some((x) => x.text.startsWith("Date:")), false, "this sheet has no date");
});

test("KiCad's installed templates read without an error (when the nightly's templates are on this machine)", () => {
  const dir = "/usr/share/kicad-nightly/template";
  for (const name of ["A4_ISO5457-1999_ISO7200-2004_EN.kicad_wks", "gost_landscape.kicad_wks", "A3_ISO5457-1999_ISO7200-2004_DE.kicad_wks"]) {
    const path = `${dir}/${name}`;
    if (!existsSync(path)) continue;
    const r = parseDrawingSheet(readFileSync(path, "utf8"), name);
    assert.equal(r.error, null, `${name}: ${r.error}`);
    assert.ok(r.model.items.length > 10, name);
    const items = buildDrawItems(r.model, { ...A4, widthNm: name.startsWith("A3") ? 420_000_000 : 297_000_000, heightNm: name.startsWith("A3") ? 297_000_000 : 210_000_000 });
    assert.ok(items.length >= r.model.items.length - r.model.items.filter((i) => i.pageOption !== "all").length, name);
  }
  const iso = `${dir}/A4_ISO5457-1999_ISO7200-2004_EN.kicad_wks`;
  if (existsSync(iso)) {
    const r = parseDrawingSheet(readFileSync(iso, "utf8"), "iso");
    assert.equal(r.model.version, 0, "a page_layout file has no version");
    assert.equal(r.model.leftMargin, 20);
    // the outer rect at -5 mm outside the margins: start (-5 -5 ltcorner) -> (15, 5), end (-5 -5) from the bottom right -> (292, 205)
    const first = buildDrawItems(r.model, A4)[0]!;
    assert.ok(first.kind === "rect");
    assert.deepEqual([first.start, first.end, first.widthNm], [[mm(15), mm(5)], [mm(292), mm(205)], mm(0.35)]);
  }
});

test("page1only and notonpage1 (BuildDrawItemsList's page options)", () => {
  const r = parseDrawingSheet('(kicad_wks (version 20231118) (setup (linewidth 0.2)) (line (start 1 1) (end 2 2) (option page1only)) (line (start 3 3) (end 4 4) (option notonpage1)) (rect (start 5 5) (end 6 6)))');
  assert.equal(r.error, null);
  assert.deepEqual(buildDrawItems(r.model, A4).map((i) => i.kind), ["line", "rect"]);
  assert.equal((buildDrawItems(r.model, A4)[0] as { start: number[] }).start[0], mm(286));
  assert.deepEqual(buildDrawItems(r.model, { ...A4, isFirstPage: false }).map((i) => (i.kind === "line" ? i.start[0] : i.kind)), [mm(284), "rect"]);
});

test("an empty but well-formed sheet stays empty; a broken one keeps what was read and says why", () => {
  const empty = parseDrawingSheet("(kicad_wks (version 20231118) (setup (textsize 1.5 1.5)))");
  assert.equal(empty.error, null);
  assert.equal(buildDrawItems(empty.model, A4).length, 0, "AllowVoidList: the person wants no sheet");
  const broken = parseDrawingSheet('(kicad_wks (version 20231118)\n(line (start 1 1) (end 2 2))\n(circle (center 1 1)))', "mine.kicad_wks");
  assert.equal(broken.model.items.length, 1);
  assert.equal(broken.error, "Unexpected circle in 'mine.kicad_wks', line 3, offset 2.");
  const nothing = parseDrawingSheet("(kicad_wks (version 20231118) (bogus))");
  assert.ok(nothing.error);
  assert.equal(effectiveModel(nothing.model), defaultModel(), "nothing read and no setup: the default sheet");
  const future = parseDrawingSheet('(kicad_wks (version 20991231) (generator "pl_editor") (generator_version "11.0") (setup (linewidth 0.1)))');
  assert.match(future.error ?? "", /upgrade KiCad to version 11.0 or later \(file format dated 20991231 or later\)/);
  assert.equal(buildDrawItems(parseDrawingSheet(EMPTY_DRAWING_SHEET).model, A4).length, 1, "KiCad's empty sheet is its zero-length line");
});

test("text variables: the sheet's own, then the title block's (whose values may hold variables), then the project's; an unknown one stays", () => {
  const ctx: DsPageContext = { ...A4, titleBlock: { ...A4.titleBlock, title: "Board ${REV_NOTE}", comments: ["${PAPER} sheet"] }, projectVar: (n) => ({ REV_NOTE: "rev C", LOOP: "${REVISION}" })[n] ?? null };
  assert.equal(buildFullText("Title: ${TITLE}", ctx), "Title: Board rev C");
  // a project variable's value is not expanded again (`PROJECT::TextVarResolver` hands it back as it is)
  assert.equal(buildFullText("${LOOP}", ctx), "${REVISION}");
  assert.equal(buildFullText("${COMMENT1}", ctx), "A4 sheet");
  assert.equal(buildFullText("${COMMENT9}", ctx), "");
  assert.equal(buildFullText("${FILEPATH} | ${FILENAME}", ctx), "/work/blinky/blinky.kicad_pcb | blinky.kicad_pcb");
  assert.equal(buildFullText("${NOPE} \\${TITLE}", ctx), "${NOPE} ${TITLE}");
  assert.equal(expandTextVars("${A${B}}", (t) => ({ B: "X", AX: "nested" })[t] ?? null), "nested");
  assert.equal(expandTextVars("@{1+${B}}", (t) => (t === "B" ? "2" : null)), "@{1+2}");
});

test("legacy formats and overbars (convertLegacyVariableRefs, ConvertToNewOverbarNotation)", () => {
  assert.equal(convertLegacyVariableRefs("Rev: %R %% %C0 %C8 %Q"), "Rev: ${REVISION} % ${COMMENT1} ${COMMENT9} ");
  assert.equal(convertToNewOverbarNotation("~RESET~ x"), "~{RESET} x");
  assert.equal(convertToNewOverbarNotation("a~~b"), "a~b");
  const old = parseDrawingSheet('(page_layout (setup (textsize 1.5 1.5)) (tbtext "%T ~CS~" (pos 1 1)))');
  assert.equal(old.error, null);
  assert.equal(old.model.items[0]!.type === "text" && old.model.items[0]!.textBase, "${TITLE} ~{CS}");
});

test("labels step by their increment (IncrementLabel), multi-line texts do not, and \\n breaks a line", () => {
  assert.equal(incrementLabel("1", 9), "10");
  assert.equal(incrementLabel("Z9", 2), "Z11");
  assert.equal(incrementLabel("A", 2), "C");
  assert.deepEqual(replaceAntiSlashSequence("a\\nb\\\\c"), { text: "a\nb\\c", multiline: true });
  const r = parseDrawingSheet('(kicad_wks (version 20231118) (setup (textsize 1 1)) (tbtext "A" (pos 1 1 ltcorner) (repeat 3) (incry 10) (incrlabel 2)) (tbtext "x\\\\ny" (pos 1 1 ltcorner) (repeat 2) (incrx 5)))');
  assert.equal(r.error, null);
  assert.deepEqual(texts(buildDrawItems(r.model, A4)).map((t) => t.text), ["A", "C", "E", "x\ny", "x\ny"]);
});

test("rotated polygons and a constrained text", () => {
  const r = parseDrawingSheet('(kicad_wks (version 20231118) (setup (textsize 2 2)) (polygon (pos 10 10 ltcorner) (rotate 90) (linewidth 0.1) (pts (xy 0 0) (xy 4 0) (xy 4 2))) (tbtext "WIDE TEXT HERE" (pos 50 50) (maxlen 5)))');
  assert.equal(r.error, null);
  const items = buildDrawItems(r.model, A4);
  const poly = items[0]!;
  assert.ok(poly.kind === "poly");
  // RotatePoint by 90: (x, y) -> (y, -x)
  assert.deepEqual(poly.outlines, [[[mm(20), mm(20)], [mm(20), mm(16)], [mm(22), mm(16)]]]);
  const t = items[1]!;
  assert.ok(t.kind === "text" && t.size[0] < mm(2) && t.size[1] === mm(2), "maxlen shrinks the width only");
});

test("a bitmap keeps its PNG and is sized by its resolution and scale", () => {
  // a 2x1 PNG with a pHYs chunk of 11811 pixels per metre (300 dpi)
  const png = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAC4jAAAuIwF4pT92AAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
  const bytes = Uint8Array.from(atob(png), (c) => c.charCodeAt(0));
  assert.deepEqual(pngInfo(bytes), { width: 2, height: 1, ppi: 300 });
  const r = parseDrawingSheet(`(kicad_wks (version 20231118) (setup (textsize 1 1)) (bitmap (pos 20 20) (scale 2) (data ${png})))`);
  assert.equal(r.error, null);
  const b = buildDrawItems(r.model, A4)[0]!;
  assert.ok(b.kind === "bitmap");
  assert.deepEqual(b.size, [Math.round(2 * (25_400_000 / 300) * 2), Math.round(25_400_000 / 300 * 2)]);
  const bad = parseDrawingSheet("(kicad_wks (version 20231118) (setup (textsize 1 1)) (bitmap (pos 20 20) (data AAAA)))");
  assert.equal(bad.model.items.length, 0, "an unreadable image is dropped");
});

test("where a text's lines go (FONT::getLinePositions)", () => {
  const measure = (s: string, sx: number) => s.length * sx;
  const base = { size: [mm(2), mm(2)] as [number, number], penNm: mm(0.2), bold: false, multiline: false };
  const left = textLines({ ...base, text: "AB", hJustify: "left", vJustify: "center" }, measure);
  assert.deepEqual(left, [{ text: "AB", x: Math.trunc(mm(0.2) / 1.52), y: Math.trunc(mm(2) - mm(0.2) * 0.052) - Math.trunc(Math.trunc(mm(2) * 1.17) / 2) }]);
  const right = textLines({ ...base, text: "AB", hJustify: "right", vJustify: "top" }, measure);
  assert.equal(right[0]!.x, -(mm(4) + Math.trunc(mm(0.2) / 1.52)));
  const two = textLines({ ...base, text: "a\nb", multiline: true, hJustify: "center", vJustify: "bottom" }, measure);
  assert.equal(two.length, 2);
  assert.equal(two[1]!.y - two[0]!.y, Math.round(mm(2) * 1.68 * 0.9583));
});

test("CC8: BuildFullText evaluates `@{..}` after the variables (25ec2335), with the evaluator the caller sets; a text's face goes to its draw item", () => {
  const ctx: DsPageContext = { ...A4, titleBlock: { ...A4.titleBlock, title: "T" } };
  try {
    setDsExpressionEvaluator((t) => t.replace("@{1+1}", "2"));
    assert.equal(buildFullText("${TITLE} @{1+1}", ctx), "T 2");
    assert.equal(buildFullText("${TITLE}", ctx), "T", "no `@{` asks nothing");
  } finally {
    setDsExpressionEvaluator((t) => t);
  }
  const { model } = parseDrawingSheet('(kicad_wks (version 20231118) (tbtext "F" (name "") (pos 10 10) (font (face "DejaVu Sans") (size 2 2))))', "t");
  const text = buildDrawItems(model, ctx, (s, w) => s.length * w).find((i): i is DsDrawText => i.kind === "text" && i.text === "F");
  assert.equal(text?.face, "DejaVu Sans");
});
