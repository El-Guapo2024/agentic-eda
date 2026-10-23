// The browser half of the Embedded Files panel (crates/cli/src/embedded_api.rs): the list of an owner without the data, the URL a file is extracted from, and what Collect and
// Embed 3D Models would embed. The edits are `Cmd::Embedded` verbs (api/embeddedTypes.ts).
import type { CollectPlan, EmbeddedListing, EmbeddedOwner } from "./embeddedTypes";

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
