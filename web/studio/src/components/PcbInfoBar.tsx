// The board editor's info bar over its canvas (`WX_INFOBAR`, components/InfoBar.tsx; `state.pcbInfoBar`): what KiCad's pcbnew says there (kicad-port/pcbInfoBar.ts) --
// its icon, its time (`ShowMessageFor`: `ShowInfoBarError` 8 s, `ShowInfoBarWarning` 6 s, `ShowInfoBarMsg` 8 s, the zone fill warning 5 s), the close button the
// call asked for, the zone fill warning's "Don't show again" (`unfilledZoneCheck`'s `wxHyperlinkCtrl`: sets the option and dismisses the bar); the next message
// replaces it and starts its time again. Draw Via's "Via location violates DRC." is dismissed by a clean via too (state/store.tsx `runCmd`).
import { useEffect } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { setCommonOptions } from "../state/commonOptions";
import { InfoBar } from "./InfoBar";

export function PcbInfoBar() {
  const { pcbInfoBar } = useStudioState();
  const dispatch = useStudioDispatch();
  useEffect(() => {
    if (!pcbInfoBar || pcbInfoBar.ms === null) return;
    const t = setTimeout(() => dispatch({ type: "DISMISS_PCB_INFOBAR" }), pcbInfoBar.ms);
    return () => clearTimeout(t);
  }, [pcbInfoBar, dispatch]);
  if (!pcbInfoBar) return null;
  const key = pcbInfoBar.dontShowAgain;
  return (
    <InfoBar
      message={pcbInfoBar.message}
      kind={pcbInfoBar.kind}
      testid="pcb-infobar"
      onClose={pcbInfoBar.close ? () => dispatch({ type: "DISMISS_PCB_INFOBAR" }) : undefined}
      link={
        key
          ? {
              label: "Don't show again",
              onClick: () => {
                setCommonOptions({ [key]: true });
                dispatch({ type: "DISMISS_PCB_INFOBAR" });
              },
            }
          : undefined
      }
    />
  );
}
