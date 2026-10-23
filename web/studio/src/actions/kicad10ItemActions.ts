// The board editor's actions for KiCad 10's via stitching (KiCad 25ec2335; the 8303b2ad source the rest of the port reads predates it). Called from `registerPcbItemActions`.
//
//   pcbnew.InteractiveDrawing.drawViaStitchArea   DRAWING_TOOL::DrawZone with ZONE_MODE::STITCH: the outline tool, then Via Stitching Properties on the new stitching (Canvas.tsx)
//   pcbnew.Generator.excludeStitchVia             VIA_STITCH_TOOL::ExcludeStitchVia: each selected via a stitching made is taken out of it (`ExcludePosition`) and the stitchings
//                                                 made again -- one undo step, "Exclude Via From Stitching"; the selection is cleared
//   pcbnew.Generator.clearStitchViaExclusions     VIA_STITCH_TOOL::ClearStitchViaExclusions: every selected stitching that has exclusions loses them and is made again ("Clear Via
//                                                 Stitching Exclusions")
import { hasExclusions, stitchesExcluding, viaStitchCmd, withoutExclusions } from "../kicad-port/viaStitch";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

export function registerKicad10ItemActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);
  const { api, dispatch } = ctx;
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
