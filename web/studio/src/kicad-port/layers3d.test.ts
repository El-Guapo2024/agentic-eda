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
