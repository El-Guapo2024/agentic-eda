// The Draw > Draw Microwave Shapes tools (GAPS.md item 9, package P19; `MICROWAVE_TOOL`, pcbnew/microwave/ at KiCad 8303b2ad). Called from `registerPcbEditSweep`. Each tool is a run of
// picker sessions (kicad-port/pickerHost.ts: the next canvas click answers the session, Escape ends the tool) and of prompts (state/microwaveDialogs.ts: the tool waits for each).
//
//   pcbnew.MicrowaveTool.createGap             MICROWAVE_TOOL::addMicrowaveFootprint( GAP ): a click, "Gap Size:", and the footprint of two pads that far apart where the click was
//   pcbnew.MicrowaveTool.createStub            .. ( STUB ): "Stub Size:", a square pad and the stub
//   pcbnew.MicrowaveTool.createStubArc         .. ( STUB_ARC ): "Arc Stub Radius Value:" and "Angle in degrees:", one custom pad shaped as a pie slice
//
// `doInteractiveItemPlacement( .., IPO_REPEAT | IPO_ROTATE | IPO_FLIP )` makes the footprint and carries it on the cursor until a second click; here it is put down where the first
// click was (`Cmd::Microwave`, one undo step) and left selected, and the tool waits for the next click until Escape. KiCad's `ShowInfoBarError` is a toast.
import { fetchState } from "../api/client";
import type { Cmd, PointXY } from "../api/types";
import {
  ANGLE_DEFAULT,
  ANGLE_LABEL,
  INCORRECT_NUMBER,
  MICROWAVE_TITLE,
  SIZE_LABEL,
  arcStubAngleMillideg,
  lengthFromText,
  lengthText,
  parseDouble,
} from "../kicad-port/microwave";
import { askText } from "../state/microwaveDialogs";
import type { StudioState } from "../state/store";
import { picker } from "./pcbPicker";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";
import type { MicrowaveCmd } from "../api/microwaveTypes";

type FootprintTool = "gap" | "stub" | "arc_stub";

/** The status text of each tool's picker (`STATUS_TEXT_POPUP`). */
const PROMPT: Record<FootprintTool, string> = {
  gap: "Draw Microwave Gaps",
  stub: "Draw Microwave Stubs",
  arc_stub: "Draw Microwave Arc Stubs",
};

/** `GetCurrentTrackWidth()`: the track width the router would draw with now. */
function currentTrackWidth(st: StudioState): number {
  return st.currentTrackWidthUm ?? st.board?.board_rules?.track_width ?? 250;
}

export function registerMicrowaveActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);
  m.set("pcbnew.MicrowaveTool.createGap", pcbOnly(() => startFootprintTool(ctx, "gap")));
  m.set("pcbnew.MicrowaveTool.createStub", pcbOnly(() => startFootprintTool(ctx, "stub")));
  m.set("pcbnew.MicrowaveTool.createStubArc", pcbOnly(() => startFootprintTool(ctx, "arc_stub")));
}

/**
 * Send `verb` as one undo step and select the footprint it made (`m_toolMgr->RunAction( ACTIONS::selectItem, inductorFP.get() )` for the inductor; the placement tools leave what
 * they placed selected too). Resolves to whether the board accepted it; a refusal is toasted by the store, with the server's words.
 */
async function put(ctx: SweepCtx, verb: MicrowaveCmd): Promise<boolean> {
  const { api, dispatch } = ctx;
  const before = new Set((api.getState().board?.parts ?? []).map((p) => p.ref));
  const ok = await api.cmdBatch([{ op: "microwave", ...verb } as Cmd]);
  if (!ok) return false;
  const fresh = await fetchState().catch(() => null);
  const made = (fresh?.parts ?? []).filter((p) => !before.has(p.ref)).map((p) => p.ref);
  dispatch({ type: "SET_SELECTION", refs: made });
  return true;
}

/** The questions of one footprint tool (`createFootprint`, `createPolygonShape`) and the verb their answers make; `null` when the user cancelled or the answer was no number. */
async function askFootprint(ctx: SweepCtx, tool: FootprintTool, at: PointXY): Promise<MicrowaveCmd | null> {
  const st = ctx.api.getState();
  const units = st.units;
  const track = currentTrackWidth(st);
  const toast = (message: string) => ctx.dispatch({ type: "TOAST", message, kind: "error" });

  // `WX_TEXT_ENTRY_DIALOG dlg( editFrame, msg, _( "Create Microwave Footprint" ), editFrame->StringFromValue( gap_size ) )`: it starts at the track width
  const text = await askText({ title: MICROWAVE_TITLE, label: SIZE_LABEL[tool], initial: lengthText(track, units) });
  if (text === null) return null;
  const size = lengthFromText(text, units);
  if (tool !== "arc_stub") return { verb: tool, at, size_um: size, track_width_um: track };

  const angleText = await askText({ title: MICROWAVE_TITLE, label: ANGLE_LABEL, initial: ANGLE_DEFAULT });
  if (angleText === null) return null;
  const degrees = parseDouble(angleText);
  if (degrees === null) {
    toast(INCORRECT_NUMBER);
    return null;
  }
  return { verb: "arc_stub", at, radius_um: size, angle_millideg: arcStubAngleMillideg(degrees), track_width_um: track };
}

/**
 * `MICROWAVE_TOOL::addMicrowaveFootprint` through `doInteractiveItemPlacement`: the first click asks the tool's questions and makes the footprint, a cancelled question makes none and
 * the tool waits for another click; after a footprint the tool waits for the next (`IPO_REPEAT`), until Escape.
 */
function startFootprintTool(ctx: SweepCtx, tool: FootprintTool): void {
  const { dispatch } = ctx;
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  const next = (): void =>
    picker.start({
      kind: "point",
      prompt: `${PROMPT[tool]}: click to place it (Escape ends the tool)`,
      onPoint: (at) => {
        void (async () => {
          const verb = await askFootprint(ctx, tool, { x: at.x, y: at.y });
          if (verb) await put(ctx, verb);
          next();
        })();
        return false;
      },
    });
  next();
}
