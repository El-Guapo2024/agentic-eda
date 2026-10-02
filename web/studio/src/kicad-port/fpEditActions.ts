// Pure logic behind the footprint-editor actions of docs/parity/UI-ACTIONS.md's
// "pcbnew: not handled, hotkeyed first" group (`pcbnew.ModuleEditor.newFootprint`,
// `pcbnew.InteractiveEdit.duplicateIncrementPads`). Each function names the KiCad
// source function it ports; the handlers (actions/useActionRunner.ts,
// state/footprintEditorStore.tsx) only gather state, call these and send the
// resulting `/api/cmd` verbs.
import type { LibraryPad } from "../api/types";
import { nextPadNumberAfter } from "./padNumbering";

/**
 * `PCB_BASE_FRAME::CreateNewFootprint` (pcbnew/footprint_libraries_utils.cpp): the
 * name is `Untitled` when none is given, made unique by appending `_1`, `_2`, ...
 * to the base name while the library already has one of that name.
 * (`while( adapter->FootprintExists( aLibName, aFootprintName ) )
 * aFootprintName = baseName + wxString::Format( wxS( "_%d" ), idx++ );`, `idx` from 1.)
 */
export function uniqueFootprintName(existing: Iterable<string>, base = "Untitled"): string {
  const taken = new Set(existing);
  const baseName = base === "" ? "Untitled" : base;
  let name = baseName;
  let idx = 1;
  while (taken.has(name)) name = `${baseName}_${idx++}`;
  return name;
}

/** `PAD::CanHaveNumber()`: every pad but a non-plated hole and an aperture (paste-only) pad can carry a number. */
export function padCanHaveNumber(pad: Pick<LibraryPad, "kind">): boolean {
  return pad.kind !== "non_plated_hole";
}

/**
 * `EDIT_TOOL::Duplicate`'s pad branch (pcbnew/tools/edit_tool.cpp): the copy of each
 * selected pad lands exactly where the original is (the move that follows pulls it
 * away from the cursor), and with `increment` (`duplicateIncrementPads`,
 * Ctrl+Shift+D) a pad that `CanHaveNumber()` takes the next free pad number:
 *
 *     padNumber = padTool->GetLastPadNumber();
 *     padNumber = parentFootprint->GetNextPadNumber( padNumber );
 *     padTool->SetLastPadNumber( padNumber );
 *     dupe->SetNumber( padNumber );
 *
 * so several selected pads are numbered consecutively, each continuing from the one
 * before. `existing` is the footprint's current pads (collision check), `lastPadNumber`
 * the pad tool's `m_lastPadNumber` (this editor seeds it from the highest-numbered
 * pad, see padNumbering.ts's header). Returns the new pads (no `id` -- the backend
 * assigns it) in selection order, plus the new last pad number.
 */
export function duplicatePads(existing: readonly LibraryPad[], selected: readonly LibraryPad[], increment: boolean, lastPadNumber: string): { pads: LibraryPad[]; lastPadNumber: string } {
  const out: LibraryPad[] = [];
  let last = lastPadNumber;
  let used: LibraryPad[] = [...existing];
  for (const src of selected) {
    const copy: LibraryPad = { ...src, id: undefined };
    if (increment && padCanHaveNumber(src)) {
      last = nextPadNumberAfter(used, last);
      copy.number = last;
    }
    out.push(copy);
    used = [...used, copy];
  }
  return { pads: out, lastPadNumber: last };
}
