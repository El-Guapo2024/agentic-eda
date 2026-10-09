// Find on the PCB: what the board editor's Find dialog (pcbnew/dialogs/dialog_find.cpp, DIALOG_FIND) matches, in which order it lists the hits, how Find Next and
// Find Previous walk them, and how the view is brought to a hit. The dialog is components/PcbFindDialog.tsx; this is its pure part.
//
//   pcbnew/dialogs/dialog_find.cpp      DIALOG_FIND::search          the hit list (footprints, then texts and zones, then markers, then nets) and the iterator
//   common/eda_item.cpp                 EDA_ITEM::Matches            plain, whole word and wildcard matching of one text
//   pcbnew/pcb_field.cpp                PCB_FIELD::Matches           a hidden field is searched only with "Include hidden fields"
//   pcbnew/tools/pcb_selection_tool.cpp PCB_SELECTION_TOOL::FindItem  what showing a hit does: select it, focus on it, zoom out if it is big
//   common/eda_draw_frame.cpp           EDA_DRAW_FRAME::FocusOnLocation   the view is only moved when the point is outside the middle 80% of it
//
// What this board has to search: footprint references and values (the footprints this studio writes carry those two fields, both visible), the board's own
// texts, DRC markers (their message), and the nets (their names). A zone has no name in the IR, so none matches (`ZONE::Matches` reads `GetZoneName()`).
// Pure: no store, no DOM. Unit-tested in pcbFind.test.ts.
import type { BoardState } from "../api/types";
import { boxCentre, centerViewOn, setScaleAboutCentre } from "./zoomFit";
import type { ViewTransform } from "./view";

/** The dialog's checkboxes (`m_matchCase` ... `m_includeNets`) and the text (`m_searchCombo`). */
export interface PcbFindOptions {
  text: string;
  matchCase: boolean;
  /** "Whole words only" (`EDA_SEARCH_MATCH_MODE::WHOLEWORD`). */
  wholeWord: boolean;
  /** "Wildcards" (`WILDCARD`): `*` and `?`; ignored when whole words is on, as in the C++ (`if( m_matchWords ) ... else if( m_wildcards )`). */
  wildcards: boolean;
  wrap: boolean;
  references: boolean;
  values: boolean;
  /** "Search other text items": the board's texts, a footprint's other text and fields, zones. */
  texts: boolean;
  markers: boolean;
  nets: boolean;
  /** "Include hidden fields" (`searchAllFields`). */
  hidden: boolean;
}

/** The dialog as it opens: Wrap, references, markers, values, nets and other text on; the rest off (`dialog_find_base.cpp`). */
export function defaultFindOptions(): PcbFindOptions {
  return { text: "", matchCase: false, wholeWord: false, wildcards: false, wrap: true, references: true, values: true, texts: true, markers: true, nets: true, hidden: false };
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- matching

const isWordChar = (c: string): boolean => /[\p{L}\p{N}_]/u.test(c);

/** `wxString::Matches( mask )`: the whole string against a mask with `*` (any run) and `?` (any one character). */
function wildcardMatches(text: string, mask: string): boolean {
  const source = [...mask].map((c) => (c === "*" ? ".*" : c === "?" ? "." : c.replace(/[.+^${}()|[\]\\]/g, "\\$&"))).join("");
  return new RegExp(`^${source}$`, "su").test(text);
}

/**
 * `EDA_ITEM::Matches( text, data )`: unless `matchCase`, both are put in upper case. Whole words: the search text occurs with no word character (a letter, a digit
 * or `_`) just before or after it. Wildcards: the mask matches the whole text. Otherwise: the search text occurs in the text.
 */
export function textMatches(aText: string, o: Pick<PcbFindOptions, "text" | "matchCase" | "wholeWord" | "wildcards">): boolean {
  let text = aText;
  let search = o.text;
  if (!o.matchCase) {
    text = text.toUpperCase();
    search = search.toUpperCase();
  }
  if (o.wholeWord) {
    let from = 0;
    while (from < text.length) {
      const at = text.indexOf(search, from);
      if (at < 0) return false;
      const end = at + search.length;
      const startOk = at === 0 || !isWordChar(text.charAt(at - 1));
      const endOk = end === text.length || !isWordChar(text.charAt(end));
      if (startOk && endOk) return true;
      from = at + 1;
    }
    return false;
  }
  if (o.wildcards) return wildcardMatches(text, search);
  return text.includes(search);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- the hits

/** One thing the search found: a footprint (by reference), a board text (by id), a DRC marker (by its index in the last run) or a net (by name). */
export type PcbHit = { kind: "footprint"; id: string } | { kind: "text"; id: string } | { kind: "marker"; index: number } | { kind: "net"; id: string };

/** Every net the board knows: those its pads, tracks, vias and zones are on, sorted by name. */
export function boardNetNames(board: BoardState): string[] {
  const nets = new Set<string>();
  for (const p of board.parts) for (const pad of p.pads ?? []) if (pad.net) nets.add(pad.net);
  for (const t of board.routing?.tracks ?? []) if (t.net) nets.add(t.net);
  for (const v of board.routing?.vias ?? []) if (v.net) nets.add(v.net);
  for (const z of board.routing?.zones ?? []) if (z.net && !z.is_rule_area) nets.add(z.net);
  return [...nets].sort();
}

/**
 * `DIALOG_FIND::search`'s list: when texts, values or references are on, each footprint whose reference (if on) or value (if on) matches; then, with texts on,
 * the board's texts; then the markers whose message matches; then the nets whose name does. In that order, the footprints in the board's order.
 */
export function findHits(board: BoardState, markers: readonly { description: string }[], o: PcbFindOptions): PcbHit[] {
  if (o.text === "") return [];
  const hits: PcbHit[] = [];
  if (o.texts || o.values || o.references) {
    for (const part of board.parts) {
      if (!part.placed) continue;
      if ((o.references && textMatches(part.ref, o)) || (o.values && part.value != null && textMatches(part.value, o))) hits.push({ kind: "footprint", id: part.ref });
    }
    if (o.texts) for (const text of board.drawings?.texts ?? []) if (textMatches(text.content, o)) hits.push({ kind: "text", id: text.id });
  }
  if (o.markers) markers.forEach((m, index) => textMatches(m.description, o) && hits.push({ kind: "marker", index }));
  if (o.nets) for (const net of boardNetNames(board)) if (textMatches(net, o)) hits.push({ kind: "net", id: net });
  return hits;
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the walk

export interface FindStep {
  /** `m_it`: the position in the hit list after the step; the list's length is "end". */
  cursor: number;
  /** The hit to show, null when the end was reached (or there is no hit). */
  hit: PcbHit | null;
  /** `endIsReached`: Find Next ran off the last hit with Wrap off, or Find Previous off the first. */
  endReached: boolean;
}

/**
 * One press of Find Next (`forward`) or Find Previous, as `DIALOG_FIND::search` walks its list. `cursor` is `m_it` from the last press; `fresh` is the first press on a
 * list that was just built, which starts from the beginning (forward) or the end (backward) without stepping first. A press past the last hit wraps to the first with
 * Wrap on, else reports the end and stays on the last real hit; Find Previous before the first wraps to the last.
 */
export function stepFind(hits: readonly PcbHit[], cursor: number, forward: boolean, wrap: boolean, fresh: boolean): FindStep {
  const n = hits.length;
  if (n === 0) return { cursor: 0, hit: null, endReached: false };
  let it = fresh ? (forward ? 0 : n) : Math.max(0, Math.min(cursor, n));
  let endReached = false;
  if (forward) {
    if (it !== n && !fresh) it++;
    if (it === n) {
      if (wrap) it = 0;
      else {
        endReached = true;
        it--; // "point to the last REAL result"
      }
    }
  } else {
    if (it === 0) {
      if (wrap) it = n;
      else endReached = true;
    }
    if (it !== 0) it--;
  }
  return { cursor: it, hit: endReached || it === n ? null : hits[it]!, endReached };
}

/** `m_status`: "Hit(s): 2 / 5" for a shown hit, "No hits" past the end, "'text' not found" with no hit at all. */
export function findStatus(text: string, hits: readonly PcbHit[], step: FindStep): string {
  if (hits.length === 0) return `'${text}' not found`;
  if (step.hit == null) return "No hits";
  return `Hit(s): ${step.cursor + 1} / ${hits.length}`;
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ the preload

/**
 * `PCB_EDIT_FRAME::ShowFindDialog`'s `findString`: with one item selected, a footprint's value, or the first line of a text; otherwise nothing (the search text
 * of the last time stays).
 */
export function preloadText(board: BoardState | null, selection: ReadonlySet<string>): string {
  if (!board || selection.size !== 1) return "";
  const id = [...selection][0]!;
  const part = board.parts.find((p) => p.ref === id && p.placed);
  if (part) return part.value ?? "";
  const text = board.drawings?.texts.find((t) => t.id === id);
  return text ? (text.content.split("\n")[0] ?? "") : "";
}

// -------------------------------------------------------------------------------------------------------------------------------------------- the view

/** A board box `[x0, y0, x1, y1]`, um. */
type Box = readonly [number, number, number, number];

/**
 * Where the view goes to show a hit with box `box` (`PCB_SELECTION_TOOL::FindItem`): `FocusOnLocation( position )` centres the view on the hit only when it lies outside the
 * middle 80% of the view ("Center if we're off the current view, or within 10% of its edge"); then, "if the item has a bounding box, then zoom out if needed": a box that
 * does not fit in half the view is shown by zooming out until it does, and the view is centred on it (it never zooms in). The C++ tests the box against a half-size
 * rectangle placed at the viewport's origin; this tests whether the box is larger than half the view, which is what the zoom is for.
 */
export function focusView(view: ViewTransform, width: number, height: number, box: Box): ViewTransform {
  if (!(view.scale > 0) || !(width > 0) || !(height > 0)) return view;
  const centre = boxCentre([...box] as [number, number, number, number]);
  const left = -view.x / view.scale;
  const top = -view.y / view.scale;
  const viewW = width / view.scale;
  const viewH = height / view.scale;
  const inside = centre.x >= left + viewW / 10 && centre.x <= left + (viewW * 9) / 10 && centre.y >= top + viewH / 10 && centre.y <= top + (viewH * 9) / 10;
  let next = inside ? view : (centerViewOn(view, width, height, centre) ?? view);
  const boxW = box[2] - box[0];
  const boxH = box[3] - box[1];
  if (boxW > 0 && boxH > 0) {
    const marginFactor = 2;
    const factor = Math.min(viewW / boxW, viewH / boxH) / marginFactor;
    if (factor < 1) {
      next = setScaleAboutCentre(next, width, height, next.scale * factor);
      next = centerViewOn(next, width, height, centre) ?? next;
    }
  }
  return next;
}
