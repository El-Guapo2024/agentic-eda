// Mounts the schematic control dialogs once, for `App.tsx`: whichever dialog `state.dialog` names (`schControlStore.tsx`).
import { useSchControlDispatch, useSchControlState } from "../../state/schControlStore";
import { AssignFootprintsDialog } from "./AssignFootprintsDialog";
import { BusSyntaxHelpDialog } from "./BusSyntaxHelpDialog";
import { ExportSymbolsDialog } from "./ExportSymbolsDialog";
import { IncrementAnnotationsDialog } from "./IncrementAnnotationsDialog";
import { LegacyBomDialog } from "./LegacyBomDialog";
import { LibLinksDialog } from "./LibLinksDialog";
import { PageNumberDialog } from "./PageNumberDialog";
import { SymbolCheckerDialog } from "./SymbolCheckerDialog";
import { SymbolDiffDialog } from "./SymbolDiffDialog";

export function SchControlHost() {
  const { dialog } = useSchControlState();
  const dispatch = useSchControlDispatch();
  const close = () => dispatch({ type: "CLOSE_DIALOG" });
  return (
    <>
      {/* The Net Navigator is a pane of the schematic's left column (components/panels/SchematicDock.tsx), not mounted here. */}
      {dialog?.kind === "bus_syntax" && <BusSyntaxHelpDialog onClose={close} />}
      {dialog?.kind === "increment_annotations" && <IncrementAnnotationsDialog onClose={close} />}
      {dialog?.kind === "page_number" && <PageNumberDialog path={dialog.path} onClose={close} />}
      {dialog?.kind === "library_links" && <LibLinksDialog onClose={close} />}
      {dialog?.kind === "assign_footprints" && <AssignFootprintsDialog onClose={close} />}
      {dialog?.kind === "export_symbols" && <ExportSymbolsDialog onClose={close} />}
      {dialog?.kind === "legacy_bom" && <LegacyBomDialog onClose={close} />}
      {dialog?.kind === "symbol_check" && <SymbolCheckerDialog onClose={close} />}
      {dialog?.kind === "symbol_diff" && <SymbolDiffDialog reference={dialog.ref} onClose={close} />}
    </>
  );
}
