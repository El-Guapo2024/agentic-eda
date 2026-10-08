// Cut, Copy, Paste, Paste Special and Duplicate on the Schematic tab -- `SCH_EDITOR_CONTROL::Cut / Copy / Paste / Duplicate` and `doCopy`
// (eeschema/tools/sch_editor_control.cpp at 8303b2ad), registered into useActionRunner's handler map (actions/schActionRegistry.ts: the PCB's own
// handlers of the same names keep their tab).
//
//  * Copy puts KiCad's clipboard text for the selection on the system clipboard (`POST /api/sch/clipboard/copy`: a `(lib_symbols ...)` and the selected
//    items, what real KiCad pastes). Cut is a Copy and then the delete of what was copied, locked items excepted (`DoDelete`).
//  * Paste reads the system clipboard -- KiCad's text from KiCad, or from this studio -- and shows what it would add following the cursor; a click
//    sends the `paste_sch` command (one undo step), Escape throws the paste away. Text that is not a fragment is pasted as a text item.
//  * Paste Special asks first how the reference designators of the pasted symbols are handled (components/SchPasteSpecialDialog.tsx).
//  * Duplicate copies into a buffer of its own (`m_duplicateClipboard`: the system clipboard is left alone) and pastes it at once, carried by the
//    connection point of the copy nearest the cursor -- so it appears in place and follows the cursor.
//
// The browser may refuse to read the system clipboard (no permission); the text of the last copy made here is the fallback.
import type { Dispatch } from "react";
import { fetchSchematic } from "../api/client";
import { postSchCopy, postSchParse } from "../api/schClipboardClient";
import { clipboardTextOrNull, newIdsAfter, pasteOffset, type SchPasteMode } from "../kicad-port/schClipboard";
import { deleteCmds } from "../kicad-port/schDelete";
import { getSchPaste, openPasteSpecial, setSchPaste } from "../state/schPasteStore";
import type { Action, StudioApi, StudioState } from "../state/store";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

/** The text of the last Copy made here: what Paste falls back on when the browser will not let the page read the system clipboard. */
let lastCopied: string | null = null;
/** `SCH_EDITOR_CONTROL::m_duplicateClipboard`. */
let duplicateBuffer: string | null = null;

async function writeSystemClipboard(text: string): Promise<void> {
  lastCopied = text;
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Refused (no permission, insecure page): the copy still pastes inside the studio.
  }
}

async function readSystemClipboard(): Promise<string | null> {
  try {
    const text = clipboardTextOrNull(await navigator.clipboard.readText());
    if (text) return text;
  } catch {
    // Refused: fall through to the last copy made here.
  }
  return clipboardTextOrNull(lastCopied);
}

const toast = (dispatch: Dispatch<Action>, message: string, kind: "error" | "info" = "info") => dispatch({ type: "TOAST", message, kind });

/** What starting a paste needs of the studio. */
export interface SchPasteContext {
  state: StudioState;
  dispatch: Dispatch<Action>;
}

/**
 * `SCH_EDITOR_CONTROL::Paste` up to the Move tool: read the clipboard (or, for Duplicate, the text given), have the server work out what it would
 * add to the sheet in view and where the cursor holds it, and carry that with the cursor. Nothing is stored until the click.
 */
export async function startPaste(ctx: SchPasteContext, opts: { mode: SchPasteMode; duplicate: boolean; text?: string | null }): Promise<void> {
  const { state, dispatch } = ctx;
  const text = clipboardTextOrNull(opts.text) ?? (opts.duplicate ? clipboardTextOrNull(duplicateBuffer) : await readSystemClipboard());
  if (!text) {
    toast(dispatch, "The clipboard is empty.");
    return;
  }
  let reply;
  try {
    reply = await postSchParse({ text, sheet: state.currentSheetPath, mode: opts.mode, duplicate: opts.duplicate, cursor: state.cursorUm ? [state.cursorUm.x, state.cursorUm.y] : null });
  } catch (e) {
    toast(dispatch, e instanceof Error ? e.message : String(e), "error");
    return;
  }
  if (!reply.ok || !reply.preview) {
    toast(dispatch, reply.message ?? "There is nothing to paste.", "error");
    return;
  }
  // `m_MaxPastedTextLength`: a long text asks first.
  if (reply.kind === "text" && reply.long_text && !window.confirm("Pasting a long text string may be very slow.  Do you want to continue?")) return;
  if (reply.notes?.length) toast(dispatch, reply.notes.join("; "));
  // `selectionClear`, then the pasted items are the selection.
  dispatch({ type: "CLEAR_SELECTION" });
  setSchPaste({ fragment: reply.fragment, preview: reply.preview, anchor: reply.anchor ?? [0, 0], sheet: [...state.currentSheetPath], mode: opts.mode, origin: opts.duplicate ? "duplicate" : "paste", notes: reply.notes ?? [] });
}

/** The click that drops the paste (`commit.Push( _( "Paste" ) )`): one `paste_sch` command, then the new items are the selection. `snapped` is the grid-snapped cursor. */
export async function placeSchPaste(api: StudioApi, dispatch: Dispatch<Action>, snapped: [number, number]): Promise<void> {
  const paste = getSchPaste();
  if (!paste) return;
  const [dx, dy] = pasteOffset(paste.anchor, snapped);
  setSchPaste(null);
  const before = api.getState().schematic;
  const ok = await api.cmd({ op: "paste_sch", fragment: paste.fragment, dx, dy, mode: paste.mode });
  if (!ok || !before) return;
  try {
    const ids = newIdsAfter(before, await fetchSchematic(paste.sheet));
    if (ids.length > 0) dispatch({ type: "SET_SELECTION", refs: ids });
  } catch {
    // The poll will show the new items; only the selection of them is lost.
  }
}

export function registerSchClipboardActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, dispatch, api, requestSelection } = ctx;
  const m = schematicActions(registry, state.tab);
  const sch = state.schematic;

  /** `doCopy`: KiCad's clipboard text for `ids`, or null (with the reason shown) when there is none. */
  const clipboardTextFor = async (ids: string[]): Promise<string | null> => {
    if (ids.length === 0) return null;
    try {
      const reply = await postSchCopy(ids, state.currentSheetPath);
      if (!reply.ok || !reply.text) {
        toast(dispatch, reply.message ?? "Nothing in the selection can be copied.", "error");
        return null;
      }
      if (reply.skipped?.length) toast(dispatch, `Not copied: ${reply.skipped.join(", ")}.`);
      return reply.text;
    } catch (e) {
      toast(dispatch, e instanceof Error ? e.message : String(e), "error");
      return null;
    }
  };

  const copy = async (ids: string[]): Promise<boolean> => {
    const text = await clipboardTextFor(ids);
    if (!text) return false;
    await writeSystemClipboard(text);
    return true;
  };

  // Ctrl+C -- SCH_EDITOR_CONTROL::Copy: `RequestSelection` (the selection, else the item under the cursor), onto the system clipboard.
  m.set("common.Interactive.copy", () => void copy(requestSelection()));

  // Ctrl+X -- SCH_EDITOR_CONTROL::Cut: `doCopy()`, and only when that worked `ACTIONS::doDelete` -- one undo step, locked items stay.
  m.set("common.Interactive.cut", () => {
    const ids = requestSelection();
    void copy(ids).then((copied) => {
      if (!copied || !sch) return;
      const cmds = deleteCmds(sch, ids, new Set(sch.locked ?? []));
      dispatch({ type: "CLEAR_SELECTION" });
      if (cmds.length > 0) void api.cmdBatch(cmds);
    });
  });

  // Ctrl+V -- SCH_EDITOR_CONTROL::Paste: the unique-reference mode (`annotation.automatic` is on by default).
  m.set("common.Interactive.paste", () => void startPaste(ctx, { mode: "unique", duplicate: false }));

  // Ctrl+Shift+V -- the same through DIALOG_PASTE_SPECIAL.
  m.set("common.Interactive.pasteSpecial", () => openPasteSpecial());

  // Ctrl+D -- SCH_EDITOR_CONTROL::Duplicate: `doCopy( true )` into the buffer of its own, then Paste.
  m.set("common.Interactive.duplicate", () => {
    void clipboardTextFor(requestSelection()).then((text) => {
      if (!text) return;
      duplicateBuffer = text;
      return startPaste(ctx, { mode: "unique", duplicate: true, text });
    });
  });
}
