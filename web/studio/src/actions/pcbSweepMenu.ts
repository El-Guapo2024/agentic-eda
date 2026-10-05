// The right-click entries of the pcbnew edit tools (actions/pcbEditSweep.ts), each
// shown under the same selection condition KiCad's `CONDITIONAL_MENU`s give it:
// EDIT_TOOL::Init (pcbnew/tools/edit_tool.cpp: Break Track, Fillet Tracks, the
// Routing / Mirror / Shape Modification / Positioning submenus) and the
// selection tool's `SELECT_MENU` (pcb_selection_tool.cpp). The studio's context
// menu is a flat list, so the submenus' entries are listed in place and only
// when they apply.

import type { StudioApi, StudioState } from "../state/store";
import type { MenuEntry } from "../components/canvas/ContextMenu";
import actionsData from "../kicad/actions.json";
import { itemKind } from "../kicad-port/pcbItems";

const LABELS = new Map<string, string>((actionsData as { actions: { name: string; label: string }[] }).actions.map((a) => [a.name, a.label]));

/** The action's own label from `actions.json` ("Fillet Lines..."). */
const label = (name: string): string => LABELS.get(name) ?? name;

export function pcbSweepMenuEntries(state: StudioState, api: StudioApi, refs: readonly string[], run: (name: string) => void, isEnabled: (name: string) => boolean): MenuEntry[] {
  const board = state.board;
  if (!board || refs.length === 0) return [];
  const kinds = refs.map((id) => itemKind(board, id));
  const shapeKinds = refs.map((id) => api.shapeById(id)?.kind ?? null);
  const count = refs.length;
  const only = (pred: (i: number) => boolean): boolean => refs.every((_, i) => pred(i));
  const has = (pred: (i: number) => boolean): boolean => refs.some((_, i) => pred(i));
  const shapeIn = (i: number, ks: readonly string[]): boolean => shapeKinds[i] != null && ks.includes(shapeKinds[i]!);

  const out: MenuEntry[] = [];
  const add = (name: string, when: boolean): void => {
    if (when && isEnabled(name)) out.push({ label: label(name), onSelect: () => run(name) });
  };

  // SELECT_MENU
  add("pcbnew.InteractiveSelection.FilterSelection", true);
  add("pcbnew.InteractiveSelection.SelectNet", has((i) => kinds[i] === "track" || kinds[i] === "via" || kinds[i] === "zone"));
  add("pcbnew.InteractiveSelection.SelectSameSheet", count === 1 && kinds[0] === "part");
  add("pcbnew.InteractiveSelection.SelectOnSchematic", has((i) => kinds[i] === "part"));

  // Break Track / Fillet Tracks (trackTypes = track, arc, via)
  add("pcbnew.InteractiveRouter.BreakTrack", count === 1 && kinds[0] === "track");
  add("pcbnew.InteractiveEdit.filletTracks", only((i) => kinds[i] === "track" || kinds[i] === "via"));

  // Routing submenu (`isRoutable`)
  add("pcbnew.InteractiveSelection.unrouteSelected", has((i) => kinds[i] === "track" || kinds[i] === "via" || kinds[i] === "part"));

  // Mirror / Rotate submenu (`canMirror`)
  const mirrorable = has((i) => kinds[i] === "track" || kinds[i] === "via" || kinds[i] === "zone" || kinds[i] === "shape" || kinds[i] === "text" || kinds[i] === "group");
  add("pcbnew.InteractiveEdit.mirrorHoriontally", mirrorable);
  add("pcbnew.InteractiveEdit.mirrorVertically", mirrorable);

  // Shape Modification submenu
  add("pcbnew.InteractiveEdit.healShapes", has((i) => shapeIn(i, ["segment", "arc", "bezier"])));
  add("pcbnew.InteractiveEdit.simplifyPolygons", has((i) => shapeIn(i, ["polygon"]) || kinds[i] === "zone"));
  const filletTypes = only((i) => shapeIn(i, ["polygon", "rect", "segment"]));
  add("pcbnew.InteractiveEdit.filletLines", filletTypes);
  add("pcbnew.InteractiveEdit.chamferLines", filletTypes);
  add("pcbnew.InteractiveEdit.dogboneCorners", filletTypes);
  add("pcbnew.InteractiveEdit.extendLines", only((i) => shapeIn(i, ["segment"])) && count === 2);
  const booleanTypes = only((i) => shapeIn(i, ["rect", "polygon", "circle"])) && count > 1;
  add("pcbnew.InteractiveEdit.mergePolygons", booleanTypes);
  add("pcbnew.InteractiveEdit.subtractPolygons", booleanTypes);
  add("pcbnew.InteractiveEdit.intersectPolygons", booleanTypes);
  return out;
}
