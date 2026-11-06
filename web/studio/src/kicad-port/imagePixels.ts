// What Rotate and Flip do to the pixels of an image (UI-LEFT P17): `PCB_REFERENCE_IMAGE::Rotate` / `Flip` -> `REFERENCE_IMAGE::Rotate` / `Flip` -> `BITMAP_BASE::Rotate` /
// `Mirror` (common/reference_image.cpp, common/bitmap_base.cpp at KiCad 8303b2ad), and on the sheet `SCH_BITMAP::Rotate` / `MirrorHorizontally` / `MirrorVertically`
// (eeschema/sch_bitmap.cpp). KiCad turns the `wxImage` and writes it back as a PNG, a JPEG staying a JPEG (`BITMAP_BASE::updateImageDataBuffer`). The workspace has no image codec,
// so the browser does it (actions/imagePixels.ts) and sends the new file in the batch of the turn (`set_ref_image_data`, `set_image_data`): one undo step. This file is the plan:
// which images a batch turns, and how, as a net transform of the pixels.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, Cmd, Group, Part } from "../api/types";
import type { SchTurn } from "../api/schEditTypes";

/** One step: quarter turns counter-clockwise on the screen (`wxImage::Rotate90( !aRotateCCW )`), or a mirror (`mirrorImageInPlace`: LEFT_RIGHT swaps the columns, TOP_BOTTOM the rows). */
export type PixelStep = { turnCcw: number } | { mirror: "left_right" | "top_bottom" };

/**
 * `REFERENCE_IMAGE::Rotate`'s quarter turns for an angle (KiCad's, counter-clockwise, degrees): "each call to m_bitmapBase->Rotate() rotates 90 degrees", once for every `ang` of 45, 135,
 * 225 and 315 below the angle normalised to [0, 360) -- a turn rounds to the nearest quarter, an eighth going down.
 */
export function imageQuarterTurns(angleCcwDeg: number): number {
  const norm = ((angleCcwDeg % 360) + 360) % 360;
  let n = 0;
  for (let ang = 45; ang < norm - 1e-9; ang += 90) n++;
  return n % 4;
}

/** A transform of the plane of an image's pixels: `[a, b, c, d]` maps the offset `(x, y)` from the image's middle (y down) to `(a x + c y, b x + d y)`, a canvas transform's linear part. */
export type PixelMatrix = readonly [number, number, number, number];

export const IDENTITY: PixelMatrix = [1, 0, 0, 1];

/** The matrix of one step. A quarter turn counter-clockwise on the screen takes the right (1, 0) to the top (0, -1). */
export function stepMatrix(s: PixelStep): PixelMatrix {
  if ("mirror" in s) return s.mirror === "left_right" ? [-1, 0, 0, 1] : [1, 0, 0, -1];
  const n = ((s.turnCcw % 4) + 4) % 4;
  return ([IDENTITY, [0, -1, 1, 0], [-1, 0, 0, -1], [0, 1, -1, 0]] as const)[n]!;
}

/** `m` after `first`: the matrix of doing `first`, then `m`. */
export function compose(m: PixelMatrix, first: PixelMatrix): PixelMatrix {
  const [a, b, c, d] = m;
  const [e, f, g, h] = first;
  // (`+ 0` keeps a negative zero out of the matrix)
  return [a * e + c * f + 0, b * e + d * f + 0, a * g + c * h + 0, b * g + d * h + 0];
}

/** The net transform of `steps` done in order. */
export function netMatrix(steps: readonly PixelStep[]): PixelMatrix {
  return steps.reduce<PixelMatrix>((m, s) => compose(stepMatrix(s), m), IDENTITY);
}

export const isIdentity = (m: PixelMatrix): boolean => m[0] === 1 && m[1] === 0 && m[2] === 0 && m[3] === 1;

/** The size in pixels of the image a transform makes of one `w` x `h` (a quarter turn swaps them). */
export function transformedSize(m: PixelMatrix, w: number, h: number): [number, number] {
  return m[0] !== 0 ? [w, h] : [h, w];
}

/** Where pixel `(x, y)` of a `w` x `h` image goes (the centres of the pixels map onto each other): the reference the browser's drawing is checked against. */
export function mapPixel(m: PixelMatrix, w: number, h: number, x: number, y: number): [number, number] {
  const [W, H] = transformedSize(m, w, h);
  const ox = x + 0.5 - w / 2;
  const oy = y + 0.5 - h / 2;
  return [Math.round(m[0] * ox + m[2] * oy + W / 2 - 0.5), Math.round(m[1] * ox + m[3] * oy + H / 2 - 0.5)];
}

// --------------------------------------------------------------------------------------------------------------------------------------------- the board

/** The steps of a board verb on a board image: a turn's quarters (`rotate_items` turns clockwise, KiCad's angle is the other way), a flip's mirror (`PCB_REFERENCE_IMAGE::Flip`). */
function boardSteps(cmd: Cmd): PixelStep[] {
  if (cmd.op === "rotate_items") {
    const n = imageQuarterTurns(-cmd.angle_millideg / 1000);
    return n === 0 ? [] : [{ turnCcw: n }];
  }
  if (cmd.op === "flip_items") return [{ mirror: cmd.direction }];
  return [];
}

/**
 * The steps of a board verb on an image of a footprint it turns or flips: a turn is passed on to the footprint's items (`FOOTPRINT::SetOrientation`: `item->Rotate( .., angleChange )`); a flip
 * flips each item top to bottom and then, for a left-right flip, turns the whole footprint half a turn (`FOOTPRINT::Flip`).
 */
function footprintSteps(cmd: Cmd): PixelStep[] {
  if (cmd.op === "flip_items") return cmd.direction === "top_bottom" ? [{ mirror: "top_bottom" }] : [{ mirror: "top_bottom" }, { turnCcw: 2 }];
  return boardSteps(cmd);
}

/** Every id below the groups among `ids` (a group's members, a group's groups' members ...), and the ids themselves. */
function expandGroups(ids: readonly string[], groups: readonly Group[]): string[] {
  const byId = new Map(groups.map((g) => [g.id, g]));
  const out: string[] = [];
  const seen = new Set<string>();
  const walk = (id: string) => {
    if (seen.has(id)) return;
    seen.add(id);
    out.push(id);
    for (const m of byId.get(id)?.member_ids ?? []) walk(m);
  };
  ids.forEach(walk);
  return out;
}

/** An image a batch turns: its id (a footprint's is `<ref>~img_..`), where its file is served, and what is done to it. */
export interface ImageTurn {
  id: string;
  hash: string;
  steps: PixelStep[];
}

/** The images of the board that the `rotate_items` and `flip_items` of `cmds` turn or mirror, in the order the batch does it, each with its steps; an image whose steps add up to nothing is left out. */
export function boardImageTurns(board: Pick<BoardState, "parts" | "drawings">, cmds: readonly Cmd[]): ImageTurn[] {
  const flat = cmds.flatMap((c) => (c.op === "batch" ? c.cmds : [c]));
  if (!flat.some((c) => c.op === "rotate_items" || c.op === "flip_items")) return [];
  const images = new Map((board.drawings?.images ?? []).map((i) => [i.id, i]));
  const parts = new Map<string, Part>(board.parts.filter((p) => p.items?.images?.length).map((p) => [p.ref, p]));
  if (images.size === 0 && parts.size === 0) return [];
  const turns = new Map<string, ImageTurn>();
  const add = (id: string, hash: string, steps: PixelStep[]) => {
    if (steps.length === 0) return;
    const t = turns.get(id) ?? { id, hash, steps: [] };
    t.steps.push(...steps);
    turns.set(id, t);
  };
  for (const cmd of flat) {
    if (cmd.op !== "rotate_items" && cmd.op !== "flip_items") continue;
    for (const id of expandGroups(cmd.ids, board.drawings?.groups ?? [])) {
      const image = images.get(id);
      if (image) add(image.id, image.hash, boardSteps(cmd));
      for (const i of parts.get(id)?.items?.images ?? []) add(i.id, i.hash, footprintSteps(cmd));
    }
  }
  return [...turns.values()].filter((t) => !isIdentity(netMatrix(t.steps)));
}

// --------------------------------------------------------------------------------------------------------------------------------------------- the sheet

/** The steps of one turn of the sheet on an image: `SCH_BITMAP::Rotate( .., ccw )` is `REFERENCE_IMAGE::Rotate( .., ccw ? ANGLE_90 : ANGLE_270 )`, a mirror is `Flip`. */
export function sheetSteps(turn: SchTurn): PixelStep[] {
  switch (turn) {
    case "rot_ccw":
      return [{ turnCcw: imageQuarterTurns(90) }];
    case "rot_cw":
      return [{ turnCcw: imageQuarterTurns(270) }];
    case "mirror_h":
      return [{ mirror: "left_right" }];
    case "mirror_v":
      return [{ mirror: "top_bottom" }];
  }
}

/** The turns of a `sch_move` command: a `rotate` (`ccw`), a `mirror` (`vertical`), or the turns made while the items were held (`turns` of a move or a drag). */
export function sheetTurnsOf(cmd: Cmd): SchTurn[] {
  if (cmd.op !== "sch_move") return [];
  const c = cmd as { verb: string; ccw?: boolean; vertical?: boolean; turns?: SchTurn[] };
  if (c.verb === "rotate") return [c.ccw === false ? "rot_cw" : "rot_ccw"];
  if (c.verb === "mirror") return [c.vertical ? "mirror_v" : "mirror_h"];
  return c.turns ?? [];
}

/** The images among `ids` of the sheet's graphics (`graphics`) that `turns` turn or mirror, with their steps. */
export function sheetImageTurns(graphics: readonly { id: string; shape: { type: string; hash?: string } }[], ids: readonly string[], turns: readonly SchTurn[]): ImageTurn[] {
  if (turns.length === 0) return [];
  const steps = turns.flatMap(sheetSteps);
  if (isIdentity(netMatrix(steps))) return [];
  const wanted = new Set(ids);
  return graphics.filter((g) => g.shape.type === "image" && wanted.has(g.id)).map((g) => ({ id: g.id, hash: g.shape.hash ?? "", steps: [...steps] }));
}

// --------------------------------------------------------------------------------------------------------------------------------------------- the file

/**
 * The JPEG `jpeg` saying `ppi` pixels per inch in its JFIF header (a canvas's encoder writes an aspect ratio and no definition, and an image without one is read at 300, so a picture of
 * another definition would change size): the APP0 segment's units set to dots per inch and both densities to `ppi`, an APP0 put after the start of image when there is none.
 */
export function withJpegPpi(jpeg: Uint8Array, ppi: number): Uint8Array {
  if (jpeg.length < 4 || jpeg[0] !== 0xff || jpeg[1] !== 0xd8) return jpeg;
  const density = Math.max(1, Math.min(65535, Math.round(ppi)));
  const isJfif = jpeg.length >= 18 && jpeg[2] === 0xff && jpeg[3] === 0xe0 && jpeg[6] === 0x4a && jpeg[7] === 0x46 && jpeg[8] === 0x49 && jpeg[9] === 0x46 && jpeg[10] === 0;
  if (isJfif) {
    const out = jpeg.slice();
    out[13] = 1;
    out[14] = density >> 8;
    out[15] = density & 0xff;
    out[16] = density >> 8;
    out[17] = density & 0xff;
    return out;
  }
  const app0 = [0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 1, density >> 8, density & 0xff, density >> 8, density & 0xff, 0, 0];
  const out = new Uint8Array(jpeg.length + app0.length);
  out.set(jpeg.subarray(0, 2), 0);
  out.set(app0, 2);
  out.set(jpeg.subarray(2), 2 + app0.length);
  return out;
}

/** `BITMAP_BASE::updateImageDataBuffer`: a JPEG is written back as a JPEG, anything else as a PNG. */
export function encodedFormat(source: "png" | "jpeg" | "gif" | "bmp"): "png" | "jpeg" {
  return source === "jpeg" ? "jpeg" : "png";
}
