// The requests of the hierarchy dialogs (crates/cli/src/sch_hierarchy_api.rs).
import type { AnnotateOptions, AnnotateReport, SheetFileCheck, SheetFilesReply } from "./hierarchyTypes";

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

/** What Annotate Schematic would do on `sheet` (the placement ids from the root, `[]` for the root): the message window, nothing written. */
export function fetchAnnotateReport(req: { reset_existing: boolean; order: "y_then_x" | "x_then_y"; ids?: string[]; options: AnnotateOptions }, sheet: readonly string[]): Promise<AnnotateReport> {
  return postJson("/api/sch/annotate", { ...req, sheet: sheet.join("/") });
}

/**
 * The reference a symbol placed on `sheet` gets (`PlaceSymbol`'s `annotate()` over every reference of the hierarchy; `POST /api/sch/next_reference`): null when the
 * server cannot answer. A unit of a part of several units joins a part of the same symbol and value that has it free (r2 D-A1).
 */
export async function fetchPlacementReference(req: { sheet: readonly string[]; prefix: string; lib_id: string; value: string; unit: number; unit_count: number; options: AnnotateOptions }): Promise<string | null> {
  try {
    const r = await postJson<{ ok: boolean; reference?: string }>("/api/sch/next_reference", { ...req, sheet: req.sheet.join("/") });
    return r.ok && r.reference ? r.reference : null;
  } catch {
    return null;
  }
}

/** The schematic files of the project that Import Sheet lists beside the path the user can type. */
export async function fetchSheetFiles(): Promise<SheetFilesReply> {
  const r = await fetch("/api/sch/sheet_files");
  return (await r.json()) as SheetFilesReply;
}

/** What taking the file at `path` in as a sheet would bring (`LoadSheetFromFile`'s checks): nothing is written. */
export function fetchSheetFileCheck(req: { path: string; name?: string; keep_annotations?: boolean }): Promise<SheetFileCheck> {
  return postJson("/api/sch/sheet_file", req);
}
