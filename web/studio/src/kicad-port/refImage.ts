// The bitmap of a reference image (`PCB_REFERENCE_IMAGE`) and of a schematic's image (`SCH_BITMAP`), seen from the browser: the size a file's pixels take at their definition and a scale,
// what the Image Properties dialogs let a scale be, and the header of a PNG, JPEG, GIF or BMP file (its size in pixels and its definition) -- read in the browser so the image can follow
// the cursor with its true size before the server has seen it. The server reads the same headers (`eda_model::image_info`) and decides what the board holds.
//
// Ported from `BITMAP_BASE::GetSize`, `GetScalingFactor`, `updatePPI`, `REFERENCE_IMAGE::updatePixelSizeInIU` (common/bitmap_base.cpp, common/reference_image.cpp),
// `PANEL_IMAGE_EDITOR::CheckValues` (common/dialogs/panel_image_editor.cpp) and `DIALOG_REFERENCE_IMAGE_PROPERTIES::onWidthChanged` at KiCad 8303b2ad.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

/** `(scale s)` is kept in millionths here: 1.0 is `SCALE_PPM`. */
export const SCALE_PPM = 1_000_000;
/** `MIN_SIZE` and `MAX_SIZE` of `PANEL_IMAGE_EDITOR::CheckValues`: the size in pixels a scaled image may have (50 mils, 20 inches, at 300 pixels per inch). */
export const MIN_IMAGE_PX = 15;
export const MAX_IMAGE_PX = 6000;
/** The definition of an image that says none (`BITMAP_BASE`'s `m_ppi`). */
export const DEFAULT_PPI = 300;

export type PxSize = readonly [number, number];

/** `BITMAP_BASE::GetSize`: the pixels at their definition (`REFERENCE_IMAGE::updatePixelSizeInIU`: an inch over the definition) times the scale, in micrometres. */
export function imageSizeUm(px: PxSize, ppi: number, scalePpm: number): [number, number] {
  const k = (25_400 / ppi) * (scalePpm / SCALE_PPM);
  return [Math.round(px[0] * k), Math.round(px[1] * k)];
}

/** The scale (in millionths) that makes the image `widthUm` wide (`DIALOG_REFERENCE_IMAGE_PROPERTIES::onWidthChanged`: the scale times the wanted width over the width now). */
export function scaleForWidthUm(px: PxSize, ppi: number, widthUm: number): number {
  return Math.round((widthUm / ((px[0] * 25_400) / ppi)) * SCALE_PPM);
}

/** The scale (in millionths) that makes the image `heightUm` high (`onHeightChanged`). */
export function scaleForHeightUm(px: PxSize, ppi: number, heightUm: number): number {
  return Math.round((heightUm / ((px[1] * 25_400) / ppi)) * SCALE_PPM);
}

/** A scale typed as a decimal number, in millionths; NaN when it is not a number. */
export function scalePpmOf(text: string): number {
  return Math.round(Number(text) * SCALE_PPM);
}

/** A scale in millionths as the decimal the dialog shows (no trailing zeros). */
export function scaleText(scalePpm: number): string {
  return String(scalePpm / SCALE_PPM);
}

export type ScaleCheck = { ok: true; confirm?: string } | { ok: false; error: string };

/**
 * `PANEL_IMAGE_EDITOR::CheckValues`: a scale must be a positive number and give an image of at least 15 pixels (the smaller side, "mandatory to be able to see the image"); one over 6000
 * pixels asks first ("This scale results in an image which is very large"). The sizes the messages give are at the 300 pixels per inch the checks assume.
 */
export function checkImageScale(px: PxSize, scalePpm: number): ScaleCheck {
  if (!(scalePpm >= 0)) return { ok: false, error: "Scale must be a positive number." };
  const scale = scalePpm / SCALE_PPM;
  const sizeMin = Math.trunc(Math.min(px[0] * scale, px[1] * scale));
  if (sizeMin < MIN_IMAGE_PX) return { ok: false, error: `This scale results in an image which is too small (${((25.4 / 300) * sizeMin).toFixed(2)} mm or ${((1000 / 300) * sizeMin).toFixed(1)} mil).` };
  const sizeMax = Math.trunc(Math.max(px[0] * scale, px[1] * scale));
  if (sizeMax > MAX_IMAGE_PX) return { ok: true, confirm: `This scale results in an image which is very large (${((25.4 / 300) * sizeMax).toFixed(1)} mm or ${(sizeMax / 300).toFixed(2)} in). Are you sure?` };
  return { ok: true };
}

// --------------------------------------------------------------------------------------------------------------------------------------- the file

export type ImageFormat = "png" | "jpeg" | "gif" | "bmp";

/** The format a file's first bytes say it is (the magic numbers wxWidgets' handlers sniff), or null. */
export function sniffImage(b: Uint8Array): ImageFormat | null {
  if (b.length >= 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) return "png";
  if (b.length >= 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return "jpeg";
  if (b.length >= 6 && b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x38) return "gif";
  if (b.length >= 2 && b[0] === 0x42 && b[1] === 0x4d) return "bmp";
  return null;
}

export const MIME: Record<ImageFormat, string> = { png: "image/png", jpeg: "image/jpeg", gif: "image/gif", bmp: "image/bmp" };

export interface ImageHeader {
  format: ImageFormat;
  px: [number, number];
  ppi: number;
}

const u32 = (b: Uint8Array, at: number): number => ((b[at]! << 24) | (b[at + 1]! << 16) | (b[at + 2]! << 8) | b[at + 3]!) >>> 0;
const u16 = (b: Uint8Array, at: number): number => (b[at]! << 8) | b[at + 1]!;
const le32 = (b: Uint8Array, at: number): number => (b[at]! | (b[at + 1]! << 8) | (b[at + 2]! << 16) | (b[at + 3]! << 24)) >>> 0;
const le16 = (b: Uint8Array, at: number): number => b[at]! | (b[at + 1]! << 8);

/**
 * The size and definition a file says. A PNG's `pHYs` chunk is pixels per metre (wxWidgets turns it into pixels per centimetre and `updatePPI` into `KiROUND( dpiX * 2.54 )`); a JPEG's JFIF
 * density is dots per inch (unit 1) or per centimetre (unit 2); a file that says none (or a GIF or BMP) has `DEFAULT_PPI`.
 */
export function imageHeader(b: Uint8Array): ImageHeader | null {
  const format = sniffImage(b);
  if (!format) return null;
  if (format === "png") {
    // signature, then chunks: length, type, data, crc; IHDR first
    if (b.length < 33 || u32(b, 12) !== 0x49484452) return null;
    const px: [number, number] = [u32(b, 16), u32(b, 20)];
    let ppi = DEFAULT_PPI;
    let at = 8;
    while (at + 12 <= b.length) {
      const len = u32(b, at);
      const type = u32(b, at + 4);
      if (type === 0x70485973 /* pHYs */ && len >= 9 && at + 17 <= b.length) {
        const perMetre = u32(b, at + 8);
        if (b[at + 16] === 1 && perMetre / 100 > 1) ppi = Math.round((perMetre / 100) * 2.54);
        break;
      }
      if (type === 0x49444154 /* IDAT */ || type === 0x49454e44 /* IEND */) break;
      at += 12 + len;
    }
    return px[0] > 0 && px[1] > 0 ? { format, px, ppi } : null;
  }
  if (format === "gif") return b.length >= 10 && le16(b, 6) > 0 && le16(b, 8) > 0 ? { format, px: [le16(b, 6), le16(b, 8)], ppi: DEFAULT_PPI } : null;
  if (format === "bmp") {
    if (b.length < 26) return null;
    const [w, h] = [le32(b, 18), Math.abs(le32(b, 22) | 0)];
    return w > 0 && h > 0 ? { format, px: [w, h], ppi: DEFAULT_PPI } : null;
  }
  // JPEG: markers until a start of frame
  let ppi = DEFAULT_PPI;
  let at = 2;
  while (at + 4 <= b.length) {
    if (b[at] !== 0xff) {
      at++;
      continue;
    }
    const marker = b[at + 1]!;
    if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7) || marker === 0xff) {
      at += marker === 0xff ? 1 : 2;
      continue;
    }
    const len = u16(b, at + 2);
    if (marker === 0xe0 && len >= 16 && b[at + 4] === 0x4a && b[at + 5] === 0x46 && b[at + 6] === 0x49 && b[at + 7] === 0x46) {
      const unit = b[at + 11];
      const density = u16(b, at + 12);
      if (density > 0 && unit === 1) ppi = density;
      else if (density > 0 && unit === 2) ppi = Math.round(density * 2.54);
    }
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      const px: [number, number] = [u16(b, at + 7), u16(b, at + 5)];
      return px[0] > 0 && px[1] > 0 ? { format, px, ppi } : null;
    }
    at += 2 + len;
  }
  return null;
}

/** The standard base64 of `bytes`, with no line breaks. */
export function base64Of(bytes: Uint8Array): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i]! << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    out += chars[(n >> 18) & 63]! + chars[(n >> 12) & 63]! + (i + 1 < bytes.length ? chars[(n >> 6) & 63]! : "=") + (i + 2 < bytes.length ? chars[n & 63]! : "=");
  }
  return out;
}

/** The URL a board's reference image is served at; `hash` changes when its pixels do. */
export function pcbImageUrl(id: string, hash: string): string {
  return `/api/pcb_image?id=${encodeURIComponent(id)}&h=${encodeURIComponent(hash)}`;
}

/** The URL a schematic image is served at, on the sheet at `sheet` (the root when empty). */
export function schImageUrl(id: string, hash: string, sheet = ""): string {
  return `/api/sch_image?id=${encodeURIComponent(id)}&h=${encodeURIComponent(hash)}${sheet ? `&sheet=${encodeURIComponent(sheet)}` : ""}`;
}

// ------------------------------------------------------------------------------------------------------------------------------------- where it lies

export type ImageBox = [number, number, number, number];

/** `REFERENCE_IMAGE::GetBoundingBox`: `BOX2I::ByCenter( m_pos, size )` -- the box of the image centred on `at`, `[x0, y0, x1, y1]`, µm. */
export function imageBoxUm(at: readonly [number, number], px: PxSize, ppi: number, scalePpm: number): ImageBox {
  const [w, h] = imageSizeUm(px, ppi, scalePpm);
  const x0 = at[0] - Math.trunc(w / 2);
  const y0 = at[1] - Math.trunc(h / 2);
  return [x0, y0, x0 + w, y0 + h];
}

/** `KIGEOM::BoxHitTest( point, box, accuracy )`: inside the box, or within `accuracy` of it. */
export function imageBoxHitDistance(box: ImageBox, x: number, y: number): number {
  const dx = Math.max(box[0] - x, 0, x - box[2]);
  const dy = Math.max(box[1] - y, 0, y - box[3]);
  return Math.hypot(dx, dy);
}

// ------------------------------------------------------------------------------------------------------------------------------------- greyscale

/**
 * `wxImage::ConvertToGreyscale( 0.299, 0.587, 0.114 )` (what `BITMAP_BASE::ConvertToGreyscale` calls): every pixel's red, green and blue become the weighted sum of the three,
 * rounded; the alpha is left alone. `rgba` is a canvas's `ImageData.data`, changed in place.
 */
export function greyscaleRgba(rgba: Uint8ClampedArray): void {
  for (let i = 0; i + 3 < rgba.length; i += 4) {
    const v = Math.floor(0.299 * rgba[i]! + 0.587 * rgba[i + 1]! + 0.114 * rgba[i + 2]! + 0.5);
    rgba[i] = v;
    rgba[i + 1] = v;
    rgba[i + 2] = v;
  }
}

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC_TABLE[(c ^ b) & 0xff]! ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/**
 * The PNG `png` with a `pHYs` chunk that says `ppi` pixels per inch (replacing the one it has): a canvas encodes a PNG with no resolution, and an image without one is read at 300
 * pixels per inch, so a picture of another definition would change size.
 */
export function withPngPpi(png: Uint8Array, ppi: number): Uint8Array {
  if (sniffImage(png) !== "png") return png;
  const perMetre = Math.round(ppi / 0.0254);
  const chunk = new Uint8Array(21);
  const view = new DataView(chunk.buffer);
  view.setUint32(0, 9);
  chunk.set([0x70, 0x48, 0x59, 0x73], 4); // pHYs
  view.setUint32(8, perMetre);
  view.setUint32(12, perMetre);
  chunk[16] = 1;
  view.setUint32(17, crc32(chunk.subarray(4, 17)));
  // copy the chunks, dropping a pHYs, and put ours after IHDR (which is first and 25 bytes long with its signature)
  const out: number[] = Array.from(png.subarray(0, 33));
  out.push(...chunk);
  let at = 33;
  while (at + 12 <= png.length) {
    const len = u32(png, at);
    const isPhys = png[at + 4] === 0x70 && png[at + 5] === 0x48 && png[at + 6] === 0x59 && png[at + 7] === 0x73;
    if (!isPhys) for (let i = 0; i < 12 + len && at + i < png.length; i++) out.push(png[at + i]!);
    at += 12 + len;
  }
  return Uint8Array.from(out);
}
