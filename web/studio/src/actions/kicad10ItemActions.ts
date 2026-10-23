// The board editor's actions for KiCad 10's via stitching (KiCad 25ec2335; the 8303b2ad source the rest of the port reads predates it). Called from `registerPcbItemActions`.
//
//   pcbnew.InteractiveDrawing.drawViaStitchArea   DRAWING_TOOL::DrawZone with ZONE_MODE::STITCH: the outline tool, then Via Stitching Properties on the new stitching (Canvas.tsx)
//   pcbnew.Generator.excludeStitchVia             VIA_STITCH_TOOL::ExcludeStitchVia: each selected via a stitching made is taken out of it (`ExcludePosition`) and the stitchings
//                                                 made again -- one undo step, "Exclude Via From Stitching"; the selection is cleared
//   pcbnew.Generator.clearStitchViaExclusions     VIA_STITCH_TOOL::ClearStitchViaExclusions: every selected stitching that has exclusions loses them and is made again ("Clear Via
//                                                 Stitching Exclusions")
//   pcbnew.InteractiveDrawing.placeDrillChart     PCB_CONTROL::PlaceDrillChart (`placeGeneratedTable`): a chart of the default template on the active layer, or the first documentation
//                                                 layer that is enabled, built from the board and carried by the cursor until a click drops it ("Place Drill Chart")
//   pcbnew.InteractiveDrawing.placeDrillMap       PCB_CONTROL::PlaceDrillMap: a map on the active layer, or the first documentation layer with none, at zero offset; it is then
//                                                 selected ("Place Drill Map")
import { hasExclusions, stitchesExcluding, viaStitchCmd, withoutExclusions } from "../kicad-port/viaStitch";
import { pickDocumentationLayer } from "../kicad-port/drillChart";
import { activeLayerName, layerIsVisible } from "../kicad-port/layerPresets";
import { pcbContext } from "../kicad-port/pcbProperties";
import { fetchState } from "../api/client";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

export function registerKicad10ItemActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);
  const { api, dispatch } = ctx;
  // `pickDocumentationLayer`: the layer, made the active one when it was not, with the warning a hidden one gets; null (and the info bar's error) when no layer will do.
  const docLayer = (available: (layer: string) => boolean, noLayer: string): string | null => {
    const st = api.getState();
    if (!st.board) return null;
    const active = st.activeLayer ? activeLayerName(st.activeLayer, "") : null;
    const layer = pickDocumentationLayer(active, pcbContext(st.board, st.units).allLayers, available);
    if (!layer) {
      dispatch({ type: "TOAST", message: noLayer, kind: "error" });
      return null;
    }
    // "Placing onto a hidden layer draws nothing at all, which is indistinguishable from the feature being broken"
    if (!layerIsVisible(st.layerVisible, layer)) dispatch({ type: "TOAST", message: `Layer '${layer}' is hidden; nothing will be shown until you make it visible.`, kind: "info" });
    return layer;
  };
  m.set(
    "pcbnew.InteractiveDrawing.placeDrillChart",
    pcbOnly(() => {
      const layer = docLayer(() => true, "No documentation layer is enabled to hold a drill chart.");
      if (layer) void api.placeNewItems((at) => ({ op: "pcb_item", verb: "place_drill_chart", layer, at }), true);
    })
  );
  m.set(
    "pcbnew.InteractiveDrawing.placeDrillMap",
    pcbOnly(async () => {
      // "Two maps on one layer would overdraw each other's marks, so an occupied layer is no more available than one that cannot hold a map at all"
      const maps = api.getState().board?.drawings?.drill_maps ?? [];
      const layer = docLayer((l) => !maps.some((x) => x.layer === l), "Every documentation layer already has a drill map.");
      if (!layer) return;
      const before = new Set(maps.map((x) => x.id));
      if (!(await api.cmdBatch([{ op: "pcb_item", verb: "place_drill_map", layer }]))) return;
      const fresh = await fetchState().catch(() => null);
      const made = (fresh?.drawings?.drill_maps ?? []).find((x) => !before.has(x.id));
      dispatch({ type: "SET_SELECTION", refs: made ? [made.id] : [] });
    })
  );
  m.set(
    "pcbnew.InteractiveDrawing.drawViaStitchArea",
    pcbOnly(() => {
      // The zone tool's outline (`activeTool === "zone"`), finished into a via stitching (`pcbx.zoneDrawMode`).
      dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: false });
      dispatch({ type: "PCBX", patch: { zoneDrawMode: { mode: "stitch", sourceId: "" } } });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "zone" });
    })
  );
  m.set(
    "pcbnew.Generator.excludeStitchVia",
    pcbOnly(() => {
      const board = api.getState().board;
      const sel = [...api.getState().selection];
      const vias = (board?.routing?.vias ?? []).filter((v) => sel.includes(v.id));
      const changed = stitchesExcluding(board?.drawings?.via_stitches ?? [], vias);
      if (changed.length === 0) return;
      dispatch({ type: "SET_SELECTION", refs: [] });
      void api.cmdBatch(changed.map((s) => viaStitchCmd(s.id, s)));
    })
  );
  m.set(
    "pcbnew.Generator.clearStitchViaExclusions",
    pcbOnly(() => {
      const board = api.getState().board;
      const sel = new Set(api.getState().selection);
      const changed = (board?.drawings?.via_stitches ?? []).filter((s) => sel.has(s.id) && hasExclusions(s)).map(withoutExclusions);
      if (changed.length === 0) return;
      void api.cmdBatch(changed.map((s) => viaStitchCmd(s.id, s)));
    })
  );
}
