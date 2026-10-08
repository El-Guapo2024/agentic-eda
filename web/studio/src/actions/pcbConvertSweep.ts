// The Convert tool's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Create Polygon / Zone /
// Rule Area / Lines / Tracks / Arc / Outsets from Selection. Each cites CONVERT_TOOL
// (pcbnew/tools/convert_tool.cpp at 8303b2ad); the polygon building is crates/ops/src/convert.rs
// (`POST /api/convert/polys`), everything else is kicad-port/pcbConvert.ts. Called from `registerPcbEditSweep`.

import { createElement } from "react";
import type { BoardState, Cmd } from "../api/types";
import { convertPolys } from "../api/client";
import {
  DEFAULT_CONVERT_SETTINGS,
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
const last: { convert: ConvertSettings; outset: OutsetParams; tracks: { layer: string; net: string; deleteOriginals: boolean } } = {
  convert: { ...DEFAULT_CONVERT_SETTINGS },
  outset: { ...DEFAULT_OUTSET_PARAMS },
  tracks: { layer: "", net: "", deleteOriginals: true },
};

/** `bds.m_LineThickness[ bds.GetLayerClass( layer ) ]` with KiCad's defaults (board_design_settings.h): copper 0.2 mm, silkscreen / fab / other 0.1 mm, edges and courtyard 0.05 mm. */
export function layerLineWidthUm(layer: string): number {
  if (layer.endsWith(".Cu")) return 200;
  if (layer === "Edge.Cuts" || layer.endsWith(".CrtYd")) return 50;
  return 100;
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
  const destLayer = (): string | null => state.activeLayer ?? board?.layers[0] ?? null;

  // ------------------------------------------------- Create Polygon / Zone / Rule Area
  // CONVERT_TOOL::CreatePolys.
  const createPolys = (kind: "poly" | "zone" | "keepout") =>
    pcbOnly(() => {
      if (!board) return;
      const ids = ctx.requestSelection();
      if (ids.length === 0 || !convertAvailability(board, ids).poly) return;
      const layer = destLayer();
      if (!layer) return;
      if (kind !== "poly" && !board.layers.includes(layer)) {
        toast("Zones and rule areas are made on copper layers: choose a copper layer first.", "error");
        return;
      }
      void (async () => {
        // "Pre-flight getPolys() to see if there's anything to convert."
        const pre = await convertPolys(ids, "bounding_hull", 0);
        if (!pre.ok || pre.rings.length === 0) return;
        // "No copy-line-width option for zones/keepouts".
        const start: ConvertSettings = kind !== "poly" && last.convert.strategy === "copy_linewidth" ? { ...last.convert, strategy: "centerline" } : { ...last.convert };
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
                const resolved = kind === "poly" ? resolveConvertSettings(chosen, layerLineWidthUm(layer)) : chosen;
                const reply = await convertPolys(ids, resolved.strategy, resolved.gapUm);
                if (!reply.ok || reply.rings.length === 0) {
                  if (kind === "poly") toast(`Could not convert selection: ${resolved.strategy === "bounding_hull" ? "Resulting polygon would be empty" : "Objects must form a closed shape"}`, "error");
                  return;
                }
                const sources = chosen.deleteOriginals ? reply.consumed : [];
                const deleteCmds = sources.map((id) => deleteCmd(board, id));
                if (kind === "poly") {
                  // COPY_LINEWIDTH takes the stroke of the top-left source item.
                  const width = resolved.strategy === "copy_linewidth" ? (copiedLineWidth(board, ids) ?? resolved.widthUm) : resolved.widthUm;
                  const adds: Cmd[] = reply.rings.map((ring) => ({ op: "add_shape", shape: { kind: "polygon", layer, stroke_width: width, filled: resolved.strategy === "centerline", pts: ring.map(([x, y]) => ({ x, y })) } }));
                  await applyEdit([...adds, ...deleteCmds], sources);
                } else {
                  // The zone dialog takes the settings; it adds the other outlines with them and removes the sources.
                  const [first, ...rest] = reply.rings;
                  dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: kind === "keepout" });
                  dispatch({ type: "PCBX", patch: { zoneConvert: { extraOutlines: rest, deleteCmds } } });
                  dispatch({ type: "SET_ZONE_PENDING", outline: first ?? null });
                }
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
            const plan = planConvertToLines(board, ids, layer, state.pcbx.drawStrokeWidthUm, chosen.deleteOriginals);
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
            const width = state.currentTrackWidthUm ?? board.board_rules?.track_width ?? 250;
            const plan = planConvertToTracks(board, ids, v.layer, v.net, width, v.deleteOriginals);
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
      const start: OutsetParams = { ...last.outset, layer: layers.includes(last.outset.layer) ? last.outset.layer : (layers[0] ?? "Edge.Cuts") };
      openSweepDialog({
        kind: "element",
        element: createElement(OutsetItemsDialog, {
          params: start,
          layers,
          layerDefaultWidth: layerLineWidthUm,
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
