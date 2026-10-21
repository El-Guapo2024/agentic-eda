// The image files the studio takes in: Place Reference Image and Place Image choose one in a file chooser of the browser (`wxFileDialog` with `FILEEXT::ImageFileWildcard()`), a file
// dropped on the schematic is one (`ddAddImage`), and the Image Properties dialogs convert one to greyscale. A file becomes the base64 text the commands carry (`REFERENCE_IMAGE::ReadImageFile`
// takes the bytes as they are and `BITMAP_BASE::SaveImageData` writes them back), after its header has been read (kicad-port/refImage.ts) and the browser has decoded it.
import { loadImage } from "../components/canvas/imageCache";
import { base64Of, DEFAULT_PPI, greyscaleRgba, imageHeader, MIME, withPngPpi, type ImageHeader } from "../kicad-port/refImage";

/** The most a file may be: the studio's server reads 64 MB bodies, and base64 is a third larger. */
export const MAX_IMAGE_BYTES = 40 << 20;

/** What the `accept` of the chooser lists: the formats KiCad's image handlers are used for here. */
export const IMAGE_ACCEPT = ".png,.jpg,.jpeg,.bmp,.gif,image/png,image/jpeg,image/bmp,image/gif";

/** An image file as the actions take it: its name and its bytes in base64 (no line breaks). */
export interface PickedImage {
  name: string;
  data: string;
}

/** A picked image the browser has decoded: what the cursor holds while it is placed. */
export interface LoadedImage extends PickedImage {
  header: ImageHeader;
  img: HTMLImageElement;
}

/** An action's parameter that is a picked image (a drop, or a test that does not open a chooser), or null. */
export function asPickedImage(param: unknown): PickedImage | null {
  if (!param || typeof param !== "object") return null;
  const p = param as Partial<PickedImage>;
  return typeof p.name === "string" && typeof p.data === "string" && p.data.length > 0 ? { name: p.name, data: p.data } : null;
}

/** The message KiCad shows when a file cannot be read (`_( "Could not load image from '%s'." )`). */
export const couldNotLoad = (name: string): string => `Could not load image from '${name}'.`;

/** A browser "Choose Image" chooser: the file chosen, or null when it is cancelled. */
export function chooseImageFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = IMAGE_ACCEPT;
    input.style.display = "none";
    let settled = false;
    const done = (f: File | null) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(f);
    };
    input.addEventListener("change", () => done(input.files?.[0] ?? null));
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/** The file read: its bytes in base64. Throws the message of `couldNotLoad` for a file that is not an image of a kind known (or too big). */
export async function readImageFile(file: File): Promise<PickedImage> {
  if (file.size > MAX_IMAGE_BYTES) throw new Error(`${file.name} is larger than 40 MB, which the studio will not embed in a board.`);
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (!imageHeader(bytes)) throw new Error(couldNotLoad(file.name));
  return { name: file.name, data: base64Of(bytes) };
}

/** The data URL of a picked image, for the browser to decode and draw. */
export function dataUrlOf(p: PickedImage): string {
  const header = headerOf(p.data);
  return `data:${header ? MIME[header.format] : "application/octet-stream"};base64,${p.data}`;
}

/** The bytes of base64 text. */
export function bytesOfBase64(data: string): Uint8Array {
  const bin = atob(data);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** The header of the file whose base64 is `data` (a prefix of it is enough: a header is in the first bytes). */
export function headerOf(data: string): ImageHeader | null {
  for (const chars of [2_048, 32_768, 524_288, Infinity]) {
    const prefix = Number.isFinite(chars) ? data.slice(0, chars - (chars % 4)) : data;
    try {
      const h = imageHeader(bytesOfBase64(prefix));
      if (h) return h;
    } catch {
      return null;
    }
    if (prefix.length >= data.length) break;
  }
  return null;
}

/** A picked image decoded, with its header; throws `couldNotLoad` when the browser cannot show it. */
export async function loadPickedImage(p: PickedImage): Promise<LoadedImage> {
  const header = headerOf(p.data);
  if (!header) throw new Error(couldNotLoad(p.name));
  try {
    return { ...p, header, img: await loadImage(dataUrlOf(p)) };
  } catch {
    throw new Error(couldNotLoad(p.name));
  }
}

/**
 * `BITMAP_BASE::ConvertToGreyscale`: the image at `url` with every pixel grey, as the base64 of a PNG that says the definition `ppi` (a canvas writes none, and a file without is read at 300).
 * wxImage keeps the alpha, so does this.
 */
export async function greyscalePng(url: string, ppi: number = DEFAULT_PPI): Promise<string> {
  const img = await loadImage(url);
  const canvas = document.createElement("canvas");
  canvas.width = img.naturalWidth;
  canvas.height = img.naturalHeight;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("The image could not be converted.");
  ctx.drawImage(img, 0, 0);
  const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height);
  greyscaleRgba(pixels.data);
  ctx.putImageData(pixels, 0, 0);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("The image could not be converted.");
  return base64Of(withPngPpi(new Uint8Array(await blob.arrayBuffer()), ppi));
}
