// Menu entries this studio adds to a menu extracted from KiCad (sym_menus.json, fp_menus.json, ... are generated -- nothing app-specific goes into them),
// appended at the end of the named top-level menu. They keep a feature reachable that the old hand-built text toolbars of the library editors had and
// KiCad's own menus do not: the Symbol Editor's "Update Symbol on Board" (KiCad saves the library file and the schematic offers the update; this studio's
// library IS the design, so the push to the placed instances is explicit) and "Show Pin Numbers".
import type { MenuNode } from "./types";

export type MenuExtras = Record<string, MenuNode[]>;

export const SYMBOL_EDITOR_MENU_EXTRAS: MenuExtras = {
  File: [{ type: "item", action: "studio.SymbolEditor.updateOnBoard", label: "Update Symbol on Board" }],
  View: [{ type: "item", action: "eeschema.SymbolLibraryControl.showPinNumbers" }],
};
