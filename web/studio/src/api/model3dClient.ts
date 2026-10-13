// What the 3D Models tab of the Footprint Properties dialogs asks the server besides the models themselves (`GET /api/3dmodel`, components/viewer3d/modelCache.ts):
// the status of each row's file name, the model chooser's folders and files, and the footprint to show beside the models (crates/cli/src/model3d_tab_api.rs).
import type { ModelCheck } from "../kicad-port/fpModels";
import type { Migration } from "../kicad-port/migrate3d";
import type { Part } from "./types";

/** `POST /api/3dmodel/check`: the status of each name, in order (`validateModelExists`). An answer that does not come leaves the cells empty: a status is a hint, not a gate. */
export async function checkModels(names: readonly string[]): Promise<ModelCheck[]> {
  if (names.length === 0) return [];
  try {
    const r = await fetch("/api/3dmodel/check", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ names }) });
    if (!r.ok) return [];
    return ((await r.json()) as { results?: ModelCheck[] }).results ?? [];
  } catch {
    return [];
  }
}

/** A folder the chooser starts from: `${NAME}` and where it is. */
export interface ModelRoot {
  name: string;
  form: string;
  path: string;
}

export interface ModelEntry {
  name: string;
  kind: "dir" | "file";
  /** The path to write in the footprint: `${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`. */
  path: string;
}

export interface ModelListing {
  roots: ModelRoot[];
  /** The folder listed, in the form a footprint writes; null for the list of roots. */
  dir: string | null;
  parent: string | null;
  entries: ModelEntry[];
  truncated: boolean;
}

export type ModelFilter = "all" | "step" | "wrl";

/** `GET /api/3dmodel/browse`: the folders and model files in `dir` (a path as a footprint writes it; empty for the roots). Throws with the server's reason for a folder it will not list. */
export async function browseModels(dir: string, filter: ModelFilter = "all"): Promise<ModelListing> {
  const r = await fetch(`/api/3dmodel/browse?dir=${encodeURIComponent(dir)}&filter=${filter}`, { cache: "no-store" });
  const j = (await r.json().catch(() => ({}))) as Partial<ModelListing> & { error?: string };
  if (!r.ok || j.error) throw new Error(j.error ?? `HTTP ${r.status}`);
  return { roots: j.roots ?? [], dir: j.dir ?? null, parent: j.parent ?? null, entries: j.entries ?? [], truncated: !!j.truncated };
}

/** What the preview is a footprint of: one on the board, or one of the libraries. */
export type PreviewOf = { part: string } | { footprint: string };

/** `GET /api/3dmodel/preview`: the footprint at the origin on the top side, not turned, as `/api/state` sends a placed part (pads, graphics, texts, courtyard). Null when there is none. */
export async function fetchPreviewPart(of: PreviewOf): Promise<Part | null> {
  const q = "part" in of ? `part=${encodeURIComponent(of.part)}` : `footprint=${encodeURIComponent(of.footprint)}`;
  try {
    const r = await fetch(`/api/3dmodel/preview?${q}`, { cache: "no-store" });
    if (!r.ok) return null;
    return ((await r.json()) as { part?: Part }).part ?? null;
  } catch {
    return null;
  }
}

/**
 * `GET /api/3dmodel/migrate`: the VRML models the board's footprints name that no search finds, each with the STEP files that could stand for it (ranked), and how many files the folders
 * held (`scanMissing`, `buildCatalog`, `rankCandidatesFor`). `dirs` are the folders added by hand ("Add Search Directory..."), in the form the chooser gives. Throws with the server's reason.
 */
export async function fetchMigration(dirs: readonly string[]): Promise<Migration> {
  const q = dirs.length > 0 ? `?dirs=${encodeURIComponent(dirs.join("|"))}` : "";
  const r = await fetch(`/api/3dmodel/migrate${q}`, { cache: "no-store" });
  const j = (await r.json().catch(() => ({}))) as Partial<Migration> & { error?: string };
  if (!r.ok || j.error) throw new Error(j.error ?? `HTTP ${r.status}`);
  return { missing: j.missing ?? [], scanned: j.scanned ?? 0, dirs: j.dirs ?? [] };
}
