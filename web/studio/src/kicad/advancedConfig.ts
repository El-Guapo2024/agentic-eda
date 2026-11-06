// `ADVANCED_CFG` (common/advanced_config.cpp, KiCad 8303b2ad): the hidden settings of a `kicad_advanced` file, each off unless the file sets it. Some menu
// entries and toolbar buttons KiCad adds only with one of them on; the extraction (tools/extract-menus.js, extract-toolbars.js) reads the code that builds the
// menus and toolbars without its conditions, so it lists them. The studio has no `kicad_advanced`, so they are left out here, as KiCad leaves them out by
// default. Their actions stay wired (Preferences > Hotkeys can bind them, as KiCad's hotkey list can).
import type { MenuNode, MenusFile, ToolbarsFile } from "./types";

/**
 * `m_ExtraZoneDisplayModes` ("ExtraZoneDisplayModes", default false): `menubar_pcb_editor.cpp` adds Draw Zone Fractures and Draw Zone Triangulation to View >
 * Drawing Mode, and `toolbars_pcb_editor.cpp` their buttons to the left toolbar, only `if( ADVANCED_CFG::GetCfg().m_ExtraZoneDisplayModes )` -- the zone
 * fill's debug views.
 */
export const ADVANCED_CFG_ONLY_ACTIONS: ReadonlySet<string> = new Set(["pcbnew.Control.zoneDisplayOutlines", "pcbnew.Control.zoneDisplayTesselation"]);

function menuNodes(nodes: readonly MenuNode[]): MenuNode[] {
  return nodes.flatMap((n): MenuNode[] => {
    if (n.type === "item") return ADVANCED_CFG_ONLY_ACTIONS.has(n.action) ? [] : [n];
    if (n.type === "submenu") return [{ ...n, items: menuNodes(n.items) }];
    return [n];
  });
}

/** The menus without the entries KiCad shows only with an advanced setting on. */
export function menusWithoutAdvanced(file: MenusFile): MenusFile {
  return { ...file, menus: file.menus.map((m) => ({ ...m, items: menuNodes(m.items) })) };
}

/** The toolbars without the buttons KiCad shows only with an advanced setting on. */
export function toolbarsWithoutAdvanced(file: ToolbarsFile): ToolbarsFile {
  return {
    ...file,
    toolbars: file.toolbars.map((t) => ({ ...t, items: t.items.filter((i) => i.type !== "action" || !ADVANCED_CFG_ONLY_ACTIONS.has(i.action)) })),
  };
}
