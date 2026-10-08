// The Footprint Editor's library tree pane (`FOOTPRINT_TREE_PANE` + the context menu `FOOTPRINT_EDITOR_CONTROL::Init` builds): every footprint the
// editor can open, grouped by library; click selects (the library actions' target, `GetTargetFPID`), double-click loads it (`EditFootprint`), and the
// right-click menu offers the actions of `FOOTPRINT_EDITOR_CONTROL` that work on the selection. Each entry runs the same registered action the
// menu bar does, so the two cannot differ.
import { useMemo } from "react";
import actionsData from "../../kicad/actions.json";
import type { ActionsFile } from "../../kicad/types";
import { useActionRunner } from "../../actions/useActionRunner";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import type { MenuEntry } from "../canvas/ContextMenu";
import { LibraryTree } from "../library/LibraryTree";
import { useLibraryNames } from "../library/useLibraryNames";

const labelOf = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a.label]));

/** `FOOTPRINT_EDITOR_CONTROL::Init`'s context menu, in its order; a separator is a `null`. */
const MENU: (string | null)[] = [
  "pcbnew.ModuleEditor.newFootprint",
  "pcbnew.ModuleEditor.createFootprint",
  null,
  "pcbnew.ModuleEditor.cutFootprint",
  "pcbnew.ModuleEditor.copyFootprint",
  "pcbnew.ModuleEditor.pasteFootprint",
  "pcbnew.ModuleEditor.duplicateFootprint",
  "pcbnew.ModuleEditor.renameFootprint",
  "pcbnew.ModuleEditor.deleteFootprint",
  "pcbnew.ModuleEditor.footprintProperties",
  null,
  "pcbnew.ModuleEditor.importFootprint",
  "pcbnew.ModuleEditor.exportFootprint",
];

export function FootprintLibraryPanel() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const names = useLibraryNames("footprint");
  const { run, isEnabled } = useActionRunner();

  const selected = useMemo(() => (state.treeSelection ? [state.treeSelection] : []), [state.treeSelection]);

  const menu = (): MenuEntry[] => {
    const entries: MenuEntry[] = [];
    for (const name of MENU) {
      if (name === null) continue; // `ContextMenu` has no separators; the order keeps the groups together
      const enabled = isEnabled(name);
      entries.push({ label: enabled ? (labelOf.get(name) ?? name) : `${labelOf.get(name) ?? name} (not ported yet)`, disabled: !enabled, onSelect: () => run(name) });
    }
    return entries;
  };

  return (
    <LibraryTree
      title="Footprint Libraries"
      items={names.items}
      installed={names.installed}
      loaded={names.loaded}
      loading={names.loading}
      onLoadLibrary={names.load}
      onSearch={names.ensureSearchIndex}
      searchIndex={names.searchIndex}
      selected={selected}
      current={state.name}
      onSelect={(names) => dispatch({ type: "SET_TREE_SELECTION", name: names[0] ?? null })}
      onOpen={(name) => {
        dispatch({ type: "SET_TREE_SELECTION", name });
        void api.openFootprint(name);
      }}
      menu={menu}
    />
  );
}
