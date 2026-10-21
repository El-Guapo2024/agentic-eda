// The bitmaps the board and the schematic draw (reference images, a schematic's image): one `HTMLImageElement` per URL, loaded the first time it is asked for, and a version number that
// moves when one has arrived so the canvas that asked repaints (a canvas draws synchronously and cannot wait for a bitmap). The URL carries the hash of the pixels
// (kicad-port/refImage.ts `pcbImageUrl`), so an image whose pixels changed is another URL and the old bitmap is simply no longer asked for.
import { useSyncExternalStore } from "react";

const images = new Map<string, HTMLImageElement>();
let version = 0;
const listeners = new Set<() => void>();
/** The most bitmaps kept at once: the least recently asked for go first. */
const MAX_IMAGES = 64;

function bump(): void {
  version++;
  for (const l of listeners) l();
}

/** The bitmap at `url` when it has loaded, else null (and it is loaded, and the version moves when it is). */
export function imageFor(url: string): HTMLImageElement | null {
  let img = images.get(url);
  if (img) {
    // most recently asked for last
    images.delete(url);
    images.set(url, img);
  } else {
    img = new Image();
    img.onload = bump;
    img.onerror = () => bump();
    img.src = url;
    images.set(url, img);
    while (images.size > MAX_IMAGES) {
      const oldest = images.keys().next().value;
      if (oldest === undefined) break;
      images.delete(oldest);
    }
  }
  return img.complete && img.naturalWidth > 0 ? img : null;
}

/** An image the caller made itself (the one following the cursor while an image is placed): never the cache's, never evicted. */
export function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("The image could not be decoded."));
    img.src = src;
  });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The number of bitmaps that have arrived: a canvas that draws one lists it among what it repaints for. */
export function useImageVersion(): number {
  return useSyncExternalStore(subscribe, () => version);
}
