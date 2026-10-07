// The board-control actions (docs/parity/UI-ACTIONS.md: pcbnew.Control, pcbnew.EditorControl, pcbnew.InspectionTool,
// pcbnew.ZoneFiller), ported from pcbnew/tools/pcb_control.cpp, board_editor_control.cpp, board_inspection_tool.cpp and
// zone_filler_tool.cpp. `useActionRunner` calls `registerBoardControlActions` once, while it builds its registry; each handler here
// is a thin layer over the pure logic in `kicad-port/boardControl.ts` and the store, or over an `/api/cmd` verb.
//
// What the actions that are not here (they have a reason in tools/ui-parity-missing.json) need instead is said there.

import type { Dispatch } from "react";
import type { Action, StudioApi, StudioState } from "../state/store";
import type { BoardControlState, BoardExportKind } from "../kicad-port/boardControlState";
import { postRepairBoard } from "../api/boardControl";
import { findNetAtCursor } from "../components/canvas/netAtCursor";
import { fetchFootprintLibraryNames } from "../api/client";
import type { Cmd, ZonePriorityMove } from "../api/types";
import { afterFill, afterUnfill, filledNow, highlightedNets, netsOfSelection, ratsnestModeCycle, selectedCopperZones, setNetsHidden, swapHighlight } from "../kicad-port/boardControl";

export interface BoardControlContext {
  state: StudioState;
  api: StudioApi;
  dispatch: Dispatch<Action>;
  /** Wraps a handler so it runs on the PCB tab only (`useActionRunner`'s own `pcbOnly`). */
  pcbOnly: <Args extends unknown[]>(fn: (...args: Args) => void) => (...args: Args) => void;
  /** `PCB_SELECTION_TOOL::RequestSelection`: the selection, or the item under the cursor when nothing is selected. */
  requestSelection: () => string[];
}

export function registerBoardControlActions(m: Map<string, () => void>, c: BoardControlContext): void {
  const { state, api, dispatch, pcbOnly } = c;
  const bcx = (patch: Partial<BoardControlState>) => dispatch({ type: "BCX", patch });
  const toast = (message: string, kind: "error" | "info" = "info") => dispatch({ type: "TOAST", message, kind });
  const board = state.board;
  const zones = board?.routing?.zones ?? [];
  /** Every zone a fill can cover: a rule area has none. */
  const fillableIds = zones.filter((z) => !z.is_rule_area).map((z) => z.id);
  const toggleTool = (tool: "drill_origin" | "local_ratsnest") => () => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === tool ? "select" : tool });

  // ------------------------------------------------------------ display options (pcb_control.cpp)

  // TrackDisplayMode-style flips of PCB_DISPLAY_OPTIONS: "Sketch Graphic Items" / "Sketch Text Items" / "Show Pad Numbers".
  m.set("pcbnew.Control.graphicOutlines", pcbOnly(() => bcx({ sketchGraphics: !state.bcx.sketchGraphics })));
  m.set("pcbnew.Control.textOutlines", pcbOnly(() => bcx({ sketchText: !state.bcx.sketchText })));
  m.set("pcbnew.Control.showPadNumbers", pcbOnly(() => bcx({ showPadNumbers: !state.bcx.showPadNumbers })));

  // ZoneDisplayMode: the two debug views of a zone's fill. The triangulation needs each island's holes (`/api/fill?polys=1`), so a fill
  // already on screen is fetched again with them.
  m.set("pcbnew.Control.zoneDisplayOutlines", pcbOnly(() => dispatch({ type: "SET_ZONE_DISPLAY_MODE", mode: "fractured" })));
  m.set(
    "pcbnew.Control.zoneDisplayTesselation",
    pcbOnly(() => {
      dispatch({ type: "SET_ZONE_DISPLAY_MODE", mode: "triangulated" });
      if (state.zoneFill) void api.fillZones();
    })
  );

  // RatsnestModeCycle: off -> all layers -> visible layers -> off. Turning the global ratsnest on or off also resets every pad's local flag
  // (`SetElementVisibility( LAYER_RATSNEST )`), which the reducer's TOGGLE_RATSNEST does.
  m.set(
    "pcbnew.Control.ratsnestModeCycle",
    pcbOnly(() => {
      const next = ratsnestModeCycle(state.showRatsnest, state.bcx.ratsnestMode);
      if (next.show !== state.showRatsnest) dispatch({ type: "TOGGLE_RATSNEST" });
      bcx({ ratsnestMode: next.mode });
    })
  );

  // LocalRatsnestTool: a picker -- click a pad (else a footprint) to show or hide its ratsnest lines (Canvas.tsx owns the click).
  m.set("pcbnew.Control.localRatsnestTool", pcbOnly(toggleTool("local_ratsnest")));

  // RepairBoard (not quiet): repairs duplicate ids and orphaned nets, and reports what it did.
  m.set(
    "pcbnew.Control.repairBoard",
    pcbOnly(() => {
      void postRepairBoard().then(async (r) => {
        await api.refresh();
        toast(r.ok ? [r.message, ...(r.details ?? [])].join(" ") : r.message, r.ok ? "info" : "error");
      });
    })
  );

  // The Zone Manager dialog (components/ZoneManagerDialog.tsx).
  m.set("pcbnew.Control.zonesManager", pcbOnly(() => bcx({ zoneManagerOpen: true })));

  // FlipPcbView: toggles `PCB_DISPLAY_OPTIONS::m_FlipBoardView`; Canvas.tsx draws the board mirrored (as seen from its other side) and
  // turns the pointer back into board coordinates.
  m.set("pcbnew.Control.flipBoard", pcbOnly(() => bcx({ boardFlipped: !state.bcx.boardFlipped })));

  // ------------------------------------------------- net highlight and ratsnest (board_inspection_tool.cpp)

  // highlightNet( ..., aUseSelection = true ): every net of the selected connected items, or -- with none selected -- the net under the cursor.
  m.set(
    "pcbnew.EditorControl.highlightNetSelection",
    pcbOnly(() => {
      if (!board) return;
      let nets = netsOfSelection(state.selection, board);
      if (nets.length === 0 && state.cursorUm) {
        const net = findNetAtCursor(board, state.cursorUm.x, state.cursorUm.y, 10 / (state.view.scale || 1));
        if (net) nets = [net];
      }
      dispatch({ type: "SET_NET_HIGHLIGHT_SET", nets });
    })
  );

  // toggleLastNetHighlight: the highlighted nets and the last highlighted ones trade places (`m_lastHighlighted`).
  m.set(
    "pcbnew.EditorControl.toggleLastNetHighlight",
    pcbOnly(() => {
      const swapped = swapHighlight(highlightedNets(state.netHighlight, state.bcx.netHighlightMore), state.bcx.lastHighlight);
      dispatch({ type: "SET_NET_HIGHLIGHT_SET", nets: swapped.current });
      bcx({ lastHighlight: swapped.last });
    })
  );

  // hideNetInRatsnest / showNetInRatsnest on the selection's nets (`doHideRatsnestNet`, netcode <= 0 and a selection).
  const setNetsInRatsnest = (hide: boolean) =>
    pcbOnly(() => {
      const nets = board ? netsOfSelection(c.requestSelection(), board) : [];
      if (nets.length === 0) {
        toast(`Select an item on the net to ${hide ? "hide" : "show"} in the ratsnest.`);
        return;
      }
      bcx({ hiddenRatsnestNets: setNetsHidden(state.bcx.hiddenRatsnestNets, nets, hide) });
    });
  m.set("pcbnew.EditorControl.hideNet", setNetsInRatsnest(true));
  m.set("pcbnew.EditorControl.showNet", setNetsInRatsnest(false));

  // ----------------------------------------------------------- the zone tools (zone_filler_tool.cpp)

  // ZoneFill ("Draft Fill Selected Zone(s)"): fills the selected zones; the others keep whatever fill they had.
  m.set(
    "pcbnew.ZoneFiller.zoneFill",
    pcbOnly(() => {
      const ids = selectedCopperZones(c.requestSelection(), zones);
      if (ids.length === 0) {
        toast("Select the zone(s) to fill.");
        return;
      }
      bcx({ zoneFilled: afterFill(fillableIds, state.bcx.zoneFilled, state.zoneFill != null, ids) });
      void api.fillZones();
    })
  );

  // ZoneUnfill: removes the fill from the selected zones.
  m.set(
    "pcbnew.ZoneFiller.zoneUnfill",
    pcbOnly(() => {
      const ids = selectedCopperZones(c.requestSelection(), zones);
      if (ids.length === 0) {
        toast("Select the zone(s) to unfill.");
        return;
      }
      if (!state.zoneFill) return;
      const left = afterUnfill(fillableIds, state.bcx.zoneFilled, true, ids);
      if (left.length === 0) {
        api.unfillZones();
        return;
      }
      bcx({ zoneFilled: left });
      dispatch({ type: "FILL_OK", fill: state.zoneFill });
    })
  );

  // ZoneFillDirty (the system action behind the auto-refill): refills the zones that have no fill. A filled zone is refilled with every
  // change to the board already (the live fill), so what is left to do is the zones that were never filled -- or were just unfilled.
  m.set(
    "pcbnew.ZoneFiller.zoneFillDirty",
    pcbOnly(() => {
      if (fillableIds.length === 0) return;
      if (state.zoneFill && filledNow(fillableIds, state.bcx.zoneFilled, true).length === fillableIds.length) return;
      bcx({ zoneFilled: null });
      void api.fillZones();
    })
  );

  // ZoneMerge: the selected zones that touch the first one are merged into it (Cmd::MergeZones); the result is selected.
  m.set(
    "pcbnew.EditorControl.zoneMerge",
    pcbOnly(() => {
      const ids = [...state.selection].filter((id) => api.zoneById(id));
      if (ids.length < 2) return;
      void api.cmd({ op: "merge_zones", ids }).then((ok) => {
        if (ok) dispatch({ type: "SET_SELECTION", refs: [ids[0]!] });
      });
    })
  );

  // ZoneDuplicate: exactly one zone selected; the Zone Properties dialog opens on a copy of its settings (components/ZoneDialog.tsx).
  m.set(
    "pcbnew.EditorControl.zoneDuplicate",
    pcbOnly(() => {
      const ids = [...state.selection];
      if (ids.length !== 1) return;
      const zone = api.zoneById(ids[0]!);
      if (zone) bcx({ duplicateZoneId: zone.id });
    })
  );

  // ZonePriorityMoveToTop / Raise / Lower / MoveToBottom: exactly one copper zone selected (Cmd::SetZonePriority).
  const priority = (to: ZonePriorityMove) =>
    pcbOnly(() => {
      const ids = [...state.selection];
      if (ids.length !== 1) return;
      const zone = api.zoneById(ids[0]!);
      if (!zone || zone.is_rule_area || zone.teardrop) return;
      void api.cmd({ op: "set_zone_priority", id: zone.id, to });
    });
  m.set("pcbnew.EditorControl.zonePriorityMoveToTop", priority("top"));
  m.set("pcbnew.EditorControl.zonePriorityRaise", priority("raise"));
  m.set("pcbnew.EditorControl.zonePriorityLower", priority("lower"));
  m.set("pcbnew.EditorControl.zonePriorityMoveToBottom", priority("bottom"));

  // ---------------------------------------------------------- board editor control (board_editor_control.cpp)

  // AutoTrackWidth: a route that starts at an existing track takes its width (Canvas.tsx reads `bcx.autoTrackWidth`).
  m.set("pcbnew.EditorControl.autoTrackWidth", pcbOnly(() => bcx({ autoTrackWidth: !state.bcx.autoTrackWidth })));

  // DrillOrigin: a picker -- one click sets the drill/place file origin (Canvas.tsx); the reset puts it back at (0, 0). Both are Cmd::SetAuxOrigin.
  m.set("pcbnew.EditorControl.drillOrigin", pcbOnly(toggleTool("drill_origin")));
  m.set(
    "pcbnew.EditorControl.drillResetOrigin",
    pcbOnly(() => {
      if (board?.aux_origin) void api.cmd({ op: "set_aux_origin", at: null });
    })
  );

  // The export / output dialogs (components/BoardExportDialog.tsx): kicad-cli's, except the .cmp file.
  const exportDialog = (kind: BoardExportKind) => pcbOnly(() => bcx({ boardExport: kind }));
  m.set("pcbnew.EditorControl.exportSTEP", exportDialog("3d"));
  m.set("pcbnew.EditorControl.exportVRML", exportDialog("vrml"));
  m.set("pcbnew.EditorControl.exportGenCAD", exportDialog("gencad"));
  m.set("pcbnew.EditorControl.generateD356File", exportDialog("ipcd356"));
  m.set("pcbnew.EditorControl.generateIPC2581File", exportDialog("ipc2581"));
  m.set("pcbnew.EditorControl.generateODBPPFile", exportDialog("odb"));
  m.set("pcbnew.EditorControl.generateBOM", exportDialog("pcb_bom"));
  m.set("pcbnew.EditorControl.exportFootprintAssociations", exportDialog("cmp"));

  // ExportFootprints ("Footprints..." under File > Export, `ExportFootprintsToLibrary( false )`): saves a copy of every footprint of the
  // board into a library. The studio has the one project footprint library (`design.footprint_library`) rather than a library table to pick
  // from, and a placed footprint is its named footprint (a library entry reaches the board only once it is published), so each distinct
  // footprint the board uses that is not an entry yet becomes one (`Cmd::OpenFootprintForEdit` copies what the board resolves the name to).
  // KiCad skips a footprint with no library item name, and its "link the board footprints to the exported ones" option has nothing to do.
  m.set(
    "pcbnew.EditorControl.exportFootprints",
    pcbOnly(() => {
      const names = [...new Set((board?.parts ?? []).map((p) => p.footprint).filter((n): n is string => !!n))];
      if (names.length === 0) {
        toast("No footprints to export!");
        return;
      }
      void fetchFootprintLibraryNames()
        .then(async (lib) => {
          const resolvable = new Set(lib.names);
          const entries = new Set(lib.project ?? []);
          const todo = names.filter((n) => resolvable.has(n) && !entries.has(n));
          if (todo.length > 0 && !(await api.cmdBatch(todo.map((name): Cmd => ({ op: "open_footprint_for_edit", name }))))) return;
          const skipped = names.filter((n) => !resolvable.has(n)).length;
          toast(`${todo.length} footprint(s) added to the project footprint library, ${names.length - todo.length - skipped} already there${skipped > 0 ? `, ${skipped} not found` : ""}.`);
        })
        .catch((e: unknown) => toast(e instanceof Error ? e.message : String(e), "error"));
    })
  );

  // ShowFootprintLinks: exactly one footprint selected.
  m.set(
    "pcbnew.InspectionTool.ShowFootprintAssociations",
    pcbOnly(() => {
      const ids = [...state.selection];
      const part = ids.length === 1 ? api.partByRef(ids[0]!) : undefined;
      if (!part) {
        toast("Select a footprint for a footprint associations report.", "error");
        return;
      }
      bcx({ associationsRef: part.ref });
    })
  );
}
