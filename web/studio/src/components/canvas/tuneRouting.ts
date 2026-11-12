// The glue between the canvas and the length tuner's API (crates/cli/src/tune_api.rs) for `7` / `8` / `9`: KiCad's `DRAWING_TOOL::PlaceTuningPattern`. Hover a
// track and the status box beside the cursor reads its length (`updateHoverStatus`); the first click starts a pattern on it; the cursor sets where the pattern ends
// and the router's placer fits the meander to it; the second click puts the pattern on the board; Esc cancels the pattern, then ends the tool. The geometry and the
// reading are the server's; this turns a pointer and a key into the calls.
import type { Dispatch } from "react";
import { tuneCancel, tuneEditMove, tuneEditStart, tuneFix, tuneHover, tuneMove, tuneRemove, tuneStart, tuneStep } from "../../api/tuneClient";
import type { TuneKind, TuneSettings, TuningPattern } from "../../api/tuneTypes";
import type { Action, StudioApi, StudioState } from "../../state/store";
import { carriedSettings, kindOfMode, TUNE_KEY_ACTIONS, tuneStateFromReply, type TuneDrawState } from "../../kicad-port/tuneTool";
import { formatLength } from "../../state/units";
import { readoutSummary } from "../../kicad-port/tuneTool";

type S = Pick<StudioState, "activeLayer" | "lengthTuningMode" | "pcbx" | "view" | "drawState" | "units">;

/** The tool's settings for the kind of tuning it is on (`meanderSettings` of `PlaceTuningPattern`). */
export function toolSettings(state: Pick<StudioState, "lengthTuningMode" | "pcbx">): TuneSettings | undefined {
  return state.pcbx.tuneSettings[state.lengthTuningMode];
}

/** How far from a track's edge a click still picks it: `HITTEST_THRESHOLD_PIXELS` (5) of the view, at least 0.15 mm. */
export function pickToleranceNm(scale: number): number {
  return Math.max(150_000, 5 / scale);
}

const tuneDraw = (s: S): TuneDrawState | null => (s.drawState?.kind === "tune" ? s.drawState : null);

/** The summary line of a reading in the board's unit. */
export function summaryOf(state: Pick<StudioState, "units">, r: NonNullable<TuneDrawState["readout"]>): string {
  return readoutSummary(r, (um) => formatLength(um, state.units));
}

/** The pointer moved with no pattern started: the status box of the track under it (`updateHoverStatus`), or none. */
export async function hoverTune(state: S, dispatch: Dispatch<Action>, x: number, y: number): Promise<void> {
  const reply = await tuneHover({ x, y, tolerance: pickToleranceNm(state.view.scale), layer: state.activeLayer, mode: state.lengthTuningMode, settings: toolSettings(state) });
  dispatch({ type: "PCBX", patch: { tuneHover: reply.ok && reply.hit ? { reply, at: [x, y] } : null } });
}

/** The first click: a pattern starts on the track under the cursor. A refusal is KiCad's own message. */
export async function startTune(state: S, dispatch: Dispatch<Action>, x: number, y: number): Promise<void> {
  const mode: TuneKind = state.lengthTuningMode;
  const reply = await tuneStart({ x, y, tolerance: pickToleranceNm(state.view.scale), layer: state.activeLayer, mode, settings: toolSettings(state) });
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "Select a track to tune first.", kind: "error" });
    return;
  }
  dispatch({ type: "PCBX", patch: { tuneHover: null } });
  dispatch({ type: "SET_DRAW_STATE", draw: tuneStateFromReply(reply, null, { mode, layer: state.activeLayer ?? "F.Cu" }) });
}

/** The cursor moved with a pattern started: the placer fits the meander from the start to it. `guard` drops a reply a newer move has overtaken. */
export async function moveTune(state: S, dispatch: Dispatch<Action>, x: number, y: number, guard?: () => boolean): Promise<void> {
  const draw = tuneDraw(state);
  if (!draw) return;
  // the edit of a pattern of the board (`genStartEdit`) has the end of the pattern follow the cursor
  if (draw.editing) return updateEditPattern(dispatch, 1, x, y, guard);
  const reply = await tuneMove(x, y);
  if (guard && !guard()) return;
  if (!reply.ok) return;
  dispatch({ type: "SET_DRAW_STATE", draw: tuneStateFromReply(reply, draw, { mode: draw.mode, layer: draw.layer }) });
}

/** The second click: the pattern goes on the board (one undo step); the tool stays on, hovering, as `PlaceTuningPattern` does. */
export async function fixTune(state: S, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">, x?: number, y?: number): Promise<boolean> {
  const draw = tuneDraw(state);
  if (!draw) return false;
  const reply = await tuneFix(x, y);
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "The tuning pattern could not be placed.", kind: "error" });
    return false;
  }
  // what the pattern left of the tool's settings (a key that stepped the spacing) is what the next one starts from
  if (draw.pattern) dispatch({ type: "PCBX", patch: { tuneSettings: { ...state.pcbx.tuneSettings, [draw.mode]: carriedSettings(draw.pattern, state.pcbx.tuneSettings[draw.mode]) } } });
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  await api.refresh();
  if (reply.pattern_id) dispatch({ type: "SET_SELECTION", refs: [reply.pattern_id] });
  return true;
}

/** Esc with a pattern started: the pattern is dropped, the tool goes on; the caller ends the tool when there was none. Resolves to whether there was a pattern. */
export function cancelTune(state: S, dispatch: Dispatch<Action>): boolean {
  const had = tuneDraw(state) != null;
  void tuneCancel();
  dispatch({ type: "PCBX", patch: { tuneHover: null } });
  if (had) dispatch({ type: "SET_DRAW_STATE", draw: null });
  return had;
}

/** `1`-`4` while a pattern is started: the spacing or the amplitude one step more or less, and the meander fitted again. Without a pattern: KiCad's own warning. */
export async function stepTune(state: S, dispatch: Dispatch<Action>, action: string): Promise<void> {
  const key = TUNE_KEY_ACTIONS[action];
  const draw = tuneDraw(state);
  if (!key) return;
  if (!draw) {
    dispatch({ type: "TOAST", message: "Select a track to tune first.", kind: "error" });
    return;
  }
  const reply = await tuneStep(key);
  if (!reply.ok) return;
  dispatch({ type: "SET_DRAW_STATE", draw: tuneStateFromReply(reply, draw, { mode: draw.mode, layer: draw.layer }) });
  if (reply.pattern) dispatch({ type: "PCBX", patch: { tuneSettings: { ...state.pcbx.tuneSettings, [draw.mode]: carriedSettings(reply.pattern, state.pcbx.tuneSettings[draw.mode]) } } });
}

/** `genStartEdit`: a pattern of the board is edited through its four edit points. Its tracks are not changed until `genFinishEdit`. Resolves to whether the edit started. */
export async function startEditPattern(dispatch: Dispatch<Action>, pattern: TuningPattern): Promise<boolean> {
  const reply = await tuneEditStart(pattern.id);
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "The tuning pattern could not be edited.", kind: "error" });
    return false;
  }
  dispatch({ type: "SET_DRAW_STATE", draw: tuneStateFromReply(reply, null, { mode: kindOfMode(pattern.mode), layer: pattern.layer }) });
  return true;
}

/** `genUpdateEdit`: an edit point of the pattern being edited is at `(x, y)` (0 the start, 1 the end, 2 the width of the meanders and the side they start on, 3 their spacing). */
export async function updateEditPattern(dispatch: Dispatch<Action>, handle: number, x: number, y: number, guard?: () => boolean): Promise<void> {
  const reply = await tuneEditMove({ handle, x, y });
  if (guard && !guard()) return;
  if (!reply.ok || !reply.pattern) return;
  dispatch({ type: "SET_DRAW_STATE", draw: tuneStateFromReply(reply, null, { mode: kindOfMode(reply.pattern.mode), layer: reply.pattern.layer }) });
}

/** `genFinishEdit`: the edit is put on the board (one undo step). */
export async function finishEditPattern(dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<boolean> {
  const reply = await tuneFix();
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "The tuning pattern could not be changed.", kind: "error" });
    return false;
  }
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  await api.refresh();
  return true;
}

/** `genCacnelEdit`: the edit is dropped. */
export function cancelEditPattern(dispatch: Dispatch<Action>): void {
  void tuneCancel();
  dispatch({ type: "SET_DRAW_STATE", draw: null });
}

/**
 * Dragging an edit point of a selected pattern (`PCB_POINT_EDITOR` over a generator): the press starts the edit session, every move puts the point at the cursor (the
 * latest position wins while a request is out, so they never overtake one another), the release puts the result on the board -- or drops the edit when the point did not move.
 */
export class TuneHandleDrag {
  moved = false;
  readonly ready: Promise<boolean>;
  private latest: { x: number; y: number } | null = null;
  private idle: Promise<void> = Promise.resolve();

  constructor(
    private readonly dispatch: Dispatch<Action>,
    pattern: TuningPattern,
    readonly handle: number
  ) {
    this.ready = startEditPattern(dispatch, pattern);
  }

  move(x: number, y: number): void {
    this.moved = true;
    this.latest = { x, y };
    this.idle = this.idle.then(() => this.pump());
  }

  private async pump(): Promise<void> {
    if (!(await this.ready)) return;
    while (this.latest) {
      const at = this.latest;
      this.latest = null;
      await updateEditPattern(this.dispatch, this.handle, at.x, at.y);
    }
  }

  async finish(api: Pick<StudioApi, "refresh">): Promise<void> {
    await this.idle;
    if (!(await this.ready)) return;
    if (this.moved) await finishEditPattern(this.dispatch, api);
    else cancelEditPattern(this.dispatch);
  }
}

/**
 * A tuning pattern moved on its own, live (UI-LEFT-2 RR5): `EDIT_TOOL::doMoveSelection` of a lone generator calls `genStartEdit` when the move starts, then at
 * every motion `item->Move( movement )` (the pattern's start and end, `PCB_TUNING_PATTERN::Move` in an edit) and `genUpdateEdit` (`resetToBaseline` puts them
 * back on the track and the pattern is tuned there: edit point 4 of `/api/tune/edit_move`), and `genFinishEdit` at the drop (`genCancelEdit` on Esc). So the
 * pattern slides along its track as the cursor moves, as the edit points' drags show it.
 */
export class PatternSlide {
  readonly ready: Promise<boolean>;
  private latest: { dx: number; dy: number } | null = null;
  private idle: Promise<void> = Promise.resolve();
  private sent = false;

  constructor(
    private readonly dispatch: Dispatch<Action>,
    readonly pattern: TuningPattern
  ) {
    this.ready = tuneEditStart(pattern.id, { move_from: { x: pattern.origin.x, y: pattern.origin.y } }).then((r) => {
      if (!r.ok) dispatch({ type: "TOAST", message: r.message ?? "The tuning pattern could not be moved.", kind: "error" });
      return r.ok;
    });
  }

  private last: { dx: number; dy: number } | null = null;

  /** The cursor has carried the pattern `(dx, dy)` from where the move started (the same place again sends nothing). */
  move(dx: number, dy: number): void {
    if (this.last && this.last.dx === dx && this.last.dy === dy) return;
    this.last = { dx, dy };
    this.latest = { dx, dy };
    this.idle = this.idle.then(() => this.pump());
  }

  private async pump(): Promise<void> {
    if (!(await this.ready)) return;
    while (this.latest) {
      const { dx, dy } = this.latest;
      this.latest = null;
      if (dx === 0 && dy === 0 && !this.sent) continue;
      this.sent = true;
      await updateEditPattern(this.dispatch, 4, this.pattern.origin.x + dx, this.pattern.origin.y + dy);
    }
  }

  /** The drop at `(dx, dy)`: `genFinishEdit`, or nothing changed when it never moved. Resolves to whether the pattern changed. */
  async finish(api: Pick<StudioApi, "refresh">, dx: number, dy: number): Promise<boolean> {
    this.move(dx, dy);
    this.idle = this.idle.then(() => this.pump());
    await this.idle;
    if (!(await this.ready)) return false;
    if (!this.sent) {
      cancelEditPattern(this.dispatch);
      return false;
    }
    return finishEditPattern(this.dispatch, api);
  }

  /** `genCancelEdit`: the move was given up. */
  cancel(): void {
    this.latest = null;
    void this.idle.then(async () => {
      if (await this.ready) cancelEditPattern(this.dispatch);
    });
  }
}

let activeSlide: PatternSlide | null = null;

/** The live slide of `pattern`, started when the move picks it up. */
export function patternSlideFor(dispatch: Dispatch<Action>, pattern: TuningPattern): PatternSlide {
  if (!activeSlide || activeSlide.pattern.id !== pattern.id) {
    activeSlide?.cancel();
    activeSlide = new PatternSlide(dispatch, pattern);
  }
  return activeSlide;
}

/** The drop takes the live slide of pattern `id`, if there is one (it is no longer the active one). */
export function takePatternSlide(id: string): PatternSlide | null {
  if (activeSlide && activeSlide.pattern.id === id) {
    const s = activeSlide;
    activeSlide = null;
    return s;
  }
  return null;
}

/** The move ended without a drop (Esc, another tool): the live slide is cancelled. */
export function dropPatternSlide(): void {
  activeSlide?.cancel();
  activeSlide = null;
}

/** Whether pattern `id` is being slid live (the move preview then does not draw it carried rigidly). */
export function isPatternSliding(id: string | undefined): boolean {
  return id != null && activeSlide?.pattern.id === id;
}

/**
 * A tuning pattern moved on its own (`EDIT_TOOL::doMoveSelection` of a lone generator): `genStartEdit`, the pattern's start and end moved by `(dx, dy)` and put
 * back on its track (`PCB_TUNING_PATTERN::Move` in an edit, then `genUpdateEdit`'s `resetToBaseline`: edit point 4 of `/api/tune/edit_move`), `genFinishEdit` -- so
 * it slides along the track and is tuned again there, one undo step. Resolves to whether it was changed.
 */
export async function slidePattern(dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">, pattern: TuningPattern, dx: number, dy: number): Promise<boolean> {
  const from = { x: pattern.origin.x, y: pattern.origin.y };
  const started = await tuneEditStart(pattern.id, { move_from: from });
  if (!started.ok) {
    dispatch({ type: "TOAST", message: started.message ?? "The tuning pattern could not be moved.", kind: "error" });
    return false;
  }
  const moved = await tuneEditMove({ handle: 4, x: from.x + dx, y: from.y + dy });
  if (!moved.ok) {
    void tuneCancel();
    dispatch({ type: "TOAST", message: moved.message ?? "The tuning pattern could not be moved.", kind: "error" });
    return false;
  }
  return finishEditPattern(dispatch, api);
}

/** Delete on tuning patterns (`PCB_TUNING_PATTERN::Remove`): the pattern's tracks go back to the baseline, one undo step for each pattern. Resolves to how many were removed. */
export async function removeTuningPatterns(ids: readonly string[], api: Pick<StudioApi, "refresh">, dispatch: Dispatch<Action>): Promise<number> {
  let n = 0;
  for (const id of ids) {
    const r = await tuneRemove(id);
    if (r.ok) n++;
    else dispatch({ type: "TOAST", message: r.message ?? "The tuning pattern could not be removed.", kind: "error" });
  }
  if (n > 0) await api.refresh();
  return n;
}
