// The embedded files of a board, a schematic, a footprint or a symbol (GAPS.md items 24 and 30, package P26): the wire form of `Cmd::Embedded`
// (crates/ops/src/embedded.rs, `{"op": "embedded", "verb": ..}`) and of what `GET /api/embedded` answers (crates/cli/src/embedded_api.rs).

/** Whose list (`EmbeddedOwner`): the board, the schematic, a footprint on the board by reference, a footprint or a symbol of the project's libraries. */
export type EmbeddedOwner =
  | { kind: "board" }
  | { kind: "schematic" }
  | { kind: "footprint"; reference: string }
  | { kind: "library_footprint"; name: string }
  | { kind: "library_symbol"; lib_id: string };

/** `EMBEDDED_FILE::FILE_TYPE`, as `(type ..)` writes it. */
export type EmbeddedFileType = "font" | "model" | "worksheet" | "datasheet" | "other";

/** One edit of a list, one undo step. */
export type EmbeddedCmd =
  /** The panel's Add (`PANEL_EMBEDDED_FILES::AddEmbeddedFile`): `data` is the base64 of the file's bytes; `replace` is the conflict question's "Replace". */
  | { verb: "add"; owner: EmbeddedOwner; name: string; data: string; replace?: boolean; kind?: EmbeddedFileType }
  /** The panel's Remove (`RemoveEmbeddedFile`), by name or by `kicad-embed://` link; `nested` also takes them out of the footprints' lists (Board Setup's question). */
  | { verb: "remove"; owner: EmbeddedOwner; names: string[]; nested?: boolean }
  /** The "Embed fonts" box. */
  | { verb: "set_fonts"; owner: EmbeddedOwner; embed: boolean }
  /** `pcbnew.EditorControl.collect3DModels`: the server reads the model files in. */
  | { verb: "collect_models" };

/** One row of `GET /api/embedded`. */
export interface EmbeddedFileInfo {
  name: string;
  type: EmbeddedFileType;
  type_label: string;
  /** `kicad-embed://name`: the "Embedded Reference" column. */
  link: string;
  /** The size of the content (bytes), from the frame's header; null when it cannot be told. */
  size: number | null;
  /** The size it is stored in (the compressed frame). */
  stored: number;
  checksum: string;
  /** The checksum of the content as KiCad holds it once the file is read (`data_hash` after `DecompressAndDecode`: the current MurmurHash3), which `AddEmbeddedFile` compares a file
   *  added again with; null when the file does not decode. */
  content_checksum: string | null;
  /** `ok`; `legacy` (an older kind of checksum, rewritten on save); `reference` (no data of its own); `error` (does not decode: `message` says why). */
  status: "ok" | "legacy" | "reference" | "error";
  message: string;
  /** For the board: the footprints whose own list has the file. */
  used_by: string[];
}

export interface EmbeddedListing {
  owner: EmbeddedOwner;
  fonts: boolean;
  files: EmbeddedFileInfo[];
  /** For the board: the files only footprints hold (`m_inheritedFiles`), shown and not removable. */
  inherited: EmbeddedFileInfo[];
}

/** `GET /api/embedded/collect`: what Collect and Embed 3D Models would embed. */
export interface CollectPlan {
  count: number;
  models: { path: string; files: string[]; footprints: string[] | null }[];
  skipped: { path: string; reason: string }[];
}
