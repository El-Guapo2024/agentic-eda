// Port of `PromptConnectedPadDecision` (pcbnew/tools/edit_tool_move_fct.cpp), the question Swap Pad Nets and Swap Gate Nets ask when the copper connected to the pads
// they swap reaches other pads: "N unselected pad(s) are connected to these nets. How do you want to proceed?" with the list of those pads (the reference, a colon,
// the number) and three buttons -- "Ignore Unselected Pads" (their nets stay; the tracks and vias still swap), "Swap All Connected Pads" and Cancel. The answer goes with
// the verb as `connected_pads`. The question is asked by the action (actions/useActionRunner.ts) after `postSwapPreview` found such pads; with none, the swap happens.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { swapNetsCmd, unselectedPadLabel } from "../kicad-port/gateSwap";
import { getOverrideLocks } from "../state/overrideLocks";

export function SwapNetsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const prompt = state.swapNetsPrompt;
  const [busy, setBusy] = useState(false);

  if (!prompt) return null;

  const close = () => dispatch({ type: "SET_SWAP_NETS_PROMPT", prompt: null });
  const answer = async (include: boolean) => {
    setBusy(true);
    try {
      // a refusal (`ValidateNetSwapLocks`' locks) is told by the command and the question closes, as KiCad's info bar follows its dialog
      await api.cmd(swapNetsCmd(prompt.cmd, include, getOverrideLocks("pcb")));
      close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} data-testid="swap-nets-prompt" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{prompt.title}</span>
        </div>
        <div className="dialog-body">
          <p data-testid="swap-nets-message" style={{ marginTop: 0 }}>
            {prompt.pads.length} unselected pad(s) are connected to these nets. How do you want to proceed?
          </p>
          <p style={{ fontSize: 12, opacity: 0.8 }}>Connected tracks, vias, and other non-zone copper items will still swap nets even if you ignore the unselected pads.</p>
          <div style={{ fontSize: 12 }}>Unselected pads:</div>
          <ul data-testid="swap-nets-pads" style={{ margin: "4px 0 0", paddingLeft: 18, maxHeight: 140, overflow: "auto", fontSize: 12 }}>
            {prompt.pads.map((id) => (
              <li key={id}>{unselectedPadLabel(id)}</li>
            ))}
          </ul>
        </div>
        <div className="dialog-footer">
          <button data-testid="swap-nets-cancel" onClick={close}>
            Cancel
          </button>
          <button data-testid="swap-nets-ignore" onClick={() => void answer(false)} disabled={busy}>
            Ignore Unselected Pads
          </button>
          <button className="primary" data-testid="swap-nets-all" onClick={() => void answer(true)} disabled={busy}>
            Swap All Connected Pads
          </button>
        </div>
      </div>
    </div>
  );
}
