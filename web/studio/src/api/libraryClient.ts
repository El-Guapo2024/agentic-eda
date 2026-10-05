// Browser-side halves of the library editors' file actions: the two parse routes (`crates/cli/src/library_api.rs`), the
// "native file dialog" stand-ins (a hidden file input for Open, a Blob + anchor for Save) and the clipboard. Kept apart from
// `client.ts` so the two library editors' own additions never grow that shared file.
import type { LibraryFootprint, LibrarySymbol } from "./types";
import { ApiError } from "./client";
import { withDefaults } from "../kicad-port/libraryDefaults";

export interface ParsedSymbols {
  symbols: LibrarySymbol[];
  warnings: string[];
}

export interface ParsedFootprint {
  footprint: LibraryFootprint;
  warnings: string[];
}

async function postText<T extends { error?: string }>(url: string, text: string): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "text/plain; charset=utf-8" }, body: text });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  const j = (await r.json()) as T;
  if (j.error) throw new ApiError(j.error);
  return j;
}

/** `.kicad_sym` text (a library file, or KiCad's clipboard text of `(symbol ...)` forms) -> the editable symbols it holds. */
export async function parseSymbolText(text: string): Promise<ParsedSymbols> {
  const r = await postText<ParsedSymbols & { error?: string }>("/api/symbol_library/parse", text);
  return { symbols: (r.symbols ?? []).map(withDefaults), warnings: r.warnings ?? [] };
}

/** `.kicad_mod` text -> the editable footprint (the IR omits empty lists, so they are filled in here like `fetchFootprint` does). */
export async function parseFootprintText(text: string): Promise<ParsedFootprint> {
  const r = await postText<{ footprint: LibraryFootprint; warnings?: string[]; error?: string }>("/api/footprint/parse", text);
  const f = r.footprint;
  return { footprint: { ...f, pads: f.pads ?? [], graphics: f.graphics ?? [], texts: f.texts ?? [], fields: f.fields ?? [] }, warnings: r.warnings ?? [] };
}

/** The server reads at most 1 MiB of a request body (`studio.rs::handle`); a bigger file would arrive cut off and fail to parse, so say so up front. */
export const MAX_IMPORT_BYTES = 1 << 20;

/** A browser "Open file" chooser: resolves to the chosen file's name and text, or `null` when it is cancelled. */
export function pickTextFile(accept: string): Promise<{ name: string; text: string } | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    let settled = false;
    const done = (v: { name: string; text: string } | null) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(v);
    };
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      if (!file) return done(null);
      if (file.size > MAX_IMPORT_BYTES) {
        settled = true;
        input.remove();
        return reject(new ApiError(`${file.name} is larger than 1 MB, which the studio's server will not read`));
      }
      try {
        done({ name: file.name, text: await file.text() });
      } catch (e) {
        settled = true;
        input.remove();
        reject(e);
      }
    });
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/** A browser download of `text` as `fileName` (the web equivalent of a native Save dialog). */
export function saveTextFile(text: string, fileName: string, type = "text/plain"): void {
  saveBlob(new Blob([text], { type }), fileName);
}

export function saveBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/** The system clipboard's text, or `null` when the browser will not hand it over (no permission, an insecure context). */
export async function readClipboardText(): Promise<string | null> {
  try {
    return (await navigator.clipboard?.readText()) ?? null;
  } catch {
    return null;
  }
}

/** Put `text` on the system clipboard; `false` when the browser refuses. */
export async function writeClipboardText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** A canvas as a PNG download (`SYMBOL_EDIT_FRAME::SaveCanvasImageToFile`). */
export function saveCanvasPng(canvas: HTMLCanvasElement, fileName: string): Promise<boolean> {
  return new Promise((resolve) => {
    canvas.toBlob((blob) => {
      if (!blob) return resolve(false);
      saveBlob(blob, fileName);
      resolve(true);
    }, "image/png");
  });
}

async function getLibrary<T extends { error?: string }>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  const j = (await r.json()) as T;
  if (j.error) throw new ApiError(j.error);
  return j;
}

/** Any symbol of the tree -- the project entry or the one a library file / the builtin table defines -- without opening (materializing) it. */
export async function fetchAnySymbol(libId: string): Promise<{ symbol: LibrarySymbol; inProject: boolean }> {
  const r = await getLibrary<{ symbol: LibrarySymbol; in_project: boolean; error?: string }>(`/api/library/symbol?lib_id=${encodeURIComponent(libId)}`);
  return { symbol: withDefaults(r.symbol), inProject: r.in_project };
}

/** Any footprint of the tree, the footprint sibling of `fetchAnySymbol` (the IR omits empty lists, so they are filled in here). */
export async function fetchAnyFootprint(name: string): Promise<{ footprint: LibraryFootprint; inProject: boolean }> {
  const r = await getLibrary<{ footprint: LibraryFootprint; in_project: boolean; error?: string }>(`/api/library/footprint?name=${encodeURIComponent(name)}`);
  const f = r.footprint;
  return { footprint: { ...f, pads: f.pads ?? [], graphics: f.graphics ?? [], texts: f.texts ?? [], fields: f.fields ?? [] }, inProject: r.in_project };
}

/** `GET /api/symbol/export?lib_id=` text (the derived `.kicad_sym`), for Copy and Export. */
export async function fetchSymbolKicadSymText(libId: string): Promise<string> {
  const r = await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(libId)}`, { cache: "no-store" });
  if (!r.ok) throw new ApiError(await r.text());
  return r.text();
}

/** `GET /api/footprint/export?name=` text (the derived `.kicad_mod`). */
export async function fetchFootprintKicadModText(name: string): Promise<string> {
  const r = await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`, { cache: "no-store" });
  if (!r.ok) throw new ApiError(await r.text());
  return r.text();
}
