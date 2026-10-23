// The Embedded Files panel (`PANEL_EMBEDDED_FILES`, common/dialogs/panel_embedded_files.cpp at KiCad 8303b2ad): the rows of its grid and what its buttons decide before a verb is sent.
// The panel is components/EmbeddedFilesPanel.tsx; the verbs are `Cmd::Embedded` (crates/ops/src/embedded.rs), the list `GET /api/embedded` (crates/cli/src/embedded_api.rs).
//
// KiCad's panel edits a copy of the list (`m_localFiles`) and writes it back on OK (`TransferDataFromWindow`); here each Add, Remove and the "Embed fonts" box is a verb of its
// own, one undo step, as the studio's other setup pages that are not a table of rows do.
import type { Cmd } from "../api/types";
import type { EmbeddedFileInfo, EmbeddedListing, EmbeddedOwner } from "../api/embeddedTypes";

/** The largest file the Add verb takes (`eda_ops::embedded::MAX_FILE_BYTES`): the server reads at most 64 MiB of a request and the content comes as base64 in it. */
export const MAX_FILE_BYTES = 40 * 1024 * 1024;

/** `FILEEXT::KiCadUriPrefix + "://"`. */
export const URI_PREFIX = "kicad-embed://";

/** One row of the grid: the file, and whether it is inherited (`m_inheritedFileNames`: Board Setup's files that only footprints hold). */
export interface EmbeddedRow extends EmbeddedFileInfo {
  inherited: boolean;
}

/** The grid's rows (`TransferDataToWindow`): the owner's files and the inherited ones in one list sorted by name -- `m_localFiles` is a `std::map` -- an own file winning a name. */
export function rowsOf(listing: EmbeddedListing | null): EmbeddedRow[] {
  if (!listing) return [];
  const byName = new Map<string, EmbeddedRow>();
  for (const f of listing.files) byName.set(f.name, { ...f, inherited: false });
  for (const f of listing.inherited) if (!byName.has(f.name)) byName.set(f.name, { ...f, inherited: true });
  return [...byName.values()].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** `wxFileName::GetFullName()`: what a file is filed under (`AddEmbeddedFile`). */
export function fullNameOf(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/** A name as the panel takes it: the name, or the link with its prefix taken off (`RemoveEmbeddedFile`). */
export function plainName(name: string): string {
  return name.startsWith(URI_PREFIX) ? name.slice(URI_PREFIX.length) : name;
}

/**
 * What Add does with a file of this name (`AddEmbeddedFile`): `add` it; ask the conflict question ("A file named '%s' is already embedded, but the file on disk has different
 * content. Would you like to replace the existing file or reuse it?") when the owner has a file of that name; or refuse a file that is too large or not readable. KiCad asks
 * only when the content differs; the page cannot tell without the content's checksum, so it asks whenever the name is taken, and the server answers "already embedded" for the same
 * content.
 */
export function addDecision(listing: EmbeddedListing | null, name: string, size: number): { kind: "add" } | { kind: "conflict"; message: string } | { kind: "refuse"; message: string } {
  const file = fullNameOf(name);
  if (file.trim() === "") return { kind: "refuse", message: `Failed to add file '${name}'.` };
  if (size > MAX_FILE_BYTES) return { kind: "refuse", message: `'${file}' is ${size} bytes; the largest file that can be embedded is ${MAX_FILE_BYTES} bytes.` };
  if (listing?.files.some((f) => f.name === file)) {
    return { kind: "conflict", message: `A file named '${file}' is already embedded, but the file on disk has different content.\nWould you like to replace the existing file or reuse it?` };
  }
  return { kind: "add" };
}

/** `RemoveEmbeddedFile`'s refusal for an inherited file. */
export const INHERITED_MESSAGE = "Embedded files inherited from a parent symbol cannot be removed.";

/** `TransferDataFromWindow`'s question when a file the board drops is also in a footprint's own list. */
export const NESTED_QUESTION = "Deleted embedded files are also referenced in some footprints.\nDelete from footprints as well?";

/**
 * What Remove does with the rows `names` (`onDeleteEmbeddedFile`, `RemoveEmbeddedFile`, and `TransferDataFromWindow`'s nested removal): refused with KiCad's message when one is
 * inherited; else the names to remove, and whether to ask the footprints question first (the board, when some footprint lists one of them).
 */
export function removePlan(owner: EmbeddedOwner, rows: readonly EmbeddedRow[], names: readonly string[]): { error: string } | { names: string[]; askNested: boolean } {
  const wanted = names.map(plainName);
  if (wanted.length === 0) return { error: "No file is selected." };
  const picked = wanted.map((n) => rows.find((r) => r.name === n));
  if (picked.some((r) => r?.inherited)) return { error: INHERITED_MESSAGE };
  const own = picked.filter((r): r is EmbeddedRow => !!r);
  if (own.length === 0) return { error: `No file named '${wanted[0]}' is embedded.` };
  return { names: own.map((r) => r.name), askNested: owner.kind === "board" && own.some((r) => r.used_by.length > 0) };
}

export function addCmd(owner: EmbeddedOwner, name: string, data: string, replace = false): Cmd {
  return { op: "embedded", verb: "add", owner, name: fullNameOf(name), data, ...(replace ? { replace: true } : {}) };
}

export function removeCmd(owner: EmbeddedOwner, names: string[], nested = false): Cmd {
  return { op: "embedded", verb: "remove", owner, names, ...(nested ? { nested: true } : {}) };
}

export function fontsCmd(owner: EmbeddedOwner, embed: boolean): Cmd {
  return { op: "embedded", verb: "set_fonts", owner, embed };
}

/** Collect and Embed 3D Models: the server reads the files in (`with_collected_models`). */
export const COLLECT_MODELS_CMD: Cmd = { op: "embedded", verb: "collect_models" };

/** `PCB_CONTROL::CollectAndEmbed3DModels`' info bar message. */
export function collectedMessage(count: number): string {
  return `${count} 3D model(s) successfully embedded.`;
}

/** A size as the grid shows it. */
export function sizeText(bytes: number | null): string {
  if (bytes == null) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
