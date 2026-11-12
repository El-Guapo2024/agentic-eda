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
export function readAppendBoard(file: { name: string; text: string }, layerMap?: Record<string, string | null>): Promise<AppendInfo> {
  return postJson<AppendInfo>("/api/append_board/read", layerMap ? { ...file, layer_map: layerMap } : file);
}

/** The items an interactive Import Graphics would put on the board and the point they are held by (`POST /api/import_graphics/preview`; nothing on the board changes). */
export function previewGraphics(cmd: unknown): Promise<{ ok: boolean; message?: string; shapes?: unknown[]; texts?: unknown[]; reference?: { x: number; y: number } }> {
  return postJson("/api/import_graphics/preview", { cmd });
}

/** Load and Test Netlist (`POST /api/import_netlist`, crates/cli/src/netlist_api.rs): what Update PCB would do to the board with this netlist and these choices -- nothing changes. */
export function testNetlist(req: { text: string; name: string; options: import("../kicad-port/importNetlist").NetlistOptions }): Promise<import("../kicad-port/importNetlist").NetlistReport> {
  return postJson("/api/import_netlist", req);
}
