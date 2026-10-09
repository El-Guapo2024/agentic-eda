// The Appearance panel's per-project settings (crates/cli/src/appearance_api.rs): `GET /api/appearance` reads `appearance.json`, `POST` replaces its `local`
// and `project` sections. The shape is kicad-port/appearanceFile.ts's `AppearanceFile`.
import type { AppearanceFile } from "../kicad-port/appearanceFile";

export async function fetchAppearance(): Promise<unknown> {
  const r = await fetch("/api/appearance", { cache: "no-store" });
  if (!r.ok) throw new Error(`/api/appearance: HTTP ${r.status}`);
  return r.json();
}

export async function postAppearance(file: AppearanceFile): Promise<void> {
  const r = await fetch("/api/appearance", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ local: file.local, project: file.project }) });
  const reply = (await r.json().catch(() => null)) as { error?: string } | null;
  if (!r.ok || reply?.error) throw new Error(reply?.error ?? `/api/appearance: HTTP ${r.status}`);
}
