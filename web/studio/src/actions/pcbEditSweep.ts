// The pcbnew edit-tool rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md):
// router modes, selection by net / filter / unroute, Mirror, Change Track
// Width, the shape-modification routines (Fillet / Chamfer / Dogbone / Extend
// Lines, Simplify / Heal Shapes, polygon booleans), Fillet Tracks, Break
// Track and Close Outline. Each handler cites the KiCad function it ports
// (pcbnew/ at 8303b2ad); the geometry is in kicad-port/pcb*.ts, the dialogs
// in components/PcbSweepDialogs.tsx.
//
// `registerPcbEditSweep` adds the handlers to useActionRunner's registry; it
// is called once per registry build, so `ctx.state` is the render's snapshot
// -- anything read after an `await` goes through `ctx.api.getState()`.

import type { BoardState, Cmd, RouteMode, Shape } from "../api/types";
import { dpMove, routeMove, routeSetMode, routeSetSettings } from "../api/client";
import { nextCornerMode, type CornerMode } from "../kicad-port/routerSettings";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";
import { drawStateFromPreview } from "../kicad-port/routeTool";
import { dpStateFromPreview } from "../kicad-port/dpTool";
import { modificationSnapFor } from "../state/modificationSnap";
import { openSweepDialog } from "./pcbSweepDialogs";
import { registerPcbRouterSweep } from "./pcbRouterSweep";
import { registerRouterViaActions } from "./routerViaActions";
import { registerPcbSelectionSync } from "./pcbSelectionSync";
import { registerPcbConvertSweep } from "./pcbConvertSweep";
import { registerPcbGlobalEditSweep } from "./pcbGlobalEditSweep";
import { registerEmbeddedActions } from "./embeddedActions";
import { registerPcbPointEditSweep } from "./pcbPointEditSweep";
import { registerPcbReferenceSweep } from "./pcbReferenceSweep";
import { registerPcbItemActions } from "./pcbItemActions";
import { registerMicrowaveActions } from "./microwaveActions";
import { registerPcbMultichannelActions } from "./pcbMultichannelActions";
import { registerPcbAssignNetclass } from "./assignNetclass";
import { DEFAULT_FILTER_OPTIONS, filterSelection, netItems, netsOfItems, unrouteSelected, type FilterOptions } from "../kicad-port/pcbSelectionOps";
import { itemKind } from "../kicad-port/pcbItems";
import { isLocked } from "../kicad-port/pcbTransform";
import { mirrorReferencePoint, mirrorableIds, planMirror, type FlipDirection } from "../kicad-port/pcbMirror";
import { modifyResultCmds } from "../kicad-port/shapeFill";
import { healShapes, isLineModifiable, lineSelectionError, modifyLines, simplifyPolygonRing, type LineRoutine, type ModifyResult } from "../kicad-port/pcbModify";
import { boardOutlineRings } from "../kicad-port/pcbOutline";
import { breakTrack, filletTracks, type EditBoard, type EditTrack } from "../kicad-port/pcbTrackEdit";
import { askConfirm } from "../components/library/libraryDialogs";

/** Values the dialogs start from, remembered between invocations like the C++ `static`s. */
const last = {
  filletRadiusNm: 1_000_000,
  chamferSetbackNm: 1_000_000,
  dogbone: { radiusNm: 1_000_000, addSlots: true },
  simplifyToleranceNm: 3_000_000,
  healToleranceNm: 3_000_000,
  filletTracksRadiusNm: 0,
  filterOptions: { ...DEFAULT_FILTER_OPTIONS } as FilterOptions,
};

/** The Track Corner Mode actions' friendly names (`ACT_SwitchCornerMode45` ...: "Track Corner Mode 45", "... Arc 45", "... 90", "... Arc 90"). */
const CORNER_MODE_LABEL: Record<CornerMode, string> = { mitered_45: "45", rounded_45: "Arc 45", mitered_90: "90", rounded_90: "Arc 90" };
const ROUTER_MODE_LABEL: Record<RouteMode, string> = { mark_obstacles: "Highlight collisions", shove: "Shove", walkaround: "Walk around" };

/** `ROUTER_TOOL::CycleRouterMode`: MarkObstacles -> Shove -> Walkaround -> MarkObstacles. */
export function nextRouterMode(mode: RouteMode): RouteMode {
  return mode === "mark_obstacles" ? "shove" : mode === "shove" ? "walkaround" : "mark_obstacles";
}

/** The `EditBoard` the track edits (Break Track, Fillet Tracks) read their connectivity from. */
export function editBoardOf(board: BoardState): EditBoard {
  const pads: EditBoard["pads"][number][] = [];
  for (const p of board.parts) {
    if (!p.placed) continue;
    const layers = board.layers;
    for (const pad of p.pads ?? []) pads.push({ net: pad.net, x: pad.x, y: pad.y, layers: pad.th ? layers : [p.side === "bottom" ? (layers[layers.length - 1] ?? "B.Cu") : (layers[0] ?? "F.Cu")] });
  }
  return {
    tracks: (board.routing?.tracks ?? []).map((t) => ({ id: t.id, net: t.net, layer: t.layer, width: t.width, pts: t.pts, arc_mid: t.arc_mid })),
    vias: (board.routing?.vias ?? []).map((v) => ({ net: v.net, x: v.x, y: v.y, from: v.from, to: v.to })),
    pads,
    layers: board.layers,
  };
}

export type { SweepCtx };

export function registerPcbEditSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch, api } = ctx;
  const { toast, pcbOnly, lockedIds, requestFiltered, applyEdit } = createSweepHelpers(ctx);
  const board = state.board;

  // --------------------------------------------------------------- router modes
  // router_tool.cpp ChangeRouterMode / CycleRouterMode: `settings.SetMode( mode )`. The studio keeps the mode in
  // `state.routerSettings` (read by the next route/drag session) and pushes it to a running session.
  const setRouterMode = (mode: RouteMode) => {
    dispatch({ type: "SET_ROUTER_SETTINGS", settings: { ...state.routerSettings, mode } });
    toast(`Router mode: ${ROUTER_MODE_LABEL[mode]}`);
    const draw = state.drawState;
    const cur = state.cursorNm;
    if (draw?.kind === "route" && cur) {
      void routeSetMode(mode).then(() => routeMove(cur.x, cur.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) })));
    } else if (draw?.kind === "diffpair" && cur) {
      void routeSetMode(mode).then(() => dpMove(cur.x, cur.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, preview) })));
    } else if (draw?.kind === "drag") {
      void routeSetMode(mode);
    }
  };
  m.set("pcbnew.InteractiveRouter.ShoveMode", pcbOnly(() => setRouterMode("shove")));
  m.set("pcbnew.InteractiveRouter.WalkaroundMode", pcbOnly(() => setRouterMode("walkaround")));
  m.set("pcbnew.InteractiveRouter.HighlightMode", pcbOnly(() => setRouterMode("mark_obstacles")));
  m.set("pcbnew.InteractiveRouter.CycleRouterMode", pcbOnly(() => setRouterMode(nextRouterMode(state.routerSettings.mode))));

  // ------------------------------------------------------------ track corner mode
  // router_tool.cpp handlePnSCornerModeChange: `Settings().SetCornerMode( mode )`, then `UpdateMessagePanel()` and `m_router->Move( .. )` to refresh the
  // head. The studio keeps the mode in `state.routerSettings` (sent with the next route) and pushes it to a running session.
  const setCornerMode = (cornerMode: CornerMode) => {
    const settings = { ...state.routerSettings, cornerMode };
    dispatch({ type: "SET_ROUTER_SETTINGS", settings });
    toast(`Track corner mode: ${CORNER_MODE_LABEL[cornerMode]}`);
    const draw = state.drawState;
    const cur = state.cursorNm;
    if (draw?.kind === "route" || draw?.kind === "drag") {
      void routeSetSettings(settings).then(() => draw.kind === "route" && cur && routeMove(cur.x, cur.y).then((preview) => preview.ok && dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, preview) })));
    }
  };
  // They are `AS_CONTEXT` actions of the router tool (`Go( &ROUTER_TOOL::handlePnSCornerModeChange, ACT_SwitchCornerMode*.MakeEvent() )`): live
  // only while it is the active tool (single track or pair, idle or routing), so their hotkeys (Ctrl+/, Ctrl+W, ...) do nothing elsewhere.
  const inRouterTool = state.activeTool === "route" || state.activeTool === "diffpair" || state.drawState?.kind === "route" || state.drawState?.kind === "diffpair";
  if (inRouterTool) {
    m.set("pcbnew.InteractiveRouter.SwitchRoundingToNext", pcbOnly(() => setCornerMode(nextCornerMode(state.routerSettings.cornerMode ?? "mitered_45"))));
    m.set("pcbnew.InteractiveRouter.SwitchRounding45", pcbOnly(() => setCornerMode("mitered_45")));
    m.set("pcbnew.InteractiveRouter.SwitchRoundingArc45", pcbOnly(() => setCornerMode("rounded_45")));
    m.set("pcbnew.InteractiveRouter.SwitchRounding90", pcbOnly(() => setCornerMode("mitered_90")));
    m.set("pcbnew.InteractiveRouter.SwitchRoundingArc90", pcbOnly(() => setCornerMode("rounded_90")));
    // the via commands, Switch Track Posture and Custom Track/Via Size (actions/routerViaActions.ts)
    registerRouterViaActions(m, ctx);
  }

  // ------------------------------------------------------------ selection by net
  // PCB_SELECTION_TOOL::selectNet / deselectNet: `selectCursor()`, then SelectAllItemsOnNet for the net of every selected
  // connected item -- the tracks and vias of the net that pass the selection filter (`itemPassesFilter`).
  const selectNet = (select: boolean) =>
    pcbOnly(() => {
      if (!board) return;
      const ids = ctx.requestSelection();
      const nets = netsOfItems(board, ids);
      if (nets.size === 0) return;
      const passes = (id: string): boolean => {
        if (lockedIds.has(id) && !state.selectionFilter.lockedItems) return false;
        const kind = itemKind(board, id);
        return kind === "track" ? state.selectionFilter.tracks : kind === "via" ? state.selectionFilter.vias : true;
      };
      const next = new Set(state.selection.size > 0 ? state.selection : ids);
      for (const id of netItems(board, nets)) {
        if (!passes(id)) continue;
        if (select) next.add(id);
        else next.delete(id);
      }
      dispatch({ type: "SET_SELECTION", refs: [...next] });
    });
  m.set("pcbnew.InteractiveSelection.SelectNet", selectNet(true));
  m.set("pcbnew.InteractiveSelection.DeselectNet", selectNet(false));

  // PCB_SELECTION_TOOL::filterSelection: DIALOG_FILTER_SELECTION, then the selection keeps only what the options include.
  m.set(
    "pcbnew.InteractiveSelection.FilterSelection",
    pcbOnly(() => {
      if (!board) return;
      if (state.selection.size === 0) {
        toast("Select items first, then filter the selection.");
        return;
      }
      openSweepDialog({
        kind: "filter_selection",
        options: last.filterOptions,
        onOk: (options) => {
          last.filterOptions = options;
          const st = api.getState();
          if (!st.board) return;
          dispatch({ type: "SET_SELECTION", refs: filterSelection(st.board, [...st.selection], options) });
        },
      });
    })
  );

  // PCB_SELECTION_TOOL::unrouteSelected: delete the tracks/vias connected to the selected footprints' pads and tracks (up to the next pad).
  m.set(
    "pcbnew.InteractiveSelection.unrouteSelected",
    pcbOnly(() => {
      if (!board || state.selection.size === 0) return;
      const r = unrouteSelected(board, [...state.selection]);
      if (r.trackIds.length === 0 && r.viaIds.length === 0) {
        toast("Nothing to unroute: no tracks are connected to the selection.");
        return;
      }
      dispatch({ type: "CLEAR_SELECTION" });
      void api.cmd({ op: "commit_route", remove_track_ids: r.trackIds, remove_via_ids: r.viaIds }).then((ok) => ok && dispatch({ type: "SET_SELECTION", refs: r.reselect }));
    })
  );

  // ------------------------------------------------------------------ Mirror
  // EDIT_TOOL::Mirror: `MirrorableItems` -- shapes, fields, texts, text boxes, zones, tracks, vias, groups (tuning patterns among them) and points -- about the
  // item's position / the selection centre.
  const mirror = (direction: FlipDirection) =>
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const k = itemKind(board, id);
        return k === "track" || k === "via" || k === "zone" || k === "shape" || k === "text" || k === "group" || k === "field" || k === "point" || k === "textbox";
      });
      // `FilterCollectorForLockedItems`: an item that is locked, in a locked group, or a group with a locked item in it, is left out -- then its members.
      const free = ids.filter((id) => !isLocked(board, id));
      const items = mirrorableIds(board, free).filter((id) => !lockedIds.has(id) && !isLocked(board, id));
      if (items.length === 0) return;
      let centre = mirrorReferencePoint(board, free);
      if (!centre) return;
      // `updateModificationPoint`: several items mirror about the centre of the selection moved to the best snap anchor (anchors, else the grid).
      if (ids.length > 1) centre = modificationSnapFor(state)(centre);
      const plan = planMirror(board, items, centre, direction);
      void applyEdit(plan.cmds, plan.removed);
    });
  m.set("pcbnew.InteractiveEdit.mirrorHoriontally", mirror("leftRight"));
  m.set("pcbnew.InteractiveEdit.mirrorVertically", mirror("topBottom"));

  // ------------------------------------------------- Change Track Width / Via Size
  // EDIT_TOOL::ChangeTrackWidth: every selected track gets the current track width, every via the current via size and drill.
  m.set(
    "pcbnew.InteractiveEdit.changeTrackWidth",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const k = itemKind(board, id);
        return k === "track" || k === "via";
      });
      if (ids.length === 0) return;
      const rules = board.board_rules;
      const width = state.currentTrackWidthNm ?? rules?.track_width ?? 250_000;
      const via = state.currentViaPreset ?? { diameter: rules?.via_diameter ?? 600_000, drill: rules?.via_drill ?? 300_000 };
      void api.cmd({ op: "edit_tracks_and_vias", ids, track_width: { kind: "value", nm: width }, via_size: { kind: "value", diameter: via.diameter, drill: via.drill } });
    })
  );

  // -------------------------------------------- Fillet / Chamfer / Dogbone / Extend Lines
  // EDIT_TOOL::ModifyLines: the selected segments, rectangles and polygons (the latter two are broken into lines first),
  // every pair of lines through the routine, one undo step.
  const lineShapes = (): Shape[] => {
    if (!board) return [];
    const ids = requestFiltered((id) => {
      const s = api.shapeById(id);
      return !!s && isLineModifiable(s);
    });
    return ids.map((id) => api.shapeById(id)).filter((s): s is Shape => !!s);
  };
  const applyLines = (res: ModifyResult) => {
    if (!res.ok) {
      toast(res.message ?? "Nothing to do.");
      return;
    }
    const { cmds, removed } = modifyResultCmds(board, res);
    void applyEdit(cmds, removed, res.message);
  };
  const modifyLinesAction = (kind: LineRoutine["kind"]) =>
    pcbOnly(() => {
      const shapes = lineShapes();
      const refusal = lineSelectionError(kind, shapes);
      if (refusal) {
        toast(refusal);
        return;
      }
      if (kind === "fillet") {
        openSweepDialog({
          kind: "unit_entry",
          title: "Fillet Lines",
          label: "Radius:",
          valueNm: last.filletRadiusNm,
          onOk: (radiusNm) => {
            last.filletRadiusNm = radiusNm;
            applyLines(modifyLines(shapes, { kind: "fillet", radiusNm }));
          },
        });
      } else if (kind === "chamfer") {
        openSweepDialog({
          kind: "unit_entry",
          title: "Chamfer Lines",
          label: "Chamfer setback:",
          valueNm: last.chamferSetbackNm,
          onOk: (setbackNm) => {
            last.chamferSetbackNm = setbackNm;
            applyLines(modifyLines(shapes, { kind: "chamfer", setbackNm }));
          },
        });
      } else if (kind === "dogbone") {
        openSweepDialog({
          kind: "dogbone",
          radiusNm: last.dogbone.radiusNm,
          addSlots: last.dogbone.addSlots,
          onOk: (v) => {
            last.dogbone = v;
            applyLines(modifyLines(shapes, { kind: "dogbone", radiusNm: v.radiusNm, addSlots: v.addSlots, boardOutline: board ? boardOutlineRings(board) : [] }));
          },
        });
      } else {
        applyLines(modifyLines(shapes, { kind: "extend" }));
      }
    });
  m.set("pcbnew.InteractiveEdit.filletLines", modifyLinesAction("fillet"));
  m.set("pcbnew.InteractiveEdit.chamferLines", modifyLinesAction("chamfer"));
  m.set("pcbnew.InteractiveEdit.dogboneCorners", modifyLinesAction("dogbone"));
  m.set("pcbnew.InteractiveEdit.extendLines", modifyLinesAction("extend"));

  // EDIT_TOOL::SimplifyPolygons: polygons and zones, `SimplifyOutlines( tolerance )`.
  m.set(
    "pcbnew.InteractiveEdit.simplifyPolygons",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const k = itemKind(board, id);
        if (k === "shape") return api.shapeById(id)?.kind === "polygon";
        if (k === "zone") return !api.zoneById(id)?.teardrop;
        return false;
      });
      if (ids.length === 0) return;
      openSweepDialog({
        kind: "unit_entry",
        title: "Simplify Shapes",
        label: "Tolerance value:",
        valueNm: last.simplifyToleranceNm,
        onOk: (toleranceNm) => {
          last.simplifyToleranceNm = toleranceNm;
          if (toleranceNm <= 0) return;
          const cmds: Cmd[] = [];
          const removed: string[] = [];
          for (const id of ids) {
            const s = api.shapeById(id);
            const z = api.zoneById(id);
            if (s?.kind === "polygon") {
              const ring = simplifyPolygonRing(s.pts, toleranceNm);
              if (!ring || ring.length < 3) continue;
              // `commit.Modify( item ); poly.SimplifyOutlines( .. )`: the same polygon, its id, group and lock kept (round 3 D-10)
              cmds.push({ op: "replace_shape", id, shape: { kind: "polygon", layer: s.layer, stroke_width: s.stroke_width, filled: s.filled, pts: ring.map(([x, y]) => ({ x, y })) } });
            } else if (z) {
              const ring = simplifyPolygonRing(z.outline, toleranceNm);
              if (ring && ring.length >= 3) cmds.push({ op: "set_zone_outline", id, outline: ring.map(([x, y]) => ({ x, y })) });
            }
          }
          void applyEdit(cmds, removed, cmds.length === 0 ? "Nothing to simplify within that tolerance." : null);
        },
      });
    })
  );

  // EDIT_TOOL::HealShapes: ConnectBoardShapes over the selected segments, arcs and curves.
  m.set(
    "pcbnew.InteractiveEdit.healShapes",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const s = api.shapeById(id);
        return !!s && (s.kind === "segment" || s.kind === "arc" || s.kind === "bezier");
      });
      const shapes = ids.map((id) => api.shapeById(id)).filter((s): s is Shape => !!s);
      if (shapes.length === 0) return;
      openSweepDialog({
        kind: "unit_entry",
        title: "Heal Shapes",
        label: "Tolerance value:",
        valueNm: last.healToleranceNm,
        onOk: (toleranceNm) => {
          last.healToleranceNm = toleranceNm;
          if (toleranceNm <= 0) return;
          const res = healShapes(shapes, toleranceNm);
          const { cmds, removed } = modifyResultCmds(board, res);
          void applyEdit(cmds, removed, cmds.length === 0 ? "No shape ends were close enough to heal." : null);
        },
      });
    })
  );

  // ------------------------------------------------------------ polygon booleans
  // EDIT_TOOL::BooleanPolygons: rectangles, circles and polygons; the last selected shape is moved to the front (the base and
  // property donor), then `boolean_shapes` runs the routine (with the largest-first retry for Subtract).
  const booleanAction = (operation: "merge" | "subtract" | "intersect") =>
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const s = api.shapeById(id);
        return !!s && (s.kind === "polygon" || s.kind === "rect" || s.kind === "circle");
      });
      if (ids.length < 2) {
        toast(`Select at least two polygons, rectangles or circles to ${operation} them.`);
        return;
      }
      const order = [...ids];
      const lastAdded = [...state.selection].pop();
      if (lastAdded !== undefined && order[order.length - 1] === lastAdded) [order[0], order[order.length - 1]] = [order[order.length - 1]!, order[0]!];
      void applyEdit([{ op: "boolean_shapes", operation, ids: order }], order);
    });
  m.set("pcbnew.InteractiveEdit.mergePolygons", booleanAction("merge"));
  m.set("pcbnew.InteractiveEdit.subtractPolygons", booleanAction("subtract"));
  m.set("pcbnew.InteractiveEdit.intersectPolygons", booleanAction("intersect"));

  // -------------------------------------------------------------- Fillet Tracks
  // EDIT_TOOL::FilletTracks.
  m.set(
    "pcbnew.InteractiveEdit.filletTracks",
    pcbOnly(() => {
      if (!board) return;
      const ids = requestFiltered((id) => {
        const k = itemKind(board, id);
        return k === "track" || k === "via";
      });
      const eb = editBoardOf(board);
      const tracks = ids.map((id) => eb.tracks.find((t) => t.id === id)).filter((t): t is EditTrack => !!t);
      const segs = tracks.reduce((n, t) => n + (t.arc_mid ? 0 : Math.max(t.pts.length - 1, 0)), 0);
      if (segs < 2) {
        toast("At least two straight track segments must be selected.");
        return;
      }
      openSweepDialog({
        kind: "unit_entry",
        title: "Fillet Tracks",
        label: "Radius:",
        valueNm: last.filletTracksRadiusNm,
        allowZero: false,
        onOk: (radiusNm) => {
          last.filletTracksRadiusNm = radiusNm;
          const res = filletTracks(eb, tracks, radiusNm);
          if (!res.ok || !res.cmd) {
            if (res.message) toast(res.message);
            return;
          }
          void applyEdit([res.cmd], ids, res.message);
        },
      });
    })
  );

  // ---------------------------------------------------------------- Break Track
  // ROUTER_TOOL::InlineBreakTrack: exactly one selected track, broken where the pointer is.
  m.set(
    "pcbnew.InteractiveRouter.BreakTrack",
    pcbOnly(() => {
      // "If we're here from a context menu then we need to get the position of the cursor when the context menu was invoked."
      const cursor = state.pcbx.menuCursorNm ?? state.cursorNm;
      if (!board || state.selection.size !== 1 || !cursor) return;
      const id = [...state.selection][0]!;
      const track = board.routing?.tracks.find((t) => t.id === id);
      if (!track) return;
      dispatch({ type: "CLEAR_SELECTION" }); // `RunAction( ACTIONS::selectionClear )`, before anything else
      const res = breakTrack(editBoardOf(board), { id: track.id, net: track.net, layer: track.layer, width: track.width, pts: track.pts, arc_mid: track.arc_mid }, [cursor.x, cursor.y], board.snap ?? state.gridNm);
      if (!res.ok || !res.cmd) {
        toast(res.message ?? "Break Track: put the cursor on the track, away from its ends and joints.");
        return;
      }
      const cmd = res.cmd;
      void (async () => {
        // `m_startItem->IsLocked()`: KiCad asks, and the halves stay locked (`PNS_KICAD_IFACE::AddItem`: `if( aItem->IsLocked() ) newBoardItem->SetLocked( true )`)
        if (lockedIds.has(id)) {
          if (!(await askConfirm({ title: "Confirmation", message: "The selected item is locked.", okLabel: "Break Track", cancelLabel: "Cancel" }))) return;
          if (cmd.op === "commit_route") return void api.cmd({ ...cmd, lock_tracks: (cmd.tracks ?? []).map((_, i) => i) });
        }
        void api.cmd(cmd);
      })();
    })
  );

  // ------------------------------------------------------------- Close Outline
  // drawing_tool.cpp (DrawZone / DrawSegment polygon): `closeOutline` finishes the polygon with the points placed so far. The
  // canvas owns the draw, so the action asks it to finish (the same as Enter).
  const draw = state.drawState;
  if (draw?.kind === "zone" || (draw?.kind === "shape" && draw.shapeKind === "polygon")) {
    m.set("pcbnew.InteractiveDrawing.closeOutline", pcbOnly(() => dispatch({ type: "PCBX", patch: { drawFinishRequest: state.pcbx.drawFinishRequest + 1 } })));
  }

  registerPcbRouterSweep(m, ctx);
  registerPcbItemActions(m, ctx);
  registerMicrowaveActions(m, ctx);
  registerPcbMultichannelActions(m, ctx);
  registerPcbSelectionSync(m, ctx);
  registerPcbConvertSweep(m, ctx);
  registerPcbGlobalEditSweep(m, ctx);
  registerEmbeddedActions(m, ctx);
  registerPcbPointEditSweep(m, ctx);
  registerPcbReferenceSweep(m, ctx);
  registerPcbAssignNetclass(m, ctx);
}
