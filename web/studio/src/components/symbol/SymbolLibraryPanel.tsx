// The Symbol Editor's library tree pane (`SYMBOL_TREE_PANE` + the context menu `SYMBOL_EDITOR_CONTROL::Init` builds): every symbol the editor can
// open, grouped by library; click selects (several with Ctrl/Shift -- what `GetSelectedLibIds` returns), double click loads the symbol
// (`EditSymbol`), and the right-click menu offers `Init`'s entries in its order. The tree-level conditions of `Init` hold here as follows: there
// is always a library context (the project library), so New Symbol, Import, Paste and the fields table are always offered; the rest need a
// selected symbol. Each entry runs the same registered action the menu bar does.
import { useMemo } from "react";
import actionsData from "../../kicad/actions.json";
import type { ActionsFile } from "../../kicad/types";
import { useActionRunner } from "../../actions/useActionRunner";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import type { MenuEntry } from "../canvas/ContextMenu";
import { LibraryTree } from "../library/LibraryTree";
import { hideTreeEntry, pinEntries } from "../library/libraryTreeMenu";
import { notifyLibraryChanged, useLibraryNames } from "../library/useLibraryNames";
import { useLibraryTree } from "../../state/libraryTree";

const labelOf = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a.label]));

/** `SYMBOL_EDITOR_CONTROL::Init`'s entries, in its order (the `ctxMenu.AddItem` calls); `needs` is the condition: a library context or a selected symbol. */
const MENU: { name: string; needs: "library" | "symbol" }[] = [
  { name: "eeschema.SymbolLibraryControl.newSymbol", needs: "library" },
  { name: "eeschema.SymbolLibraryControl.deriveFromExistingSymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.saveLibraryAs", needs: "library" },
  { name: "eeschema.SymbolLibraryControl.saveSymbolAs", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.saveSymbolCopyAs", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.importSymbol", needs: "library" },
  { name: "eeschema.SymbolLibraryControl.exportSymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.cutSymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.copySymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.pasteSymbol", needs: "library" },
  { name: "eeschema.SymbolLibraryControl.duplicateSymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.deleteSymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.renameFootprint", needs: "symbol" }, // `SCH_ACTIONS::renameSymbol` (named "...renameFootprint" in KiCad's table)
  { name: "eeschema.InteractiveEdit.symbolProperties", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.flattenSymbol", needs: "symbol" },
  { name: "eeschema.SymbolLibraryControl.showLibraryFieldsTable", needs: "library" },
  { name: "eeschema.SymbolLibraryControl.showRelatedLibraryFieldsTable", needs: "symbol" },
];

export function SymbolLibraryPanel() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const names = useLibraryNames("symbol");
  const { run, isEnabled } = useActionRunner();

  const selected = useMemo(() => state.treeSelection, [state.treeSelection]);
  const { pinned } = useLibraryTree("symbol");

  // `LIBRARY_EDITOR_CONTROL::AddContextMenuItems` adds Pin / Unpin Library before `Init`'s entries and Hide Library Tree after them.
  const menu = (sel: string[], libs: string[]): MenuEntry[] => {
    const hasSymbol = sel.length > 0;
    const entries: MenuEntry[] = pinEntries(libs, pinned, run, isEnabled);
    for (const { name, needs } of MENU) {
      if (needs === "symbol" && !hasSymbol) continue;
      const enabled = isEnabled(name);
      const label = labelOf.get(name) ?? name;
      entries.push({ label: enabled ? label : `${label} (not ported yet)`, disabled: !enabled, onSelect: () => run(name) });
    }
    entries.push(hideTreeEntry(run, isEnabled));
    return entries;
  };

  return (
    <LibraryTree
      kind="symbol"
      title="Symbol Libraries"
      items={names.items}
      installed={names.installed}
      loaded={names.loaded}
      loading={names.loading}
      onLoadLibrary={names.load}
      onSearch={names.ensureSearchIndex}
      searchIndex={names.searchIndex}
      selected={selected}
      current={state.libId}
      multi
      onSelect={(names) => dispatch({ type: "SET_TREE_SELECTION", names })}
      onOpen={(name) => {
        dispatch({ type: "SET_TREE_SELECTION", names: [name] });
        void api.openSymbol(name).then(notifyLibraryChanged);
      }}
      menu={menu}
    />
  );
}
