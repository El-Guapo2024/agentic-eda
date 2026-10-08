// The schematic clipboard's client side -- `SCH_EDITOR_CONTROL::Paste` and `SCH_MOVE_TOOL::Main`'s handling of pasted items
// (eeschema/tools at 8303b2ad).
//
// KiCad adds the pasted items to the sheet at the coordinates the clipboard has, selects them and starts the Move tool: the item
// `SCH_SELECTION::GetTopLeftItem` names (for Duplicate, the connection point nearest the cursor) is carried to the cursor
// (`delta = m_cursor - *m_anchorPos`), the rest follows, and a click drops them (`commit.Push( _( "Paste" ) )`); Escape throws the
// whole paste away. Here the studio asks the server for what a paste would add (`POST /api/sch/clipboard/parse`: the new items, already
// numbered, drawn the way the sheet draws them), shows them shifted by the cursor, and a click sends `paste_sch` with that shift.
// This module is the part with no React in it: the shift, the ghost of the paste on a sheet, and which items a paste made.
import type { LibSymbols, Schematic } from "../api/types";
import type { SchGraphic, SchGraphicShape } from "../api/schEditTypes";

/** `PASTE_MODE` (dialog_paste_special.h): what Paste Special does to the reference designators. */
export type SchPasteMode = "unique" | "keep" | "remove";

/** The options of Paste Special's "Reference Designators" box (`dialog_paste_special_base.cpp`), in the dialog's order. */
export const PASTE_SPECIAL_OPTIONS: ReadonlyArray<{ mode: SchPasteMode; label: string; tip: string }> = [
  { mode: "unique", label: "Assign unique reference designators to pasted symbols", tip: "Finds the next available reference designator for any designators that already exist in the design." },
  { mode: "keep", label: "Keep existing reference designators, even if they are duplicated", tip: "" },
  { mode: "remove", label: "Clear reference designators on all pasted symbols", tip: "Replaces reference designators with '?'." },
];

/** What a paste would add to the sheet in view (`crates/cli/src/sch_clipboard_api.rs`): `GET /api/schematic`'s own item shapes, only the new ones. */
export interface SchPastePreview {
  symbols: Schematic["symbols"];
  power_symbols: Schematic["power_symbols"];
  wires: Schematic["wires"];
  labels: Schematic["labels"];
  texts: Schematic["texts"];
  no_connects: Schematic["no_connects"];
  bus_entries: Schematic["bus_entries"];
  junctions: NonNullable<Schematic["junctions"]>;
  lines: NonNullable<Schematic["lines"]>;
  graphics: SchGraphic[];
  lib_symbols: LibSymbols;
}

type Pt = [number, number];

/** `m_cursor - m_anchorPos`: the shift that puts the anchor on the (grid-snapped) cursor. */
export function pasteOffset(anchor: Pt, cursor: Pt): Pt {
  return [cursor[0] - anchor[0], cursor[1] - anchor[1]];
}

const shiftPt = (p: Pt, dx: number, dy: number): Pt => [p[0] + dx, p[1] + dy];
const shiftXY = (p: { x: number; y: number }, dx: number, dy: number) => ({ x: p.x + dx, y: p.y + dy });

/** A drawn shape moved by `(dx, dy)` (every point it has). */
export function shiftShape(shape: SchGraphicShape, dx: number, dy: number): SchGraphicShape {
  switch (shape.type) {
    case "rectangle":
    case "text_box":
      return { ...shape, start: shiftXY(shape.start, dx, dy), end: shiftXY(shape.end, dx, dy) };
    case "circle":
      return { ...shape, center: shiftXY(shape.center, dx, dy) };
    case "arc":
      return { ...shape, start: shiftXY(shape.start, dx, dy), mid: shiftXY(shape.mid, dx, dy), end: shiftXY(shape.end, dx, dy) };
    case "bezier":
      return { ...shape, start: shiftXY(shape.start, dx, dy), c1: shiftXY(shape.c1, dx, dy), c2: shiftXY(shape.c2, dx, dy), end: shiftXY(shape.end, dx, dy) };
    case "polygon":
    case "rule_area":
      return { ...shape, pts: shape.pts.map((p) => shiftXY(p, dx, dy)) };
    case "directive":
      return { ...shape, at: shiftXY(shape.at, dx, dy) };
  }
}

/** The preview moved by `(dx, dy)`. */
export function shiftPreview(p: SchPastePreview, dx: number, dy: number): SchPastePreview {
  return {
    ...p,
    symbols: p.symbols.map((s) => ({ ...s, at: shiftPt(s.at, dx, dy) })),
    power_symbols: p.power_symbols.map((s) => ({ ...s, at: shiftPt(s.at, dx, dy) })),
    wires: p.wires.map((w) => ({ ...w, pts: w.pts.map((q) => shiftPt(q, dx, dy)) })),
    labels: p.labels.map((l) => ({ ...l, at: shiftPt(l.at, dx, dy) })),
    texts: p.texts.map((t) => ({ ...t, at: shiftPt(t.at, dx, dy) })),
    no_connects: p.no_connects.map((n) => ({ ...n, at: shiftPt(n.at, dx, dy) })),
    bus_entries: p.bus_entries.map((b) => ({ ...b, at: shiftPt(b.at, dx, dy) })),
    junctions: p.junctions.map((j) => ({ ...j, at: shiftPt(j.at, dx, dy) })),
    lines: p.lines.map((l) => ({ ...l, pts: l.pts.map((q) => shiftPt(q, dx, dy)) })),
    graphics: p.graphics.map((g) => ({ ...g, shape: shiftShape(g.shape, dx, dy) })),
  };
}

/** The ids of everything the preview holds (what the canvas draws as selected while the paste is carried). */
export function previewIds(p: SchPastePreview): string[] {
  return [...p.symbols, ...p.power_symbols, ...p.wires, ...p.labels, ...p.texts, ...p.no_connects, ...p.bus_entries, ...p.junctions, ...p.lines, ...p.graphics].map((x) => x.id).filter((id) => id !== "");
}

/** `sch` with the paste drawn on it, shifted by `(dx, dy)`. A library symbol the sheet already draws is left as the sheet has it. */
export function withPaste(sch: Schematic, preview: SchPastePreview, dx: number, dy: number): Schematic {
  const p = shiftPreview(preview, dx, dy);
  return {
    ...sch,
    symbols: [...sch.symbols, ...p.symbols],
    power_symbols: [...sch.power_symbols, ...p.power_symbols],
    wires: [...sch.wires, ...p.wires],
    labels: [...sch.labels, ...p.labels],
    texts: [...sch.texts, ...p.texts],
    no_connects: [...sch.no_connects, ...p.no_connects],
    bus_entries: [...sch.bus_entries, ...p.bus_entries],
    junctions: [...(sch.junctions ?? []), ...p.junctions],
    lines: [...(sch.lines ?? []), ...p.lines],
    graphics: [...(sch.graphics ?? []), ...p.graphics],
    lib_symbols: { ...p.lib_symbols, ...sch.lib_symbols },
  };
}

/** True when the preview has nothing to draw. */
export function previewIsEmpty(p: SchPastePreview): boolean {
  return previewIds(p).length === 0 && p.symbols.length === 0 && p.power_symbols.length === 0;
}

/** The items `after` has that `before` did not: the pasted ones, to select once the paste is placed (KiCad leaves them selected). A unit that joined a reference already there selects the reference. */
export function newIdsAfter(before: Schematic, after: Schematic): string[] {
  const known = new Set<string>();
  const add = (list: ReadonlyArray<{ id: string }> | undefined) => (list ?? []).forEach((x) => known.add(x.id));
  [before.wires, before.labels, before.texts, before.power_symbols, before.no_connects, before.bus_entries, before.junctions, before.lines, before.graphics].forEach(add);
  const units = new Set(before.symbols.map((s) => `${s.id}#${s.unit}`));
  const ids: string[] = [];
  const take = (id: string) => {
    if (id && !ids.includes(id)) ids.push(id);
  };
  for (const list of [after.wires, after.labels, after.texts, after.power_symbols, after.no_connects, after.bus_entries, after.junctions ?? [], after.lines ?? [], after.graphics ?? []] as ReadonlyArray<{ id: string }>[]) {
    for (const x of list) if (!known.has(x.id)) take(x.id);
  }
  for (const s of after.symbols) if (!units.has(`${s.id}#${s.unit}`)) take(s.id);
  return ids;
}

/** The text KiCad's duplicate buffer / the system clipboard handed back, ready for `parse`; null when there is none. */
export function clipboardTextOrNull(text: string | null | undefined): string | null {
  return text != null && text.trim() !== "" ? text : null;
}
