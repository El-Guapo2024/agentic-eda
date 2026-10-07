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
import { convertAvailability } from "../kicad-port/pcbConvert";
import { activeEditPoint, canRemoveCorner } from "../kicad-port/pcbPointEdit";
import { handleToleranceUm, pointItemOf, pointsOfItem, ringOf } from "./pcbPointEditSweep";
import { routeQueueActive } from "../components/canvas/routeQueue";

const LABELS = new Map<string, string>((actionsData as { actions: { name: string; label: string }[] }).actions.map((a) => [a.name, a.label]));

/** The action's own label from `actions.json` ("Fillet Lines..."). */
const label = (name: string): string => LABELS.get(name) ?? name;

export function pcbSweepMenuEntries(state: StudioState, api: StudioApi, refs: readonly string[], run: (name: string) => void, isEnabled: (name: string) => boolean, at?: readonly [number, number]): MenuEntry[] {
  const board = state.board;
  const out: MenuEntry[] = [];
  const add = (name: string, when: boolean): void => {
    if (when && isEnabled(name)) out.push({ label: label(name), onSelect: () => run(name) });
  };
  // The router's menu offers Cancel Current Item only inside RouteSelected (`inRouteSelected`).
  add("pcbnew.InteractiveRouter.CancelCurrentItem", routeQueueActive());
  // The differential pair tool's own menu ("Select Differential Pair Size" > custom size): the dimensions dialog.
  add("pcbnew.InteractiveRouter.DiffPairDialog", state.activeTool === "diffpair" || state.drawState?.kind === "diffpair");
  if (!board || refs.length === 0) return out;
  const kinds = refs.map((id) => itemKind(board, id));
  const shapeKinds = refs.map((id) => api.shapeById(id)?.kind ?? null);
  const count = refs.length;
  const only = (pred: (i: number) => boolean): boolean => refs.every((_, i) => pred(i));
  const has = (pred: (i: number) => boolean): boolean => refs.some((_, i) => pred(i));
  const shapeIn = (i: number, ks: readonly string[]): boolean => shapeKinds[i] != null && ks.includes(shapeKinds[i]!);

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

  // Point editor corner operations (EDIT_TOOL::Init): the handle under the pointer decides which of them apply.
  const pointItem = count === 1 ? pointItemOf(board, new Set(refs)) : null;
  const ring = pointItem ? ringOf(pointItem) : null;
  const active = pointItem && at ? activeEditPoint(pointsOfItem(pointItem), at, handleToleranceUm(state.view.scale)) : null;
  add("pcbnew.InteractiveEdit.moveCorner", active?.kind === "corner");
  add("pcbnew.InteractiveEdit.moveMidpoint", active?.kind === "midpoint");
  add("pcbnew.PointEditor.removeCorner", !!ring && active?.kind === "corner" && active.id.startsWith("v") && canRemoveCorner(ring));
  add("pcbnew.PointEditor.chamferCorner", !!ring);
  add("pcbnew.InteractiveEdit.editVertices", !!ring);
  const arc = count === 1 && shapeKinds[0] === "arc";
  add("common.Interactive.cycleArcEditMode", arc);
  add("pcbnew.PointEditor.arcKeepCenter", arc);
  add("pcbnew.PointEditor.arcKeepEndpoint", arc);
  add("pcbnew.PointEditor.arcKeepRadius", arc);

  // "Create from Selection" submenu (CONVERT_TOOL::Init)
  const convert = convertAvailability(board, refs);
  add("pcbnew.Convert.convertToPoly", convert.poly);
  add("pcbnew.Convert.convertToZone", convert.zone);
  add("pcbnew.Convert.convertToKeepout", convert.keepout);
  add("pcbnew.Convert.convertToLines", convert.lines);
  add("pcbnew.Convert.outsetItems", convert.outset);
  add("pcbnew.Convert.convertToTracks", convert.tracks);
  add("pcbnew.Convert.convertToArc", convert.arc);
  return out;
}
