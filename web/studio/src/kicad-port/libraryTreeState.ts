// What the library tree of the two library editors remembers: which libraries are open or folded (Expand All / Collapse All), which are pinned (Pin
// Library / Unpin Library) and the rules that read them. Ported from common/tool/library_editor_control.cpp (`LIBRARY_EDITOR_CONTROL::
// changeSelectedPinStatus`, `AddContextMenuItems`), common/lib_tree_model.cpp (`LIB_TREE_NODE::Compare`: pinned libraries sort first) and
// common/lib_tree_model_adapter.cpp (`GetPinningSymbol`, "☆ " in front of a pinned library's name), commit 8303b2ad. Pure: the store is
// state/libraryTree.ts and the tree is components/library/LibraryTree.tsx. (Whether the tree is shown at all is the dock layout's: the Libraries column.)

/** `GetPinningSymbol()`: not an ASCII7 character, a unicode one. */
export const PIN_GLYPH = "☆ ";

/**
 * Which libraries are open. A group starts the way the tree decides (`auto`: the libraries holding entries the editor already lists are open, an installed
 * library nobody asked for is folded and loads when it is opened); Expand All and Collapse All set every group at once (`open`, `closed`), and the
 * libraries clicked since are the exceptions (`explicit`).
 */
export interface TreeFold {
  readonly mode: "auto" | "open" | "closed";
  readonly explicit: ReadonlyMap<string, boolean>;
}

export const FOLD_AUTO: TreeFold = { mode: "auto", explicit: new Map() };
/** `wxDataViewCtrl::ExpandAll` / `CollapseAll`, `LIB_TREE::onKeyDown` and the tree's configuration menu. */
export const ALL_EXPANDED: TreeFold = { mode: "open", explicit: new Map() };
export const ALL_COLLAPSED: TreeFold = { mode: "closed", explicit: new Map() };

/** What the tree knows about a group that its open state depends on. */
export interface GroupFacts {
  /** The group lists items now (the project's, the board's, or an installed library's that has been loaded). */
  hasItems: boolean;
  /** Whether the group opens by itself when nobody chose (`defaultGroupOpen` of kicad-port/libraryTreeModel.ts). */
  defaultOpen: boolean;
}

/**
 * Whether `lib` is open. A choice made on it wins; else Expand All opens the libraries that have items listed (an installed library nobody has
 * loaded stays folded: opening it is what loads its items, and expanding all of a hundred and fifty libraries would load every one), Collapse
 * All folds every one, and with neither the tree's own default holds.
 */
export function isGroupOpen(fold: TreeFold, lib: string, facts: GroupFacts): boolean {
  const chosen = fold.explicit.get(lib);
  if (chosen !== undefined) return chosen;
  if (fold.mode === "open") return facts.hasItems;
  if (fold.mode === "closed") return false;
  return facts.defaultOpen;
}

/** A click on a library's row or its arrow: the group goes to `open`. */
export function setGroupOpen(fold: TreeFold, lib: string, open: boolean): TreeFold {
  const explicit = new Map(fold.explicit);
  explicit.set(lib, open);
  return { mode: fold.mode, explicit };
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
