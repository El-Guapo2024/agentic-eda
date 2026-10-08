// The shared actions of the two library editors' tree pane and of their browser windows: Library Tree / Hide Library Tree, Pin Library /
// Unpin Library, Expand All / Collapse All, the tree's search field, and the Footprint / Symbol Library Browsers. COMMON_CONTROL and
// LIBRARY_EDITOR_CONTROL (common/tool/library_editor_control.cpp) run them in every library editor frame; `registerCommonActions`
// (commonActions.ts) calls `registerLibraryActions` once while the registry is built. The tree's state is state/libraryTree.ts.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { collapseAllLibraries, expandAllLibraries, pinSelectedLibraries, setLibraryTreeShown, toggleLibraryTreeShown, type TreeKind } from "../state/libraryTree";

/** `FocusLibraryTreeInput()`: `m_treePane->FocusSearchFieldIfExists()`. The tree draws its search box after the pane is shown, so this waits a frame. */
function focusTreeSearch(kind: TreeKind): void {
  requestAnimationFrame(() => {
    const input = document.querySelector<HTMLInputElement>(`[data-library-tree-search="${kind}"]`);
    input?.focus();
    input?.select();
  });
}

export function registerLibraryActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const { dispatch } = ctx;
  const kind: TreeKind | null = ctx.tab === "footprint" ? "footprint" : ctx.tab === "symbol" ? "symbol" : null;

  if (kind) {
    // ACTIONS::showLibraryTree / hideLibraryTree -- both are `LIBRARY_EDITOR_CONTROL::ToggleLibraryTree`: `m_frame->ToggleLibraryTree()`,
    // `treePane.Show( !IsLibraryTreeShown() )`. (The PCB and schematic editors' tree is the design blocks' pane, which the studio has
    // not; they do not offer the action.)
    m.set("common.Control.showLibraryTree", () => toggleLibraryTreeShown(kind));
    m.set("common.Control.hideLibraryTree", () => toggleLibraryTreeShown(kind));

    // ACTIONS::pinLibrary / unpinLibrary -- `changeSelectedPinStatus( true / false )`: the selected library rows are pinned (kept at the
    // top of the list) or let go, then the tree is regenerated.
    m.set("common.Control.pinLibrary", () => void pinSelectedLibraries(kind, true));
    m.set("common.Control.unpinLibrary", () => void pinSelectedLibraries(kind, false));

    // ACTIONS::expandAll / collapseAll -- `LIB_TREE`'s `m_tree_ctrl->ExpandAll()` / `CollapseAll()`.
    m.set("common.Control.expandAll", () => expandAllLibraries(kind));
    m.set("common.Control.collapseAll", () => collapseAllLibraries(kind));

    // ACTIONS::libraryTreeSearch -- LIBRARY_EDITOR_CONTROL::LibraryTreeSearch (Ctrl+L): show the tree when it is hidden, then focus its
    // search field.
    m.set("common.Control.libraryTreeSearch", () => {
      setLibraryTreeShown(kind, true);
      focusTreeSearch(kind);
    });
  }

  // ACTIONS::showFootprintBrowser / showSymbolBrowser -- COMMON_CONTROL::ShowPlayer( FRAME_FOOTPRINT_VIEWER / FRAME_SCH_VIEWER ): the
  // read-only library viewer window. The studio's editors are the one place the libraries are listed, with the tree and a canvas
  // for the item opened, so the browser is the editor with its library tree shown (and a tree that was hidden comes back).
  m.set("common.Control.showFootprintBrowser", () => {
    setLibraryTreeShown("footprint", true);
    dispatch({ type: "SET_TAB", tab: "footprint" });
  });
  m.set("common.Control.showSymbolBrowser", () => {
    setLibraryTreeShown("symbol", true);
    dispatch({ type: "SET_TAB", tab: "symbol" });
  });
}
