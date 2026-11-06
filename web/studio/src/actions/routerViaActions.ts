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
// with its type, one `Cmd::CommitRoute` (crates/cli/src/route_api.rs `via`, crates/ops `via_kinds`). Place Through Via (V, `ACT_PlaceThroughVia`, which
// wins over the global layerToggle while the router tool is active: `AS_CONTEXT`) is a via command too, with its implicit target the nearest ratsnest
// anchor's layer (components/canvas/routerViaArm.ts, UI-LEFT R5). All of them work in the pair tool as in the single track's (`handleLayerSwitch` is the
// router tool's in both modes; `DIFF_PAIR_PLACER::makeVia` takes the sizes' via type and layer pair), UI-LEFT R2.

import { createElement } from "react";
import { dpMove, dpToggleVia, routeMove, routeSwitchLayer, routeToggleVia } from "../api/client";
import { withEventSizes } from "../kicad-port/routerSizes";
import { planThroughVia } from "../components/canvas/routerViaArm";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { dpStateFromPreview } from "../kicad-port/dpTool";
import { chosenViaOf, layerPairMap, pickableLayers, planViaCommand, routerLayerTarget, VIA_COMMANDS, type ViaChoice, type ViaCommand, type ViaPlan, type ViaRefusal } from "../kicad-port/routerVia";
import { SelectLayerDialog, TrackViaSizeDialog } from "../components/PcbRouterDialogs";
import { openSweepDialog } from "./pcbSweepDialogs";
import { layerPairsOf } from "./pcbRouterSweep";
import { otherLayerOfPair } from "../kicad-port/layerPairs";
import type { SweepCtx } from "./pcbSweepKit";

/** The studio's via size choice (kicad-port/routerVia.ts `ViaChoice`): every via command of the router sizes its via from it (R6-R1). */
export function viaChoiceOf(state: SweepCtx["state"]): ViaChoice {
  return { presets: state.board?.routing?.via_presets ?? [], index: state.currentViaIndex, useCustomTrackVia: state.pcbx.useCustomTrackVia, custom: state.pcbx.customTrackVia };
}

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
  // ROUTER_TOOL::onViaCommand: a via already armed is taken back (`ToggleViaPlacement()`, whichever command); else handleLayerSwitch arms one --
  // for a route of one track and for a pair alike (`handleLayerSwitch` needs a route in progress for a picked layer).
  const viaCommand = (name: string) =>
    pcbOnly(() => {
      const cmd: ViaCommand | undefined = VIA_COMMANDS[name];
      if (!board || !cmd || (draw?.kind !== "route" && draw?.kind !== "diffpair")) return;
      const last = draw.kind === "route" ? draw.pts[draw.pts.length - 1] : draw.ptsA[draw.ptsA.length - 1];
      const cursor = state.cursorNm ?? { x: last?.[0] ?? 0, y: last?.[1] ?? 0 };
      // the new state, then the head re-resolved at the cursor with the via armed or not (`m_router->Move( m_endSnapPoint, m_endItem )`)
      const update = (patch: { placingVia: boolean; pendingViaLayer: string | undefined; sizesPairs?: Record<string, string> }) => {
        if (draw.kind === "route") {
          const next = { ...draw, ...patch };
          dispatch({ type: "SET_DRAW_STATE", draw: next });
          void routeMove(cursor.x, cursor.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(next, preview) }));
        } else {
          const next = { ...draw, ...patch };
          dispatch({ type: "SET_DRAW_STATE", draw: next });
          void dpMove(cursor.x, cursor.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(next, preview) }));
        }
      };
      if (draw.placingVia) {
        // `ToggleViaPlacement()`, then `frame()->SetActiveLayer( m_router->GetCurrentLayer() )`
        const off = draw.kind === "route" ? routeToggleVia(false) : dpToggleVia(false);
        void off.then(() => update({ placingVia: false, pendingViaLayer: undefined }));
        return;
      }
      const copper = board.layers;
      const layerPair = layerPairsOf(board, state.pcbx.layerPairs).current;
      const armPlan = (plan: ViaPlan | ViaRefusal) => {
        if (!plan.ok) {
          if (plan.message) toast(plan.message, "error");
          return;
        }
        // the net class's sizes (or a microvia's): none sent, the server asks the rules for the route's net (`handleLayerSwitch`, r3 R3-4).
        // The choice is the via list's index (0 = net class), not the size (r4 R4-8).
        const size = chosenViaOf(plan.viaType, viaChoiceOf(state));
        // a pair's vias span the sizes' layer pair whatever their type (`DIFF_PAIR_PLACER::makeVia`): the server spans them to `toLayer`
        const armed = draw.kind === "route" ? routeToggleVia(true, size?.diameter, size?.drill, plan.toLayer, { viaType: plan.viaType, span: plan.span }) : dpToggleVia(true, plan.toLayer, size?.diameter, size?.drill, plan.viaType);
        void armed.then((reply) => {
          if (!reply.ok) {
            if (reply.message) toast(reply.message, "error");
            return;
          }
          update({ placingVia: true, pendingViaLayer: plan.toLayer, sizesPairs: plan.sizesPairs });
        });
      };
      const arm = (picked: string | null) => armPlan(planViaCommand({ cmd, currentLayer: draw.layer, copper, layerPair, picked, sizesPairs: draw.sizesPairs }));
      if (cmd.type === "through" && !cmd.selectLayer) {
        // Place Through Via: "Try to switch to the nearest ratnest item's layer if we have one" (`GetNearestRatnestAnchor`, UI-LEFT R5)
        void planThroughVia(cursor, draw.layer, copper, layerPair, draw.sizesPairs).then(armPlan);
        return;
      }
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
  // V while a route or a pair is drawn: `ACT_PlaceThroughVia` (`AS_CONTEXT`, the router tool's) wins over the global `layerToggle` on the same key
  if (draw?.kind === "route" || draw?.kind === "diffpair") {
    m.set("pcbnew.InteractiveRouter.PlaceVia", viaCommand("pcbnew.InteractiveRouter.PlaceVia"));
    m.set("pcbnew.Control.layerToggle", viaCommand("pcbnew.InteractiveRouter.PlaceVia"));
  }
  m.set("pcbnew.InteractiveRouter.PlaceBlindVia", viaCommand("pcbnew.InteractiveRouter.PlaceBlindVia"));
  m.set("pcbnew.InteractiveRouter.PlaceMicroVia", viaCommand("pcbnew.InteractiveRouter.PlaceMicroVia"));
  m.set("pcbnew.InteractiveRouter.SelLayerAndPlaceVia", viaCommand("pcbnew.InteractiveRouter.SelLayerAndPlaceVia"));
  m.set("pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia", viaCommand("pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia"));
  m.set("pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia", viaCommand("pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia"));

  // ------------------------------------------------------------------------------ layer commands while routing
  // While the router runs, the layer actions are its own (`ROUTER_TOOL::setTransitions`: `onLayerCommand` for layerTop .. layerBottom, layerNext,
  // layerPrev), `handleLayerSwitch( aEvent, false )`: the head moves to the layer when nothing holds it to its own (`ROUTER::SwitchLayer`), else a
  // through via to the layer is armed, sized as every via command sizes one (R6-R3). Without a route drawn they are the board editor's (active layer).
  if (board && draw?.kind === "route") {
    const layerCommand = (cmd: string) =>
      pcbOnly(() => {
        const target = routerLayerTarget(cmd, draw.layer, board.layers, (l) => state.layerVisible[l] !== false);
        if (!target) return;
        const cursor = state.cursorNm ?? { x: draw.pts[draw.pts.length - 1]?.[0] ?? 0, y: draw.pts[draw.pts.length - 1]?.[1] ?? 0 };
        const refresh = (next: typeof draw) => void routeMove(cursor.x, cursor.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(next, preview) }));
        void routeSwitchLayer(target, cursor).then((r) => {
          if (!r.ok) return;
          // `updateEndItem( aEvent ); updateSizesAfterRouterEvent( targetLayer, m_endSnapPoint ); m_router->Move( m_endSnapPoint, m_endItem ); // refresh`
          if (r.switched) return refresh(withEventSizes(draw, r.sizes));
          // `if( !m_router->IsPlacingVia() ) m_router->ToggleViaPlacement();` with the target in the sizes' layer pair
          const size = chosenViaOf("through", viaChoiceOf(state));
          void routeToggleVia(true, size?.diameter, size?.drill, target).then((armed) => {
            if (!armed.ok) return;
            // `sizes.ClearLayerPairs(); .. sizes.AddLayerPair( currentLayer, targetLayer )`
            const next = { ...draw, placingVia: true, pendingViaLayer: target, sizesPairs: layerPairMap(draw.layer, target) };
            dispatch({ type: "SET_DRAW_STATE", draw: next });
            refresh(next);
          });
        });
      });
    m.set("pcbnew.Control.layerNext", layerCommand("next"));
    m.set("pcbnew.Control.layerPrev", layerCommand("prev"));
    m.set("pcbnew.Control.layerTop", layerCommand("F.Cu"));
    m.set("pcbnew.Control.layerBottom", layerCommand("B.Cu"));
    for (let i = 1; i <= 30; i++) m.set(`pcbnew.Control.layerInner${i}`, layerCommand(`In${i}.Cu`));
  }
  // The same while a pair is drawn (R7-R3): `onLayerCommand` is the router tool's in both modes, and `ROUTER::SwitchLayer` is the pair placer's
  // `DIFF_PAIR_PLACER::SetLayer` (crates/pns/src/dp_placer.rs `set_layer`: a pair going on from two vias on the layer moves to it); refused, a pair
  // of through vias to the layer is armed, sized as V sizes them (R7-R2).
  if (board && draw?.kind === "diffpair") {
    const layerCommand = (cmd: string) =>
      pcbOnly(() => {
        const target = routerLayerTarget(cmd, draw.layer, board.layers, (l) => state.layerVisible[l] !== false);
        if (!target) return;
        const cursor = state.cursorNm ?? { x: draw.ptsA[draw.ptsA.length - 1]?.[0] ?? 0, y: draw.ptsA[draw.ptsA.length - 1]?.[1] ?? 0 };
        const refresh = (next: typeof draw) => void dpMove(cursor.x, cursor.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(next, preview) }));
        void routeSwitchLayer(target, cursor).then((r) => {
          if (!r.ok) return;
          if (r.switched) return refresh(withEventSizes({ ...draw, layer: target }, r.sizes));
          const size = chosenViaOf("through", viaChoiceOf(state));
          void dpToggleVia(true, target, size?.diameter, size?.drill).then((armed) => {
            if (!armed.ok) return;
            // `sizes.ClearLayerPairs(); .. sizes.AddLayerPair( currentLayer, targetLayer )`
            const next = { ...draw, placingVia: true, pendingViaLayer: target, sizesPairs: layerPairMap(draw.layer, target) };
            dispatch({ type: "SET_DRAW_STATE", draw: next });
            refresh(next);
          });
        });
      });
    m.set("pcbnew.Control.layerNext", layerCommand("next"));
    m.set("pcbnew.Control.layerPrev", layerCommand("prev"));
    m.set("pcbnew.Control.layerTop", layerCommand("F.Cu"));
    m.set("pcbnew.Control.layerBottom", layerCommand("B.Cu"));
    for (let i = 1; i <= 30; i++) m.set(`pcbnew.Control.layerInner${i}`, layerCommand(`In${i}.Cu`));
  }

  // ------------------------------------------------------------------------------ V with the route tool armed
  // `ROUTER_TOOL::MainLoop`: "else if( evt->IsAction( &ACT_PlaceThroughVia ) ) { m_startWithVia = true; m_toolMgr->RunAction( PCB_ACTIONS::layerToggle ); }"
  // -- the active layer flips to the other layer of the pair (`PCB_CONTROL::LayerToggle`) and the next route starts with a via armed (`performRouting`:
  // `if( m_startWithVia ) { m_startWithVia = false; handleLayerSwitch( ACT_PlaceThroughVia.MakeEvent(), true ); }`, components/canvas/routing.ts), R6-R5.
  // `MainLoop` is the loop of both modes (`Go( &ROUTER_TOOL::MainLoop, PCB_ACTIONS::routeDiffPair.MakeEvent() )`), so V with the pair tool armed does
  // the same, and the pair starts with its vias armed (components/canvas/diffPairRouting.ts `startDiffPairRoute`), R7-R3.
  if (board && !draw && (state.activeTool === "route" || state.activeTool === "diffpair")) {
    const startWithVia = pcbOnly(() => {
      const pair = layerPairsOf(board, state.pcbx.layerPairs).current;
      dispatch({ type: "SET_ACTIVE_LAYER", layer: otherLayerOfPair(pair, state.activeLayer ?? "") });
      dispatch({ type: "PCBX", patch: { startWithVia: true } });
    });
    m.set("pcbnew.Control.layerToggle", startWithVia);
    m.set("pcbnew.InteractiveRouter.PlaceVia", startWithVia);
  }

  // ------------------------------------------------------------------------------ Switch Track Posture
  // performRouting: `else if( evt->IsAction( &ACT_SwitchPosture ) ) { m_router->FlipPosture(); updateEndItem( *evt ); m_router->Move( m_endSnapPoint, m_endItem ); }`
  // -- while a route is being drawn; the pair's spine flips the same way (`DIFF_PAIR_PLACER::FlipPosture`).
  m.set(
    "pcbnew.InteractiveRouter.SwitchPosture",
    pcbOnly(() => {
      const cur = state.cursorNm;
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
            // `m_TempOverrideTrackWidth = true; bds.UseCustomTrackViaSize( true );` -- "Use Starting Track Width" is left as it is (r5 R5-7)
            dispatch({ type: "PCBX", patch: { customTrackVia: v, useCustomTrackVia: true } });
            dispatch({ type: "SET_CURRENT_TRACK_WIDTH", widthNm: v.trackNm });
            dispatch({ type: "SET_CURRENT_VIA_PRESET", preset: { diameter: v.viaDiameterNm, drill: v.viaDrillNm } });
            const live = ctx.api.getState().drawState;
            const at = ctx.api.getState().cursorNm;
            if (live?.kind === "route" && at) {
              const next = { ...live, width: v.trackNm, widthOverride: true, sizes: live.sizes ? { ...live.sizes, width_source: "user choice" } : undefined };
              dispatch({ type: "SET_DRAW_STATE", draw: next });
              void routeMove(at.x, at.y, undefined, v.trackNm).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(next, preview) }));
            }
          },
        }),
      });
    })
  );
}
