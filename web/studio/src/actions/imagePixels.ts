// The browser's half of what Rotate and Flip do to an image's pixels (UI-LEFT P17; the plan is kicad-port/imagePixels.ts): the file decoded, drawn turned or mirrored on a canvas
// (`BITMAP_BASE::Rotate`, `Mirror`) and encoded again -- a PNG, or a JPEG for a JPEG (`BITMAP_BASE::updateImageDataBuffer`) -- with the definition the file had (`wxImage::Rotate90`
// "clears resolution metadata, so preserve it"), and the new file added to the batch of the turn so the turn and the pixels are one undo step. The server has no codec:
// a turn made without the browser (the CLI, an agent's command) moves the image and leaves its pixels as they were.
import type { BoardState, Cmd, Schematic } from "../api/types";
import { base64Of, imageHeader, MIME, pcbImageUrl, schImageUrl, withPngPpi } from "../kicad-port/refImage";
import { boardImageTurns, encodedFormat, isIdentity, netMatrix, sheetImageTurns, sheetTurnsOf, transformedSize, withJpegPpi, type PixelMatrix } from "../kicad-port/imagePixels";

/** The file at `url` turned by `m`, as base64; null when the browser cannot read or draw it. */
export async function turnedImageData(url: string, m: PixelMatrix): Promise<string | null> {
  try {
    const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
    const header = imageHeader(bytes);
    if (!header) return null;
    const bitmap = await createImageBitmap(new Blob([bytes], { type: MIME[header.format] }));
    const [w, h] = [bitmap.width, bitmap.height];
    const [W, H] = transformedSize(m, w, h);
    const canvas = document.createElement("canvas");
    canvas.width = W;
    canvas.height = H;
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.imageSmoothingEnabled = false;
    ctx.setTransform(m[0], m[1], m[2], m[3], W / 2, H / 2);
    ctx.drawImage(bitmap, -w / 2, -h / 2);
    bitmap.close();
    const format = encodedFormat(header.format);
    // wxImage's JPEG handler saves at quality 75 unless told otherwise
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, MIME[format], format === "jpeg" ? 0.75 : undefined));
    if (!blob) return null;
    const out = new Uint8Array(await blob.arrayBuffer());
    return base64Of(format === "jpeg" ? withJpegPpi(out, header.ppi) : withPngPpi(out, header.ppi));
  } catch {
    return null;
  }
}

/**
 * `cmds` with the pixels of every board image they turn or flip (the board's own and the footprints', kicad-port/imagePixels.ts `boardImageTurns`) sent after them: one `set_ref_image_data`
 * per image, the file turned by what the whole batch does to it. An image the browser cannot redraw keeps its pixels.
 */
export async function withBoardImagePixels(board: BoardState | null | undefined, cmds: Cmd[]): Promise<Cmd[]> {
  if (!board) return cmds;
  const turns = boardImageTurns(board, cmds);
  if (turns.length === 0) return cmds;
  const datas = await Promise.all(turns.map((t) => turnedImageData(pcbImageUrl(t.id, t.hash), netMatrix(t.steps))));
  const extra: Cmd[] = [];
  turns.forEach((t, i) => {
    const data = datas[i];
    if (data) extra.push({ op: "pcb_item", verb: "set_ref_image_data", id: t.id, data });
  });
  return extra.length > 0 ? [...cmds, ...extra] : cmds;
}

/** A sheet's `sch_move` command with the pixels of the images it turns or mirrors (`SCH_BITMAP::Rotate`, `MirrorHorizontally`, `MirrorVertically`) sent after it, as one batch. */
export async function withSheetImagePixels(sch: Schematic | null | undefined, cmd: Cmd, sheetPath: readonly string[]): Promise<Cmd> {
  const turns = sheetTurnsOf(cmd);
  if (!sch || turns.length === 0 || cmd.op !== "sch_move") return cmd;
  const ids = (cmd as { ids?: string[] }).ids ?? [];
  const images = sheetImageTurns((sch.graphics ?? []) as { id: string; shape: { type: string; hash?: string } }[], ids, turns);
  if (images.length === 0) return cmd;
  const sheet = sheetPath.join("/");
  const datas = await Promise.all(images.map((t) => (isIdentity(netMatrix(t.steps)) ? null : turnedImageData(schImageUrl(t.id, t.hash, sheet), netMatrix(t.steps)))));
  const extra: Cmd[] = [];
  images.forEach((t, i) => {
    const data = datas[i];
    if (data) extra.push({ op: "sch_edit", verb: "set_image_data", id: t.id, data });
  });
  return extra.length > 0 ? { op: "batch", cmds: [cmd, ...extra] } : cmd;
}
