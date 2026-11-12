// Append Board (`PCB_CONTROL::AppendBoard` and `placeBoardItems`, pcbnew/tools/pcb_control.cpp): the pure half. The file is a KiCad board (`KICTL_KICAD_ONLY`; the drop's
// `m_acceptedExts` take `.kicad_pcb` and the legacy `.brd`, which needs a plugin this studio does not have). `AppendBoard` loads the file into the board, so nothing of it is left
// out: the board takes the file's copper layers when they are more, every technical and user layer it enables (a new layer with the file's name and type), and every net of it
// by name; with the Design Blocks pane's "Place as group" (`m_DesignBlockChooserPanel.place_as_group`, which `AppendBoard` reads for a plain board too) what came in is one
// group named after the file's stem, which alone is selected. That is Place Design Block's command with no block (`place_design_block`, `block: ""`; crates/ops/src/design_blocks.rs).
// What Append Board adds here is the point the items are held by while they follow the cursor: the position of the top left item (`PCB_SELECTION::GetTopLeftItem`), which the
// server works out (`POST /api/append_board/read`, crates/cli/src/append_api.rs). Unit-tested in appendBoard.test.ts.
import type { Cmd, PointXY } from "../api/types";

/** `POST /api/append_board/read`'s reply. */
export interface AppendInfo {
  ok: boolean;
  message?: string;
  /** The point the items are held by, in the file's own coordinates (micrometres). */
  reference?: PointXY;
  counts?: Record<string, number>;
  /** What the append will add to or change on this board (copper layers, user layers, nets, references already in use). */
  notes?: string[];
  /** KiCad 25ec2335 `remapAppendedLayers`: the rows of "Edit Mapping of Imported Layers" when the file's layers do not all fit the board; empty or absent when they do. */
  layers?: import("./foreignImport").AppendLayerDesc[];
  /** With a `layer_map` in the request: the file's text with its layers moved, which is what is appended. */
  text?: string | null;
}

/** Is `name` a board file Append Board reads? */
export function isKicadBoardName(name: string): boolean {
  return name.toLowerCase().endsWith(".kicad_pcb");
}

/** `wxFileName( fileName ).GetName()`: the file's name without its folder and its extension, which names the group. */
export function fileStemOf(name: string): string {
  const base = name.split(/[\\/]/).pop() ?? name;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}

/**
 * The command that appends the board with its reference point on `at`: the file's text, its origin moved to `at` less the reference; with `asGroup` (Place as group) what came in
 * is a group named after the file's stem (`group->SetName( wxFileName( fileName ).GetName() )`).
 */
export function appendBoardCmd(text: string, reference: PointXY, at: PointXY, fileName: string, asGroup: boolean): Cmd {
  return { op: "place_design_block", text, at: { x: at.x - reference.x, y: at.y - reference.y }, block: "", as_group: asGroup, group_name: fileStemOf(fileName) };
}

/** "3 footprints, 12 tracks, 1 via" -- what the file held. */
export function describeAppended(counts: Record<string, number> | undefined): string {
  const kinds: Array<[string, string, string]> = [
    ["footprints", "footprint", "footprints"],
    ["tracks", "track", "tracks"],
    ["vias", "via", "vias"],
    ["zones", "zone", "zones"],
    ["shapes", "graphic", "graphics"],
    ["texts", "text", "texts"],
    ["dimensions", "dimension", "dimensions"],
  ];
  const parts = kinds.filter(([k]) => (counts?.[k] ?? 0) > 0).map(([k, one, many]) => `${counts![k]} ${counts![k] === 1 ? one : many}`);
  return parts.length > 0 ? parts.join(", ") : "nothing";
}
