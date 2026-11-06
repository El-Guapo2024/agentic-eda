// The Embedded Files panel (`PANEL_EMBEDDED_FILES`, common/dialogs/panel_embedded_files.cpp at KiCad 8303b2ad): the rows of its grid and what its buttons decide before a verb is sent.
// The panel is components/EmbeddedFilesPanel.tsx; the verbs are `Cmd::Embedded` (crates/ops/src/embedded.rs), the list `GET /api/embedded` (crates/cli/src/embedded_api.rs).
//
// KiCad's panel edits a copy of the list (`m_localFiles`) and writes it back on OK (`TransferDataFromWindow`). So does this one: Add Files, Remove and the "Embed fonts" box
// change an [`EmbeddedDraft`] over the list the server gave, the grid shows the copy ([`draftRows`]), and the OK of the dialog -- the page's Apply in Board Setup and Schematic
// Setup, whose pages each apply on their own -- sends it as one `apply` verb ([`applyCmd`]), one undo step.
import type { Cmd } from "../api/types";
import type { EmbeddedFileInfo, EmbeddedFileType, EmbeddedListing, EmbeddedOwner, TextFont } from "../api/embeddedTypes";

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

// ------------------------------------------------------------------------------------------------------------------------------------- the panel's copy

/** A file Add Files took into the copy (`AddEmbeddedFile`): its content as base64, and the `Blob` it came from (its checksum is worked out when a name is taken again). */
export interface PendingFile {
  name: string;
  data: string;
  size: number;
  blob?: Blob;
}

/**
 * The panel's copy of the list (`m_localFiles`) as what was done to the list the server gave: the owner's files taken out (`removed`), the files Add Files took in (`added`, each
 * replacing a file of its name), and the "Embed fonts" box -- `fonts` null while it was not clicked; a click (`onFontEmbedClick`) drops every font of the copy (`refreshFonts`)
 * and, ticked, takes in the fonts the text uses (`textFonts`, `GET /api/embedded/fonts`).
 */
export interface EmbeddedDraft {
  removed: string[];
  added: PendingFile[];
  fonts: boolean | null;
  refreshFonts: boolean;
  textFonts: TextFont[];
}

export const EMPTY_DRAFT: EmbeddedDraft = { removed: [], added: [], fonts: null, refreshFonts: false, textFonts: [] };

/** Whether the copy differs from the list: what the dialog's OK has to send, and what Board Setup's page marks as not applied. */
export function draftDirty(d: EmbeddedDraft): boolean {
  return d.removed.length > 0 || d.added.length > 0 || d.refreshFonts || d.fonts !== null;
}

/** The "Embed fonts" box as the copy has it (`m_cbEmbedFonts`). */
export function draftFontsOn(listing: EmbeddedListing | null, d: EmbeddedDraft): boolean {
  return d.fonts ?? !!listing?.fonts;
}

/** `EMBEDDED_FILES::AddFile`'s types by extension (`eda_model::embedded_files::EmbeddedFileType::of_file_name`). */
export function typeOfName(name: string): EmbeddedFileType {
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1).toUpperCase() : "";
  if (["STP", "STPZ", "STEP", "WRL", "WRZ"].includes(ext)) return "model";
  if (["WOFF", "WOFF2", "TTF", "OTF"].includes(ext)) return "font";
  if (ext === "PDF") return "datasheet";
  if (ext === "KICAD_WKS") return "worksheet";
  return "other";
}

const TYPE_LABELS: Record<EmbeddedFileType, string> = { font: "Font", model: "3D model", worksheet: "Worksheet", datasheet: "Datasheet", other: "Other" };

/** One row of the copy: a file of the list, or one the panel took in and has not applied (`pending`). */
export interface DraftRow extends EmbeddedRow {
  pending: boolean;
}

function pendingRow(name: string, type: EmbeddedFileType, size: number): DraftRow {
  return { name, type, type_label: TYPE_LABELS[type], link: URI_PREFIX + name, size, stored: 0, checksum: "", content_checksum: null, status: "ok", message: "", used_by: [], inherited: false, pending: true };
}

/** The grid (`TransferDataToWindow` over `m_localFiles`, kept up by the copy's callbacks): sorted by name, as the `std::map` is. */
export function draftRows(listing: EmbeddedListing | null, d: EmbeddedDraft): DraftRow[] {
  const byName = new Map<string, DraftRow>();
  const gone = (f: EmbeddedFileInfo) => d.refreshFonts && f.type === "font";
  for (const r of rowsOf(listing)) if (!(r.inherited ? gone(r) : d.removed.includes(r.name) || gone(r))) byName.set(r.name, { ...r, pending: false });
  for (const a of d.added) byName.set(a.name, pendingRow(a.name, typeOfName(a.name), a.size));
  for (const f of d.textFonts) byName.set(f.name, pendingRow(f.name, "font", f.size));
  return [...byName.values()].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** Add Files' file into the copy (`m_localFiles->AddFile( fileName, true )`): it replaces a row of its name, a font row the click took in too. */
export function draftAdd(d: EmbeddedDraft, file: PendingFile): EmbeddedDraft {
  const name = fullNameOf(file.name);
  return { ...d, added: [...d.added.filter((a) => a.name !== name), { ...file, name }], textFonts: d.textFonts.filter((f) => f.name !== name) };
}

/** Remove of a row of the copy (`RemoveEmbeddedFile`): a file the panel took in is dropped; a file of the list is taken out, also when a file taken in had replaced it. */
export function draftRemove(listing: EmbeddedListing | null, d: EmbeddedDraft, name: string): EmbeddedDraft {
  const own = listing?.files.some((f) => f.name === name && !(d.refreshFonts && f.type === "font")) ?? false;
  return {
    ...d,
    added: d.added.filter((a) => a.name !== name),
    textFonts: d.textFonts.filter((f) => f.name !== name),
    removed: own && !d.removed.includes(name) ? [...d.removed, name] : d.removed,
  };
}

/** A click on "Embed fonts" (`onFontEmbedClick`): every font row goes -- the ones Add Files took in too -- and, ticked, the fonts the text uses come in. */
export function draftFontsClick(d: EmbeddedDraft, checked: boolean, used: readonly TextFont[]): EmbeddedDraft {
  return { ...d, fonts: checked, refreshFonts: true, added: d.added.filter((a) => typeOfName(a.name) !== "font"), textFonts: checked ? [...used] : [] };
}

/** What Add does with a file of this name in the copy: [`addDecision`] against the copy's own rows (a row taken in counts with the checksum of its content). */
export function draftListing(listing: EmbeddedListing | null, d: EmbeddedDraft, pendingChecksums: ReadonlyMap<string, string> = new Map()): EmbeddedListing | null {
  if (!listing) return null;
  const rows = draftRows(listing, d).filter((r) => !r.inherited);
  return { ...listing, files: rows.map((r) => (r.pending ? { ...r, content_checksum: pendingChecksums.get(r.name) ?? null } : r)) };
}

/** The dialog's OK (`TransferDataFromWindow`): the copy as one `apply` verb, or null when it is the list. `nested` is the answer to the footprints question. */
export function applyCmd(owner: EmbeddedOwner, listing: EmbeddedListing | null, d: EmbeddedDraft, nested = false): Cmd | null {
  if (!draftDirty(d)) return null;
  return {
    op: "embedded",
    verb: "apply",
    owner,
    removed: d.removed,
    added: d.added.map((a) => ({ name: a.name, data: a.data })),
    fonts: draftFontsOn(listing, d),
    ...(d.refreshFonts ? { refresh_fonts: true, text_fonts: d.textFonts.map((f) => f.name) } : {}),
    ...(nested ? { nested: true } : {}),
  };
}

/** Whether the OK asks the footprints question (`TransferDataFromWindow`'s `confirmDelete`): the board drops a file some footprint lists too. */
export function asksNested(owner: EmbeddedOwner, listing: EmbeddedListing | null, d: EmbeddedDraft): boolean {
  if (owner.kind !== "board" || !listing) return false;
  const kept = new Set(draftRows(listing, d).filter((r) => !r.inherited).map((r) => r.name));
  return listing.files.some((f) => !kept.has(f.name) && f.used_by.length > 0);
}

// ------------------------------------------------------------------------------------------------------------------------------------- Export

/** `onExportFiles`' question for a file that is there (`KIDIALOG`, "Confirmation", Overwrite / Skip, with "Do not show again"). */
export function existsMessage(name: string): string {
  return `File '${name}' already exists.`;
}
export const EXPORT_DIR_TITLE = "Select a directory to export files";
export const failedOpenMessage = (name: string) => `Failed to open file '${name}'.`;
export const failedWriteMessage = (name: string) => `Failed to write file '${name}'.`;

/** `KIDIALOG::DoNotShowCheckbox`'s memory (`g_doNotShowAgainDlgs`): the answer a question was given with "Do not show again" ticked, for as long as the page is open. */
const doNotShowAgain = new Map<string, "overwrite" | "skip">();
export function rememberedAnswer(key: string): "overwrite" | "skip" | undefined {
  return doNotShowAgain.get(key);
}
export function rememberAnswer(key: string, answer: "overwrite" | "skip"): void {
  doNotShowAgain.set(key, answer);
}
/** `KIDIALOG::ClearDoNotShowAgainDialogs`. */
export function clearRememberedAnswers(): void {
  doNotShowAgain.clear();
}

/** The folder Export writes into, as the File System Access API hands one out (the subset used). */
export interface ExportFolder {
  getFileHandle(name: string, options?: { create?: boolean }): Promise<{ createWritable(): Promise<{ write(data: Uint8Array): Promise<void>; close(): Promise<void> }> }>;
}

/**
 * `PANEL_EMBEDDED_FILES::onExportFiles` into `folder`: every file of the copy in order, a file that is there asks first (`ask`: Overwrite or Skip; with "Do not show again" the
 * answer stands for the rest of the session), one that cannot be opened or written is said so (`report`) and the next one goes on. Returns the names written.
 */
export async function exportInto(
  folder: ExportFolder,
  files: readonly { name: string; bytes: () => Promise<Uint8Array> }[],
  ask: (message: string) => Promise<{ answer: "overwrite" | "skip"; remember: boolean }>,
  report: (message: string) => void
): Promise<string[]> {
  const written: string[] = [];
  for (const f of files) {
    let exists = false;
    try {
      await folder.getFileHandle(f.name);
      exists = true;
    } catch {
      exists = false;
    }
    if (exists) {
      let answer = rememberedAnswer("embedded-export-exists");
      if (!answer) {
        const r = await ask(existsMessage(f.name));
        answer = r.answer;
        if (r.remember) rememberAnswer("embedded-export-exists", answer);
      }
      if (answer !== "overwrite") continue;
    }
    let out: Awaited<ReturnType<Awaited<ReturnType<ExportFolder["getFileHandle"]>>["createWritable"]>>;
    try {
      out = await (await folder.getFileHandle(f.name, { create: true })).createWritable();
    } catch {
      report(failedOpenMessage(f.name));
      continue;
    }
    try {
      await out.write(await f.bytes());
      await out.close();
      written.push(f.name);
    } catch {
      report(failedWriteMessage(f.name));
    }
  }
  return written;
}

/** The bytes of a base64 text (a file the panel took in). */
export function bytesOfBase64(data: string): Uint8Array {
  const bin = atob(data);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ------------------------------------------------------------------------------------------------------------------------------------- documents

/** The lists `GetAssociatedDocument` looks a `kicad-embed://` document up in, in order (`aFilesStack`). */
export type FilesStack = EmbeddedOwner[];

/** Whether a datasheet names an embedded file (`uri.GetScheme().Lower() == FILEEXT::KiCadUriPrefix`). */
export function isEmbeddedLink(text: string | null | undefined): boolean {
  return /^kicad-embed:\/\//i.test((text ?? "").trim());
}
