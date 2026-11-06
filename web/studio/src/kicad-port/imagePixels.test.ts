import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Cmd, Part } from "../api/types";
import type { BoardRefImage } from "../api/pcbItemTypes";
import { boardImageTurns, compose, encodedFormat, imageQuarterTurns, isIdentity, mapPixel, netMatrix, sheetImageTurns, sheetSteps, sheetTurnsOf, stepMatrix, transformedSize, withJpegPpi } from "./imagePixels";
import { imageHeader } from "./refImage";

test("a turn rounds to quarter turns as REFERENCE_IMAGE::Rotate's loop does", () => {
  // 45 < norm, 135 < norm ...: an eighth goes down
  assert.deepEqual([0, 45, 46, 90, 135, 136, 180, 270, 359, 360, -90].map(imageQuarterTurns), [0, 0, 1, 1, 1, 2, 2, 3, 0, 0, 3]);
});

test("the steps compose as the pixels are moved, a quarter turn counter-clockwise taking the right to the top", () => {
  // a 4 x 2 image, the pixel at the top right (3, 0)
  const ccw = stepMatrix({ turnCcw: 1 });
  assert.deepEqual(transformedSize(ccw, 4, 2), [2, 4]);
  assert.deepEqual(mapPixel(ccw, 4, 2, 3, 0), [0, 0], "turned counter-clockwise, the top right corner is the top left");
  assert.deepEqual(mapPixel(stepMatrix({ turnCcw: 3 }), 4, 2, 3, 0), [1, 3], "turned clockwise, it is the bottom right");
  assert.deepEqual(mapPixel(stepMatrix({ mirror: "left_right" }), 4, 2, 3, 0), [0, 0]);
  assert.deepEqual(mapPixel(stepMatrix({ mirror: "top_bottom" }), 4, 2, 3, 0), [3, 1]);
  // four quarters, and a mirror twice, are nothing
  assert.ok(isIdentity(netMatrix([{ turnCcw: 1 }, { turnCcw: 1 }, { turnCcw: 2 }])));
  assert.ok(isIdentity(netMatrix([{ mirror: "top_bottom" }, { mirror: "top_bottom" }])));
  // FOOTPRINT::Flip's top-bottom flip and half turn are a left-right mirror
  assert.deepEqual(netMatrix([{ mirror: "top_bottom" }, { turnCcw: 2 }]), stepMatrix({ mirror: "left_right" }));
  // a turn then a mirror is not a mirror then a turn
  assert.notDeepEqual(compose(stepMatrix({ mirror: "left_right" }), stepMatrix({ turnCcw: 1 })), compose(stepMatrix({ turnCcw: 1 }), stepMatrix({ mirror: "left_right" })));
});

const image = (id: string): BoardRefImage => ({ id, layer: "F.SilkS", x: 0, y: 0, w: 1, h: 1, scale_ppm: 1_000_000, px: [4, 2], ppi: 300, hash: `h-${id}` });
const part = (ref: string, images: BoardRefImage[]): Part => ({ ref, placed: true, items: { images } }) as unknown as Part;

test("a batch turns the board's images and the images of the footprints it turns, each by what the whole batch does", () => {
  const board = { parts: [part("U1", [image("U1~img_aaaaaaaaaaaa")]), part("U2", [image("U2~img_bbbbbbbbbbbb")])], drawings: { images: [image("img_cccccccccccc")], groups: [{ id: "g1", name: "", member_ids: ["U2"] }] } } as unknown as BoardState;
  const pivot = { x: 0, y: 0 };
  // R on U1 and the board's image: a clockwise-positive -90000 is a quarter counter-clockwise
  let turns = boardImageTurns(board, [{ op: "rotate_items", ids: ["U1", "img_cccccccccccc"], pivot, angle_millideg: -90_000 }]);
  assert.deepEqual(turns.map((t) => [t.id, t.hash, t.steps]), [
    ["U1~img_aaaaaaaaaaaa", "h-U1~img_aaaaaaaaaaaa", [{ turnCcw: 1 }]],
    ["img_cccccccccccc", "h-img_cccccccccccc", [{ turnCcw: 1 }]],
  ]);
  // F of a group holding U2: the footprint's flip (top-bottom, then half a turn); a board image is mirrored the way the flip goes
  turns = boardImageTurns(board, [{ op: "flip_items", ids: ["g1", "img_cccccccccccc"], pivot, direction: "left_right" }]);
  assert.deepEqual(turns.find((t) => t.id === "U2~img_bbbbbbbbbbbb")?.steps, [{ mirror: "top_bottom" }, { turnCcw: 2 }]);
  assert.deepEqual(turns.find((t) => t.id === "img_cccccccccccc")?.steps, [{ mirror: "left_right" }]);
  // a move turns nothing; a batch whose turns add up to a whole turn leaves the pixels alone
  assert.deepEqual(boardImageTurns(board, [{ op: "move_items", ids: ["U1"], dx: 1, dy: 1 }] as Cmd[]), []);
  const half: Cmd = { op: "rotate_items", ids: ["U1"], pivot, angle_millideg: 180_000 };
  assert.deepEqual(boardImageTurns(board, [{ op: "batch", cmds: [half, half] }]), []);
});

test("the sheet's turns and mirrors of an image are SCH_BITMAP's", () => {
  assert.deepEqual(sheetSteps("rot_ccw"), [{ turnCcw: 1 }]);
  assert.deepEqual(sheetSteps("rot_cw"), [{ turnCcw: 3 }], "ANGLE_270");
  assert.deepEqual(sheetSteps("mirror_h"), [{ mirror: "left_right" }]);
  assert.deepEqual(sheetSteps("mirror_v"), [{ mirror: "top_bottom" }]);
  assert.deepEqual(sheetTurnsOf({ op: "sch_move", verb: "rotate", ids: ["a"], ccw: false } as Cmd), ["rot_cw"]);
  assert.deepEqual(sheetTurnsOf({ op: "sch_move", verb: "mirror", ids: ["a"], vertical: true } as Cmd), ["mirror_v"]);
  assert.deepEqual(sheetTurnsOf({ op: "sch_move", verb: "move", ids: ["a"], dx: 0, dy: 0, turns: ["rot_ccw", "rot_ccw"] } as Cmd), ["rot_ccw", "rot_ccw"]);
  const graphics = [{ id: "img_1", shape: { type: "image", hash: "h1" } }, { id: "rect_1", shape: { type: "rect" } }];
  assert.deepEqual(sheetImageTurns(graphics, ["img_1", "rect_1"], ["rot_ccw"]), [{ id: "img_1", hash: "h1", steps: [{ turnCcw: 1 }] }]);
  assert.deepEqual(sheetImageTurns(graphics, ["img_1"], ["mirror_h", "mirror_h"]), []);
});

test("a JPEG keeps its definition and anything but a JPEG becomes a PNG", () => {
  const jfif = Uint8Array.from([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xc0, 0, 11, 8, 0, 2, 0, 4, 1, 1, 0x11, 0]);
  assert.equal(imageHeader(jfif)?.ppi, 300, "an aspect ratio says no definition");
  const kept = withJpegPpi(jfif, 96);
  assert.deepEqual([imageHeader(kept)?.ppi, imageHeader(kept)?.px], [96, [4, 2]]);
  const bare = Uint8Array.from([0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 2, 0, 4, 1, 1, 0x11, 0]);
  assert.deepEqual([imageHeader(withJpegPpi(bare, 200))?.ppi, imageHeader(withJpegPpi(bare, 200))?.px], [200, [4, 2]], "an APP0 is put in front of a JPEG that has none");
  assert.deepEqual((["png", "jpeg", "gif", "bmp"] as const).map(encodedFormat), ["png", "jpeg", "png", "png"]);
});
