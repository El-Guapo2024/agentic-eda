// The Draw > Draw Microwave Shapes tools (GAPS.md item 9, package P19; `MICROWAVE_TOOL`, pcbnew/microwave/ at KiCad 8303b2ad). Called from `registerPcbEditSweep`. Each tool is a run of
// picker sessions (kicad-port/pickerHost.ts: the next canvas click answers the session, Escape ends the tool) and of prompts (state/microwaveDialogs.ts: the tool waits for each).
//
//   pcbnew.MicrowaveTool.createGap             MICROWAVE_TOOL::addMicrowaveFootprint( GAP ): a click, "Gap Size:", and the footprint of two pads that far apart where the click was
//   pcbnew.MicrowaveTool.createStub            .. ( STUB ): "Stub Size:", a square pad and the stub
//   pcbnew.MicrowaveTool.createStubArc         .. ( STUB_ARC ): "Arc Stub Radius Value:" and "Angle in degrees:", one custom pad shaped as a pie slice
//   pcbnew.MicrowaveTool.createFunctionShape   .. ( FUNCTION_SHAPE ): the Complex Shape dialog and the polygon of a shape description file between two pads
//   pcbnew.MicrowaveTool.createLine            MICROWAVE_TOOL::drawMicrowaveInductor: two clicks, "Length of track:", "Component value:", and the S-shaped coil between the points
//
// `doInteractiveItemPlacement( .., IPO_REPEAT | IPO_ROTATE | IPO_FLIP )` makes the footprint and carries it on the cursor until a second click; here it is put down where the first
// click was (`Cmd::Microwave`, one undo step) and left selected, and the tool waits for the next click until Escape. KiCad's `ShowInfoBarError` is a toast.
import { fetchState } from "../api/client";
import { checkInductor } from "../api/microwaveClient";
import type { Cmd, PointXY } from "../api/types";
import {
  ANGLE_DEFAULT,
  ANGLE_LABEL,
  INCORRECT_NUMBER,
  LENGTH_LABEL,
  MICROWAVE_TITLE,
  SIZE_LABEL,
  VALUE_DEFAULT,
  VALUE_LABEL,
  arcStubAngleMillideg,
  inductorEnd,
  inductorMinLengthUm,
  isValidValueName,
  lengthFromText,
  lengthText,
  lengthTextAtLeast,
  parseDouble,
  shapeSizeOverride,
} from "../kicad-port/microwave";
import { setToolPreview } from "../components/canvas/pcbItemsPaint";
import { askShape, askText } from "../state/microwaveDialogs";
import type { StudioState } from "../state/store";
import { picker } from "./pcbPicker";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";
import type { PickerSession } from "../kicad-port/pickerHost";
import type { MicrowaveCmd } from "../api/microwaveTypes";

type FootprintTool = "gap" | "stub" | "arc_stub" | "function_shape";

/** The status text of each tool's picker (`STATUS_TEXT_POPUP`). */
const PROMPT: Record<FootprintTool, string> = {
  gap: "Draw Microwave Gaps",
  stub: "Draw Microwave Stubs",
  arc_stub: "Draw Microwave Arc Stubs",
  function_shape: "Draw Microwave Polygonal Shapes",
};

/**
 * `g_ShapeScaleX` and `g_ShapeScaleY` are statics of `microwave_polygon.cpp`: once a shape description file has been read they keep their scales, so a dialog closed without reading
 * one finds a size and no points ("Shape has no points."); before any file was read there is no size ("Shape has a null size.").
 */
let shapeScalesRead = false;

/** `GetCurrentTrackWidth()`: the track width the router would draw with now. */
function currentTrackWidth(st: StudioState): number {
  return st.currentTrackWidthUm ?? st.board?.board_rules?.track_width ?? 250;
}

export function registerMicrowaveActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);
  m.set("pcbnew.MicrowaveTool.createGap", pcbOnly(() => startFootprintTool(ctx, "gap")));
  m.set("pcbnew.MicrowaveTool.createStub", pcbOnly(() => startFootprintTool(ctx, "stub")));
  m.set("pcbnew.MicrowaveTool.createStubArc", pcbOnly(() => startFootprintTool(ctx, "arc_stub")));
  m.set("pcbnew.MicrowaveTool.createFunctionShape", pcbOnly(() => startFootprintTool(ctx, "function_shape")));
  m.set("pcbnew.MicrowaveTool.createLine", pcbOnly(() => startInductorTool(ctx)));
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

  if (tool === "function_shape") {
    // `MWAVE_POLYGONAL_SHAPE_DLG dlg( &editFrame, wxDefaultPosition ); dlg.ShowModal()`
    const a = await askShape(units);
    if (a === null) return null;
    if (a.file === null) {
      toast(shapeScalesRead ? "Shape has no points." : "Shape has a null size.");
      return null;
    }
    shapeScalesRead = true;
    return { verb: "function_shape", at, file: a.file, kind: a.kind, size_x_um: shapeSizeOverride(a.sizeX, a.shownX, units), size_y_um: shapeSizeOverride(a.sizeY, a.shownY, units), track_width_um: track };
  }

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

/**
 * `MICROWAVE_TOOL::drawMicrowaveInductor` and `createInductorBetween`: a click sets the origin (a rectangle along the centre line, half as wide as the line is long, follows the cursor,
 * its end held by the angle snap), Escape takes the origin back, and the second click asks "Length of track:" (starting at the distance between the points) and, once the coil can be
 * made, "Component value:". The tool then waits for the next origin, until Escape.
 */
function startInductorTool(ctx: SweepCtx): void {
  const { api, dispatch } = ctx;
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  const toast = (message: string) => dispatch({ type: "TOAST", message, kind: "error" });

  const first = (): void =>
    picker.start({
      kind: "point",
      prompt: "Draw Microwave Lines: click where the coil starts (Escape ends the tool)",
      onPoint: (start) => second({ x: start.x, y: start.y }),
    });

  const second = (start: PointXY): PickerSession => {
    setToolPreview({ kind: "centreline_rect", from: [start.x, start.y], aspect: 0.5 });
    return {
      kind: "point",
      prompt: "Draw Microwave Lines: click where the coil ends (Escape takes the start back)",
      onPoint: (at) => {
        setToolPreview(null);
        const end = inductorEnd(start, { x: at.x, y: at.y }, api.getState().pcbx.angleSnapMode);
        void create(start, end).then(first);
        return false;
      },
      // `if( originSet ) cleanup();` -- the start is forgotten and the tool waits for another; another tool taking over ends it.
      onCancel: (activated) => {
        setToolPreview(null);
        if (!activated) first();
      },
    };
  };

  /** `createInductorBetween` + `createMicrowaveInductor`. */
  const create = async (start: PointXY, end: PointXY): Promise<void> => {
    const st = api.getState();
    const units = st.units;
    const track = currentTrackWidth(st);
    const lengthAnswer = await askText({ title: MICROWAVE_TITLE, label: LENGTH_LABEL, initial: lengthTextAtLeast(inductorMinLengthUm(start, end), units) });
    if (lengthAnswer === null) return;
    const length = lengthFromText(lengthAnswer, units);
    // the length is judged before the value is asked: `aErrorMessage` and `ShowInfoBarError`
    const verdict = await checkInductor({ start, end, length_um: length, track_width_um: track }).catch(() => null);
    if (verdict && !verdict.ok) {
      toast(verdict.message ?? "The inductor cannot be made.");
      return;
    }
    const name = await askText({ title: MICROWAVE_TITLE, label: VALUE_LABEL, initial: VALUE_DEFAULT, accept: isValidValueName, hint: 'A footprint name cannot have any of  % $ < > " \\ / :  or a tab or a line break in it.' });
    if (name === null) return;
    await put(ctx, { verb: "inductor", start, end, length_um: length, name, track_width_um: track });
  };

  first();
}
