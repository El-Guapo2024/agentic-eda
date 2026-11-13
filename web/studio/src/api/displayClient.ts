// The board editor's Display Options that need the server (UI-LEFT-3 P3-6): the clearance outlines' clearances, `POST /api/display/clearances`
// (crates/cli/src/display_api.rs). Kept apart from `client.ts` so the painter's port does not grow that shared file.
import type { DisplayClearances } from "./types";

/** Every track's (one per segment), via's and pad's own clearance on copper layer `layer` (`GetOwnClearance( layer )`); `{ ok: false }` on any failure. */
export async function fetchDisplayClearances(layer: string): Promise<DisplayClearances> {
  try {
    const r = await fetch("/api/display/clearances", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ layer }) });
    if (!r.ok) return { ok: false, message: `HTTP ${r.status}` };
    return (await r.json()) as DisplayClearances;
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}
