// Assign Footprints (CvPcb) and the server (crates/cli/src/cvpcb_api.rs): the footprint the preview pane draws, the project's footprint association files and
// Automatically Assign Footprints. The assignments themselves are commands (`edit_symbol_fields`), applied by the dialog's OK as one undo step.
import type { LibraryFootprint } from "./types";

async function getJson<T>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  return (await r.json()) as T;
}

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

/** What the footprint viewer shows: the footprint, or `GetFootprint`'s message. `footprint` is null for an empty id. */
export interface CvpcbFootprint {
  footprint: LibraryFootprint | null;
  lib?: string;
  in_project?: boolean;
  error?: string;
}

/** `GET /api/cvpcb/footprint?id=`. */
export async function fetchCvpcbFootprint(id: string): Promise<CvpcbFootprint> {
  const r = await getJson<CvpcbFootprint>(`/api/cvpcb/footprint?id=${encodeURIComponent(id)}`);
  const f = r.footprint;
  return f ? { ...r, footprint: { ...f, pads: f.pads ?? [], graphics: f.graphics ?? [], texts: f.texts ?? [], fields: f.fields ?? [] } } : r;
}

/** The project's list (`cvpcb.equivalence_files`), where each entry leads, and the grid's two variables (`KIPRJMOD`, `KICAD10_FOOTPRINT_DIR`). */
export interface EquFilesView {
  files: string[];
  resolved: { entry: string; path: string; found: boolean }[];
  vars: { name: string; value: string }[];
  project_file: string;
  ok?: boolean;
  error?: string;
}

/** `GET /api/cvpcb/equ_files`. */
export function fetchEquFiles(): Promise<EquFilesView> {
  return getJson<EquFilesView>("/api/cvpcb/equ_files");
}

/** `POST /api/cvpcb/equ_files`: the dialog's OK (`OnOkClick`), the list saved at once, no undo step. */
export function saveEquFiles(files: string[]): Promise<EquFilesView> {
  return postJson<EquFilesView>("/api/cvpcb/equ_files", { files });
}

/** One folder of the server for Add's file dialog: its folders and `.equ` files. */
export interface EquBrowse {
  path: string;
  parent: string | null;
  entries: { name: string; dir: boolean }[];
  error?: string;
}

/** `GET /api/cvpcb/equ_browse?path=`. */
export function browseEqu(path: string): Promise<EquBrowse> {
  return getJson<EquBrowse>(`/api/cvpcb/equ_browse?path=${encodeURIComponent(path)}`);
}

/** What Automatically Assign Footprints would assign, and KiCad's messages about it. */
export interface AutoAssociateReply {
  assignments: { ref: string; footprint: string }[];
  found: number;
  /** "%lu footprint/symbol equivalences found." */
  status: string;
  /** "Equivalence File Load Error". */
  load_errors: string | null;
  /** "CvPcb Warning". */
  warnings: string | null;
  error?: string;
}

/** `POST /api/cvpcb/auto_associate` over the symbols the dialog lists, each with the footprint it has there (staged ones included). */
export function autoAssociate(components: { ref: string; value: string; footprint: string; lib_id: string }[]): Promise<AutoAssociateReply> {
  return postJson<AutoAssociateReply>("/api/cvpcb/auto_associate", { components });
}
