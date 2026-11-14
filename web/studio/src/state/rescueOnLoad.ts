// Rescue Symbols run as a legacy schematic comes in (UI-LEFT-3 S3-26): `SCH_EDIT_FRAME::OpenProjectFiles` runs `RescueSymbolLibTableProject( false )` -- not on demand --
// for a KiCad 5 schematic whose symbols name their libraries and that has no cache library beside it, unless Preferences > Schematic Editor > Editing Options has "Never
// show Rescue Symbols tool" on (`m_RescueNeverShow`). Not on demand, the dialog says nothing when there is nothing to rescue and offers "Never Show Again"
// (`RESCUER::RescueProject( .., aRunningOnDemand = false )`, `InvokeDialog( aParent, aAskShowAgain = true )`).
//
// The import (actions/importActions.ts) marks the next Rescue Symbols as such a run; the dialog (components/schControl/RescueSymbolsDialog.tsx) takes the mark when it opens.

let onLoad = false;

/** The next Rescue Symbols dialog is the one a schematic coming in runs. */
export function markRescueOnLoad(): void {
  onLoad = true;
}

/** Whether the dialog opening now is that run; the mark is spent. */
export function takeRescueOnLoad(): boolean {
  const was = onLoad;
  onLoad = false;
  return was;
}
