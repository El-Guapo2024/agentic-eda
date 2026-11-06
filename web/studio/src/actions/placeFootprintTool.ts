// Place Footprint (`A`, `pcbnew.EditorControl.placeFootprint`) as KiCad's tool loop runs it: `BOARD_EDITOR_CONTROL::PlaceFootprint`
// (pcbnew/tools/board_editor_control.cpp at 8303b2ad), UI-LEFT P19.
//
//   - `A` opens the Footprint Chooser at once (`immediate_actions`: `PrimeTool( { 0, 0 } )` with the prime position ignored). A footprint chosen is put on
//     the board where the mouse is and carried with it (`fp->SetPosition( cursorPos )` on every motion; R and F turn and flip it -- the edit tool's actions,
//     the footprint being selected); a click puts it down (`commit.Push( _( "Place Footprint" ) )`, one undo step) and clears the selection.
//   - The tool then stays: the next click opens the chooser again and that footprint is carried from where the click was, and so on.
//   - Escape while a footprint is carried takes it away (`cleanup()`: `commit.Revert()`) and the tool goes on; Escape with none ends the tool, as does another
//     tool taking over. A chooser cancelled leaves the tool waiting for a click (`if( fp == nullptr ) continue;`).
//   - `A` again while the tool runs does nothing (`if( m_inPlaceFootprint ) return 0;`).
//   - The Footprint Editor's Insert hands its footprint to the same tool (`pcb_ToolManager->RunAction( PCB_ACTIONS::placeFootprint, newFootprint )`,
//     `FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard`): it is carried from the board's cursor, and the tool goes on after its drop; an insert while a
//     footprint is carried is refused ("Previous footprint placement still in progress.", `PlacingFootprint()`).
//
// The studio's carry is the one Duplicate and Paste use (`StudioApi.placeNewItems`: the step held open on the server, crates/cli/src/carry.rs; the Move tool
// carries the selected footprint, and its drop joins the step), so the footprint is on the board while it is carried, where KiCad's is a footprint added to
// a commit not yet pushed. The Move tool snaps the carried footprint as a move does (`BestSnapAnchor` on the footprints' grid), where KiCad's loop puts it on
// the grid point under the cursor (`GetCursorPosition( true )`). The chooser's "-- Unplaced --" group (the schematic's parts not on the board, the studio's own)
// arms the part for a click as before, and the tool goes on after that click.
import type { Dispatch } from "react";
import type { Cmd, PointXY } from "../api/types";
import type { Action, StudioApi } from "../state/store";
import { picker } from "./pcbPicker";
import { gridFrameAngleAt } from "../kicad-port/gridFrame";

/** Where the tool stands: off, the chooser open, waiting for the click that opens it, a footprint carried, or an unplaced part armed for its click. */
export type PlaceFootprintStage = "idle" | "choosing" | "waiting" | "carrying" | "armed";

export interface PlaceFootprintCtx {
  api: StudioApi;
  dispatch: Dispatch<Action>;
}

/** `PlacingFootprint()`'s refusal of a second insert (`FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard`). */
export const PLACEMENT_IN_PROGRESS = "Previous footprint placement still in progress.";

/** The prompt next to the pointer while the tool waits (KiCad shows the pencil cursor only). */
export const WAITING_PROMPT = "Place Footprint: click to choose a footprint (Escape ends the tool)";

let stage: PlaceFootprintStage = "idle";
let ctx: PlaceFootprintCtx | null = null;
/** The click that opened the chooser: where the chosen footprint is put (null when `A` opened it: the cursor). */
let chooserAt: PointXY | null = null;

/** The tool's stage (the test hook reports it). */
export function placeFootprintStage(): PlaceFootprintStage {
  return stage;
}

/** `m_placingFootprint`: a footprint is being carried. */
export function placingFootprint(): boolean {
  return stage === "carrying";
}

/** `PlaceFootprint` from `A`. */
export function startPlaceFootprint(c: PlaceFootprintCtx): void {
  // `if( m_inPlaceFootprint ) return 0;` -- unless the session it waits in is gone (another tool took the picker over without telling)
  if (stage !== "idle" && !(stage === "waiting" && picker.session()?.prompt !== WAITING_PROMPT)) return;
  ctx = c;
  // `m_toolMgr->RunAction( ACTIONS::selectionClear )`
  c.dispatch({ type: "SET_SELECTION", refs: [] });
  openChooser(null);
}

function openChooser(at: PointXY | null): void {
  if (!ctx) return;
  stage = "choosing";
  chooserAt = at;
  ctx.dispatch({ type: "PCBX", patch: { pcbDialog: "place_footprint" } });
}

/** The chooser was closed without a footprint: the tool waits for a click (`if( fp == nullptr ) continue;`); opened by anything else, nothing more happens. */
export function placeFootprintChooserCancelled(): void {
  if (stage === "choosing") waitForClick();
}

/**
 * The chooser chose: a library footprint is carried from the click that opened the chooser (or from the cursor); an unplaced part is armed for the next click.
 * A chooser opened outside the tool (an older caller) starts the tool with this pick.
 */
export function placeFootprintChosen(c: PlaceFootprintCtx, pick: { kind: "footprint"; name: string } | { kind: "unplaced"; ref: string }): void {
  ctx = c;
  if (pick.kind === "unplaced") {
    stage = "armed";
    c.dispatch({ type: "SET_SELECTION", refs: [] });
    c.dispatch({ type: "SET_ARMED", ref: pick.ref });
    return;
  }
  // `fp->SetOrientation( ANGLE_0 ); ... prevFrameAngle = ANGLE_0; applyPlacementFrameOrientation();`: a footprint fresh from the library has no frame turn yet.
  carry((at) => ({ op: "place_footprint", footprint: pick.name, at }), chooserAt, 0);
}

/** The armed part was put down (or Escape took the arming back): the tool waits for the next click. */
export function placeFootprintArmedDone(): void {
  if (stage === "armed") waitForClick();
}

/**
 * The Footprint Editor's Insert (`SaveFootprintToBoard`'s insert branch, then `RunAction( PCB_ACTIONS::placeFootprint, newFootprint )`): `build` makes the
 * command for where the board's cursor is; the footprint is carried and the tool goes on after it. False, and nothing done, while another footprint is carried.
 */
export function placeFootprintFromEditor(c: PlaceFootprintCtx, build: (at: PointXY) => Cmd): boolean {
  if (placingFootprint()) return false;
  if (stage === "waiting") picker.cancel();
  ctx = c;
  // "A footprint handed over from another command may already carry the frame rotation of the grid it sits in; count that as applied, like a move pick-up":
  // `SaveFootprintToBoard` hands it over at (0, 0), so the frame at the origin is what it carries.
  const grids = c.api.getState().board?.drawings?.grid_items ?? [];
  carry(build, null, grids.length ? gridFrameAngleAt(grids, [0, 0], "placement") : 0);
  return true;
}

/** `frameDeg`: `PlaceFootprint`'s `prevFrameAngle` for the footprint carried (kicad-port/gridFrame.ts): the carry turns it to a placement grid's frame from that. */
function carry(build: (at: PointXY) => Cmd, at: PointXY | null, frameDeg: number): void {
  const c = ctx;
  if (!c) return;
  stage = "carrying";
  void c.api
    // made but nothing new to carry: what it made stands, and the tool goes on
    .placeNewItems(build, true, placed, { at: at ?? undefined, frameDeg, afterCancel: waitForClick, afterSettle: settled, afterNothing: placed })
    .then((ok) => {
      // refused: no footprint to carry, the tool waits for the next click
      if (!ok && stage === "carrying") waitForClick();
    });
}

/** `m_toolMgr->RunAction( ACTIONS::selectionClear ); commit.Push( _( "Place Footprint" ) ); fp = nullptr;` -- and the loop goes on. */
function placed(): void {
  ctx?.dispatch({ type: "SET_SELECTION", refs: [] });
  waitForClick();
}

/** The carry was settled by leaving the Move tool: a click that never moved the footprint put it down where it was; another tool taking over ends this one. */
function settled(): void {
  if (ctx?.api.getState().activeTool === "select") placed();
  else end();
}

/** The tool on the stack with nothing carried: the next click opens the chooser, Escape (or another tool) ends it. */
function waitForClick(): void {
  if (!ctx) return end();
  stage = "waiting";
  picker.start({
    kind: "point",
    prompt: WAITING_PROMPT,
    onPoint: (at) => {
      openChooser({ x: Math.round(at.x), y: Math.round(at.y) });
      return false;
    },
    // `m_frame->PopTool( pushedEvent ); break;` -- Escape, or another tool activated
    onCancel: () => {
      if (stage === "waiting") end();
    },
  });
}

function end(): void {
  stage = "idle";
  ctx = null;
  chooserAt = null;
}
