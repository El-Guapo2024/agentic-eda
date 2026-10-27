// The Convert tool's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Create Polygon / Zone /
// Rule Area / Lines / Tracks / Arc / Outsets from Selection. Each cites CONVERT_TOOL
// (pcbnew/tools/convert_tool.cpp at 8303b2ad); the polygon building is crates/ops/src/convert.rs
// (`POST /api/convert/polys`), everything else is kicad-port/pcbConvert.ts. Called from `registerPcbEditSweep`.

import { createElement } from "react";
import type { TextGraphicsDefaults } from "../api/types";
import { lineThicknessNm } from "../kicad-port/layerDefaults";
import { activeLayerName } from "../kicad-port/layerPresets";
import type { BoardState, Cmd } from "../api/types";
import { convertPolys } from "../api/client";
import {
  DEFAULT_CONVERT_SETTINGS,
  DEFAULT_CONVERTED_LINE_WIDTH_NM,
  DEFAULT_CONVERTED_TRACK_WIDTH_NM,
  DEFAULT_OUTSET_PARAMS,
  convertAvailability,
  copiedLineWidth,
  deleteCmd,
  outsetShapes,
  planConvertToLines,
  planConvertToTracks,
  planSegmentToArc,
  resolveConvertSettings,
  type ConvertSettings,
  type OutsetParams,
} from "../kicad-port/pcbConvert";
import { ConversionSettingsDialog, CreateTracksDialog, OutsetItemsDialog } from "../components/PcbConvertDialogs";
import { STANDARD_LAYERS } from "../components/canvas/layers";
import { openSweepDialog } from "./pcbSweepDialogs";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** Remembered between invocations, like the C++ `m_userSettings` and `static` outset parameters. */
const last: { convert: ConvertSettings; outset: OutsetParams | null; tracks: { layer: string; net: string; deleteOriginals: boolean } } = {
  convert: { ...DEFAULT_CONVERT_SETTINGS },
  // `static outset_params_pcb_edit`, made on the first use with the board's Edge.Cuts line thickness (`GetLineThickness( Edge_Cuts )`).
  outset: null,
  tracks: { layer: "", net: "", deleteOriginals: true },
};

/** What the zone dialog's "Conversion Settings" box chose, kept for the next conversion as `m_userSettings` is (Create Zone / Rule Area from Selection). */
export function rememberZoneConversion(chosen: Pick<ConvertSettings, "strategy" | "gapNm" | "deleteOriginals">): void {
  last.convert = { ...last.convert, ...chosen };
}

/** `bds.m_LineThickness[ bds.GetLayerClass( layer ) ]`: the board's Text & Graphics default for the layer's class, KiCad's own table (copper 0.2 mm, silkscreen / fab / other 0.1 mm, edges and courtyard 0.05 mm) when the backend sends none. */
export function layerLineWidthNm(layer: string, defaults?: TextGraphicsDefaults): number {
  return lineThicknessNm(defaults, layer);
}

/** Every net a pad of the board carries (the nets a new track can belong to). */
function boardNets(board: BoardState): string[] {
  const set = new Set<string>();
  for (const p of board.parts) for (const pad of p.pads ?? []) if (pad.net) set.add(pad.net);
  return [...set].sort();
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
              last.convert = chosen;
              void (async () => {
                const resolved = kind === "poly" ? resolveConvertSettings(chosen, layerLineWidthNm(layer, board?.board_rules?.text_graphics)) : chosen;
                const reply = await convertPolys(ids, resolved.strategy, resolved.gapNm);
                if (!reply.ok || reply.rings.length === 0) {
                  if (kind === "poly") toast(`Could not convert selection: ${resolved.strategy === "bounding_hull" ? "Resulting polygon would be empty" : "Objects must form a closed shape"}`, "error");
                  return;
                }
                const sources = chosen.deleteOriginals ? reply.consumed : [];
                const deleteCmds = sources.map((id) => deleteCmd(board, id));
                // COPY_LINEWIDTH takes the stroke of the top-left source item.
                const width = resolved.strategy === "copy_linewidth" ? (copiedLineWidth(board, ids) ?? resolved.widthNm) : resolved.widthNm;
                const adds: Cmd[] = reply.rings.map((ring) => ({ op: "add_shape", shape: { kind: "polygon", layer, stroke_width: width, filled: resolved.strategy === "centerline", pts: ring.map(([x, y]) => ({ x, y })) } }));
                await applyEdit([...adds, ...deleteCmds], sources);
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
      const ids = requestFiltered((id) => {
        const s = api.shapeById(id);
        return s ? s.kind === "segment" || s.kind === "arc" || s.kind === "polygon" || s.kind === "rect" : !!api.zoneById(id);
      });
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
            void applyEdit(plan.cmds, plan.removed);
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
      });
      if (ids.length === 0 || !convertAvailability(board, ids).tracks) return;
      const nets = boardNets(board);
      if (nets.length === 0) {
        toast("There is no net to give the new tracks.", "error");
        return;
      }
      const active = destLayer();
      const layer = active && board.layers.includes(active) ? active : (last.tracks.layer && board.layers.includes(last.tracks.layer) ? last.tracks.layer : (board.layers[0] ?? "F.Cu"));
      openSweepDialog({
        kind: "element",
        element: createElement(CreateTracksDialog, {
          nets,
          copperLayers: board.layers,
          layer,
          net: nets.includes(last.tracks.net) ? last.tracks.net : (nets[0] ?? ""),
          deleteOriginals: last.tracks.deleteOriginals,
          onOk: (v: { layer: string; net: string; deleteOriginals: boolean }) => {
            last.tracks = v;
            const plan = planConvertToTracks(board, ids, v.layer, v.net, DEFAULT_CONVERTED_TRACK_WIDTH_NM, v.deleteOriginals);
            if (plan.empty) return;
            void applyEdit(plan.cmds, plan.removed);
          },
        }),
      });
    })
  );

  // ------------------------------------------------------------------ Create Arc
  // CONVERT_TOOL::SegmentToArc: the first selected line or track becomes an arc (the source stays).
  m.set(
    "pcbnew.Convert.convertToArc",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => !!api.shapeById(id) || !!board.routing?.tracks.some((t) => t.id === id));
      const first = ids[0];
      if (!first) return;
      const cmds = planSegmentToArc(board, first);
      if (!cmds) {
        toast("Create Arc needs a single straight line or track (or an arc track).");
        return;
      }
      void applyEdit(cmds);
    })
  );

  // --------------------------------------------------------------------- Outsets
  // CONVERT_TOOL::OutsetItems: DIALOG_OUTSET_ITEMS, then OUTSET_ROUTINE over the selected graphics; the new items are selected.
  m.set(
    "pcbnew.Convert.outsetItems",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => !!api.shapeById(id));
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
            const shapes = ids.map((id) => api.shapeById(id)).filter((s): s is NonNullable<typeof s> => !!s);
            const res = outsetShapes(shapes, params);
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
