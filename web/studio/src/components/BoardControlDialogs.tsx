// The dialogs the board-control actions open (state.bcx): the export / output dialogs, the Zone Manager and the footprint
// associations report. One mount point, so App.tsx carries a single line for all of them.
import { BoardExportDialog } from "./BoardExportDialog";
import { FootprintAssociationsDialog } from "./FootprintAssociationsDialog";
import { ZoneManagerDialog } from "./ZoneManagerDialog";

export function BoardControlDialogs() {
  return (
    <>
      <BoardExportDialog />
      <ZoneManagerDialog />
      <FootprintAssociationsDialog />
    </>
  );
}
