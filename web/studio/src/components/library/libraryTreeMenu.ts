// The entries every library tree's right-click menu shares, from `LIBRARY_EDITOR_CONTROL::AddContextMenuItems`
// (common/tool/library_editor_control.cpp): Pin Library / Unpin Library at the top (priority 1), whichever the selected libraries allow
// (kicad-port/libraryTreeState.ts `pinMenu`), and Hide Library Tree at the bottom (priority 400). Each entry runs the registered action,
// so the menu and the menu bar cannot differ.
import actionsData from "../../kicad/actions.json";
import type { ActionsFile } from "../../kicad/types";
import { pinMenu } from "../../kicad-port/libraryTreeState";
import type { MenuEntry } from "../canvas/ContextMenu";

const labelOf = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a.label]));

type Run = (name: string) => void;
type IsEnabled = (name: string) => boolean;

function entry(name: string, run: Run, isEnabled: IsEnabled): MenuEntry {
  const enabled = isEnabled(name);
  const label = labelOf.get(name) ?? name;
  return { label: enabled ? label : `${label} (not ported yet)`, disabled: !enabled, onSelect: () => run(name) };
}

/** Pin Library (when none of the selected libraries is pinned) or Unpin Library (when all are); none for a selection without a library row. */
export function pinEntries(libs: readonly string[], pinned: ReadonlySet<string>, run: Run, isEnabled: IsEnabled): MenuEntry[] {
  const offer = pinMenu(libs, pinned);
  const out: MenuEntry[] = [];
  if (offer.pin) out.push(entry("common.Control.pinLibrary", run, isEnabled));
  if (offer.unpin) out.push(entry("common.Control.unpinLibrary", run, isEnabled));
  return out;
}

/** The last entry: `ACTIONS::hideLibraryTree`, "SELECTION_CONDITIONS::ShowAlways". */
export function hideTreeEntry(run: Run, isEnabled: IsEnabled): MenuEntry {
  return entry("common.Control.hideLibraryTree", run, isEnabled);
}
