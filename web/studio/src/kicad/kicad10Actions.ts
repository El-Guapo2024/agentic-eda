// Actions of KiCad 25ec2335 (the box's kicad-cli, KiCad 10) that are newer than the extracted table (actions.json, KiCad 8303b2ad), with what the table would say of
// them: the five Draw Arc modes and Cycle Arc Drawing Mode (eeschema/tools/sch_actions.cpp `drawArcCenter` .. `drawArcStartDirEnd`, common/tool/actions.cpp
// `cycleArcDrawMode`; UI-LEFT-2 SS3). The toolbar and the menus read them where the table has no entry; their icons are in public/icons (see its LICENSE).
import type { ArcDrawMode } from "../kicad-port/arcGeom";
import { ARC_MODE_ACTIONS, CYCLE_ARC_DRAW_MODE } from "../kicad-port/schArcDraw";

export interface Kicad10Action {
  name: string;
  label: string;
  tooltip: string;
  icon: string | null;
}

export const KICAD10_ACTIONS: readonly Kicad10Action[] = [
  ...(Object.keys(ARC_MODE_ACTIONS) as ArcDrawMode[]).map((m) => ({ name: ARC_MODE_ACTIONS[m].action, label: ARC_MODE_ACTIONS[m].label, tooltip: ARC_MODE_ACTIONS[m].tooltip, icon: ARC_MODE_ACTIONS[m].icon })),
  { name: CYCLE_ARC_DRAW_MODE, label: "Cycle Arc Drawing Mode", tooltip: "Switch to the next method of drawing arcs", icon: null },
];

/** The icons of those actions, by name: the file in public/icons/{light,dark}. */
export const KICAD10_ICONS: Readonly<Record<string, string>> = Object.fromEntries(KICAD10_ACTIONS.flatMap((a) => (a.icon ? [[a.icon, `${a.icon}.svg`]] : [])));

/**
 * KiCad 25ec2335's schematic and Symbol Editor drawing toolbars put the five arc modes in one group ("Arc", `TOOLBAR_GROUP_CONFIG`) where 8303b2ad has Draw Arcs; the
 * schematic's tool draws in all five, the Symbol Editor's still in 8303b2ad's way, so only the schematic's toolbar takes the group.
 */
export const SCH_ARC_GROUP = { type: "group" as const, label: "Arc", icon: "add_arc_center", items: KICAD10_ACTIONS.filter((a) => a.icon).map((a) => a.name) };
