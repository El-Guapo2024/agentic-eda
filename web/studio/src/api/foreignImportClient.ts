// The browser half of File > Import > Non-KiCad Board File... and Non-KiCad Schematic File...: the file as bytes (an Altium board is not text; a browser gives the page a file's
// content and never its path) and the server's answer (`POST /api/import_foreign`, crates/cli/src/foreign_api.rs). The import itself is a command (`replace_board`,
// `replace_schematic`), sent with `postCmd` by `actions/importActions.ts`.
import type { ForeignFile, ForeignReply, ImportKind } from "../kicad-port/foreignImport";
import { ApiError } from "./client";

/** The largest file the page posts: the server reads 64 MB of request, and base64 is a third longer than the bytes. */
export const MAX_FOREIGN_BYTES = 40 << 20;

/** `bytes` as base64, in pieces (`btoa` of a very long string overflows the stack of `String.fromCharCode.apply`). */
export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  const piece = 0x8000;
  for (let i = 0; i < bytes.length; i += piece) binary += String.fromCharCode(...bytes.subarray(i, i + piece));
  return btoa(binary);
}

/** A browser "Open file" chooser for a file of any kind: resolves to the chosen file's name and content (base64), or `null` when it is cancelled. */
export function pickBinaryFile(accept: string): Promise<ForeignFile | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    let settled = false;
    const done = (v: ForeignFile | null) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(v);
    };
    const fail = (e: unknown) => {
      if (settled) return;
      settled = true;
      input.remove();
      reject(e);
    };
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      if (!file) return done(null);
      if (file.size > MAX_FOREIGN_BYTES) return fail(new ApiError(`${file.name} is larger than 40 MB, which the studio does not post to its server`));
      try {
        done({ name: file.name, data: toBase64(new Uint8Array(await file.arrayBuffer())) });
      } catch (e) {
        fail(e);
      }
    });
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/**
 * kicad-cli's conversion of `file` as a `kind` (nothing on the board changes): the KiCad file, the format it recognised, what it converted, its errors and warnings and, for a board,
 * where each source layer went. `layerMap` (source layer -> KiCad layer) is `--layer-map`: the layers the person moved.
 */
export async function postImportForeign(kind: ImportKind, file: ForeignFile, layerMap?: Record<string, string>): Promise<ForeignReply> {
  const body: Record<string, unknown> = { kind, name: file.name, data: file.data };
  if (file.format) body.format = file.format;
  if (layerMap && Object.keys(layerMap).length > 0) body.layer_map = layerMap;
  const r = await fetch("/api/import_foreign", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new ApiError(`/api/import_foreign: HTTP ${r.status}`);
  return (await r.json()) as ForeignReply;
}
