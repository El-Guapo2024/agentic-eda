// The slow proof of the text layout (UI-LEFT-2 PP1, CC1; `EDA_SLOW_TESTS=1`): a board of 28 `gr_text`s in the stroke font and in DejaVu Sans, in several justifications,
// turned, mirrored, italic, bold, wider than high, on several lines and with markup, plotted by kicad-cli (`pcb export svg`), against `kicad-port/kicadText.ts`:
//   - the stroke font: every pen stroke kicad-cli plots (`<g class="stroked-text">`'s `<path d="M .. L ..">`) is one of the layout's, within 0.15 um (the SVG's 4 decimals);
//   - every line's `StringBoundaryLimits` width (`GetTextBox`'s) is the `textLength` kicad-cli gives a stroke line's hidden `<text>` (`GRTextWidth`);
//   - an outline face: the box of the glyphs' polygons kicad-cli fills is the layout's ink box, within 0.003 of the size (FreeType hints the outlines' heights to whole
//     pixels at 358 to the em). The face is read here from the file fontconfig matches (`fc-match`, as the server's `GET /api/fonts/file`) by kicad-port/sfnt.ts, as the
//     canvas reads it; `hmtx`'s advances stand in for the browser's `measureText` (no kerning pair is in the texts).
// Each text sits in a cell of its own, 20 mm apart, so the plot's paths are told apart by where they are.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { layoutKicadText, layoutSegments, runInk, stringBoundaryLimits, worldToFrame, type KicadTextSpec, type OutlineFace, type KicadFont } from "./kicadText";
import { readSfnt, sfntInk } from "./sfnt";

const SLOW = process.env.EDA_SLOW_TESTS === "1";
const CLI = process.env.KICAD_CLI ?? "/usr/local/bin/kicad-cli";
const MM = 1_000_000;

interface Case {
  text: string;
  at: [number, number];
  size: [number, number];
  pen: number;
  h: "left" | "center" | "right";
  v: "top" | "center" | "bottom";
  angle?: number;
  mirror?: boolean;
  italic?: boolean;
  bold?: boolean;
  face?: string;
}

const CASES: Case[] = [];
const add = (c: Omit<Case, "at">) => CASES.push({ ...c, at: [20 + (CASES.length % 8) * 20, 20 + Math.floor(CASES.length / 8) * 20] });
for (const face of [undefined, "DejaVu Sans"]) {
  for (const [h, v] of [["left", "top"], ["center", "center"], ["right", "bottom"], ["left", "bottom"], ["right", "top"]] as const) add({ text: "Hxg", size: [1, 1], pen: 0.1, h, v, face });
  add({ text: "AB\nC", size: [1, 1], pen: 0.1, h: "center", v: "center", face });
  add({ text: "H\nHx\nH", size: [1, 1], pen: 0.15, h: "right", v: "bottom", face });
  add({ text: "H", size: [1, 1], pen: 0.1, h: "left", v: "center", angle: 90, face });
  add({ text: "Hx", size: [1, 1], pen: 0.1, h: "center", v: "top", angle: 30, face });
  add({ text: "H", size: [1, 2], pen: 0.1, h: "left", v: "center", mirror: true, face });
  add({ text: "H", size: [1.5, 1], pen: 0.12, h: "center", v: "center", face });
  add({ text: "Hx", size: [1, 1], pen: 0.1, h: "left", v: "center", italic: true, face });
  add({ text: "Hx", size: [1, 1], pen: 0.2, h: "left", v: "center", bold: true, face });
  add({ text: "~{H}x_{2}", size: [1, 1], pen: 0.1, h: "left", v: "center", face });
}

const mm = (v: number) => (Number.isInteger(v) ? `${v}` : v.toFixed(4));
function board(): string {
  const texts = CASES.map((c) => {
    const font = [c.face ? `(face "${c.face}")` : "", `(size ${mm(c.size[1])} ${mm(c.size[0])})`, `(thickness ${mm(c.pen)})`, c.bold ? "bold" : "", c.italic ? "italic" : ""].filter(Boolean).join(" ");
    const justify = [c.h !== "center" ? c.h : "", c.v !== "center" ? c.v : "", c.mirror ? "mirror" : ""].filter(Boolean).join(" ");
    return `  (gr_text "${c.text.replace(/\n/g, "\\n")}" (at ${c.at[0]} ${c.at[1]} ${c.angle ?? 0}) (layer "F.SilkS") (effects (font ${font})${justify ? ` (justify ${justify})` : ""}))`;
  });
  return `(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A3")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user "F.Silkscreen") (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
${texts.join("\n")}
)
`;
}

/** The cell a plotted point is in. */
const cellOf = (x: number, y: number): number => {
  const col = Math.round((x - 20) / 20);
  const row = Math.round((y - 20) / 20);
  return row * 8 + col;
};

// ------------------------------------------------------------------------------------------------------ an outline face from its file

/** A face read from its file (kicad-port/sfnt.ts, what the canvas reads too), its advances `hmtx`'s (the browser's `measureText` shapes them; no kerning pair is in the texts). */
function fileFace(path: string, fakeItalic: boolean): OutlineFace {
  const f = readSfnt(readFileSync(path))!;
  const advance = (t: string) => Array.from(t).reduce((s, c) => s + f.advanceOf(f.glyphOf(c.codePointAt(0)!)), 0) / f.unitsPerEm;
  return { family: path, unitsPerEm: f.unitsPerEm, ascender: f.ascender, descender: f.descender, height: f.height, fakeBold: false, fakeItalic, advance, ink: (t) => sfntInk(f, t, advance) ?? null };
}

/** `FONTCONFIG::FindFont` as the server asks it (`fc-match` of the family with a Bold and an Italic style), and whether the italic is faked (the file is not one). */
function matchFace(face: string, bold: boolean, italic: boolean): OutlineFace {
  const styles = [bold ? "Bold" : "", italic ? "Italic" : ""].filter(Boolean).join(",");
  const out = execFileSync("fc-match", ["-f", "%{file}\t%{style[0]}", styles ? `${face}:style=${styles}` : face], { encoding: "utf8" });
  const [file, style = ""] = out.split("\t");
  return fileFace(file!, italic && !/italic|oblique|slant/i.test(style));
}

test("kicad-cli's plot of stroke and outline texts is the layout's (slow: EDA_SLOW_TESTS=1)", () => {
  if (!SLOW || !existsSync(CLI)) return;
  const dir = mkdtempSync("/tmp/kicad-text-proof-");
  writeFileSync(`${dir}/t.kicad_pcb`, board());
  execFileSync(CLI, ["pcb", "export", "svg", "--layers", "F.SilkS", "--mode-single", "--exclude-drawing-sheet", "-o", `${dir}/t.svg`, `${dir}/t.kicad_pcb`], { stdio: "ignore" });
  const svg = readFileSync(`${dir}/t.svg`, "utf8");
  const strokes = new Map<number, number[][]>();
  for (const g of svg.matchAll(/<g class="stroked-text"><desc>[\s\S]*?<\/desc>([\s\S]*?)<\/g>/g)) {
    for (const d of g[1]!.matchAll(/d="M([\d.]+) ([\d.]+)\s*L([\d.]+) ([\d.]+)/g)) {
      const seg = [Number(d[1]), Number(d[2]), Number(d[3]), Number(d[4])];
      const k = cellOf((seg[0]! + seg[2]!) / 2, (seg[1]! + seg[3]!) / 2);
      (strokes.get(k) ?? strokes.set(k, []).get(k)!).push(seg);
    }
  }
  const fills = new Map<number, [number, number][]>();
  for (const p of svg.matchAll(/<path style="fill[^"]*"\s*d="([^"]*)"/g)) {
    for (const m of p[1]!.matchAll(/([\d.]+),([\d.]+)/g)) {
      const [x, y] = [Number(m[1]), Number(m[2])];
      const k = cellOf(x, y);
      (fills.get(k) ?? fills.set(k, []).get(k)!).push([x * MM, y * MM]);
    }
  }
  // the hidden `<text>` of each line of a stroke text (`SVG_PLOTTER::Text`): its `textLength` is `GRTextWidth`, `KiROUND( StringBoundaryLimits( .. ).x )` -- the box
  // `INTER_CHAR` short of the cursor and 1.5 pens each side
  const lengths = new Map<number, { text: string; length: number }[]>();
  for (const m of svg.matchAll(/<text x="([\d.]+)" y="([\d.]+)"\s*(?:transform="[^"]*"\s*)?textLength="([\d.]+)"[^>]*>([^<]*)<\/text>/g)) {
    const k = cellOf(Number(m[1]), Number(m[2]));
    (lengths.get(k) ?? lengths.set(k, []).get(k)!).push({ text: m[4]!.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"'), length: Number(m[3]) });
  }
  let compared = 0;
  CASES.forEach((c, i) => {
    const spec: KicadTextSpec = { text: c.text, pos: [c.at[0] * MM, c.at[1] * MM], size: [c.size[0] * MM, c.size[1] * MM], pen: Math.round(c.pen * MM), halign: c.h, valign: c.v, angleDeg: c.angle ?? 0, mirror: c.mirror, italic: c.italic, bold: c.bold };
    const font: KicadFont = c.face ? { kind: "outline", face: matchFace(c.face, !!c.bold, !!c.italic) } : { kind: "stroke" };
    const l = layoutKicadText(spec, font);
    const label = `case ${i} ${JSON.stringify(c)}`;
    // (an outline text is plotted as its glyphs' polygons only: no hidden `<text>`)
    const plottedLines = lengths.get(i) ?? [];
    assert.equal(plottedLines.length, font.kind === "stroke" ? c.text.split("\n").length : 0, `${label}: kicad-cli's lines ${JSON.stringify(plottedLines)}`);
    for (const pl of plottedLines) {
      const w = stringBoundaryLimits(pl.text, font, spec.size, spec.pen, !!c.italic)[0] / MM;
      assert.ok(Math.abs(w - pl.length) <= 0.00015, `${label}: the box of ${JSON.stringify(pl.text)} is ${w.toFixed(4)} mm wide, kicad-cli's textLength ${pl.length}`);
    }
    if (font.kind === "stroke") {
      const want = (strokes.get(i) ?? []).map((s) => s.map((v) => Math.round(v * 1e4)));
      const got = layoutSegments(l).map((s) => s.map((v) => Math.round((v / MM) * 1e4)));
      assert.equal(got.length, want.length, `${label}: ${got.length} strokes, kicad-cli ${want.length}`);
      // every stroke kicad-cli plots is one of the layout's (either way round), within a unit of the fourth decimal
      for (const w of want) {
        const hit = got.some((g) => [g, [g[2]!, g[3]!, g[0]!, g[1]!]].some((h) => h.every((v, k) => Math.abs(v - w[k]!) <= 1.5)));
        assert.ok(hit, `${label}: kicad-cli's stroke ${JSON.stringify(w.map((v) => v / 1e4))} is not drawn`);
      }
    } else {
      const pts = fills.get(i);
      assert.ok(pts && pts.length > 0, `${label}: kicad-cli filled nothing`);
      // in the text's frame (turned back about its position, mirrored back): the box of kicad-cli's polygons, and the runs' ink (an overbar is a stroke, which
      // kicad-cli does not fill)
      const framed = pts!.map(([x, y]) => worldToFrame(l, x, y));
      const want = [Math.min(...framed.map((p) => p[0])), Math.min(...framed.map((p) => p[1])), Math.max(...framed.map((p) => p[0])), Math.max(...framed.map((p) => p[1]))].map((v) => v / MM);
      const inks = l.runs.map((r) => runInk(l, r)).filter((b) => b !== null);
      const got = [Math.min(...inks.map((b) => b!.x0)), Math.min(...inks.map((b) => b!.y0)), Math.max(...inks.map((b) => b!.x1)), Math.max(...inks.map((b) => b!.y1))].map((v) => v / MM);
      const tol = 0.003 * Math.max(c.size[0], c.size[1]) + 0.0002;
      // a fake italic's glyphs are slanted outlines; the ink here slants each glyph's box, which reaches as far right as the glyph's top corner would: no less than
      // kicad-cli's, and at most 0.02 of the size more ('x' is narrower at its top than at its feet)
      const slanted = (k: number) => !!font.face.fakeItalic && k === 2 && got[k]! >= want[k]! - tol && got[k]! - want[k]! <= 0.02 * c.size[1];
      got.forEach((v, k) => assert.ok(Math.abs(v - want[k]!) <= tol || slanted(k), `${label}: ink ${JSON.stringify(got.map((x) => +x.toFixed(4)))} vs kicad-cli ${JSON.stringify(want.map((x) => +x.toFixed(4)))}`));
    }
    compared++;
  });
  assert.equal(compared, CASES.length);
});
