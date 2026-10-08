// What the library tree of the two library editors remembers: which libraries are folded (Expand All / Collapse All), which are pinned (Pin
// Library / Unpin Library) and the rules that read them. Ported from common/tool/library_editor_control.cpp (`LIBRARY_EDITOR_CONTROL::
// changeSelectedPinStatus`, `AddContextMenuItems`), common/lib_tree_model.cpp (`LIB_TREE_NODE::Compare`: pinned libraries sort first) and
// common/lib_tree_model_adapter.cpp (`GetPinningSymbol`, "☆ " in front of a pinned library's name), commit 8303b2ad. Pure: the store is
// state/libraryTree.ts and the tree is components/library/LibraryTree.tsx.

/** `LIB_TREE_MODEL_ADAPTER::GetPinningSymbol()`: not an ASCII7 character, a unicode one. */
export const PIN_GLYPH = "☆ ";

/**
 * Which libraries are folded. Expand All and Collapse All set a default for every library, present and future, and the libraries clicked
 * since are the exceptions to it; a tree that has just been made shows every library open.
 */
export interface TreeFold {
  readonly defaultCollapsed: boolean;
  readonly flipped: ReadonlySet<string>;
}

/** `wxDataViewCtrl::ExpandAll` / `CollapseAll`, `LIB_TREE::onKeyDown` and the tree's configuration menu. */
export const ALL_EXPANDED: TreeFold = { defaultCollapsed: false, flipped: new Set() };
export const ALL_COLLAPSED: TreeFold = { defaultCollapsed: true, flipped: new Set() };

export function isFolded(fold: TreeFold, lib: string): boolean {
  return fold.defaultCollapsed !== fold.flipped.has(lib);
}

/** A click on a library's row or its arrow: open it when folded, fold it when open. */
export function toggleFold(fold: TreeFold, lib: string): TreeFold {
  const flipped = new Set(fold.flipped);
  if (!flipped.delete(lib)) flipped.add(lib);
  return { defaultCollapsed: fold.defaultCollapsed, flipped };
}

/**
 * `LIB_TREE_NODE::Compare`: "Pinned nodes go next" -- the pinned libraries come before the others, each group keeping its own order.
 * (The recently-used group that precedes them has no counterpart in these trees.)
 */
export function pinnedFirst<T extends { lib: string }>(groups: readonly T[], pinned: ReadonlySet<string>): T[] {
  return [...groups.filter((g) => pinned.has(g.lib)), ...groups.filter((g) => !pinned.has(g.lib))];
}

/** The label a library's row shows: `GetPinningSymbol() + name` when pinned. */
export function libraryLabel(lib: string, pinned: ReadonlySet<string>): string {
  return pinned.has(lib) ? PIN_GLYPH + lib : lib;
}

/**
 * `changeSelectedPinStatus( aPin )`: every selected library whose status differs from `pin` is pinned (or unpinned); the others are left
 * as they are. Returns the new set, or the same one when nothing changed.
 */
export function withPinned(pinned: ReadonlySet<string>, libs: readonly string[], pin: boolean): ReadonlySet<string> {
  const next = new Set(pinned);
  let changed = false;
  for (const lib of libs) {
    if (pin && !next.has(lib)) {
      next.add(lib);
      changed = true;
    } else if (!pin && next.delete(lib)) {
      changed = true;
    }
  }
  return changed ? next : pinned;
}

/**
 * `AddContextMenuItems`' `pinnedLibSelectedCondition` / `unpinnedLibSelectedCondition`: Unpin Library is offered when every selected library
 * is pinned, Pin Library when every one is not. KiCad also offers both when no library at all is selected (its loop never clears the
 * flag), where they do nothing; here they need a selected library.
 */
export function pinMenu(libs: readonly string[], pinned: ReadonlySet<string>): { pin: boolean; unpin: boolean } {
  if (libs.length === 0) return { pin: false, unpin: false };
  return { pin: libs.every((l) => !pinned.has(l)), unpin: libs.every((l) => pinned.has(l)) };
}
