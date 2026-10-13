// Inspect > Footprint Checker in the Footprint Editor (`pcbnew.ModuleEditor.checkFootprint`, `FOOTPRINT_EDITOR_CONTROL::CheckFootprint`, pcbnew/dialogs/dialog_footprint_checker.cpp at
// KiCad 8303b2ad):
//
//   Footprint Checker...      opens the dialog (it does not run anything: the badges say "-"); with the markers of an earlier run still on the board it shows them
//   Run Checks                `runChecks`: delete the markers, check the footprint, put a marker where each check failed
//   Delete Marker / Delete All Markers / selecting a marker (`OnSelectItem`: the marker's items are selected and framed, its layer brought to the front)
//
// The checks are kicad-cli's: its DRC on a scratch board that holds the footprint as the editor has it (POST /api/footprint/check, crates/kicad-engine/src/footprint_check.rs); nothing
// here checks a footprint. A footprint opened after a run clears the markers, as loading one clears the editor's board.
import type { Dispatch } from "react";
import type { FootprintEditorApi, FpAction } from "../state/footprintEditorStore";
import { fetchFootprintCheck } from "../api/client";
import { getFootprintChecker, setFootprintChecker } from "../state/footprintChecker";
import { markerItems, markerLayer, visibleMarkers } from "../kicad-port/footprintChecker";

/** `CheckFootprint`: the dialog comes up (created, or only shown again). The markers of another footprint are not this one's. */
export function showFootprintChecker(api: FootprintEditorApi): void {
  const f = getFootprintChecker();
  const name = api.getState().name;
  const sameFootprint = f.name === name;
  setFootprintChecker({ open: true, shown: f.shown + 1, ...(sameFootprint ? {} : { name, markers: [], selected: null, checksRun: false, engine: null, error: null }) });
}

/** `OnCancelClick` -> `DestroyCheckerDialog`: the dialog goes; the markers stay on the board. */
export function closeFootprintChecker(): void {
  setFootprintChecker({ open: false });
}

/** `runChecks`: `deleteAllMarkers()`, then the checks. */
export async function runChecks(api: FootprintEditorApi, dispatch: Dispatch<FpAction>): Promise<void> {
  const name = api.getState().name;
  deleteAllMarkers(dispatch);
  if (!name) return;
  setFootprintChecker({ running: true, name, error: null });
  try {
    const reply = await fetchFootprintCheck(name);
    if (!reply.ok) {
      setFootprintChecker({ running: false, error: reply.message ?? "The footprint could not be checked." });
      return;
    }
    // the footprint may have been changed for another while kicad-cli ran
    if (api.getState().name !== name) {
      setFootprintChecker({ running: false });
      return;
    }
    setFootprintChecker({ running: false, checksRun: true, engine: reply.engine ?? null, markers: reply.markers ?? [], selected: null });
  } catch (e) {
    setFootprintChecker({ running: false, error: e instanceof Error ? e.message : String(e) });
  }
}

/** `deleteAllMarkers`: the selection is cleared first (it may be the selected marker's items), the markers go, the badges say "-" again. */
export function deleteAllMarkers(dispatch: Dispatch<FpAction>): void {
  dispatch({ type: "CLEAR_SELECTION" });
  setFootprintChecker({ markers: [], selected: null, checksRun: false, engine: null });
}

/** `OnDeleteOneClick`: the selected marker. */
export function deleteSelectedMarker(dispatch: Dispatch<FpAction>): void {
  const f = getFootprintChecker();
  if (f.selected === null) return;
  dispatch({ type: "CLEAR_SELECTION" });
  const markers = f.markers.filter((_, i) => i !== f.selected);
  setFootprintChecker({ markers, selected: null });
}

/** The canvas's size, to centre a marker in it. */
function canvasSize(): { width: number; height: number } | null {
  const r = document.querySelector(".footprint-editor-view .editor-canvas-col")?.getBoundingClientRect();
  return r && r.width >= 50 && r.height >= 50 ? { width: r.width, height: r.height } : null;
}

/** `OnSelectItem`: select the items the marker names, centre the view on the marker, bring its layer to the front. */
export function selectMarker(index: number, api: FootprintEditorApi, dispatch: Dispatch<FpAction>): void {
  const f = getFootprintChecker();
  const m = f.markers[index];
  if (!m) return;
  setFootprintChecker({ selected: index });
  dispatch({ type: "SET_SELECTION", refs: markerItems(m) });
  const layer = markerLayer(m);
  if (layer) dispatch({ type: "SET_ACTIVE_LAYER", layer });
  const size = canvasSize();
  const view = api.getState().view;
  if (size && view.scale > 0) dispatch({ type: "SET_VIEW", view: { scale: view.scale, x: size.width / 2 - m.at[0] * view.scale, y: size.height / 2 - m.at[1] * view.scale } });
}

/** `NextMarker` / `PreviousMarker` over the rows the Show boxes let through (the arrow keys of the list). */
export function stepMarker(delta: 1 | -1, api: FootprintEditorApi, dispatch: Dispatch<FpAction>): void {
  const f = getFootprintChecker();
  const rows = visibleMarkers(f.markers, f.show);
  if (rows.length === 0) return;
  const at = f.selected === null ? -1 : rows.indexOf(f.selected);
  const next = rows[(at + delta + rows.length) % rows.length]!;
  selectMarker(next, api, dispatch);
}
