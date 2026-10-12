// Append Board (`PCB_CONTROL::AppendBoard` and `placeBoardItems`, pcbnew/tools/pcb_control.cpp): the pure half. The file is a KiCad board (`KICTL_KICAD_ONLY`; the drop's
// `m_acceptedExts` take `.kicad_pcb` and the legacy `.brd`, which needs a plugin this studio does not have), its text is the clipboard form `Cmd::PasteClipboard` reads, and what Append
// Board adds is the point the items are held by while they follow the cursor: the position of the top left item (`PCB_SELECTION::GetTopLeftItem`), which the server works out
// (`POST /api/append_board/read`, crates/cli/src/append_api.rs). Unit-tested in appendBoard.test.ts.
import type { Cmd, PointXY } from "../api/types";

/** `POST /api/append_board/read`'s reply. */
export interface AppendInfo {
  ok: boolean;
  message?: string;
  /** The point the items are held by, in the file's own coordinates (micrometres). */
  reference?: PointXY;
  counts?: Record<string, number>;
  /** What the paste will leave out or change on this board (copper on layers it lacks, nets it lacks, references already in use). */
  notes?: string[];
}

/** Is `name` a board file Append Board reads? */
export function isKicadBoardName(name: string): boolean {
  return name.toLowerCase().endsWith(".kicad_pcb");
}

/** The paste that appends the board with its reference point on `at`: the file's text, its origin moved to `at` less the reference. */
export function appendBoardCmd(text: string, reference: PointXY, at: PointXY): Cmd {
  return { op: "paste_clipboard", text, at: { x: at.x - reference.x, y: at.y - reference.y } };
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
