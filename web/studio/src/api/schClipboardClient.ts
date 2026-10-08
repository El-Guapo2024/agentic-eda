// Browser-side halves of the schematic clipboard that need the server: the selection as KiCad's clipboard text (`SCH_EDITOR_CONTROL::doCopy`)
// and a clipboard text as a paste (`SCH_EDITOR_CONTROL::Paste`). The edit itself is the `paste_sch` command (api/types.ts). Kept apart from
// `client.ts` so that shared file does not grow.
import type { SchPasteMode, SchPastePreview } from "../kicad-port/schClipboard";
import { postJson } from "./schControlClient";

/** `POST /api/sch/clipboard/copy`: the text KiCad's clipboard would hold for the selection, how many items it has, and what a copy leaves out. */
export interface SchCopyReply {
  ok: boolean;
  message?: string;
  text?: string;
  items?: number;
  skipped?: string[];
}

/** `ids` are what the studio selects by on the sheet at `sheet` (the path of `SheetInstance::id`s from the root; `[]` is the root). */
export function postSchCopy(ids: readonly string[], sheet: readonly string[]): Promise<SchCopyReply> {
  return postJson("/api/sch/clipboard/copy", { ids, sheet: sheet.join("/") });
}

/** `POST /api/sch/clipboard/parse`: a clipboard text as a paste -- see crates/cli/src/sch_clipboard_api.rs. */
export interface SchParseReply {
  ok: boolean;
  message?: string;
  /** `"text"`: the clipboard is not a schematic fragment, so it comes as a single text item (what KiCad pastes for it). */
  kind?: "fragment" | "text";
  /** A plain text longer than `m_MaxPastedTextLength`: KiCad asks before pasting it. */
  long_text?: boolean;
  /** What a `paste_sch` command carries -- opaque here. */
  fragment?: unknown;
  /** The point of the fragment the cursor carries (KiCad's frame), or null for none. */
  anchor?: [number, number] | null;
  /** What the paste would add to the sheet in view. */
  preview?: SchPastePreview;
  /** What the reader could not carry across. */
  notes?: string[];
}

export function postSchParse(body: { text: string; sheet: readonly string[]; mode: SchPasteMode; duplicate: boolean; cursor: [number, number] | null }): Promise<SchParseReply> {
  return postJson("/api/sch/clipboard/parse", { text: body.text, sheet: body.sheet.join("/"), mode: body.mode, duplicate: body.duplicate, cursor: body.cursor });
}
