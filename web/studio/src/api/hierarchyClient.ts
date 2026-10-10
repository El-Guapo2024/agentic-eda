// The requests of the hierarchy dialogs (crates/cli/src/sch_hierarchy_api.rs).
import type { AnnotateOptions, AnnotateReport } from "./hierarchyTypes";

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

/** What Annotate Schematic would do on `sheet` (the placement ids from the root, `[]` for the root): the message window, nothing written. */
export function fetchAnnotateReport(req: { reset_existing: boolean; order: "y_then_x" | "x_then_y"; ids?: string[]; options: AnnotateOptions }, sheet: readonly string[]): Promise<AnnotateReport> {
  return postJson("/api/sch/annotate", { ...req, sheet: sheet.join("/") });
}
