// The browser half of the file-in actions: what a DXF or SVG holds (`POST /api/import_graphics/read`, crates/cli/src/graphics_api.rs) and the board a `.kicad_pcb` file
// becomes for Append Board (`POST /api/append_board/read`, crates/cli/src/append_api.rs). A browser hands the page a file's text and not its path, so the page posts what it read.
import type { AppendInfo } from "../kicad-port/appendBoard";
import type { GraphicsInfo } from "../kicad-port/importGraphics";
import { ApiError } from "./client";

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

/** What a DXF or SVG file holds, at an import scale and with the DXF's default unit and line width (nanometres). */
export function readGraphics(req: { format: "dxf" | "svg"; text: string; dxf_units?: string; scale?: number; line_width_nm?: number }): Promise<GraphicsInfo> {
  return postJson<GraphicsInfo>("/api/import_graphics/read", req);
}

/** What a `.kicad_pcb` file would bring to this board and the point its items are held by (nothing on the board changes). */
export function readAppendBoard(file: { name: string; text: string }): Promise<AppendInfo> {
  return postJson<AppendInfo>("/api/append_board/read", file);
}
