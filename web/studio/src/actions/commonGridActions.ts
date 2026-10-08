// The grid origin actions: Grid Origin (the picker), Reset Grid Origin and Grid Origin... (the entry dialog). The PCB Editor and the Footprint Editor offer
// them (`PCB_CONTROL`, which runs in both); `registerCommonActions` (commonActions.ts) calls `registerGridActions` once while the registry is built.
//
//   pcbnew/tools/pcb_control.cpp      PCB_CONTROL::GridPlaceOrigin / GridResetOrigin / DoSetGridOrigin   ACTIONS::gridSetOrigin, gridResetOrigin
//   common/tool/common_tools.cpp      COMMON_TOOLS::GridOrigin                                           ACTIONS::gridOrigin (common.Control.editGridOrigin)
//
// The board's grid origin is a verb (`set_grid_origin`, undoable, saved in the `.kicad_pcb` as `(grid_origin x y)`); the Footprint Editor's is the session's
// (state/gridOrigin.ts). Both are what `components/canvas/gridHelper.ts` snaps to and the painters anchor the grid at.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { picker } from "./pcbPicker";
import { NO_ORIGIN, originIsSet, originOf, type Origin } from "../kicad-port/gridOrigin";
import { getFootprintGridOrigin, setFootprintGridOrigin } from "../state/gridOrigin";
import { setGridOriginDialogOpen } from "../state/commonDialogs";

/** The `VECTOR2D*` an action's event carries: `{ x, y }` or `[x, y]`, in um. */
function pointOfParameter(arg: unknown): Origin | null {
  if (Array.isArray(arg) && arg.length === 2 && typeof arg[0] === "number" && typeof arg[1] === "number") return { x: arg[0], y: arg[1] };
  const p = arg as { x?: unknown; y?: unknown } | null;
  if (p && typeof p === "object" && typeof p.x === "number" && typeof p.y === "number") return { x: p.x, y: p.y };
  return null;
}

export function registerGridActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const tab = ctx.tab;
  if (tab !== "pcb" && tab !== "footprint") return;

  /** The origin the editor on screen has now. */
  const current = (): Origin => (tab === "pcb" ? originOf(ctx.api.getState().board?.grid_origin) : getFootprintGridOrigin());

  /** `DoSetGridOrigin( view, frame, item, point )`: the grid is anchored at `at` ((0, 0) is the reset). Nothing happens when it is already there. */
  const setOrigin = (at: Origin) => {
    const next = originIsSet(at) ? { x: Math.round(at.x), y: Math.round(at.y) } : NO_ORIGIN;
    const now = current();
    if (next.x === now.x && next.y === now.y) return;
    if (tab === "pcb") void ctx.api.cmd({ op: "set_grid_origin", at: originIsSet(next) ? next : null });
    else setFootprintGridOrigin(next);
  };

  // ACTIONS::gridSetOrigin -- PCB_CONTROL::GridPlaceOrigin: with a point as the event parameter (`VECTOR2D*`) the origin goes there; without, the picker
  // runs with the PLACE cursor and the next click -- snapped like every pick -- is the origin, a one-shot ("don't continue with tool").
  m.set("common.Control.gridSetOrigin", (arg) => {
    const direct = pointOfParameter(arg);
    if (direct) return setOrigin(direct);
    picker.start({
      kind: "point",
      prompt: "Grid Origin: click to place it, Esc to cancel",
      onPoint: (at) => {
        setOrigin({ x: at.x, y: at.y });
        return false;
      },
    });
  });

  // ACTIONS::gridResetOrigin -- PCB_CONTROL::GridResetOrigin: `DoSetGridOrigin( ..., VECTOR2D( 0, 0 ) )`.
  m.set("common.Control.gridResetOrigin", () => setOrigin(NO_ORIGIN));

  // ACTIONS::gridOrigin ("Grid Origin...", common.Control.editGridOrigin) -- COMMON_TOOLS::GridOrigin: a dialog with the X and Y of the origin
  // (components/GridOriginDialog.tsx); OK sets it (`SetGridOrigin`, then `gridSetOrigin` with the point as its parameter).
  m.set("common.Control.editGridOrigin", () => setGridOriginDialogOpen(true));
}
