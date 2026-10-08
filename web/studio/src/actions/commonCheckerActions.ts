// The Checker actions of the Inspect menus: Next Marker, Previous Marker and Exclude Marker.
//
//   pcbnew/tools/drc_tool.cpp                 DRC_TOOL::NextMarker / PrevMarker / ExcludeMarker
//   eeschema/tools/sch_inspection_tool.cpp    SCH_INSPECTION_TOOL::NextMarker / PrevMarker / ExcludeMarker
//   common/rc_item.cpp                        RC_TREE_MODEL::NextMarker / PrevMarker (kicad-port/checkerNav.ts)
//   eeschema/dialogs/dialog_erc.cpp           DIALOG_ERC::ExcludeMarker
//
// The tools show the Checker window (when it is hidden) and move its selection by one marker; selecting a marker selects the items it names and
// frames them on the canvas -- here the same "click a row" the DRC and ERC dialogs do. Exclude Marker is the schematic's only: ERC exclusions are
// stored (`add_erc_exclusion`); the board has none (DRC exclusions are not modelled), so the entry stays off on the PCB tab.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import type { DrcViolation, ErcViolation } from "../api/types";
import { fetchVersion } from "../api/client";
import { nextMarkerIndex, prevMarkerIndex } from "../kicad-port/checkerNav";
import { revisionAfterOwnEdit } from "../kicad-port/checkRevision";
import { fitTransform } from "../kicad-port/view";
import { ercMarkerPosition } from "../components/schematic/ercMarkerPosition";
import { canvasRect } from "./canvasEvents";

/** Our id for a violation's item -> the id the canvas selects by: a pad (`REF.PAD`) selects its part, a track segment (`id#n`) its track. */
const baseRef = (id: string) => id.split("#")[0]!.split(".")[0]!;

/** A close-up needs some room around a single point: 2 mm either side (the dialogs' own padding). */
const FRAME_PAD_UM = 2_000;

export function registerCheckerActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const { api, dispatch } = ctx;

  /** Select the violation the way a click on its row does: select and highlight what it names, frame the canvas on it. */
  const jumpToDrc = (v: DrcViolation, index: number) => {
    dispatch({ type: "SET_DRC_SELECTED", index });
    const refs = v.items.flatMap((it) => (it.id && it.id !== "outline" ? [baseRef(it.id)] : []));
    dispatch({ type: "SET_SELECTION", refs });
    dispatch({ type: "SET_HOT", refs });
    const rect = canvasRect();
    if (!rect || rect.width < 50 || rect.height < 50 || v.items.length === 0) return;
    const xs = v.items.map((it) => it.pos[0]);
    const ys = v.items.map((it) => it.pos[1]);
    const bounds = { minX: Math.min(...xs) - FRAME_PAD_UM, minY: Math.min(...ys) - FRAME_PAD_UM, maxX: Math.max(...xs) + FRAME_PAD_UM, maxY: Math.max(...ys) + FRAME_PAD_UM };
    dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height, 60) });
  };
  const jumpToErc = (v: ErcViolation, index: number) => {
    dispatch({ type: "SET_ERC_SELECTED", index });
    const resolved = ercMarkerPosition(v.location, api.getState().schematic);
    if (!resolved) return;
    if (resolved.refs.length > 0) {
      dispatch({ type: "SET_SELECTION", refs: resolved.refs });
      dispatch({ type: "SET_HOT", refs: resolved.refs });
    }
    const rect = canvasRect();
    if (rect && rect.width >= 50 && rect.height >= 50) {
      const [x, y] = resolved.at;
      dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform({ minX: x - FRAME_PAD_UM, minY: y - FRAME_PAD_UM, maxX: x + FRAME_PAD_UM, maxY: y + FRAME_PAD_UM }, rect.width, rect.height, 60) });
    }
  };

  /**
   * `DRC_TOOL::NextMarker`: with the dialog there, show it and step; without one, `ShowDRCDialog` -- the dialog opens (and runs the check when
   * it has not been run) and that is all. `SCH_INSPECTION_TOOL::NextMarker` always steps the (existing) ERC dialog.
   */
  const step = (dir: "next" | "prev"): ActionHandler => () => {
    const s = api.getState();
    const pick = dir === "next" ? nextMarkerIndex : prevMarkerIndex;
    if (ctx.tab === "pcb") {
      if (!s.drcDialogOpen) {
        dispatch({ type: "SET_DRC_OPEN", open: true });
        return;
      }
      const list = s.drc?.violations ?? [];
      const to = pick(list.length, s.drcSelected);
      if (to !== null) jumpToDrc(list[to]!, to);
    } else if (ctx.tab === "schematic") {
      if (!s.ercDialogOpen) dispatch({ type: "SET_ERC_DIALOG_OPEN", open: true });
      const list = s.erc?.violations ?? [];
      const to = pick(list.length, s.ercSelected);
      if (to !== null) jumpToErc(list[to]!, to);
    }
  };
  if (ctx.tab === "pcb" || ctx.tab === "schematic") {
    m.set("common.Checker.nextMarker", step("next"));
    m.set("common.Checker.prevMarker", step("prev"));
  }

  // ACTIONS::excludeMarker -- SCH_INSPECTION_TOOL::ExcludeMarker -> DIALOG_ERC::ExcludeMarker( marker ): the violation the ERC list has selected
  // (a marker not excluded yet) is excluded, which stores it (`add_erc_exclusion`) and marks it in the report on screen without making the run
  // look out of date.
  if (ctx.tab === "schematic") {
    m.set("common.Checker.excludeMarker", () => {
      void (async () => {
        const s = api.getState();
        const v = s.ercSelected !== null ? s.erc?.violations[s.ercSelected] : undefined;
        if (!v || !v.location || v.severity === "excluded") return;
        const before = api.getState();
        if (!(await api.cmd({ op: "add_erc_exclusion", check: v.check, location: v.location }))) return;
        const boardAfter = await fetchVersion().catch(() => null);
        const version = revisionAfterOwnEdit(before.ercVersion, before.version, boardAfter);
        dispatch({ type: "ERC_MARK_EXCLUDED", check: v.check, location: v.location, excluded: true, version });
      })();
    });
  }
}
