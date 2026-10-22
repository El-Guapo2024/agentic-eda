// The browser half of the microwave tools (crates/cli/src/microwave_api.rs): what a shape description file holds, and whether an inductor can be as long as asked. A browser hands the
// page a file's text and not its path, so the page posts what it read. The footprints themselves are `Cmd::Microwave` verbs (api/microwaveTypes.ts).
import type { PointXY } from "./types";
import type { MicrowaveShapeInfo } from "./microwaveTypes";
import { ApiError } from "./client";

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

/** The scales (nanometres) and the point count of a shape description file (`ReadDataShapeDescr`). */
export function readShapeFile(text: string): Promise<MicrowaveShapeInfo> {
  return postJson<MicrowaveShapeInfo>("/api/microwave/shape", { text });
}

/** Whether the inductor between `start` and `end` can be `length_um` long, or the words of the info bar that says why not (`createMicrowaveInductor`). */
export function checkInductor(req: { start: PointXY; end: PointXY; length_um: number; track_width_um: number }): Promise<{ ok: boolean; message?: string }> {
  return postJson<{ ok: boolean; message?: string }>("/api/microwave/inductor", req);
}
