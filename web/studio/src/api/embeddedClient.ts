// The browser half of the Embedded Files panel (crates/cli/src/embedded_api.rs): the list of an owner without the data, the URL a file is extracted from, and what Collect and
// Embed 3D Models would embed. The edits are `Cmd::Embedded` verbs (api/embeddedTypes.ts).
import type { CollectPlan, EmbeddedListing, EmbeddedOwner, TextFonts } from "./embeddedTypes";

/** The query that names an owner: `owner=board`, `owner=footprint&id=U1`, ... */
export function ownerQuery(owner: EmbeddedOwner): string {
  switch (owner.kind) {
    case "board":
    case "schematic":
      return `owner=${owner.kind}`;
    case "footprint":
      return `owner=footprint&id=${encodeURIComponent(owner.reference)}`;
    case "library_footprint":
      return `owner=library_footprint&id=${encodeURIComponent(owner.name)}`;
    case "library_symbol":
      return `owner=library_symbol&id=${encodeURIComponent(owner.lib_id)}`;
  }
}

/** `GET /api/embedded`. Throws with the server's reason (an owner that is not there). */
export async function fetchEmbedded(owner: EmbeddedOwner): Promise<EmbeddedListing> {
  const r = await fetch(`/api/embedded?${ownerQuery(owner)}`, { cache: "no-store" });
  const j = (await r.json().catch(() => ({}))) as Partial<EmbeddedListing> & { error?: string };
  if (!r.ok || j.error) throw new Error(j.error ?? `HTTP ${r.status}`);
  return { owner: j.owner ?? owner, fonts: !!j.fonts, files: j.files ?? [], inherited: j.inherited ?? [] };
}

/** `GET /api/embedded/file`: where the content of a file is extracted from. */
export function embeddedFileUrl(owner: EmbeddedOwner, name: string): string {
  return `/api/embedded/file?${ownerQuery(owner)}&name=${encodeURIComponent(name)}`;
}

/** `GET /api/embedded/collect`. */
export async function fetchCollectPlan(): Promise<CollectPlan> {
  const r = await fetch("/api/embedded/collect", { cache: "no-store" });
  const j = (await r.json().catch(() => ({}))) as Partial<CollectPlan> & { error?: string };
  if (!r.ok || j.error) throw new Error(j.error ?? `HTTP ${r.status}`);
  return { count: j.count ?? 0, models: j.models ?? [], skipped: j.skipped ?? [] };
}

/** The bytes of a file the person picked, as base64 (what the Add verb carries). */
export function readFileBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("the file could not be read"));
    reader.onload = () => {
      const url = String(reader.result ?? "");
      const comma = url.indexOf(",");
      resolve(comma >= 0 ? url.slice(comma + 1) : "");
    };
    reader.readAsDataURL(file);
  });
}

/** `GET /api/embedded/fonts`: the fonts the owner's texts use that may be embedded (what ticking "Embed fonts" takes in). */
export async function fetchTextFonts(owner: EmbeddedOwner): Promise<TextFonts> {
  const r = await fetch(`/api/embedded/fonts?${ownerQuery(owner)}`, { cache: "no-store" });
  const j = (await r.json().catch(() => ({}))) as Partial<TextFonts> & { error?: string };
  if (!r.ok || j.error) throw new Error(j.error ?? `HTTP ${r.status}`);
  return { fonts: j.fonts ?? [], skipped: j.skipped ?? [] };
}

/** `GET /api/embedded/fonts/file`: the content of a font the owner's texts use (Export of a font row the click took in). */
export function textFontUrl(owner: EmbeddedOwner, name: string): string {
  return `/api/embedded/fonts/file?${ownerQuery(owner)}&name=${encodeURIComponent(name)}`;
}

/** `GET /api/embedded/doc`: where a `kicad-embed://` document is found, through the lists of `stack` in order (`GetAssociatedDocument`'s `aFilesStack`). */
export function embeddedDocUrl(stack: readonly EmbeddedOwner[], link: string): string {
  return `/api/embedded/doc?name=${encodeURIComponent(link.trim())}&stack=${encodeURIComponent(JSON.stringify(stack))}`;
}

/**
 * `GetAssociatedDocument` of a `kicad-embed://` link: the document opened in a tab of its own when a list of the stack has it. The tab is opened at once (a browser lets a page
 * open one only in answer to a click or a key) and closed again when the file is not found -- KiCad then opens nothing and says nothing ("Failed to get temp file '%s' for
 * kicad_embed URI" goes to its trace only). Resolves to whether it was found.
 */
export async function openEmbeddedDocument(stack: readonly EmbeddedOwner[], link: string): Promise<boolean> {
  const url = embeddedDocUrl(stack, link);
  const tab = window.open("", "_blank");
  try {
    const r = await fetch(`${url}&probe=1`, { cache: "no-store" });
    const j = (await r.json().catch(() => ({}))) as { found?: boolean };
    if (!r.ok || !j.found) {
      tab?.close();
      return false;
    }
    if (tab) {
      tab.opener = null;
      tab.location.href = url;
    } else {
      window.open(url, "_blank", "noopener,noreferrer");
    }
    return true;
  } catch {
    tab?.close();
    return false;
  }
}
