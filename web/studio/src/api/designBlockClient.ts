// The requests of the Design Blocks panels (crates/cli/src/design_block_api.rs): the libraries and the blocks in them, a block's board file, its preview (kicad-cli's drawing),
// and the dialogs' reads and writes. Placing a block is a command (`import_sheet` with a `block`, `place_design_block`), not a request here.

/** One default field of a block (`DESIGN_BLOCK::GetFields`), in the order the metadata file has them. */
export interface BlockField {
  name: string;
  value: string;
}

/** A block of a library as the tree lists it (`DESIGN_BLOCK`). */
export interface Block {
  name: string;
  description: string;
  keywords: string;
  fields: BlockField[];
  /** The block has a `.kicad_sch` / a `.kicad_pcb`. */
  schematic: boolean;
  board: boolean;
  schematic_path: string | null;
  board_path: string | null;
}

export interface BlockLibrary {
  name: string;
  description: string;
  scope: "global" | "project";
  path: string;
  /** `IsDesignBlockLibWritable`: a block can be saved to it. */
  writable: boolean;
  blocks: Block[];
}

export interface BlocksReply {
  ok: boolean;
  message?: string;
  libraries: BlockLibrary[];
  /** A library or a block that could not be read, and why. */
  problems: { nickname: string; block?: string; error: string }[];
  /** Rows of the library tables that could not be read (KiCad's default table, chained by the global one, is not installed). */
  notes: string[];
  project_dir: string;
}

export interface Refused {
  ok: false;
  message: string;
  /** The block exists already: say so and ask (`checkOverwriteDb`). */
  exists?: boolean;
}

async function getJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const r = await fetch(url, { cache: "no-store", signal });
  return (await r.json()) as T;
}

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

/** `GET /api/design_blocks`. */
export function fetchBlocks(): Promise<BlocksReply> {
  return getJson<BlocksReply>("/api/design_blocks");
}

/** A block's board file (`/file?what=pcb`) or schematic. */
export async function fetchBlockFile(lib: string, name: string, what: "pcb" | "sch"): Promise<{ ok: true; text: string; path: string; name: string } | Refused> {
  const q = new URLSearchParams({ lib, name, what });
  return getJson(`/api/design_blocks/file?${q.toString()}`);
}

/** kicad-cli's drawing of a block's schematic or board. */
export async function fetchBlockPreview(lib: string, name: string, what: "pcb" | "sch", signal?: AbortSignal): Promise<{ ok: true; svg: string; what: string } | Refused> {
  const q = new URLSearchParams({ lib, name, what });
  return getJson(`/api/design_blocks/preview?${q.toString()}`, signal);
}

/** What a Save as Design Block saves: the sheet in view, a selection of it, the board, or a selection of the board. */
export type SaveSource =
  | { what: "sheet"; sheet: string }
  | { what: "selection"; sheet: string; ids: string[] }
  | { what: "board" }
  | { what: "board_selection"; ids: string[] };

export interface PreparedSave {
  ok: true;
  name: string;
  description: string;
  keywords: string;
  fields: BlockField[];
  /** An update: the block already has the design the save would replace. */
  has_design?: boolean;
  items: number;
  library: string;
}

/** The refusals that come before the dialog, and what the dialog opens with. `update` names the block an Update Design Block... saves to. */
export function prepareSave(req: SaveSource & { lib: string; update?: { name: string } }): Promise<PreparedSave | Refused> {
  return postJson("/api/design_blocks/prepare", req);
}

export interface SavedBlock {
  ok: true;
  lib: string;
  name: string;
  lib_id: string;
  created: boolean;
  /** The selection was grouped and linked to the block. */
  linked: boolean;
  link_error?: string | null;
}

/** The block written to its library (and a selection linked to it, one undoable step). */
export function saveBlock(req: SaveSource & { lib: string; name: string; description?: string; keywords?: string; fields?: BlockField[]; overwrite?: boolean; update?: boolean }): Promise<SavedBlock | Refused> {
  return postJson("/api/design_blocks/save", req);
}

/** `EditDesignBlockProperties`: the metadata changes; a new name saves the block under it and deletes the old one. */
export function saveBlockProperties(req: { lib: string; name: string; new_name?: string; description: string; keywords: string; fields: BlockField[]; overwrite?: boolean }): Promise<{ ok: true; lib: string; name: string; lib_id: string; renamed: boolean } | Refused> {
  return postJson("/api/design_blocks/properties", req);
}

/** `DeleteDesignBlockFromLibrary`: the block's whole folder. */
export function deleteBlock(lib: string, name: string): Promise<{ ok: true; message: string } | Refused> {
  return postJson("/api/design_blocks/delete", { lib, name });
}
