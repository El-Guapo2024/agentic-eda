// Find and Replace in the Symbol Editor (`ACTIONS::find`, `findAndReplace`, `findNext`, `findPrevious`, `replaceAndFindNext`, `replaceAll` in a
// `SYMBOL_EDIT_FRAME`): `SCH_FIND_REPLACE_TOOL` over the symbol being edited, with the dialog's settings and the find cursor kept here (the frame's
// `m_findReplaceData`, the tool's `m_afterItem` and `m_wrapAroundTimer`). The walk and its rules are kicad-port/symFind.ts; the matching of each text is the
// schematic's matcher on the server (`POST /api/find/text`); every Replace is one `/api/cmd` batch, one undo step of the Symbol Editor.
import { useSyncExternalStore, type Dispatch } from "react";
import type { Cmd, SchSearchData } from "../api/types";
import { postJson } from "../api/schControlClient";
import { defaultSearch, findStartCursor } from "../kicad-port/schFind";
import { allFields, wireField, type FieldRow } from "../kicad-port/symFields";
import {
  currentSymMatch,
  editorFields,
  findStringFromSelection,
  matchedItems,
  nextSymMatch,
  replaceEdit,
  replaceTexts,
  symFindItems,
  SYMBOL_END_MESSAGE,
  textsOf,
  type SymFindItem,
  type SymReplaceEdit,
} from "../kicad-port/symFind";
import { worldToScreen } from "../kicad-port/view";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";

export interface SymFindState {
  /** The dialog: `find` (Find) or `replace` (Find and Replace), or closed. */
  open: "find" | "replace" | null;
  search: SchSearchData;
  /** "Search the current selection only" (`searchSelectedOnly`). */
  selectedOnly: boolean;
  /** `m_afterItem`: the id of the last match found. */
  cursor: string | null;
  /** When "Reached end of symbol." was last shown (`m_wrapAroundTimer` started), ms; a Find within 4 s of it starts over. */
  wrapArmedAt: number | null;
  status: string;
}

/** `SCH_SEARCH_DATA` as the dialog leaves it in the Symbol Editor: `searchAllPins = true` (and its box hidden). */
const initial = (): SymFindState => ({ open: null, search: { ...defaultSearch(), search_pins: true }, selectedOnly: false, cursor: null, wrapArmedAt: null, status: "" });

let current: SymFindState = initial();
const listeners = new Set<() => void>();
function set(patch: Partial<SymFindState>): void {
  current = { ...current, ...patch };
  for (const l of listeners) l();
}
export function getSymFind(): SymFindState {
  return current;
}
export function useSymFind(): SymFindState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    getSymFind,
    getSymFind
  );
}

/** A change to the search (the find text or an option) starts the walk over (`m_findDirty`, `m_afterItem`) and clears the status. */
export function updateSymSearch(patch: Partial<SchSearchData>): void {
  set({ search: { ...current.search, ...patch }, cursor: null, wrapArmedAt: null, status: "" });
}
export function setSymSelectedOnly(on: boolean): void {
  set({ selectedOnly: on, cursor: null, wrapArmedAt: null, status: "" });
}
export function closeSymFind(): void {
  set({ open: null, status: "" });
}

/** `SCH_BASE_FRAME::ShowFindReplaceDialog( aReplace )`: the dialog, with the one selected field's or text's text to search for when there is one. */
export function showSymFind(replace: boolean, api: SymbolEditorApi): void {
  const st = api.getState();
  const fields = st.symbol ? editorFields(st.symbol) : [];
  const text = findStringFromSelection([...st.selection], st.symbol, fields);
  set({ open: replace ? "replace" : "find", search: { ...current.search, find: text || current.search.find, search_pins: true }, status: "" });
}

interface MatchReply {
  ok: boolean;
  matches?: boolean[];
  replaced?: (string | null)[];
  message?: string;
}

async function askMatcher(search: SchSearchData, texts: string[], replace: boolean): Promise<MatchReply> {
  return postJson<MatchReply>("/api/find/text", { search: { ...search, search_pins: true }, texts, replace });
}

/** `EDA_DRAW_FRAME::FocusOnLocation`: the view is moved to put the item in the middle when it is off the canvas. */
function focusOn(api: SymbolEditorApi, dispatch: Dispatch<SymAction>, at: [number, number]): void {
  const rect = document.querySelector(".editor-canvas-col .pcb-canvas-container")?.getBoundingClientRect();
  if (!rect) return;
  const view = api.getState().view;
  const [sx, sy] = worldToScreen(view, at[0], at[1]);
  if (sx >= 0 && sy >= 0 && sx <= rect.width && sy <= rect.height) return;
  dispatch({ type: "SET_VIEW", view: { scale: view.scale, x: rect.width / 2 - at[0] * view.scale, y: rect.height / 2 - at[1] * view.scale } });
}

function items(api: SymbolEditorApi, selectedOnly: boolean): { items: SymFindItem[]; fields: FieldRow[] } {
  const st = api.getState();
  if (!st.symbol) return { items: [], fields: [] };
  const fields = editorFields(st.symbol);
  return { items: symFindItems(st.symbol, fields, current.search.search_hidden_fields, selectedOnly ? st.selection : null), fields };
}

/** `SCH_FIND_REPLACE_TOOL::FindNext` (`findNext`, `findPrevious`): select the next match and show it, or say the end of the symbol was reached. */
export async function symFindNext(api: SymbolEditorApi, dispatch: Dispatch<SymAction>, reversed: boolean): Promise<void> {
  const st = api.getState();
  if (!st.symbol) return;
  // `else if( data.findString.IsEmpty() ) return FindAndReplace( ACTIONS::find.MakeEvent() );`
  if (current.search.find === "") return showSymFind(false, api);
  const now = Date.now();
  const after = findStartCursor(current.cursor, current.wrapArmedAt, now, false);
  const wrapped = after !== current.cursor;
  const { items: list } = items(api, current.selectedOnly);
  let reply: MatchReply;
  try {
    reply = await askMatcher(current.search, textsOf(list).texts, false);
  } catch (e) {
    return set({ status: e instanceof Error ? e.message : String(e) });
  }
  const found = nextSymMatch(list, matchedItems(list, reply.matches ?? []), after, reversed);
  if (found) {
    set({ cursor: found.id, wrapArmedAt: null, status: "" });
    // "Normal find modifies the selection, but selection-based find does not"
    if (!current.selectedOnly) dispatch({ type: "SET_SELECTION", refs: [found.id] });
    focusOn(api, dispatch, found.at);
  } else {
    // `ShowFindReplaceStatus( msg + " " + _( "Find again to wrap around to the start." ), 4000 )` and `m_wrapAroundTimer.StartOnce( 4000 )`
    set({ cursor: wrapped ? null : current.cursor, wrapArmedAt: now, status: SYMBOL_END_MESSAGE });
    dispatch({ type: "TOAST", message: SYMBOL_END_MESSAGE, kind: "info" });
  }
}

/** The verbs of a set of edits: the pins and graphics each their own, the fields together (the symbol's own list with the replaced ones put in). */
function editCmds(api: SymbolEditorApi, libId: string, edits: readonly SymReplaceEdit[]): Cmd[] {
  const st = api.getState();
  const derived = st.derivedOwn !== null;
  const cmds: Cmd[] = [];
  const fields: FieldRow[] = [];
  for (const e of edits) {
    if (e.kind === "field") fields.push(e.field);
    // a derived symbol's pins and graphics are its parent's: only its fields are its own to edit
    else if (derived) continue;
    else if (e.kind === "pin") cmds.push({ op: "edit_symbol_pin", lib_id: libId, id: e.pin.id!, pin: e.pin });
    else cmds.push({ op: "replace_symbol_graphic", lib_id: libId, id: e.graphic.id!, graphic: e.graphic });
  }
  if (fields.length > 0) {
    const own = allFields(st.derivedOwn ?? st.symbol!);
    for (const f of fields) {
      const i = own.findIndex((o) => o.name === f.name);
      if (i < 0) own.push(f);
      else own[i] = f;
    }
    cmds.push({ op: "edit_library_symbol_fields", lib_id: libId, fields: own.map(wireField) });
  }
  return cmds;
}

/** `SCH_FIND_REPLACE_TOOL::ReplaceAndFindNext`: the current match replaced (when it still matches), one undo step "Find and Replace", then Find Next. */
export async function symReplaceAndFindNext(api: SymbolEditorApi, dispatch: Dispatch<SymAction>): Promise<void> {
  const st = api.getState();
  if (!st.symbol || !st.libId) return;
  if (current.search.find === "") return showSymFind(false, api);
  const id = currentSymMatch([...st.selection], current.cursor, current.selectedOnly);
  const { items: all, fields } = items(api, false);
  const item = all.find((it) => it.id === id);
  if (item) {
    // `HasMatch()`: the item still matches
    const asked = await askMatcher(current.search, [...item.texts, ...replaceTexts(item, st.symbol, fields)], true).catch(() => null);
    const n = item.texts.length;
    if (asked && (asked.matches ?? []).slice(0, n).some(Boolean)) {
      const edit = replaceEdit(item, st.symbol, fields, (asked.replaced ?? []).slice(n));
      const cmds = edit ? editCmds(api, st.libId, [edit]) : [];
      if (cmds.length > 0) await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    }
  }
  await symFindNext(api, dispatch, false);
}

/** Replace All: every match replaced, one undo step "Find and Replace All" (kicad-port/symFind.ts says how this differs from KiCad's Symbol Editor). */
export async function symReplaceAll(api: SymbolEditorApi): Promise<void> {
  const st = api.getState();
  if (!st.symbol || !st.libId) return;
  if (current.search.find === "") return showSymFind(false, api);
  const { items: list, fields } = items(api, current.selectedOnly);
  const sym = st.symbol;
  const reps = list.map((it) => replaceTexts(it, sym, fields));
  const flat = [...textsOf(list).texts, ...reps.flat()];
  let reply: MatchReply;
  try {
    reply = await askMatcher(current.search, flat, true);
  } catch (e) {
    return set({ status: e instanceof Error ? e.message : String(e) });
  }
  const nMatch = textsOf(list).texts.length;
  const matched = matchedItems(list, (reply.matches ?? []).slice(0, nMatch));
  const replaced = (reply.replaced ?? []).slice(nMatch);
  const edits: SymReplaceEdit[] = [];
  let at = 0;
  list.forEach((it, i) => {
    const mine = replaced.slice(at, at + reps[i]!.length);
    at += reps[i]!.length;
    if (!matched[i]) return;
    const e = replaceEdit(it, sym, fields, mine);
    if (e) edits.push(e);
  });
  const cmds = editCmds(api, st.libId, edits);
  if (cmds.length === 0) return;
  await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
}
