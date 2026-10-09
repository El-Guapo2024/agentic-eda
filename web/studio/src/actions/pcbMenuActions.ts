// The entries of the board's right-click menus that have no KiCad action of their own in actions.json (kicad-port/pcbContextMenu.ts, components/canvas/pcbMenuBuilder.ts):
//
//   router_tool.cpp  ACT_PlaceThroughVia ("V")   -> the studio's `V` while routing (layerToggle: drop a via and go on to the other layer)
//   router_tool.cpp  TRACK_WIDTH_MENU / DIFF_PAIR_MENU's event handlers: the entries pick the width and via size the next route starts with
//
// `studio.Router.*` are this studio's own ids, the way kicad/menuExtras.ts's `studio.*` entries are.
import type { Dispatch } from "react";
import type { Action } from "../state/store";
import type { SweepCtx } from "./pcbSweepKit";
import { routeMove } from "../api/client";
import { drawStateFromPreview } from "../kicad-port/routeTool";

type Handler = (param?: unknown) => void;

export function registerPcbMenuActions(m: Map<string, Handler>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const pcbOnly =
    (fn: Handler): Handler =>
    (param) => {
      if (state.tab === "pcb") fn(param);
    };
  const base = { width: board?.board_rules?.track_width ?? 250, via: { diameter: board?.board_rules?.via_diameter ?? 600, drill: board?.board_rules?.via_drill ?? 300 } };
  const widths = (): number[] => [base.width, ...(board?.routing?.track_width_presets ?? [])];
  const vias = () => [base.via, ...(board?.routing?.via_presets ?? [])];
  const index = (param: unknown): number => (typeof param === "number" ? Math.round(param) : 0);

  /** A width picked while a route is being drawn takes effect at once, as `W` does: the live head is re-resolved at the cursor with it. */
  const reroute = (d: Dispatch<Action>, width: number) => {
    const draw = state.drawState;
    if (draw?.kind !== "route") return;
    const cursor = state.cursorUm ?? { x: draw.pts[draw.pts.length - 1]?.[0] ?? 0, y: draw.pts[draw.pts.length - 1]?.[1] ?? 0 };
    d({ type: "SET_DRAW_STATE", draw: { ...draw, width } });
    void routeMove(cursor.x, cursor.y, undefined, width).then((preview) => {
      if (preview.ok) d({ type: "SET_DRAW_STATE", draw: drawStateFromPreview({ ...draw, width }, preview) });
    });
  };

  // "Use Net Class Values": the board's own track width and via size, not the starting track's.
  m.set(
    "studio.Router.useNetclassSizes",
    pcbOnly(() => {
      dispatch({ type: "BCX", patch: { autoTrackWidth: false } });
      dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthUm: base.width });
      dispatch({ type: "SET_CURRENT_VIA_PRESET", preset: base.via });
      reroute(dispatch, base.width);
    })
  );
  // A track width of the list (entry 0 is the net class's).
  m.set(
    "studio.Router.setTrackWidth",
    pcbOnly((param) => {
      const width = widths()[index(param)];
      if (width == null) return;
      dispatch({ type: "BCX", patch: { autoTrackWidth: false } });
      dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthUm: width });
      reroute(dispatch, width);
    })
  );
  // A via size of the list (entry 0 is the net class's).
  m.set(
    "studio.Router.setViaSize",
    pcbOnly((param) => {
      const preset = vias()[index(param)];
      if (!preset) return;
      dispatch({ type: "BCX", patch: { autoTrackWidth: false } });
      dispatch({ type: "SET_CURRENT_VIA_PRESET", preset });
    })
  );
  // "Use Net Class Values" of the pair menu (`bds.UseCustomDiffPairDimensions( false )`).
  m.set(
    "studio.Router.useNetclassDiffPair",
    pcbOnly(() => dispatch({ type: "PCBX", patch: { customDiffPair: null } }))
  );
  // ACT_PlaceThroughVia ("V" while routing) is the studio's layerToggle: only registered while a route is being drawn.
  const placeVia = m.get("pcbnew.Control.layerToggle");
  if (placeVia) m.set("pcbnew.InteractiveRouter.PlaceVia", placeVia);
}
