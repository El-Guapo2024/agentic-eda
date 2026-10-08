// Menu entries KiCad does not have, added to the menus extracted from its source (`src/kicad/*_menus.json`, which `tools/extract-*-menus.js` writes afresh
// each run, so nothing of ours lives in them). An entry names a studio action (`studio.*`) and carries its own label and tooltip, since `actions.json` is
// KiCad's and has no row for it.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { MenuNode, MenusFile } from "../kicad/types";

/** The menu items to append, keyed by the label of the top-level menu that gets them. */
export type StudioMenuItems = Readonly<Record<string, readonly MenuNode[]>>;

/** The schematic editor's additions: Tools > Reorganize into Module Sheets (one hierarchical sheet per functional module). */
export const SCH_STUDIO_MENU_ITEMS: StudioMenuItems = {
  Tools: [
    { type: "separator" },
    {
      type: "item",
      action: "studio.Sheets.reorganize",
      label: "Reorganize into Module Sheets",
      tooltip: "Split a flat schematic into a root sheet and one hierarchical sheet per functional module (MCU with its passives, repeated channels, connectors). Undo puts the flat sheet back.",
    },
  ],
};

/** `file` with `extra` appended to the menus it names; a menu `file` does not have is left alone. `file` itself is not changed. */
export function withStudioItems(file: MenusFile, extra: StudioMenuItems): MenusFile {
  return { ...file, menus: file.menus.map((menu) => (extra[menu.label] ? { ...menu, items: [...menu.items, ...extra[menu.label]!] } : menu)) };
}
