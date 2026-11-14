// The Convert tool's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Create Polygon / Zone /
// Rule Area / Lines / Tracks / Arc / Outsets from Selection. Each cites CONVERT_TOOL
// (pcbnew/tools/convert_tool.cpp at 8303b2ad); the polygon building is crates/ops/src/convert.rs
// (`POST /api/convert/polys`), everything else is kicad-port/pcbConvert.ts. Called from `registerPcbEditSweep`.

import { createElement } from "react";
import type { TextGraphicsDefaults } from "../api/types";
import { lineThicknessNm } from "../kicad-port/layerDefaults";
import { activeLayerName } from "../kicad-port/layerPresets";
import type { Cmd } from "../api/types";
import { convertPolys } from "../api/client";
import {
  DEFAULT_CONVERT_SETTINGS,
  DEFAULT_CONVERTED_LINE_WIDTH_NM,
  DEFAULT_CONVERTED_TRACK_WIDTH_NM,
  DEFAULT_OUTSET_PARAMS,
  convertAvailability,
  copiedLineWidth,
  deleteCmd,
  outsetItems,
  outsetPadOf,
  planConvertToLines,
  planConvertToTracks,
  planSegmentToArc,
  resolveConvertSettings,
  type ConvertSettings,
  type OutsetParams,
  type OutsetSource,
} from "../kicad-port/pcbConvert";
import { padById } from "../kicad-port/pcbItems";
import { lockedForTools } from "../kicad-port/pcbTransform";
import { ConversionSettingsDialog, OutsetItemsDialog } from "../components/PcbConvertDialogs";
import { SelectLayerDialog } from "../components/PcbRouterDialogs";
import { STANDARD_LAYERS } from "../components/canvas/layers";
import { openSweepDialog } from "./pcbSweepDialogs";
import { showError } from "../components/library/libraryDialogs";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** Remembered between invocations, like the C++ `m_userSettings` and `static` outset parameters. */
const last: { convert: ConvertSettings; outset: OutsetParams | null } = {
  convert: { ...DEFAULT_CONVERT_SETTINGS },
  // `static outset_params_pcb_edit`, made on the first use with the board's Edge.Cuts line thickness (`GetLineThickness( Edge_Cuts )`).
  outset: null,
};

/** What the zone dialog's "Conversion Settings" box chose, kept for the next conversion as `m_userSettings` is (Create Zone / Rule Area from Selection). */
export function rememberZoneConversion(chosen: Pick<ConvertSettings, "strategy" | "gapNm" | "deleteOriginals">): void {
  last.convert = { ...last.convert, ...chosen };
}

/** `bds.m_LineThickness[ bds.GetLayerClass( layer ) ]`: the board's Text & Graphics default for the layer's class, KiCad's own table (copper 0.2 mm, silkscreen / fab / other 0.1 mm, edges and courtyard 0.05 mm) when the backend sends none. */
export function layerLineWidthNm(layer: string, defaults?: TextGraphicsDefaults): number {
  return lineThicknessNm(defaults, layer);
}

export function registerPcbConvertSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch, api } = ctx;
  const board = state.board;
  const { toast, pcbOnly, requestFiltered, applyEdit } = createSweepHelpers(ctx);
  /** `PCB_LAYER_ID destLayer = m_frame->GetActiveLayer()`. */
  const destLayer = (): string | null => (state.activeLayer ? activeLayerName(state.activeLayer, "") : (board?.layers[0] ?? null));

  // ------------------------------------------------- Create Polygon / Zone / Rule Area
  // CONVERT_TOOL::CreatePolys.
  const createPolys = (kind: "poly" | "zone" | "keepout") =>
    pcbOnly(() => {
      if (!board) return;
      const ids = ctx.requestSelection();
      if (ids.length === 0 || !convertAvailability(board, ids).poly) return;
      const layer = destLayer();
      if (!layer) return;
      // A zone on a technical layer is a non-copper zone (`CreatePolys`: `nonCopper = IsNonCopperLayer( destLayer )` opens `InvokeNonCopperZonesEditor`;
      // ZoneDialog picks the dialog by the layer; round 5 B5-9).
      void (async () => {
        // "Pre-flight getPolys() to see if there's anything to convert."
        const pre = await convertPolys(ids, "bounding_hull", 0);
        if (!pre.ok || pre.rings.length === 0) return;
        // "No copy-line-width option for zones/keepouts".
        const start: ConvertSettings = kind !== "poly" && last.convert.strategy === "copy_linewidth" ? { ...last.convert, strategy: "centerline" } : { ...last.convert };
        if (kind !== "poly") {
          // One dialog: the zone editor with `&m_userSettings` (`InvokeCopperZonesEditor( frame, nullptr, &zoneInfo, &m_userSettings )`, the non-copper and
          // rule-area editors alike), which shows a "Conversion Settings" box; `getPolys( m_userSettings )` runs after its OK (round 6 D6-2). The pre-flight's
          // first outline stands for the zone until then.
          const [first] = pre.rings;
          dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: kind === "keepout" });
          dispatch({ type: "PCBX", patch: { zoneConvert: { ids, strategy: start.strategy === "bounding_hull" ? "bounding_hull" : "centerline", gapNm: start.gapNm, deleteOriginals: start.deleteOriginals } } });
          dispatch({ type: "SET_ZONE_PENDING", outline: first ?? null });
          return;
        }
        openSweepDialog({
          kind: "element",
          element: createElement(ConversionSettingsDialog, {
            settings: start,
            showCopyLineWidth: kind === "poly",
            showCenterline: true,
            showHull: true,
            onOk: (chosen: ConvertSettings) => {
              // `CONVERT_SETTINGS previousSettings = m_userSettings`: the dialog writes `m_userSettings`, and a failed conversion puts the old ones back.
              const previous = last.convert;
              last.convert = chosen;
              void (async () => {
                const resolved = kind === "poly" ? resolveConvertSettings(chosen, layerLineWidthNm(layer, board?.board_rules?.text_graphics)) : chosen;
                const reply = await convertPolys(ids, resolved.strategy, resolved.gapNm);
                if (!reply.ok || reply.rings.length === 0) {
                  // `DisplayErrorMessage( m_frame, _( "Could not convert selection" ), msg ); m_userSettings = previousSettings;`
                  last.convert = previous;
                  void showError("Could not convert selection", resolved.strategy === "bounding_hull" ? "Resulting polygon would be empty" : "Objects must form a closed shape");
                  return;
                }
                const sources = chosen.deleteOriginals ? reply.consumed : [];
                const deleteCmds = sources.map((id) => deleteCmd(board, id));
                // COPY_LINEWIDTH takes the stroke of the top-left source item.
                const width = resolved.strategy === "copy_linewidth" ? (copiedLineWidth(board, ids) ?? resolved.widthNm) : resolved.widthNm;
                const adds: Cmd[] = reply.rings.map((ring) => ({ op: "add_shape", shape: { kind: "polygon", layer, stroke_width: width, filled: resolved.strategy === "centerline", pts: ring.map(([x, y]) => ({ x, y })) } }));
                // `commit.Add( graphic )` without selecting it; with Delete Originals `m_selectionTool->ClearSelection()` first.
                await applyEdit([...adds, ...deleteCmds], sources, null, { keep: !chosen.deleteOriginals, selectCreated: false });
              })();
            },
          }),
        });
      })();
    });
  m.set("pcbnew.Convert.convertToPoly", createPolys("poly"));
  m.set("pcbnew.Convert.convertToZone", createPolys("zone"));
  m.set("pcbnew.Convert.convertToKeepout", createPolys("keepout"));

  // ---------------------------------------------------------------- Create Lines
  // CONVERT_TOOL::CreateLines (convertToLines): rectangles, polygons and zones as line segments on the active layer.
  m.set(
    "pcbnew.Convert.convertToLines",
    pcbOnly(() => {
      if (!board) return;
      // The client filter keeps locked items (no `FilterCollectorForLockedItems`).
      const ids = requestFiltered((id) => {
        const s = api.shapeById(id);
        return s ? s.kind === "segment" || s.kind === "arc" || s.kind === "polygon" || s.kind === "rect" : !!api.zoneById(id);
      }, true);
      if (ids.length === 0 || !convertAvailability(board, ids).lines) return;
      const layer = destLayer();
      if (!layer) return;
      openSweepDialog({
        kind: "element",
        element: createElement(ConversionSettingsDialog, {
          settings: { ...last.convert },
          showCopyLineWidth: false,
          showCenterline: false,
          showHull: false,
          onOk: (chosen: ConvertSettings) => {
            last.convert = { ...last.convert, deleteOriginals: chosen.deleteOriginals };
            const plan = planConvertToLines(board, ids, layer, DEFAULT_CONVERTED_LINE_WIDTH_NM, chosen.deleteOriginals);
            if (plan.empty) return;
            // `commit.Add` only; Delete Originals clears the selection first.
            void applyEdit(plan.cmds, plan.removed, null, { keep: !chosen.deleteOriginals, selectCreated: false });
          },
        }),
      });
    })
  );

  // --------------------------------------------------------------- Create Tracks
  // CONVERT_TOOL::CreateLines (convertToTracks): graphic segments and arcs, and polygon outlines, as tracks on a copper layer.
  m.set(
    "pcbnew.Convert.convertToTracks",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const s = api.shapeById(id);
        return s ? s.kind === "segment" || s.kind === "arc" || s.kind === "polygon" || s.kind === "rect" : !!api.zoneById(id);
      }, true);
      if (ids.length === 0 || !convertAvailability(board, ids).tracks) return;
      // `m_userSettings.m_DeleteOriginals`, the setting the other conversions' dialog keeps: Create Tracks has no dialog of its own. The tracks have no net
      // (`new PCB_TRACK( parent )`) and the selection is cleared when the sources go (`m_selectionTool->ClearSelection()`); one commit, "Create Lines".
      const make = (layer: string) => {
        const plan = planConvertToTracks(board, ids, layer, DEFAULT_CONVERTED_TRACK_WIDTH_NM, last.convert.deleteOriginals);
        if (plan.empty) return;
        void applyEdit(plan.cmds, plan.removed, null, { keep: !last.convert.deleteOriginals, selectCreated: false });
      };
      const active = destLayer();
      // `targetLayer = m_frame->GetActiveLayer()`; `if( !IsCopperLayer( targetLayer ) ) targetLayer = frame->SelectOneLayer( F_Cu, LSET::AllNonCuMask() )`, a cancel
      // makes nothing (UI-LEFT-3 P3-20)
      if (active && board.layers.includes(active)) {
        make(active);
        return;
      }
      const copper = board.layers;
      openSweepDialog({
        kind: "element",
        element: createElement(SelectLayerDialog, { layers: copper, copper, current: copper[0] ?? "F.Cu", onPick: (layer: string) => make(layer), onCancel: () => {} }),
      });
    })
  );

  // ------------------------------------------------------------------ Create Arc
  // CONVERT_TOOL::SegmentToArc: the first selected line or track becomes an arc (the source stays).
  m.set(
    "pcbnew.Convert.convertToArc",
    pcbOnly(() => {
      if (!board) return;
      // `IsType( { PCB_SHAPE_T, PCB_TRACE_T, PCB_ARC_T } )`, locked items kept.
      const ids = requestFiltered((id) => !!api.shapeById(id) || !!board.routing?.tracks.some((t) => t.id === id), true);
      const first = ids[0];
      if (!first) return;
      // `getStartEndPoints` empty or a shape that is no segment: `return -1`, nothing said.
      const cmds = planSegmentToArc(board, first);
      if (!cmds) return;
      // `commit.Add( arc ); commit.Push( _( "Create Arc" ) )`: the source stays selected, the arc is not.
      void applyEdit(cmds, [], null, { selectCreated: false });
    })
  );

  // --------------------------------------------------------------------- Outsets
  // CONVERT_TOOL::OutsetItems: DIALOG_OUTSET_ITEMS, then OUTSET_ROUTINE over the selected graphics and pads, as one commit ("Outset Items");
  // the originals are deselected and the new items selected, and the routine's status message goes to the infobar.
  m.set(
    "pcbnew.Convert.outsetItems",
    pcbOnly(() => {
      if (!board) return;
      // `IsType( { PCB_PAD_T, PCB_SHAPE_T } )`, then `FilterCollectorForLockedItems` (a pad is locked with its footprint: `PAD::IsLocked`).
      const ids = requestFiltered((id) => {
        if (api.shapeById(id)) return true;
        const hit = padById(board, id);
        return !!hit && !lockedForTools(board, hit.part.ref);
      });
      if (ids.length === 0) return;
      const layers = [...board.layers, ...STANDARD_LAYERS.map((l) => l.label)];
      const saved: OutsetParams = last.outset ?? { ...DEFAULT_OUTSET_PARAMS, lineWidthNm: lineThicknessNm(board.board_rules?.text_graphics, "Edge.Cuts") };
      const start: OutsetParams = { ...saved, layer: layers.includes(saved.layer) ? saved.layer : (layers[0] ?? "Edge.Cuts") };
      openSweepDialog({
        kind: "element",
        element: createElement(OutsetItemsDialog, {
          params: start,
          layers,
          layerDefaultWidth: (layer: string) => layerLineWidthNm(layer, board?.board_rules?.text_graphics),
          onOk: (params: OutsetParams) => {
            last.outset = params;
            // The selection in its order, pads and shapes alike (`for( EDA_ITEM* item : selection ) outset_routine.ProcessItem( *board_item )`).
            const sources: OutsetSource[] = [];
            for (const id of ids) {
              const shape = api.shapeById(id);
              if (shape) {
                sources.push({ kind: "shape", shape });
                continue;
              }
              const hit = padById(board, id);
              const pad = hit ? outsetPadOf(id, hit.pad, hit.part) : null;
              if (pad) sources.push({ kind: "pad", pad });
            }
            const res = outsetItems(sources, params);
            const cmds: Cmd[] = [...res.add.map((shape): Cmd => ({ op: "add_shape", shape })), ...res.remove.map((id): Cmd => ({ op: "delete_shape", id }))];
            if (cmds.length === 0) {
              if (res.message) toast(res.message);
              return;
            }
            void applyEdit(cmds, res.remove, res.message, { keep: false });
          },
        }),
      });
    })
  );
}
