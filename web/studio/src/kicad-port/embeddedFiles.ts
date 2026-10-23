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

/** `EMBEDDED_FILES::Seed()`. */
export const SEED = 0xabba2345;

const M64 = (1n << 64n) - 1n;
const C1 = 0x87c37b91114253d5n;
const C2 = 0x4cf5ad432745937fn;
const rotl = (x: bigint, r: bigint) => ((x << r) | (x >> (64n - r))) & M64;
const fmix = (k: bigint) => {
  k ^= k >> 33n;
  k = (k * 0xff51afd7ed558ccdn) & M64;
  k ^= k >> 33n;
  k = (k * 0xc4ceb9fe1a85ec53n) & M64;
  k ^= k >> 33n;
  return k;
};

/** `MMH3_HASH::addData` and `digest().ToString()` (libs/kimath/include/mmh3_hash.h, hash_128.h): MurmurHash3_x64_128 of `data`, the two halves in upper-case hexadecimal. The
 *  length is KiCad's 32-bit counter. */
export function mmh3x64_128(data: Uint8Array, seed: number): string {
  let h1 = BigInt(seed >>> 0);
  let h2 = h1;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const whole = data.length - (data.length % 16);
  for (let i = 0; i < whole; i += 16) {
    let k1 = view.getBigUint64(i, true);
    let k2 = view.getBigUint64(i + 8, true);
    k1 = (rotl((k1 * C1) & M64, 31n) * C2) & M64;
    h1 ^= k1;
    h1 = (((rotl(h1, 27n) + h2) & M64) * 5n + 0x52dce729n) & M64;
    k2 = (rotl((k2 * C2) & M64, 33n) * C1) & M64;
    h2 ^= k2;
    h2 = (((rotl(h2, 31n) + h1) & M64) * 5n + 0x38495ab5n) & M64;
  }
  const tail = data.length - whole;
  let k1 = 0n;
  let k2 = 0n;
  for (let i = tail - 1; i >= 8; i--) k2 ^= BigInt(data[whole + i] ?? 0) << BigInt((i - 8) * 8);
  if (tail > 8) h2 ^= (rotl((k2 * C2) & M64, 33n) * C1) & M64;
  for (let i = Math.min(tail, 8) - 1; i >= 0; i--) k1 ^= BigInt(data[whole + i] ?? 0) << BigInt(i * 8);
  if (tail > 0) h1 ^= (rotl((k1 * C1) & M64, 31n) * C2) & M64;
  const len = BigInt(data.length >>> 0);
  h1 ^= len;
  h2 ^= len;
  h1 = (h1 + h2) & M64;
  h2 = (h2 + h1) & M64;
  h1 = fmix(h1);
  h2 = fmix(h2);
  h1 = (h1 + h2) & M64;
  h2 = (h2 + h1) & M64;
  return h1.toString(16).toUpperCase().padStart(16, "0") + h2.toString(16).toUpperCase().padStart(16, "0");
}

/** `EMBEDDED_FILES::ComputeFileHash`: the checksum KiCad gives a file with this content. */
export function checksumOf(content: Uint8Array): string {
  return mmh3x64_128(content, SEED);
}

/** The title of `AddEmbeddedFile`'s question. */
export const CONFLICT_TITLE = "Embedded File Conflict";

/**
 * What Add does with a file of this name (`AddEmbeddedFile`): `add` it; when the owner has a file of that name, `reuse` it without a word if its content is the same
 * (`existingFile->data_hash == newFileHash`; `checksum` is the content's, [`checksumOf`]) or ask KiCad's question (Replace / Reuse Existing / Cancel); or refuse a file that is
 * empty (`EMBEDDED_FILES::AddFile` fails on it: "Failed to add file '%s'.") or too large. Without `checksum` a name that is taken always asks.
 */
export function addDecision(
  listing: EmbeddedListing | null,
  name: string,
  size: number,
  checksum?: string
): { kind: "add" } | { kind: "reuse" } | { kind: "conflict"; message: string } | { kind: "refuse"; message: string } {
  const file = fullNameOf(name);
  if (file.trim() === "") return { kind: "refuse", message: `Failed to add file '${name}'.` };
  if (size > MAX_FILE_BYTES) return { kind: "refuse", message: `'${file}' is ${size} bytes; the largest file that can be embedded is ${MAX_FILE_BYTES} bytes.` };
  const existing = listing?.files.find((f) => f.name === file);
  if (existing) {
    if (checksum !== undefined && existing.content_checksum === checksum) return { kind: "reuse" };
    return {
      kind: "conflict",
      message: `A file named '${file}' is already embedded, but the file on disk has different content.\n\nDo you want to replace the embedded file with the new version?`,
    };
  }
  if (size === 0) return { kind: "refuse", message: `Failed to add file '${file}'.` };
  return { kind: "add" };
}

/** `RemoveEmbeddedFile`'s refusal for an inherited file. */
export const INHERITED_MESSAGE = "Embedded files inherited from a parent symbol cannot be removed.";

/** `TransferDataFromWindow`'s question when a file the board drops is also in a footprint's own list (`IsOK`: "Confirmation", Yes / No). */
export const NESTED_QUESTION = "Deleted embedded files are also referenced in some footprints.\nDelete from footprints as well?";
export const NESTED_TITLE = "Confirmation";

/**
 * What Remove does with the rows `names` (`onDeleteEmbeddedFile`: `RemoveEmbeddedFile` for each row, and `TransferDataFromWindow`'s nested removal): the board's own files to
 * remove, and whether to ask the footprints question first (the board, when some footprint lists one of them); an inherited row is not removed and KiCad's message says so
 * (`error`, beside the names when there are some to remove).
 */
export function removePlan(owner: EmbeddedOwner, rows: readonly EmbeddedRow[], names: readonly string[]): { error: string } | { names: string[]; askNested: boolean; error?: string } {
  const wanted = names.map(plainName);
  if (wanted.length === 0) return { error: "No file is selected." };
  const picked = wanted.map((n) => rows.find((r) => r.name === n));
  const inherited = picked.some((r) => r?.inherited);
  const own = picked.filter((r): r is EmbeddedRow => !!r && !r.inherited);
  if (own.length === 0) return { error: inherited ? INHERITED_MESSAGE : `No file named '${wanted[0]}' is embedded.` };
  return { names: own.map((r) => r.name), askNested: owner.kind === "board" && own.some((r) => r.used_by.length > 0), ...(inherited ? { error: INHERITED_MESSAGE } : {}) };
}

/** The files the panel's Export writes: every file of its list, the inherited ones too, whatever is selected (`onExportFiles` walks `m_localFiles`). */
export function exportNames(rows: readonly EmbeddedRow[]): string[] {
  return rows.map((r) => r.name);
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
