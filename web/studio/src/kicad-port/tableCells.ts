// Table cells as the selection tools and the clipboard take them, in both editors:
//
//  * a drag that starts on a cell selects the cells under it (pcbnew/tools/pcb_selection_tool.cpp `PCB_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )` with
//    `hoverCells`, then `selectTableCells`; eeschema/tools/sch_selection_tool.cpp `SCH_SELECTION_TOOL::Main`, `selectTableCells` and `SelectCellsBetween`);
//  * Paste with cells selected and a table on the clipboard pastes into them (`PCB_CONTROL::Paste` / `SCH_EDITOR_CONTROL::Paste`: `hasTableCells`, the
//    clipboard's first table, `pasteCellsIntoSelection` -- the verb is the server's `paste_cells`, crates/ops/src/table_paste_cells.rs);
//  * a sheet's paste preview carries the tables it adds (a table moved by the cursor's shift).
import type { BoardTable, BoardTableCell } from "../api/types";
import { cellHitDistance, isSelectableCell, parseCellItemId } from "./tables";

type Pt = readonly [number, number];

/** The modifiers `selectTableCells` reads (`m_additive`, `m_subtractive`, `m_exclusive_or`: kicad-port/selection.ts `computeClickModifiers`). */
export interface CellDragModifiers {
  additive: boolean;
  subtractive: boolean;
  exclusiveOr: boolean;
}

/** The cell of `tables` under `(x, y)` within `tolerance` (`hoverCells.Collect( board(), { PCB_TABLECELL_T }, evt->DragOrigin() )`, `CollectHits( .., { SCH_TABLECELL_T } )`): the nearest. */
export function cellUnder(tables: readonly BoardTable[], x: number, y: number, tolerance: number): { table: BoardTable; cell: BoardTableCell } | null {
  let best: { table: BoardTable; cell: BoardTableCell; d: number } | null = null;
  for (const table of tables) {
    for (const cell of table.cells) {
      if (!isSelectableCell(cell)) continue;
      const d = cellHitDistance(cell, x, y);
      if (d <= tolerance && (best == null || d < best.d)) best = { table, cell, d };
    }
  }
  return best ? { table: best.table, cell: best.cell } : null;
}

/** Is every item of `ids` a cell of one of `tables`? (`SELECTION_CONDITIONS::OnlyTypes( { PCB_TABLECELL_T } )`; true for an empty selection.) */
export function onlyCells(ids: Iterable<string>, tables: readonly BoardTable[]): boolean {
  const known = new Set(tables.map((t) => t.id));
  for (const id of ids) {
    const p = parseCellItemId(id);
    if (!p || !known.has(p.table) || known.has(id)) return false;
  }
  return true;
}

/** `hasTableCells`: the selection holds a cell of one of `tables`. */
export function hasTableCells(ids: Iterable<string>, tables: readonly BoardTable[]): boolean {
  const known = new Set(tables.map((t) => t.id));
  for (const id of ids) {
    const p = parseCellItemId(id);
    if (p && known.has(p.table) && !known.has(id)) return true;
  }
  return false;
}

/** The box of a cell's corners. */
function cellBox(c: BoardTableCell): [number, number, number, number] {
  const xs = c.corners.map((p) => p[0]);
  const ys = c.corners.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

/**
 * `selectTableCells` (board) and `SelectCellsBetween` (sheet) for one drag position: the selection the drag from `start` to `end` makes of `table`'s cells. A cell
 * whose box the rectangle touches (`cell->HitTest( selectionRect, false )`: `BOX2I::Intersects`, edges included) is selected -- unselected with Ctrl+Shift
 * (`m_subtractive`), toggled from what it was when the drag began with Ctrl (`m_exclusive_or`) -- and a cell it does not touch keeps what it was at the start only
 * with a modifier (`m_additive || m_subtractive || m_exclusive_or`). `before` is the selection the drag began with (the `CANDIDATE` flags); its other items
 * stay. A cell another one covers is not a selectable item here (a click cannot pick one) and is left out.
 */
export function cellsBetween(table: BoardTable, start: Pt, end: Pt, before: readonly string[], m: CellDragModifiers): string[] {
  const rect = [Math.min(start[0], end[0]), Math.min(start[1], end[1]), Math.max(start[0], end[0]), Math.max(start[1], end[1])] as const;
  const was = new Set(before);
  const mine = new Set(table.cells.map((c) => c.id));
  const chosen = new Set<string>();
  for (const cell of table.cells) {
    if (!isSelectableCell(cell)) continue;
    const b = cellBox(cell);
    const hit = rect[0] <= b[2] && b[0] <= rect[2] && rect[1] <= b[3] && b[1] <= rect[3];
    const wasSelected = was.has(cell.id);
    const doSelect = hit ? (m.subtractive ? false : m.exclusiveOr ? !wasSelected : true) : wasSelected ? m.additive || m.subtractive || m.exclusiveOr : false;
    if (doSelect) chosen.add(cell.id);
  }
  // `select( cell )` / `unselect( cell )`: the items the drag began with keep their order, the cells it adds come after them in the table's order
  const kept = before.filter((id) => !mine.has(id) || chosen.has(id));
  const added = table.cells.filter((c) => chosen.has(c.id) && !was.has(c.id)).map((c) => c.id);
  return [...kept, ...added];
}

/**
 * Whether a KiCad clipboard text holds a table among its top-level items -- what Paste looks for before pasting into selected cells: `clipBoard->Drawings()` of a
 * board's `(kicad_pcb ..)` (`depth` 1; a table inside a footprint is not one) or the items of a sheet's fragment (`tempScreen->Items()`, `depth` 0). Strings are
 * skipped, so a `(table` inside a text is no table.
 */
export function clipboardHasTable(text: string, depth: 0 | 1): boolean {
  let level = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === '"') {
      for (i++; i < text.length && text[i] !== '"'; i++) if (text[i] === "\\") i++;
      continue;
    }
    if (ch === "(") {
      if (level === depth && /^\(table[\s)]/.test(text.slice(i, i + 7))) return true;
      level++;
    } else if (ch === ")") {
      level--;
    }
  }
  return false;
}

/** A sheet's table as the paste preview carries it, moved by `(dx, dy)`: its corner, its box, its border lines, and each cell's corners and placed lines. */
export function shiftTable(t: BoardTable, dx: number, dy: number): BoardTable {
  const s = (p: readonly [number, number]): [number, number] => [p[0] + dx, p[1] + dy];
  return {
    ...t,
    x: t.x + dx,
    y: t.y + dy,
    bbox: [t.bbox[0] + dx, t.bbox[1] + dy, t.bbox[2] + dx, t.bbox[3] + dy],
    borders: t.borders.map((b) => ({ ...b, a: s(b.a), b: s(b.b) })),
    cells: t.cells.map((c) => ({ ...c, corners: c.corners.map(s) as BoardTableCell["corners"], lines: c.lines.map((l) => ({ ...l, x: l.x + dx, y: l.y + dy })) })),
  };
}
