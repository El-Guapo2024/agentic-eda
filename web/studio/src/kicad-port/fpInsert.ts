// Insert footprint into PCB (`pcbnew.ModuleEditor.saveFootprintToBoard`, FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard, pcbnew/footprint_libraries_utils.cpp at 8303b2ad), the branch
// where the board has no footprint to update: the footprint is added to the board as a part of its own. KiCad then hangs it on the cursor of the board editor; the studio puts it at the
// centre of the board outline (the server snaps it to the grid) and leaves it selected.
//
// Pure functions, micrometres.

import type { Part } from "../api/types";

/** The middle of the outline's bounding box; the origin when the board has no outline (KiCad's `SetPosition( VECTOR2I( 0, 0 ) )`). */
export function outlineCentre(outline: readonly (readonly [number, number])[] | null | undefined): { x: number; y: number } {
  if (!outline || outline.length === 0) return { x: 0, y: 0 };
  const xs = outline.map((p) => p[0]);
  const ys = outline.map((p) => p[1]);
  return { x: Math.round((Math.min(...xs) + Math.max(...xs)) / 2), y: Math.round((Math.min(...ys) + Math.max(...ys)) / 2) };
}

/** The part a command added: the one reference of `after` that `before` did not have. */
export function addedPart(before: readonly Pick<Part, "ref">[], after: readonly Pick<Part, "ref">[]): string | null {
  const had = new Set(before.map((p) => p.ref));
  return after.find((p) => !had.has(p.ref))?.ref ?? null;
}

/** What the toast says after an insert. */
export function insertedMessage(footprint: string, reference: string | null): string {
  return reference ? `${footprint}: inserted into the board as ${reference}, at the centre of the board` : `${footprint}: inserted into the board`;
}
