// The entries of the board's right-click menus that have no KiCad action of their own in actions.json (kicad-port/pcbContextMenu.ts, components/canvas/pcbMenuBuilder.ts):
//
//   router_tool.cpp  ACT_PlaceThroughVia ("V")   -> the studio's `V` while routing (layerToggle: drop a via and go on to the other layer)
//   router_tool.cpp  TRACK_WIDTH_MENU / DIFF_PAIR_MENU's event handlers: the entries pick the width and via size the next route starts with
//
// `studio.Router.*` are this studio's own ids, the way kicad/menuExtras.ts's `studio.*` entries are.
import type { Dispatch } from "react";
import type { Action } from "../state/store";
import type { SweepCtx } from "./pcbSweepKit";
import { dpMove, dpSetDims, routeMove } from "../api/client";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { dpStateFromPreview } from "../kicad-port/dpTool";
import { chosenDiffPair, routeWidthAfterChange } from "../kicad-port/routerSizes";

type Handler = (param?: unknown) => void;

export function registerPcbMenuActions(m: Map<string, Handler>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const pcbOnly =
    (fn: Handler): Handler =>
    (param) => {
      if (state.tab === "pcb") fn(param);
    };
  const base = { width: board?.board_rules?.track_width ?? 250_000, via: { diameter: board?.board_rules?.via_diameter ?? 600_000, drill: board?.board_rules?.via_drill ?? 300_000 } };
  const widths = (): number[] => [base.width, ...(board?.routing?.track_width_presets ?? [])];
  const vias = () => [base.via, ...(board?.routing?.via_presets ?? [])];
  const index = (param: unknown): number => (typeof param === "number" ? Math.round(param) : 0);

  /** A width picked while a route is being drawn takes effect at once, as `W` does: the live head is re-resolved at the cursor with it
   * (`trackViaSizeChanged` -> `ImportSizes` with `m_TempOverrideTrackWidth` set: the list's entry, or at entry 0 the net's rule width, the
   * start reply's `rule_width`). `index` is the list entry chosen, or undefined for one the width is given for. */
  const reroute = (d: Dispatch<Action>, width: number, index?: number) => {
    const draw = state.drawState;
    if (draw?.kind !== "route") return;
    const cursor = state.cursorNm ?? { x: draw.pts[draw.pts.length - 1]?.[0] ?? 0, y: draw.pts[draw.pts.length - 1]?.[1] ?? 0 };
    const w = index === undefined ? width : routeWidthAfterChange(board?.routing?.track_width_presets ?? [], index, false, 0, draw.sizes?.rule_width ?? width);
    const source = index === 0 ? (draw.sizes?.rule_width_source ?? draw.sizes?.width_source) : "user choice";
    const next = { ...draw, width: w, widthOverride: true, sizes: draw.sizes ? { ...draw.sizes, width_source: source } : undefined };
    d({ type: "SET_DRAW_STATE", draw: next });
    void routeMove(cursor.x, cursor.y, undefined, w).then((preview) => {
      if (preview.ok) d({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(next, preview) });
    });
  };

  // "Use Net Class Values": the board's own track width and via size, not the starting track's.
  m.set(
    "studio.Router.useNetclassSizes",
    pcbOnly(() => {
      dispatch({ type: "BCX", patch: { autoTrackWidth: false } });
      dispatch({ type: "PCBX", patch: { useCustomTrackVia: false } });
      // `SetTrackWidthIndex( 0 )` / `SetViaSizeIndex( 0 )`: the dummy entry is the net class's.
      dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthNm: base.width, index: 0 });
      dispatch({ type: "SET_CURRENT_VIA_PRESET", preset: base.via, index: 0 });
      reroute(dispatch, base.width, 0);
    })
  );
  // A track width of the list (entry 0 is the net class's). `TRACK_WIDTH_MENU::eventHandler`: `UseCustomTrackViaSize( false )`,
  // `m_TempOverrideTrackWidth = true`, `SetTrackWidthIndex( .. )` -- "Use Starting Track Width" (`m_UseConnectedTrackWidth`) is left as it is:
  // the choice overrides it for the route in progress only, and the next route that starts on a track inherits its width again (r5 R5-7).
  m.set(
    "studio.Router.setTrackWidth",
    pcbOnly((param) => {
      const width = widths()[index(param)];
      if (width == null) return;
      // `SetTrackWidthIndex`: "m_useCustomTrackVia = false"
      dispatch({ type: "PCBX", patch: { useCustomTrackVia: false } });
      dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthNm: width, index: index(param) });
      reroute(dispatch, width, index(param));
    })
  );
  // A via size of the list (entry 0 is the net class's): `UseCustomTrackViaSize( false )`, `SetViaSizeIndex( .. )`, nothing else (r5 R5-7).
  m.set(
    "studio.Router.setViaSize",
    pcbOnly((param) => {
      const preset = vias()[index(param)];
      if (!preset) return;
      // `SetViaSizeIndex`: "m_useCustomTrackVia = false"
      dispatch({ type: "PCBX", patch: { useCustomTrackVia: false } });
      dispatch({ type: "SET_CURRENT_VIA_PRESET", preset, index: index(param) });
    })
  );
  // "Use Net Class Values" of the pair menu (`bds.UseCustomDiffPairDimensions( false ); bds.SetDiffPairIndex( 0 );`).
  m.set(
    "studio.Router.useNetclassDiffPair",
    pcbOnly(() => dispatch({ type: "PCBX", patch: { useCustomDiffPair: false, diffPairIndex: 0 } }))
  );
  // A pair of Pre-defined Sizes' Differential Pairs (`ID_POPUP_PCB_SELECT_DIFFPAIR1 + i - 1`: `UseCustomDiffPairDimensions( false )`,
  // `SetDiffPairIndex( i )`), then `trackViaSizeChanged`: a pair being routed takes it at once (r5 R5-5).
  m.set(
    "studio.Router.setDiffPair",
    pcbOnly((param) => {
      const i = index(param);
      const dims = chosenDiffPair(board?.routing?.diff_pair_presets ?? [], i, false, null);
      if (!dims) return;
      dispatch({ type: "PCBX", patch: { useCustomDiffPair: false, diffPairIndex: i } });
      const draw = state.drawState;
      if (draw?.kind !== "diffpair") return;
      const cursor = state.cursorNm;
      const sizes = draw.sizes ? { ...draw.sizes, width_source: "user choice", gap_source: "user choice" } : undefined;
      void dpSetDims(dims.width, dims.gap, dims.viaGap).then(() => cursor && dpMove(cursor.x, cursor.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview({ ...draw, sizes }, preview) })));
    })
  );
  // "Use Custom Values..." of the pair menu (`DIFF_PAIR_MENU::eventHandler`, `ID_POPUP_PCB_SELECT_CUSTOM_DIFFPAIR`): "bds.UseCustomDiffPairDimensions(
  // true ); toolManager->RunAction( PCB_ACTIONS::routerDiffPairDialog );" -- the flag stays set whatever the dialog returns.
  m.set(
    "studio.Router.useCustomDiffPair",
    pcbOnly(() => {
      dispatch({ type: "PCBX", patch: { useCustomDiffPair: true } });
      m.get("pcbnew.InteractiveRouter.DiffPairDialog")?.();
    })
  );
  // ACT_PlaceThroughVia ("V" while routing) is the studio's layerToggle: only registered while a route is being drawn.
  const placeVia = m.get("pcbnew.Control.layerToggle");
  if (placeVia) m.set("pcbnew.InteractiveRouter.PlaceVia", placeVia);
}
