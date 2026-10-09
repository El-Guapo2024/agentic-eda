// What stays selected when a fresh read of the sheet arrives. KiCad keeps the selection across a commit (Lock, Rotate, Change To
// all leave their items selected); here the selection is a set of ids and the sheet is re-read from the backend after every
// command, so ids that no longer exist anywhere have to be dropped at that point -- and only then: a board refresh knows nothing
// of a sheet's wires, labels or shapes and must not drop them.
import type { Schematic } from "../api/types";
import { fieldItems, isShownField } from "./schFieldEdit";

/** Every id a sheet can have selected: its wires, bus entries, junctions, graphic lines, drawn shapes, sheets, no-connects, labels, texts, power symbols, symbol references and the shown fields of those. */
export function sheetItemIds(sch: Schematic): Set<string> {
  const ids = new Set<string>();
  const add = (items: ReadonlyArray<{ id?: string }> | undefined) => {
    for (const i of items ?? []) if (i.id) ids.add(i.id);
  };
  add(sch.wires);
  add(sch.bus_entries);
  add(sch.junctions);
  add(sch.lines);
  add(sch.graphics);
  add(sch.sheets);
  add(sch.no_connects);
  add(sch.labels);
  add(sch.texts);
  add(sch.power_symbols);
  add(sch.symbols);
  // the shown fields of symbols, power symbols and sheets (a hidden one cannot be selected, so a field hidden by Delete leaves the selection)
  for (const f of fieldItems(sch)) if (isShownField(f.field)) ids.add(f.id);
  return ids;
}

/** The selection that survives reading `sch`: the ids still on the sheet (the same set back when none was lost, so nothing re-renders). */
export function keepOnSheet(selection: Set<string>, sch: Schematic): Set<string> {
  if (selection.size === 0) return selection;
  const ids = sheetItemIds(sch);
  const kept = [...selection].filter((id) => ids.has(id));
  return kept.length === selection.size ? selection : new Set(kept);
}
