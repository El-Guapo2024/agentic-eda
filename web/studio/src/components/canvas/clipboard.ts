// The PCB clipboard. A Copy puts KiCad's own clipboard text on the system clipboard (`CLIPBOARD_IO::SaveSelection`, written by the backend:
// `POST /api/clipboard/copy`, crates/kicad/src/clipboard.rs) and keeps the same text here, so a Paste still works where the browser will not let a page
// read the system clipboard; a Paste takes whichever KiCad text is on the system clipboard first -- a copy made in KiCad itself, or on another
// board -- else this one (`paste_clipboard`, crates/ops/src/pcb_paste.rs).
import type { BoardState } from "../../api/types";

export interface ClipboardContents {
  /** KiCad's clipboard text for the last Copy. */
  text: string;
  /** The point the Copy was measured from (the paste puts it on the cursor); a plain copy measures from the cursor's grid point. */
  reference?: { x: number; y: number };
}

/**
 * `PCB_IO_KICAD_SEXPR::Parse` accepts a whole board or a single footprint and nothing else: whether `text` is one of those (a quick look at its
 * first form; the backend does the real parse and says why when it is not).
 */
export function isKicadPcbText(text: string | null | undefined): boolean {
  const t = (text ?? "").trimStart();
  return t.startsWith("(kicad_pcb") || t.startsWith("(footprint") || t.startsWith("(module");
}

/**
 * Every id of an item on the board -- footprints (their references), tracks, vias, zones, shapes, texts, dimensions and groups -- used to diff
 * before/after a duplicate or paste to find out what is new (neither command's reply says so directly; see state/store.tsx). Pads are not
 * in it: a pad is never created on its own.
 */
export function allItemIds(board: BoardState): Set<string> {
  const ids = new Set<string>();
  for (const p of board.parts) if (p.placed) ids.add(p.ref);
  for (const t of board.routing?.tracks ?? []) ids.add(t.id);
  for (const v of board.routing?.vias ?? []) ids.add(v.id);
  for (const z of board.routing?.zones ?? []) ids.add(z.id);
  for (const s of board.drawings?.shapes ?? []) ids.add(s.id);
  for (const t of board.drawings?.texts ?? []) ids.add(t.id);
  for (const d of board.drawings?.dimensions ?? []) ids.add(d.id);
  for (const g of board.drawings?.groups ?? []) ids.add(g.id);
  return ids;
}

/** The new items between two boards: what a Duplicate or a Paste added, group last (a group is selected as the unit it is). */
export function newItemIds(before: ReadonlySet<string>, after: BoardState): string[] {
  const fresh = [...allItemIds(after)].filter((id) => !before.has(id));
  // A new group's members are selected through the group.
  const members = new Set((after.drawings?.groups ?? []).filter((g) => fresh.includes(g.id)).flatMap((g) => g.member_ids));
  return fresh.filter((id) => !members.has(id));
}
