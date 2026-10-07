// The router rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md) that need more than a
// registry entry: Cancel Current Item, Set Layer Pair... and Differential Pair Dimensions...
// Each cites the KiCad function it ports (pcbnew/router/router_tool.cpp, pcbnew/sel_layer.cpp,
// dialogs/dialog_pns_diff_pair_dimensions.cpp at 8303b2ad). Called from `registerPcbEditSweep`.

import { createElement } from "react";
import type { BoardState } from "../api/types";
import { dpMove, dpSetDims } from "../api/client";
import { dpStateFromPreview } from "../kicad-port/dpTool";
import { cycleLayerPairPreset, defaultLayerPairSettings, layerPairName, sanitizeLayerPair, type LayerPairSettings } from "../kicad-port/layerPairs";
import type { CustomDiffPair } from "../kicad-port/pcbParityState";
import { skipCurrentRoute } from "../components/canvas/routeQueue";
import { DiffPairDimensionsDialog, LayerPairDialog } from "../components/PcbRouterDialogs";
import { openSweepDialog } from "./pcbSweepDialogs";
import type { SweepCtx } from "./pcbSweepKit";

/** `m_pairs` of the board as the studio knows them: the stored settings (or the default outer pair), never naming a layer the board lacks. */
export function layerPairsOf(board: BoardState | null, stored: LayerPairSettings | null): LayerPairSettings {
  const copper = board?.layers ?? [];
  return sanitizeLayerPair(stored ?? defaultLayerPairSettings(copper), copper);
}

/**
 * `SIZES_SETTINGS` defaults for a pair: the net class's `diff_pair_width` / `diff_pair_gap` (the model's own fallbacks are
 * 125 / 180 um, upstream's `m_diffPairWidth` / `m_diffPairGap`), the via gap following the track gap.
 */
export function netClassDiffPair(board: BoardState | null): CustomDiffPair {
  const classes = board?.board_rules?.net_classes ?? [];
  const cls = classes.find((c) => c.name === "Default") ?? classes.find((c) => c.nets.includes("*"));
  const gapUm = cls?.diff_pair_gap ?? 180;
  return { widthUm: cls?.diff_pair_width ?? 125, gapUm, viaGapUm: cls?.diff_pair_via_gap ?? gapUm, viaGapSameAsTrackGap: cls?.diff_pair_via_gap == null };
}

export function registerPcbRouterSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  const pcbOnly =
    (fn: () => void) =>
    () => {
      if (state.tab === "pcb") fn();
    };

  // ------------------------------------------------------------ Cancel Current Item
  // `cancelCurrentItem` in the router's loops (router_tool.cpp performRouting / InlineDrag): ends the route being drawn but not the
  // tool. Inside RouteSelected (`m_inRouteSelected`, the context menu's only case for it) the loop goes on with the next connection.
  const live = state.drawState?.kind;
  if (live === "route" || live === "diffpair" || live === "drag") {
    m.set(
      "pcbnew.InteractiveRouter.CancelCurrentItem",
      pcbOnly(() => {
        void skipCurrentRoute(dispatch);
      })
    );
  }

  // -------------------------------------------------------------- Set Layer Pair...
  // ROUTER_TOOL::SelectCopperLayerPair: SELECT_COPPER_LAYERS_PAIR_DIALOG on the board's layer-pair settings; the same-layer pair is
  // allowed but warned about.
  m.set(
    "pcbnew.InteractiveRouter.SelectLayerPair",
    pcbOnly(() => {
      if (!board) return;
      const settings = layerPairsOf(board, state.pcbx.layerPairs);
      openSweepDialog({
        kind: "element",
        element: createElement(LayerPairDialog, {
          settings,
          copper: board.layers,
          onOk: (next: LayerPairSettings) => {
            dispatch({ type: "PCBX", patch: { layerPairs: next } });
            if (next.current.a === next.current.b) toast("Warning: top and bottom layers are same.");
          },
        }),
      });
    })
  );

  // ------------------------------------------------ Differential Pair Dimensions...
  // ROUTER_TOOL::DpDimensionsDialog: DIALOG_PNS_DIFF_PAIR_DIMENSIONS on the router's sizes; accepted values become the board's custom
  // pair dimensions (`SetCustomDiffPairWidth/Gap/ViaGap`) and apply to the pair being routed right now.
  m.set(
    "pcbnew.InteractiveRouter.DiffPairDialog",
    pcbOnly(() => {
      const draw = state.drawState;
      const base = state.pcbx.customDiffPair ?? netClassDiffPair(board);
      const value: CustomDiffPair = draw?.kind === "diffpair" && !state.pcbx.customDiffPair ? { ...base, widthUm: draw.width } : base;
      openSweepDialog({
        kind: "element",
        element: createElement(DiffPairDimensionsDialog, {
          value,
          onOk: (v: CustomDiffPair) => {
            dispatch({ type: "PCBX", patch: { customDiffPair: v } });
            const cur = state.cursorUm;
            if (draw?.kind === "diffpair") {
              void dpSetDims(v.widthUm, v.gapUm).then(() => cur && dpMove(cur.x, cur.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) })));
            }
          },
        }),
      });
    })
  );

  // The same pair settings drive Shift+V (pcb_control.cpp PCB_CONTROL::CycleLayerPresets): the next enabled pair becomes current.
  if (board) {
    m.set(
      "pcbnew.Control.layerPairPresetCycle",
      pcbOnly(() => {
        const next = cycleLayerPairPreset(layerPairsOf(board, state.pcbx.layerPairs));
        if (!next) return; // "if( presets.size() < 2 ) return 0;"
        dispatch({ type: "PCBX", patch: { layerPairs: next } });
        toast(`Layer pair: ${layerPairName(next.current)}`);
      })
    );
  }
}
