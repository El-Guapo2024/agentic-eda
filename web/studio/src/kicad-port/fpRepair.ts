// `FOOTPRINT_EDITOR_CONTROL::RepairFootprint` (pcbnew/tools/footprint_editor_control.cpp): "Repair duplicate IDs and missing nets".
// The footprint's items are walked in the order the C++ does -- the footprint itself, its pads (the principal use of an id is a DRC
// marker pointing at a pad), the Reference and Value fields, the graphics, zones and groups -- and every item whose id was already
// seen counts as a duplicate and gets a new one. The studio keeps a footprint's ids on its pads, graphics and text; the backend verb
// (`repair_footprint`) does the re-assigning, this is the count that decides what to tell the person ("%d duplicate IDs replaced." /
// "No footprint problems found.").
import type { LibraryFootprint } from "../api/types";

/** How many pads / graphics / texts hold an id that an earlier item (pads first, then graphics, then text) already holds. */
export function duplicateIdCount(fp: Pick<LibraryFootprint, "pads" | "graphics" | "texts">): number {
  const seen = new Set<string>();
  let duplicates = 0;
  const process = (id: string | undefined) => {
    if (!id) return;
    if (seen.has(id)) duplicates++;
    seen.add(id);
  };
  for (const p of fp.pads) process(p.id);
  for (const g of fp.graphics) process(g.id);
  for (const t of fp.texts) process(t.id);
  return duplicates;
}

/** The dialog text `RepairFootprint` shows: `{ title, details }`, `details` empty when nothing was wrong. */
export function repairMessage(duplicates: number): { title: string; details: string } {
  if (duplicates === 0) return { title: "No footprint problems found.", details: "" };
  return { title: `${duplicates} potential problems repaired.`, details: `${duplicates} duplicate IDs replaced.` };
}
