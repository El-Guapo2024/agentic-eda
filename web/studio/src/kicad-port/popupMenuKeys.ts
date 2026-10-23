// What a key does while a popup (context) menu is open. A wxMenu shown by `PopupMenu` (common/tool/tool_menu.cpp, `TOOL_MANAGER::DispatchContextMenu`)
// is modal: Escape dismisses the menu and nothing else sees the key -- the dialog the menu was opened in (the DRC and ERC windows' marker menus) stays
// open and the tool under it is not cancelled. Pure; components/canvas/ContextMenu.tsx applies it.

export interface PopupMenuKey {
  /** The menu closes. */
  close: boolean;
  /** The key goes no further: neither the window's Escape (`escapeTopDialog`, `common.Interactive.cancel`) nor the canvas sees it. */
  consume: boolean;
}

export function popupMenuKey(key: string): PopupMenuKey {
  return key === "Escape" ? { close: true, consume: true } : { close: false, consume: false };
}
