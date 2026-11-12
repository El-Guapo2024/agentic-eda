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
import { hideTreeEntry, pinEntries } from "../library/libraryTreeMenu";
import { useLibraryNames } from "../library/useLibraryNames";
import { useLibraryTree } from "../../state/libraryTree";

const labelOf = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a.label]));

/**
 * `FOOTPRINT_EDITOR_CONTROL::Init`'s context menu, in its order; a separator is a `null`. `when` is the row's CONDITIONAL_MENU condition (a row whose
 * condition fails is hidden): `lib` = `libSelectedCondition` (a library row, no footprint), `fp` = `fpSelectedCondition` (a footprint row),
 * `libOrFp` = `libSelectedCondition || fpSelectedCondition`, `inferred` = `libInferredCondition` (a library is known, from either), `loaded` =
 * `fpExportCondition` (`GetBoard()->GetFirstFootprint() != nullptr`), `always` = `ShowAlways`.
 */
type When = "lib" | "fp" | "libOrFp" | "inferred" | "loaded" | "always";
const MENU: ({ name: string; when: When } | null)[] = [
  { name: "pcbnew.ModuleEditor.newFootprint", when: "lib" },
  { name: "pcbnew.ModuleEditor.createFootprint", when: "lib" },
  null,
  { name: "common.Control.save", when: "always" },
  { name: "common.Control.saveAs", when: "libOrFp" },
  { name: "common.Control.revert", when: "inferred" }, // `libSelectedCondition || libInferredCondition`
  null,
  { name: "pcbnew.ModuleEditor.cutFootprint", when: "fp" },
  { name: "pcbnew.ModuleEditor.copyFootprint", when: "fp" },
  { name: "pcbnew.ModuleEditor.pasteFootprint", when: "inferred" },
  { name: "pcbnew.ModuleEditor.duplicateFootprint", when: "fp" },
  { name: "pcbnew.ModuleEditor.renameFootprint", when: "fp" },
  { name: "pcbnew.ModuleEditor.deleteFootprint", when: "fp" },
  { name: "pcbnew.ModuleEditor.footprintProperties", when: "fp" },
  null,
  { name: "pcbnew.ModuleEditor.importFootprint", when: "inferred" },
  { name: "pcbnew.ModuleEditor.exportFootprint", when: "loaded" },
];

export function FootprintLibraryPanel() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const names = useLibraryNames("footprint");
  const { run, isEnabled } = useActionRunner();

  const selected = useMemo(() => (state.treeSelection ? [state.treeSelection] : []), [state.treeSelection]);

  const { pinned } = useLibraryTree("footprint");

  // `LIBRARY_EDITOR_CONTROL::AddContextMenuItems` adds Pin / Unpin Library before `FOOTPRINT_EDITOR_CONTROL::Init`'s entries and Hide Library Tree after them.
  const menu = (sel: string[], libs: string[]): MenuEntry[] => {
    const entries: MenuEntry[] = pinEntries(libs, pinned, run, isEnabled);
    // The tree's selected LIB_ID: a footprint row gives `sel`, a library row `libs` (LibraryTree.tsx's `onContextMenu`).
    const fpSel = sel.length > 0;
    const libSel = !fpSel && libs.length > 0;
    const shown: Record<When, boolean> = { lib: libSel, fp: fpSel, libOrFp: libSel || fpSel, inferred: libSel || fpSel, loaded: !!state.footprint, always: true };
    for (const row of MENU) {
      if (row === null) continue; // `ContextMenu` has no separators; the order keeps the groups together
      if (!shown[row.when]) continue;
      const { name } = row;
      const enabled = isEnabled(name);
      entries.push({ label: enabled ? (labelOf.get(name) ?? name) : `${labelOf.get(name) ?? name} (not ported yet)`, disabled: !enabled, onSelect: () => run(name) });
    }
    entries.push(hideTreeEntry(run, isEnabled));
    return entries;
  };

  return (
    <LibraryTree
      kind="footprint"
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
      modified={names.modified}
      onSelect={(names) => dispatch({ type: "SET_TREE_SELECTION", name: names[0] ?? null })}
      onOpen={(name) => {
        dispatch({ type: "SET_TREE_SELECTION", name });
        void api.openFootprint(name);
      }}
      menu={menu}
    />
  );
}
