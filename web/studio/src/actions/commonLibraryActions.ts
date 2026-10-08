// The shared actions of the two library editors' tree pane and of their browser windows: Hide Library Tree, Pin Library / Unpin Library, Expand All /
// Collapse All, the tree's search field, and the Footprint / Symbol Library Browsers. COMMON_CONTROL and LIBRARY_EDITOR_CONTROL
// (common/tool/library_editor_control.cpp) run them in every library editor frame; `registerCommonActions` (commonActions.ts) calls
// `registerLibraryActions` once while the registry is built. The tree's pins, folds and library selection are state/libraryTree.ts; whether it is
// shown is the dock layout's (the Libraries column: `ACTIONS::showLibraryTree` itself is `registerEditorFrameActions`'s).
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { datasheetTarget } from "../kicad-port/itemText";
import { footprintDocumentationUrl } from "../kicad-port/footprintDatasheet";
import { collapseAllLibraries, expandAllLibraries, pinSelectedLibraries, type TreeKind } from "../state/libraryTree";
import { getDockLayout, setDockColumnCollapsed } from "../state/dockLayoutStore";

/** `FocusLibraryTreeInput()`: `m_treePane->FocusSearchFieldIfExists()`. The tree draws its search box after the column opens, so this waits a frame. */
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
    // ACTIONS::hideLibraryTree -- LIBRARY_EDITOR_CONTROL::ToggleLibraryTree, the same function as `showLibraryTree`'s: `treePane.Show( !IsLibraryTreeShown() )`.
    // (The PCB and schematic editors' tree is the design blocks' pane, which the studio has not; they do not offer the action.)
    m.set("common.Control.hideLibraryTree", () => setDockColumnCollapsed("tree", !getDockLayout().treeCollapsed));

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
      setDockColumnCollapsed("tree", false);
      focusTreeSearch(kind);
    });
  }

  // ACTIONS::showDatasheet in the Footprint Editor -- FOOTPRINT_EDITOR_CONTROL::ShowDatasheet: `GetFootprintDocumentationURL` (the footprint's Datasheet field, else
  // the first web address in its description) is opened like the symbol's datasheet is; none says "No datasheet found in the footprint.". (The Symbol Editor's
  // is `editorFrameActions.ts`'s.)
  if (ctx.tab === "footprint") {
    m.set("common.Control.showDatasheet", () => {
      const open = ctx.fpApi.getState().footprint;
      if (!open) return dispatch({ type: "TOAST", message: "Open a footprint first.", kind: "info" });
      const target = datasheetTarget(footprintDocumentationUrl(open.fields ?? [], open.description ?? ""));
      if (target.kind === "url") window.open(target.url, "_blank", "noopener,noreferrer");
      else if (target.kind === "unresolvable") dispatch({ type: "TOAST", message: `Cannot open datasheet '${target.text}': only absolute http(s)/file URLs can be opened from a browser.`, kind: "error" });
      else dispatch({ type: "TOAST", message: "No datasheet found in the footprint.", kind: "info" });
    });
  }

  // ACTIONS::showFootprintBrowser / showSymbolBrowser -- COMMON_CONTROL::ShowPlayer( FRAME_FOOTPRINT_VIEWER / FRAME_SCH_VIEWER ): the
  // read-only library viewer window. The studio's editors are the one place the libraries are listed, with the tree and a canvas
  // for the item opened, so the browser is the editor with its library tree shown (and a tree that was folded comes back).
  m.set("common.Control.showFootprintBrowser", () => {
    setDockColumnCollapsed("tree", false);
    dispatch({ type: "SET_TAB", tab: "footprint" });
  });
  m.set("common.Control.showSymbolBrowser", () => {
    setDockColumnCollapsed("tree", false);
    dispatch({ type: "SET_TAB", tab: "symbol" });
  });
}
