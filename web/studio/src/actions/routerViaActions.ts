// The router tool's own `static const TOOL_ACTION`s that are not the corner modes (pcbnew/router/router_tool.cpp at 8303b2ad), all `AS_CONTEXT`:
// live only while the router is the active tool (registered by `registerPcbEditSweep` under its `inRouterTool`):
//
//   PlaceBlindVia (Alt+Shift+V), PlaceMicroVia (Ctrl+V)              ROUTER_TOOL::onViaCommand -> handleLayerSwitch( aEvent, true )
//   SelLayerAndPlaceVia (<), SelLayerAndPlaceBlindVia (Alt+<),        the same, after the layer picker (`SelectOneLayer`,
//   SelLayerAndPlaceMicroVia                                          PCB_ONE_LAYER_SELECTOR)
//   SwitchPosture (/)                                                 performRouting: `m_router->FlipPosture()` and a refreshing Move
//   CustomTrackViaSize (Q)                                            ROUTER_TOOL::CustomTrackWidthDialog (DIALOG_TRACK_VIA_SIZE),
//                                                                     then onTrackViaSizeChanged
//
// The via's type, layers and size come from kicad-port/routerVia.ts; the router places it at the next click and the finished route commits it
// with its type, one `Cmd::CommitRoute` (crates/cli/src/route_api.rs `via`, crates/ops `via_kinds`). Place Through Via (V) is the studio's
// layerToggle (actions/pcbMenuActions.ts).

import { createElement } from "react";
import { dpMove, routeMove, routeToggleVia } from "../api/client";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { dpStateFromPreview } from "../kicad-port/dpTool";
import { pickableLayers, planViaCommand, viaSizeFor, VIA_COMMANDS, type ViaCommand } from "../kicad-port/routerVia";
import { SelectLayerDialog, TrackViaSizeDialog } from "../components/PcbRouterDialogs";
import { openSweepDialog } from "./pcbSweepDialogs";
import { layerPairsOf } from "./pcbRouterSweep";
import type { SweepCtx } from "./pcbSweepKit";

export function registerRouterViaActions(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const pcbOnly =
    (fn: () => void) =>
    () => {
      if (state.tab === "pcb") fn();
    };
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  const draw = state.drawState;

  // ------------------------------------------------------------------------------ via commands
  // ROUTER_TOOL::onViaCommand: a via already armed is taken back (`ToggleViaPlacement()`, whichever command); else handleLayerSwitch arms one.
  // Only a route of one track: `handleLayerSwitch` needs a route in progress for a picked layer, and the studio's pair placer places through
  // vias alone (crates/pns/src/dp_placer.rs), so in the pair tool these do nothing.
  const viaCommand = (name: string) =>
    pcbOnly(() => {
      const cmd: ViaCommand | undefined = VIA_COMMANDS[name];
      if (!board || !cmd || draw?.kind !== "route") return;
      const cursor = state.cursorUm ?? { x: draw.pts[draw.pts.length - 1]?.[0] ?? 0, y: draw.pts[draw.pts.length - 1]?.[1] ?? 0 };
      // the new state, then the head re-resolved at the cursor with the via armed or not (`m_router->Move( m_endSnapPoint, m_endItem )`)
      const update = (next: typeof draw) => {
        dispatch({ type: "SET_DRAW_STATE", draw: next });
        void routeMove(cursor.x, cursor.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(next, preview) }));
      };
      if (draw.placingVia) {
        void routeToggleVia(false).then(() => update({ ...draw, placingVia: false, pendingViaLayer: undefined }));
        return;
      }
      const copper = board.layers;
      const layerPair = layerPairsOf(board, state.pcbx.layerPairs).current;
      const arm = (picked: string | null) => {
        const plan = planViaCommand({ cmd, currentLayer: draw.layer, copper, layerPair, picked });
        if (!plan.ok) {
          if (plan.message) toast(plan.message, "error");
          return;
        }
        const size = viaSizeFor(board, draw.net, plan.viaType, state.currentViaPreset);
        void routeToggleVia(true, size.diameter, size.drill, plan.toLayer, { viaType: plan.viaType, span: plan.span }).then((reply) => {
          if (!reply.ok) {
            if (reply.message) toast(reply.message, "error");
            return;
          }
          update({ ...draw, placingVia: true, pendingViaLayer: plan.toLayer });
        });
      };
      if (!cmd.selectLayer) {
        arm(null);
        return;
      }
      // `frame()->SelectOneLayer( currentLayer, not_allowed_ly, endPoint )`
      openSweepDialog({
        kind: "element",
        element: createElement(SelectLayerDialog, { layers: pickableLayers(cmd, copper, draw.layer), copper, current: draw.layer, onPick: (layer: string) => arm(layer), onCancel: () => arm(null) }),
      });
    });
  m.set("pcbnew.InteractiveRouter.PlaceBlindVia", viaCommand("pcbnew.InteractiveRouter.PlaceBlindVia"));
  m.set("pcbnew.InteractiveRouter.PlaceMicroVia", viaCommand("pcbnew.InteractiveRouter.PlaceMicroVia"));
  m.set("pcbnew.InteractiveRouter.SelLayerAndPlaceVia", viaCommand("pcbnew.InteractiveRouter.SelLayerAndPlaceVia"));
  m.set("pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia", viaCommand("pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia"));
  m.set("pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia", viaCommand("pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia"));

  // ------------------------------------------------------------------------------ Switch Track Posture
  // performRouting: `else if( evt->IsAction( &ACT_SwitchPosture ) ) { m_router->FlipPosture(); updateEndItem( *evt ); m_router->Move( m_endSnapPoint, m_endItem ); }`
  // -- while a route is being drawn; the pair's spine flips the same way (`DIFF_PAIR_PLACER::FlipPosture`).
  m.set(
    "pcbnew.InteractiveRouter.SwitchPosture",
    pcbOnly(() => {
      const cur = state.cursorUm;
      if (!cur) return;
      if (draw?.kind === "route") {
        void routeMove(cur.x, cur.y, true).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) }));
      } else if (draw?.kind === "diffpair") {
        void dpMove(cur.x, cur.y, true).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) }));
      }
    })
  );

  // ------------------------------------------------------------------------------ Custom Track/Via Size...
  // ROUTER_TOOL::CustomTrackWidthDialog: DIALOG_TRACK_VIA_SIZE on the board's custom sizes; OK sets `m_TempOverrideTrackWidth = true` and
  // `UseCustomTrackViaSize( true )`, then onTrackViaSizeChanged: the route in progress takes the sizes at once (`UpdateSizes`, a refreshing Move).
  m.set(
    "pcbnew.InteractiveRouter.CustomTrackViaSize",
    pcbOnly(() => {
      openSweepDialog({
        kind: "element",
        element: createElement(TrackViaSizeDialog, {
          value: state.pcbx.customTrackVia,
          onOk: (v) => {
            dispatch({ type: "PCBX", patch: { customTrackVia: v, useCustomTrackVia: true } });
            dispatch({ type: "BCX", patch: { autoTrackWidth: false } });
            dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthUm: v.trackUm });
            dispatch({ type: "SET_CURRENT_VIA_PRESET", preset: { diameter: v.viaDiameterUm, drill: v.viaDrillUm } });
            const live = ctx.api.getState().drawState;
            const at = ctx.api.getState().cursorUm;
            if (live?.kind === "route" && at) {
              dispatch({ type: "SET_DRAW_STATE", draw: { ...live, width: v.trackUm } });
              void routeMove(at.x, at.y, undefined, v.trackUm).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview({ ...live, width: v.trackUm }, preview) }));
            }
          },
        }),
      });
    })
  );
}
