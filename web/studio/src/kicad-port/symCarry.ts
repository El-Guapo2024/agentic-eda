// What the Symbol Editor carries under the cursor until a click places it (eeschema/tools at KiCad 8303b2ad):
//
// - the pin of the Pin tool, after Pin Properties (`SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace` with `SCH_PIN_T`: the first click runs `CreatePin`, whose dialog comes first; the
//   pin then follows the cursor, `item->SetPosition( cursorPos )`, and the second click runs `PlacePin`);
// - the text of the Text tool, after Text Properties (the same loop with `SCH_TEXT_T`);
// - the copies Duplicate makes and the items Paste reads from the clipboard (`SYMBOL_EDITOR_EDIT_TOOL::Duplicate` / `Paste`: the new items, `IS_NEW`, are moved by
//   `SCH_ACTIONS::move` from the selection's reference point -- the cursor when the action ran -- so they keep their place relative to the cursor and follow it; a click
//   drops them and the commit is pushed, Escape reverts it).
//
// The carried items are kept as they were made; `deltaNm` is how far the snapped cursor is from `originNm`, the point they are held by. Pure: no React, no DOM (compiled by
// `npm run test:unit`). Lengths of the items are the wire's mm, +y up; the origin and the delta are the editor's internal nm, +y down.
import type { LibrarySymbolGraphic, LibrarySymbolPin, MmPoint } from "../api/types";

export type CarryKind = "pin" | "text" | "paste" | "duplicate";

export interface SymCarry {
  kind: CarryKind;
  pins: LibrarySymbolPin[];
  graphics: LibrarySymbolGraphic[];
  /** The point the items are held by (internal nm): for a pin or a text its own position, for Paste and Duplicate the snapped cursor when the action ran. */
  originNm: [number, number];
  /** The snapped cursor minus `originNm`: the items are drawn, and placed, moved by it. */
  deltaNm: [number, number];
  /** Each graphic's looks kept beside it (kicad-port/symLooks.ts), in step with `graphics`: a copy is the original's (`SCH_ITEM::Duplicate`, `Paste`), UI-LEFT-2 YY2. */
  looks?: (import("./symLooks").GraphicState | null)[];
}

const IU_PER_MM = 1e4;
const round = (v: number): number => Math.round(v * IU_PER_MM) / IU_PER_MM;
const moved = (p: MmPoint, dx: number, dy: number): MmPoint => ({ x: round(p.x + dx), y: round(p.y + dy) });

/** A graphic moved by (`dxMm`, `dyMm`) in the symbol's +y up frame (`SCH_SHAPE::Move`, `SCH_TEXT::Move`: every point). */
export function translateGraphic(g: LibrarySymbolGraphic, dxMm: number, dyMm: number): LibrarySymbolGraphic {
  const m = (p: MmPoint) => moved(p, dxMm, dyMm);
  switch (g.kind) {
    case "rectangle":
    case "text_box":
      return { ...g, start: m(g.start), end: m(g.end) };
    case "polyline":
      return { ...g, pts: g.pts.map(m) };
    case "circle":
      return { ...g, center: m(g.center) };
    case "arc":
      return { ...g, start: m(g.start), mid: m(g.mid), end: m(g.end) };
    case "bezier":
      return { ...g, start: m(g.start), c1: m(g.c1), c2: m(g.c2), end: m(g.end) };
    case "text":
      return { ...g, at: m(g.at) };
  }
}

/** The carried pins and graphics where they are now: moved by the delta (internal nm, +y down, so the symbol's dy is its negative). */
export function carriedItems(c: Pick<SymCarry, "pins" | "graphics" | "deltaNm">): { pins: LibrarySymbolPin[]; graphics: LibrarySymbolGraphic[] } {
  const dx = c.deltaNm[0] / 1e6;
  const dy = -c.deltaNm[1] / 1e6;
  if (dx === 0 && dy === 0) return { pins: c.pins, graphics: c.graphics };
  return { pins: c.pins.map((p) => ({ ...p, at: moved(p.at, dx, dy) })), graphics: c.graphics.map((g) => translateGraphic(g, dx, dy)) };
}

/** `GetCursorPosition( true )`: the cursor on the grid (whose origin is the symbol's), the reference point Paste and Duplicate hold their items by. */
export function snappedOrigin(cursorNm: { x: number; y: number } | null, gridNm: number): [number, number] {
  if (!cursorNm) return [0, 0];
  if (!(gridNm > 0)) return [Math.round(cursorNm.x), Math.round(cursorNm.y)];
  return [Math.round(cursorNm.x / gridNm) * gridNm, Math.round(cursorNm.y / gridNm) * gridNm];
}

/** A carry that follows the cursor from now: its items as they are now (the delta baked in) and the origin moved to where they are held. For a turn or a mirror while carried. */
export function rebased(c: SymCarry): SymCarry {
  const now = carriedItems(c);
  return { ...c, ...now, originNm: [c.originNm[0] + c.deltaNm[0], c.originNm[1] + c.deltaNm[1]], deltaNm: [0, 0] };
}

/** "Limit of 5000 is totally arbitrary": a pasted text longer than that is cut, with "..." after it. */
export const PASTE_TEXT_LIMIT = 5000;

/**
 * `SYMBOL_EDITOR_EDIT_TOOL::Paste` when the clipboard is not a symbol ("If it's not a symbol then paste as text"): a `SCH_TEXT` at the origin holding the clipboard's text, cut
 * at 5000 characters; common to all units and body styles (`SetUnit( GetUnit() ? .. : 0 )` of a new text's 0).
 */
export function pastedText(clipboard: string, sizeMm = 1.27): LibrarySymbolGraphic {
  const text = clipboard.length > PASTE_TEXT_LIMIT ? `${clipboard.slice(0, PASTE_TEXT_LIMIT)}...` : clipboard;
  return { kind: "text", unit: 0, body_style: 0, text, at: { x: 0, y: 0 }, angle_deg: 0, size_mm: sizeMm };
}
