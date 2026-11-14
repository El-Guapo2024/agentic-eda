import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, FieldInfo, Pad, Part, Shape, Track, Via } from "../api/types";
import { buildLayers3d, padOnTechLayer, primCounts, viaTented, type Layers3dOptions, type Prim } from "./layers3d";

const OPTS: Layers3dOptions = {
  layerShown: () => true,
  fpText: true,
  references: true,
  values: true,
  showZones: true,
  showOffBoardSilk: false,
  clipSilkOnViaAnnuli: false,
  subtractMaskFromSilk: false,
  differentiatePlatedCopper: false,
  plating: 20_000,
};

const OUTLINE: Shape = { id: "e", kind: "rect", layer: "Edge.Cuts", stroke_width: 100_000, filled: false, start: [0, 0], end: [40_000_000, 30_000_000] } as Shape;

function board(over: { tracks?: Track[]; vias?: Via[]; parts?: Part[]; shapes?: Shape[]; texts?: unknown[]; rules?: Record<string, unknown> } = {}): BoardState {
  return {
    layers: ["F.Cu", "B.Cu"],
    parts: over.parts ?? [],
    outline: [[0, 0], [40_000_000, 0], [40_000_000, 30_000_000], [0, 30_000_000]],
    routing: { tracks: over.tracks ?? [], vias: over.vias ?? [], zones: [] },
    drawings: { shapes: [OUTLINE, ...(over.shapes ?? [])], texts: over.texts ?? [], dimensions: [] },
    board_rules: { mask_paste: { expansion_nm: 50_000, tent_vias_front: true, tent_vias_back: false, paste_margin_nm: 0, paste_margin_ratio: 0 }, ...(over.rules ?? {}) },
  } as unknown as BoardState;
}

const via = (over: Partial<Via> = {}): Via => ({ id: "v1", net: "N", x: 10_000_000, y: 10_000_000, d: 800_000, drill: 400_000, from: "F.Cu", to: "B.Cu", ...over }) as Via;
const build = (b: BoardState, o: Partial<Layers3dOptions> = {}) => buildLayers3d(b, { layerOrder: b.layers }, { ...OPTS, ...o });
const segs = (list: readonly Prim[] | undefined) => (list ?? []).filter((p): p is Extract<Prim, { k: "seg" }> => p.k === "seg");

test("a track with a solder mask opening is on its side's mask, grown by its margin (createTrackWithMargin( track, .., GetSolderMaskExpansion() ))", () => {
  const t = { id: "t", net: "N", layer: "F.Cu", width: 250_000, pts: [[1_000_000, 1_000_000], [5_000_000, 1_000_000]], mask: { margin: null } } as Track;
  const plain = { ...t, id: "u", mask: null } as Track;
  const L = build(board({ tracks: [t, plain] }));
  const mask = segs(L.layers.get("F.Mask"));
  assert.equal(mask.length, 1, "only the track with the opening");
  assert.equal(mask[0]!.w, 250_000 + 2 * 50_000, "the board's expansion when it has none of its own");
  assert.equal(segs(L.layers.get("F.Cu")).length, 2, "both are copper");
  const own = build(board({ tracks: [{ ...t, mask: { margin: -200_000 } }] }));
  assert.equal(segs(own.layers.get("F.Mask"))[0]!.w, 0, "a negative margin no more than half the track");
});

test("an untented via opens its side's mask with the board's expansion; a tented one does not (PCB_VIA::IsOnLayer, IsTented)", () => {
  const L = build(board({ vias: [via()] }));
  assert.equal(primCounts(L.layers.get("F.Mask") ?? []).circle ?? 0, 0, "Board Setup tents the front");
  const back = (L.layers.get("B.Mask") ?? []).filter((p) => p.k === "circle");
  assert.equal(back.length, 1, "and not the back");
  assert.equal((back[0] as { r: number }).r, (800_000 + 2 * 50_000) / 2);
  const own = build(board({ vias: [via({ props: { front_tenting: false } })] }));
  assert.equal(primCounts(own.layers.get("F.Mask") ?? []).circle, 1, "the via's own setting first");
  assert.equal(viaTented(via({ props: { back_tenting: true } }), false, { tent_vias_front: true, tent_vias_back: false }), true);
});

test("the through holes cull with half the plating; a blind via's hole is its layers' own (m_TH_ODs, m_layerHoleMap)", () => {
  const b = board({ vias: [via(), via({ id: "v2", type: "blind", to: "In1.Cu", x: 20_000_000 })] });
  (b as { layers: string[] }).layers = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
  const L = build(b);
  assert.equal(L.thODs.length, 1);
  assert.equal((L.thODs[0] as { r: number }).r, 200_000 + 10_000);
  assert.equal(L.layerHoles.get("F.Cu")?.length, 1);
  assert.equal(L.layerHoles.get("In1.Cu")?.length, 1);
  assert.equal(L.layerHoles.get("B.Cu")?.length ?? 0, 0);
  assert.equal(L.viaAnnuli.length, 0, "the annuli only with Clip silkscreen at via annuli");
  assert.equal(build(b, { clipSilkOnViaAnnuli: true }).viaAnnuli.length, 1);
});

test("a pad's hole: plated grown by half the plating, a non-plated one as drilled; both cull the silkscreen", () => {
  const pad = (over: Partial<Pad>): Pad => ({ num: "1", net: null, x: 5_000_000, y: 5_000_000, w: 2_000_000, h: 2_000_000, round: true, th: true, shape: "circle", size: [2_000_000, 2_000_000], drill: 1_000_000, kind: "through_hole", ...over }) as Pad;
  const part = { ref: "J1", placed: true, side: "top", rot: 0, pads: [pad({}), pad({ num: "2", x: 8_000_000, kind: "non_plated_hole" })] } as unknown as Part;
  const L = build(board({ parts: [part] }));
  const widths = segs(L.thODs).map((s) => s.w).sort((a, b) => a - b);
  assert.deepEqual(widths, [1_000_000, 1_000_000 + 2 * 10_000]);
});

test("a shape on outer copper with a solder mask opening is on the mask grown by its expansion; a dashed silkscreen line is drawn in its dashes", () => {
  const seg = { id: "s", kind: "segment", layer: "F.Cu", stroke_width: 200_000, filled: false, start: [0, 0], end: [10_000_000, 0], solder_mask: true } as Shape;
  const dashed = { id: "d", kind: "segment", layer: "F.SilkS", stroke_width: 150_000, filled: false, start: [0, 2_000_000], end: [10_000_000, 2_000_000], dash: "dash" } as Shape;
  const L = build(board({ shapes: [seg, dashed] }));
  assert.equal(segs(L.layers.get("F.Mask"))[0]!.w, 200_000 + 2 * 50_000);
  assert.equal(segs(L.layers.get("F.Cu"))[0]!.w, 200_000);
  const silk = segs(L.layers.get("F.SilkS"));
  // a dash is ( 12 - 1 ) widths and a gap ( 3 + 1 ): 10 mm of a 0.15 mm line is 5 dashes and a bit
  assert.ok(silk.length >= 5 && silk.every((s) => s.w === 150_000), `${silk.length} pieces`);
});

test("a counterbored via: its hole in every layer, the body's cut and the plug under it (createLayers, backfillPostMachine)", () => {
  const v = via({ props: { front_post_machining: { mode: "counterbore", size: 1_000_000, depth: 300_000, angle_ddeg: 0 } } });
  const L = build(board({ vias: [v] }));
  assert.equal(L.bodyCuts.length, 1);
  assert.equal((L.layerHoles.get("F.Cu")?.[0] as { r: number }).r, 500_000 + 10_000);
  assert.equal(L.plugs.length, 1);
  assert.deepEqual(L.plugs[0]!.top, { layer: "F.Cu", face: "top", depth: 300_000 });
  assert.ok(L.barrelVias.has("v1") && L.barrels.length === 2, "the counterbore's plating and the barrel below it");
  assert.equal(L.covers.front.length, 0, "a post-machined side has no cover");
});

test("a custom pad's paste is its outline Inflated by the margin, the server's job; the text of a knockout is a box less its strokes", () => {
  const outline: [number, number][][] = [[[0, 0], [1_000_000, 0], [1_000_000, 1_000_000], [0, 1_000_000]]];
  const pad = { num: "1", net: null, x: 500_000, y: 500_000, w: 1_000_000, h: 1_000_000, round: false, th: false, shape: "custom", size: [500_000, 500_000], kind: "smd", outline, paste_margin: -50_000 } as unknown as Pad;
  const part = { ref: "U1", placed: true, side: "top", rot: 0, pads: [pad], fields: [] } as unknown as Part;
  const L = build(board({ parts: [part] }));
  const paste = L.layers.get("F.Paste") ?? [];
  assert.equal(paste.length, 1);
  assert.equal(paste[0]!.k, "job");
  const job = L.jobs.find((j) => paste[0]!.k === "job" && j.key === (paste[0] as { key: string }).key);
  assert.equal(job?.inflate, -50_000);
  const ko: FieldInfo = { id: "k", name: "Reference", text: "U1", x: 0, y: 0, angle: 0, w: 1_000_000, h: 1_000_000, thickness: 150_000, layer: "F.SilkS", visible: true, halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: false, knockout: true, lx: 0, ly: 0, langle: 0, custom: false } as FieldInfo;
  const K = build(board({ parts: [{ ...part, pads: [], fields: [ko] } as unknown as Part] }));
  const silk = K.layers.get("F.SilkS") ?? [];
  assert.equal(silk.length, 1);
  assert.equal(silk[0]!.tag, "part-ref");
  const kj = K.jobs[0]!;
  assert.ok(kj.a.length === 1 && (kj.b?.length ?? 0) > 0, "the box less the strokes");
});

test("with Off-board Silkscreen a footprint off the board has its pads drawn apart (m_offboardPadsFront)", () => {
  const pad = { num: "1", net: null, x: 60_000_000, y: 5_000_000, w: 1_000_000, h: 600_000, round: false, th: false, shape: "rect", size: [1_000_000, 600_000], kind: "smd" } as unknown as Pad;
  const on = { ...pad, x: 5_000_000 } as Pad;
  const parts = [{ ref: "R1", placed: true, side: "top", rot: 0, pads: [pad] }, { ref: "R2", placed: true, side: "top", rot: 0, pads: [on] }] as unknown as Part[];
  assert.equal(build(board({ parts })).offboardPads.front.length, 0, "only with the row on");
  const L = build(board({ parts }), { showOffBoardSilk: true });
  assert.equal(L.offboardPads.front.length, 1);
});

test("a pad is on a technical layer by its layers in its footprint's frame", () => {
  const pad = { layers: ["F.Cu", "F.Paste", "F.Mask"], th: false, kind: "smd" } as Pad;
  assert.equal(padOnTechLayer(pad, { side: "top" }, "F.Paste"), true);
  assert.equal(padOnTechLayer(pad, { side: "bottom" }, "B.Paste"), true);
  assert.equal(padOnTechLayer({ th: true, kind: "through_hole" } as Pad, { side: "top" }, "B.Mask"), true, "a hole opens both masks");
});

import { setFontResolver, STROKE, type OutlineFace } from "./kicadText";
import type { BoardTextBox } from "../api/pcbItemTypes";

/** A face whose every glyph is a unit square on the baseline, one em wide (and its ink that square). */
const SQUARE_FACE: OutlineFace = {
  family: "square",
  unitsPerEm: 1000,
  ascender: 800,
  descender: -200,
  height: 1000,
  fakeBold: false,
  fakeItalic: false,
  advance: (t) => Array.from(t).length,
  ink: (t) => (t.trim() ? [0, -1, Array.from(t).length, 0] : null),
  glyphs: (t) =>
    Array.from(t).flatMap((ch, i) => (ch === " " ? [] : [{ outer: [[i, 0], [i, -1], [i + 1, -1], [i + 1, 0]] as [number, number][], holes: [] }])),
};

function withFace<T>(face: OutlineFace, fn: () => T): T {
  setFontResolver(() => ({ kind: "outline", face }));
  try {
    return fn();
  } finally {
    setFontResolver(() => STROKE);
  }
}

const text = (over: Record<string, unknown> = {}) => ({ id: "t1", content: "AB", x: 10_000_000, y: 10_000_000, angle: 0, layer: "F.SilkS", size: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false, ...over });

test("an outline font's text is its glyphs' polygons (addText's triangulation callback), the ink box only while the face's outlines are not read", () => {
  const b = board({ texts: [text({ face: "Square" })] });
  const L = withFace(SQUARE_FACE, () => build(b));
  const polys = (L.layers.get("F.SilkS") ?? []).filter((p): p is Extract<Prim, { k: "poly" }> => p.k === "poly" && p.tag === "text");
  assert.equal(polys.length, 2, "one per glyph");
  for (const p of polys) assert.equal(p.contours[0]!.length, 4);
  const noGlyphs = { ...SQUARE_FACE, glyphs: undefined };
  const boxes = withFace(noGlyphs, () => build(b));
  assert.equal((boxes.layers.get("F.SilkS") ?? []).filter((p) => p.k === "poly" && p.tag === "text").length, 1, "the run's ink box");
  // a knockout text asks the server for its box less the glyphs
  const ko = withFace(SQUARE_FACE, () => build(board({ texts: [text({ face: "Square", knockout: true })] })));
  assert.equal(ko.jobs.length, 1);
  assert.equal(ko.jobs[0]!.b?.length, 2, "the two glyphs are cut out");
});

test("the board's dimensions are drawn: their text and shapes (addShape( PCB_DIMENSION_BASE ): segments of the line width, circles as rings)", () => {
  const dim = { id: "d1", layer: "Dwgs.User", kind: "leader", stroke_width: 100_000, lines: [[[0, 0], [5_000_000, 0]], [[5_000_000, 0], [5_000_000, 3_000_000]]], circles: [[[8_000_000, 0], 1_000_000]], text: "12", text_at: [2_000_000, -1_000_000], computed_text_angle: 0, text_size_nm: 1_000_000 };
  const b = board();
  (b.drawings as { dimensions: unknown[] }).dimensions = [dim];
  const list = build(b).layers.get("Dwgs.User") ?? [];
  const mine = list.filter((p) => p.tag === "dimension");
  assert.equal(mine.filter((p) => p.k === "seg" && p.w === 100_000).length, 2, "its two lines");
  assert.deepEqual(mine.filter((p) => p.k === "ring").map((p) => [(p as { ri: number }).ri, (p as { ro: number }).ro]), [[950_000, 1_050_000]]);
  assert.ok(mine.filter((p) => p.k === "seg" && p.w !== 100_000).length > 2, "and its text's strokes");
});

test("the board's tables are drawn: DrawBorders' lines and each cell's text (addTable)", () => {
  const cell = { id: "tb:0:0", row: 0, col: 0, text: "X", col_span: 1, row_span: 1, corners: [[0, 0], [4_000_000, 0], [4_000_000, 2_000_000], [0, 2_000_000]], margins: [0, 0, 0, 0], h_align: "left", v_align: "top", size_w: 1_000_000, size_h: 1_000_000, thickness: 0, pen: 150_000, bold: false, italic: false, mirror: false, face: null, line_spacing: 1000, color: null, lines: [{ text: "X", x: 200_000, y: 1_200_000 }] };
  const covered = { ...cell, id: "tb:0:1", col_span: 0, lines: [{ text: "Y", x: 4_200_000, y: 1_200_000 }] };
  const table = { id: "tb", layer: "F.SilkS", angle: 0, borders: [{ a: [0, 0], b: [8_000_000, 0], width: 120_000, style: "solid", color: null }, { a: [0, 0], b: [0, 2_000_000], width: -1, style: "solid", color: null }], cells: [cell, covered] };
  const b = board();
  (b.drawings as { tables?: unknown[] }).tables = [table];
  const mine = (build(b).layers.get("F.SilkS") ?? []).filter((p) => p.tag === "table");
  assert.equal(mine.filter((p) => p.k === "seg" && p.w === 120_000).length, 1, "the border that has a width");
  const strokes = mine.filter((p) => p.k === "seg" && p.w === 150_000);
  assert.ok(strokes.length >= 2 && strokes.every((p) => (p as { a: number[] }).a[0]! < 4_000_000), "the X, not the covered cell's Y");
});

const textBox = (over: Partial<BoardTextBox> = {}): BoardTextBox =>
  ({ id: "x1", layer: "F.SilkS", text: "T", start: [0, 0], end: [6_000_000, 3_000_000], pts: [], angle: 0, margins: [250_000, 250_000, 250_000, 250_000], border: true, stroke_width: 100_000, line_style: "solid", stroke_color: null, knockout: false, size: 1_000_000, width: 1_000_000, thickness: 150_000, thickness_auto: false, halign: "left", valign: "top", mirror: false, bold: false, italic: false, line_spacing_permille: 1000, face: null, ...over }) as BoardTextBox;

test("a text box is its text and its border (addShape( PCB_TEXTBOX )), on any layer -- copper too; a knockout box of the board is its box less the text", () => {
  const b = board();
  (b.drawings as { text_boxes?: unknown[] }).text_boxes = [textBox(), textBox({ id: "x2", layer: "F.Cu" }), textBox({ id: "x3", layer: "B.Cu", knockout: true })];
  const L = build(b);
  const silk = (L.layers.get("F.SilkS") ?? []).filter((p) => p.tag === "text-box");
  assert.equal(silk.filter((p) => p.k === "seg" && p.w === 100_000).length, 4, "the border's four sides");
  assert.ok(silk.filter((p) => p.k === "seg" && p.w === 150_000).length >= 2, "the T's strokes");
  assert.ok((L.layers.get("F.Cu") ?? []).some((p) => p.tag === "text-box"), "a box on copper is drawn");
  const back = (L.layers.get("B.Cu") ?? []).filter((p) => p.tag === "text-box");
  const jobKey = back.find((p) => p.k === "job") as { key: string } | undefined;
  assert.ok(jobKey, "the knockout is the server's subtraction");
  const job = L.jobs.find((j) => j.key === jobKey!.key)!;
  assert.equal(job.a.length, 5, "the box and its four border ovals");
  assert.ok((job.b?.length ?? 0) >= 2, "less the T's strokes");
  // a footprint's text box is the footprint's: never a knockout
  const part = { ref: "U1", placed: true, side: "top", rot: 0, pads: [], items: { text_boxes: [textBox({ id: "x4", layer: "F.Cu", knockout: true })] } } as unknown as Part;
  const fp = build(board({ parts: [part] }));
  assert.ok((fp.layers.get("F.Cu") ?? []).filter((p) => p.tag === "text-box").every((p) => p.k !== "job"));
  // a dashed border is dashed
  const dashed = board();
  (dashed.drawings as { text_boxes?: unknown[] }).text_boxes = [textBox({ line_style: "dash" })];
  assert.ok((build(dashed).layers.get("F.SilkS") ?? []).filter((p) => p.k === "seg" && p.w === 100_000).length > 4);
});

test("a line with endings is shortened and its endings drawn (KiCad 25ec2335 addShape: ShortenSegmentForEndings, LINE_ENDING::GetShapes)", () => {
  const arrow = { style: "arrow", length: 0, width: 0, stroke_width: 0 };
  const line = { id: "l1", kind: "segment", layer: "F.SilkS", stroke_width: 100_000, filled: false, start: [0, 0], end: [10_000_000, 0], end_ending: arrow } as unknown as Shape;
  const list = (build(board({ shapes: [line] })).layers.get("F.SilkS") ?? []).filter((p) => p.tag === "shape");
  const body = list.filter((p): p is Extract<Prim, { k: "seg" }> => p.k === "seg");
  assert.equal(body.length, 1);
  assert.ok(body[0]!.b[0] < 10_000_000, `the body stops short of the arrow's tip: ${body[0]!.b[0]}`);
  const head = list.filter((p) => p.k === "poly");
  assert.equal(head.length, 1, "the arrowhead, filled");
  const open = { ...line, end_ending: { ...arrow, style: "arrow_open" } } as unknown as Shape;
  const openList = (build(board({ shapes: [open] })).layers.get("F.SilkS") ?? []).filter((p) => p.tag === "shape");
  assert.equal(openList.filter((p) => p.k === "poly").length, 0, "an open arrow is segments");
  assert.ok(openList.filter((p) => p.k === "seg").length >= 3);
});

test("KiCad 25ec2335's fab and courtyard layers are built when their rows are on", () => {
  const fab = { id: "f1", kind: "segment", layer: "F.Fab", stroke_width: 100_000, filled: false, start: [0, 0], end: [1_000_000, 0] } as Shape;
  const crt = { id: "c1", kind: "segment", layer: "B.CrtYd", stroke_width: 50_000, filled: false, start: [0, 0], end: [1_000_000, 0] } as Shape;
  const b = board({ shapes: [fab, crt] });
  const on = build(b);
  assert.equal(segs(on.layers.get("F.Fab")).length, 1);
  assert.equal(segs(on.layers.get("B.CrtYd")).length, 1);
  const off = build(b, { layerShown: (l) => !/Fab|CrtYd/.test(l) });
  assert.equal(off.layers.has("F.Fab"), false);
});

test("the custom rules' mask expansion and paste margin open the masks and lay the paste (PAD::GetSolderMaskExpansion's EvalRules, from GET /api/board3d)", () => {
  const smd = (num: string, x: number): Pad => ({ num, net: null, x, y: 5_000_000, w: 1_000_000, h: 1_000_000, round: false, th: false, shape: "rect", size: [1_000_000, 1_000_000], kind: "smd" }) as Pad;
  const part = { ref: "U1", placed: true, side: "top", rot: 0, pads: [smd("1", 5_000_000), smd("1", 8_000_000)] } as unknown as Part;
  const b = board({ parts: [part] });
  const xs = (list: readonly Prim[] | undefined) => (list ?? []).filter((p) => p.k === "poly").map((p) => {
    const c = (p as { contours: [number, number][][] }).contours[0]!;
    return Math.max(...c.map((q) => q[0])) - Math.min(...c.map((q) => q[0]));
  });
  assert.deepEqual(xs(build(b).layers.get("F.Mask")), [1_100_000, 1_100_000], "the board's 0.05 mm without rules");
  const ruleMargins = {
    mask: true,
    paste_abs: true,
    paste_rel: false,
    pads: { "U1.1": { mask: [200_000, 0], abs: [-100_000, null], rel: null }, "U1.1#2": { mask: [null, null], abs: [null, null], rel: null } },
    tracks: {},
    shapes: {},
    fp_shapes: {},
  };
  const L = buildLayers3d(b, { layerOrder: b.layers, ruleMargins: ruleMargins as never }, OPTS);
  assert.deepEqual(xs(L.layers.get("F.Mask")), [1_400_000, 1_000_000], "the rule's 0.2 mm on the first pad; the second's none is 0 (value_or( 0 ))");
  assert.deepEqual(xs(L.layers.get("F.Paste")), [800_000, 1_000_000], "the rule's -0.1 mm paste; none falls back to the board's 0");
});
