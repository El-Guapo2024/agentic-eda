// The actions of the Appearance panel that are KiCad actions, and the one entry point scripts use to drive the panel:
//
//   pcbnew.Control.netColorMode   PCB_CONTROL::NetColorModeCycle -- net colours on all copper, on the ratsnest only, off, and round again
//   studio.Appearance.op          this studio's own: `__eda.run( "studio.Appearance.op", { op: "object", id: "tracks", visible: false } )` is the click on the
//                                 Objects tab's eye (kicad-port/appearanceOps.ts has every op); the panel dispatches the same ones
//
// (The inactive-layer cycle, `common.Control.highContrastModeCycle`, is registered with the other common display actions in useActionRunner.ts.)
import type { Dispatch } from "react";
import type { Action, StudioState } from "../state/store";
import { nextNetColorMode } from "../kicad-port/appearance";
import { asAppearanceOp } from "../kicad-port/appearanceOps";

type Handler = (param?: unknown) => void;

export function registerAppearanceActions(m: Map<string, Handler>, ctx: { state: StudioState; dispatch: Dispatch<Action> }): void {
  const { state, dispatch } = ctx;
  const pcbOnly =
    (fn: Handler): Handler =>
    (param) => {
      if (state.tab === "pcb") fn(param);
    };
  m.set(
    "pcbnew.Control.netColorMode",
    pcbOnly(() => dispatch({ type: "APPEARANCE", op: { op: "net_color_mode", mode: nextNetColorMode(state.appearance.netColorMode) } }))
  );
  m.set(
    "studio.Appearance.op",
    pcbOnly((param) => {
      const op = asAppearanceOp(param);
      if (!op) throw new Error("studio.Appearance.op needs an op object: { op: \"object\" | \"opacity\" | \"net_color\" | ... }");
      dispatch({ type: "APPEARANCE", op });
    })
  );
}
