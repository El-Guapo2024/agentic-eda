// Browser-side halves of the library editors' file actions: the two parse routes (`crates/cli/src/library_api.rs`), the
// "native file dialog" stand-ins (a hidden file input for Open, a Blob + anchor for Save) and the clipboard. Kept apart from
// `client.ts` so the two library editors' own additions never grow that shared file.
import type { LibraryFootprint, LibrarySymbol } from "./types";
import { ApiError, postCmd } from "./client";
import { withDefaults } from "../kicad-port/libraryDefaults";
import { base64Of, imageHeader } from "../kicad-port/refImage";
import { ensureInProject, UNKNOWN_SYMBOL } from "../kicad-port/libraryEnsure";

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

/** `SYMBOL_EDITOR_EDIT_TOOL::Copy`: the symbol `libId` with only the pins and graphics `ids`, as `.kicad_sym` text (`POST /api/symbol/clipboard/copy`). */
export async function symbolItemsText(libId: string, ids: readonly string[]): Promise<string> {
  const r = await postText<{ text?: string; error?: string }>("/api/symbol/clipboard/copy", JSON.stringify({ lib_id: libId, ids }));
  return r.text ?? "";
}

/** `.kicad_mod` text -> the editable footprint (the IR omits empty lists, so they are filled in here like `fetchFootprint` does). */
export async function parseFootprintText(text: string): Promise<ParsedFootprint> {
  const r = await postText<{ footprint: LibraryFootprint; warnings?: string[]; error?: string }>("/api/footprint/parse", text);
  const f = r.footprint;
  return { footprint: { ...f, pads: f.pads ?? [], graphics: f.graphics ?? [], texts: f.texts ?? [], fields: f.fields ?? [] }, warnings: r.warnings ?? [] };
}

/** The server reads at most 64 MiB of a request body (`studio.rs::read_request`); a bigger file would arrive cut off and fail to parse, so say so up front. */
export const MAX_IMPORT_BYTES = 64 << 20;

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
        return reject(new ApiError(`${file.name} is larger than 64 MB, which the studio's server will not read`));
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

/**
 * The bitmap on the system clipboard (`GetImageFromClipboard`: `wxDF_BITMAP`), as its bytes in base64 -- a PNG, JPEG, BMP or GIF the reference images take -- or null for none,
 * for an image of another kind, or where the browser will not hand the clipboard over.
 */
export async function readClipboardImage(): Promise<string | null> {
  try {
    const items = (await navigator.clipboard?.read?.()) ?? [];
    for (const item of items) {
      const type = item.types.find((t) => t.startsWith("image/"));
      if (!type) continue;
      const bytes = new Uint8Array(await (await item.getType(type)).arrayBuffer());
      if (imageHeader(bytes)) return base64Of(bytes);
    }
  } catch {
    /* no clipboard read: no image */
  }
  return null;
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

/**
 * Opening a library footprint (`Lib:Name`) that the project library does not hold yet -- one of KiCad's installed libraries, or what the board's own model
 * resolved -- first copies it in, as one undoable `put_library_footprint`. `overwrite`, because the model may resolve the same name (a part that uses it)
 * and a plain put treats that as taken. A name nothing resolves is left to the open verb, which starts a blank one. The open verb copies a library's
 * footprint in itself too when the design has none by the time it lands (`crates/cli/src/library_place.rs::with_opened_footprint`): this read and the
 * open are requests of their own, and a command between them (another client deleting the design's copy) used to leave the open to start a BLANK
 * footprint under the library's name.
 */
export async function ensureFootprintInProject(name: string): Promise<string | null> {
  if (!name.includes(":")) return null;
  // Answers null when the footprint is in the project library now, or when nothing knows the name (the open verb then starts a blank one, as KiCad's New
  // Footprint would); else why it could not be copied in -- the caller must not open a blank footprint over a library's footprint it failed to read or store.
  // A failure is tried once more (a request that lost a race with another command); the put's own refusal used to be ignored, and a read error read as
  // "unknown", so either opened the footprint blank.
  let problem: string | null = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    let found: { footprint: LibraryFootprint; inProject: boolean };
    try {
      found = await fetchAnyFootprint(name);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (/no footprint with this name/.test(message)) return null; // `footprint_any`'s "unknown": nothing installed or known under this name
      problem = message;
      continue;
    }
    if (found.inProject) return null;
    const reply = await postCmd({ op: "put_library_footprint", footprint: found.footprint, overwrite: true }, false);
    if (reply.ok) return null;
    problem = reply.message;
  }
  return problem;
}

/**
 * The symbol sibling of [`ensureFootprintInProject`]: an installed `Lib:Name` is put into the project library before the editor opens it. Null when it is there now
 * or nothing knows the id (the open verb then starts a blank symbol); else why it could not be copied in -- the caller opens nothing and says it
 * (kicad-port/libraryEnsure.ts: a read error used to be taken for "unknown" and the put's refusal ignored, so a library's symbol could open blank). A library
 * symbol whose file cannot be read is such an error (`library_api::symbol_any`: `ops_bad_symbol`). The open verb copies a library's symbol in itself too
 * when the design has none by the time it lands (`crates/cli/src/library_place.rs::with_opened_symbol`): this read, the names read and the open are
 * requests of their own, and a command between them (another client deleting the design's copy) used to leave the open to start a BLANK symbol under the
 * library's id.
 */
export async function ensureSymbolInProject(libId: string): Promise<string | null> {
  return ensureInProject(
    libId,
    {
      fetchAny: async (id) => {
        const { symbol, inProject } = await fetchAnySymbol(id);
        return { item: symbol, inProject };
      },
      put: (symbol) => postCmd({ op: "put_library_symbol", symbol, overwrite: true }, false),
    },
    UNKNOWN_SYMBOL
  );
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
